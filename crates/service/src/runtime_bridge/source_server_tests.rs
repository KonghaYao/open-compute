use super::*;
use crate::p3_3_test_support::RuntimeFeatureFixture;
use open_compute_core::SecretString;
use open_compute_workers::python_artifact::{
    PreparedPythonIdentity, PythonRuntimePin, publish_prepared_python,
};
use open_compute_workers::{
    BundleLimits, CanonicalBundle, CreateVersionOutcome, CreateVersionRequest, ModuleInput,
    ModuleType, VersionContent, VersionController, VersionRuntimeFeatures,
};
use std::collections::BTreeMap;

#[tokio::test]
async fn prepared_pin_uses_verified_host_binary_and_current_embedded_system_assets() {
    let directory = tempfile::tempdir().unwrap();
    let package = open_compute_runtime::materialize_embedded_runtime(directory.path()).unwrap();
    let runtime = package
        .verify(
            Duration::from_secs(20),
            &open_compute_core::Redactor::new(),
            &directory.path().join("child.lease"),
        )
        .await
        .unwrap();
    let pin = python_runtime_pin(&runtime);
    pin.validate().unwrap();
    assert_eq!(pin.workerd_revision, runtime.lock().revision);
    assert_eq!(pin.workerd_binary_sha256, runtime.binary_sha256());
    assert_eq!(pin.process_flags, runtime.lock().process_flags);
    assert_eq!(
        pin.pyodide_bundle_sha256,
        runtime.lock().pyodide_bundle.bundle_sha256
    );
    assert_eq!(
        pin.runtime_assets_sha256,
        open_compute_runtime::embedded_runtime_assets_sha256()
    );
    assert!(!directory.path().join("child.lease").exists());
}

struct Fixture {
    base: RuntimeFeatureFixture,
    state: SourceState,
    body: serde_json::Value,
}

impl Fixture {
    async fn create() -> Self {
        let features = VersionRuntimeFeatures {
            compatibility_date: "2026-09-08".to_owned(),
            ..Default::default()
        };
        let base = RuntimeFeatureFixture::create(features.clone()).await;
        let pin = PythonRuntimePin {
            workerd_revision: "a".repeat(40),
            workerd_binary_sha256: "b".repeat(64),
            process_flags: vec!["--experimental".to_owned()],
            pyodide_bundle_sha256: "c".repeat(64),
            runtime_assets_sha256: "d".repeat(64),
        };
        let source = RuntimeSource::new(
            base.storage.clone(),
            base.artifacts.clone(),
            BundleLimits::default(),
            pin.clone(),
        )
        .unwrap();
        let validator = {
            let storage = base.storage.clone();
            let artifacts = base.artifacts.clone();
            let source = source.clone();
            move |candidate: ValidationCandidate| {
                let storage = storage.clone();
                let artifacts = artifacts.clone();
                let pin = pin.clone();
                let source = source.clone();
                async move {
                    let snapshot = source
                        .resolve(
                            &loader_key(
                                candidate.instance_id,
                                candidate.worker_id,
                                candidate.version_id,
                            ),
                            &hex::encode(candidate.worker_code_sha256),
                            RuntimeScope::Preparation,
                        )
                        .await?;
                    let identity = PreparedPythonIdentity::from_snapshot(pin, &snapshot)?;
                    publish_prepared_python(
                        &storage,
                        &artifacts,
                        &identity,
                        &SecretBytes::new(b"private native snapshot secret".to_vec()),
                        2,
                        |publish| publish(),
                    )
                    .await?;
                    Ok(())
                }
            }
        };
        let bundle = CanonicalBundle::build(
            "main.py",
            vec![ModuleInput {
                name: "main.py".to_owned(),
                module_type: ModuleType::Python,
                bytes: b"pass\n".to_vec(),
            }],
            BundleLimits::default(),
        )
        .unwrap();
        let request = CreateVersionRequest {
            instance_id: base.account,
            worker_id: base.worker,
            idempotency_key: "python-source-http".to_owned(),
            content: VersionContent::Worker {
                bundle: bundle.into_bytes().into(),
                assets: None,
            },
            vars: BTreeMap::new(),
            secrets: BTreeMap::new(),
            bindings: BTreeMap::new(),
            services: BTreeMap::new(),
            runtime_features: features,
            queue_consumers: Vec::new(),
            crons: Vec::new(),
            deployment_source: None,
            observability: None,
            request_id: RequestId::generate(),
            now_ms: 2,
        };
        let result = VersionController::new(
            &base.storage,
            base.artifacts.clone(),
            Arc::new(validator),
            BundleLimits::default(),
        )
        .create_version(request)
        .await
        .unwrap();
        let CreateVersionOutcome::Applied(result) = result else {
            panic!("unexpected replay")
        };
        let key = loader_key(base.account, base.worker, result.version.id);
        let digest = hex::encode(result.version.worker_code_sha256);
        let prepared = source
            .resolve(&key, &digest, RuntimeScope::Runtime)
            .await
            .unwrap()
            .python_prepared_sha256
            .unwrap();
        let auth = GenerationAuthRegistry::new();
        auth.activate_for_test(SecretString::new("a".repeat(64)));
        Self {
            base,
            state: SourceState { source, auth },
            body: serde_json::json!({"key":key,"expectedWorkerCodeSha256":digest,"expectedPreparedSha256":prepared,"scope":"runtime"}),
        }
    }

    fn request(&self, body: Body) -> Request {
        Request::builder()
            .method(Method::POST)
            .uri(PYTHON_SNAPSHOT_PATH)
            .header(TOKEN_HEADER, "a".repeat(64))
            .header(GENERATION_HEADER, "process-generation")
            .body(body)
            .unwrap()
    }
}

#[tokio::test]
async fn python_snapshot_endpoint_returns_only_authenticated_verified_private_binary() {
    let fixture = Fixture::create().await;
    let listener = bind_runtime_source().await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, receiver) = tokio::sync::oneshot::channel();
    let source = fixture.state.source.clone();
    let auth = fixture.state.auth.clone();
    let server = tokio::spawn(async move {
        serve_runtime_source(listener, source, auth, async move {
            let _ = receiver.await;
        })
        .await
        .unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let url = format!("http://{address}{PYTHON_SNAPSHOT_PATH}");
    for (token, generation) in [
        ("".to_owned(), "process-generation"),
        ("b".repeat(64), "process-generation"),
        ("a".repeat(64), ""),
    ] {
        let response = client
            .post(&url)
            .header(TOKEN_HEADER, token)
            .header(GENERATION_HEADER, generation)
            .header(header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&fixture.body).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(response.bytes().await.unwrap().is_empty());
    }
    let response = client
        .post(&url)
        .header(TOKEN_HEADER, "a".repeat(64))
        .header(GENERATION_HEADER, "process-generation")
        .header(header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&fixture.body).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(
        response.bytes().await.unwrap(),
        b"private native snapshot secret".as_slice()
    );
    let metadata = {
        let mut metadata = fixture.body.clone();
        metadata
            .as_object_mut()
            .unwrap()
            .remove("expectedPreparedSha256");
        metadata
    };
    let response = client
        .post(format!("http://{address}{SOURCE_PATH}"))
        .header(TOKEN_HEADER, "a".repeat(64))
        .header(GENERATION_HEADER, "process-generation")
        .header(header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&metadata).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let wire: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(
        wire["pythonPreparedSha256"],
        fixture.body["expectedPreparedSha256"]
    );
    assert!(wire.get("pythonSnapshot").is_none());
    fixture
        .base
        ._mock
        .set_fault(open_compute_artifacts::Fault::CorruptBody);
    let response = client
        .post(&url)
        .header(TOKEN_HEADER, "a".repeat(64))
        .header(GENERATION_HEADER, "process-generation")
        .header(header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&fixture.body).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.headers()[ERROR_HEADER], "ARTIFACT_INTEGRITY_ERROR");
    assert!(response.bytes().await.unwrap().is_empty());
    drop(client);
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn source_request_rejects_invalid_frames_and_rechecks_generation_before_response_commit() {
    let fixture = Fixture::create().await;
    let empty = SourceState {
        source: fixture.state.source.clone(),
        auth: GenerationAuthRegistry::new(),
    };
    assert_eq!(
        resolve_python(State(empty), fixture.request(Body::empty()))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    for body in [
        b"invalid JSON".to_vec(),
        serde_json::to_vec(&serde_json::json!({"key":"x","scope":"runtime"})).unwrap(),
        serde_json::to_vec(&{
            let mut body = fixture.body.clone();
            body["private"] = serde_json::json!(true);
            body
        })
        .unwrap(),
    ] {
        let response = resolve_python(
            State(fixture.state.clone()),
            fixture.request(Body::from(body)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[ERROR_HEADER], "BUNDLE_INVALID");
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }
    for declared in [true, false] {
        let mut request = fixture.request(Body::from(vec![0; MAX_SOURCE_REQUEST + 1]));
        if declared {
            request.headers_mut().insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&(MAX_SOURCE_REQUEST + 1).to_string()).unwrap(),
            );
        }
        let response = resolve_python(State(fixture.state.clone()), request).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(response.headers()[ERROR_HEADER], "BUNDLE_TOO_LARGE");
    }
    for binary in [true, false] {
        fixture
            .state
            .auth
            .activate_for_test(SecretString::new("a".repeat(64)));
        let mut value = fixture.body.clone();
        if !binary {
            value
                .as_object_mut()
                .unwrap()
                .remove("expectedPreparedSha256");
        }
        let bytes = bytes::Bytes::from(serde_json::to_vec(&value).unwrap());
        let (started, receiving) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let request = fixture.request(Body::from_stream(futures::stream::once(async move {
            started.send(()).unwrap();
            released.await.unwrap();
            Ok::<_, std::io::Error>(bytes)
        })));
        let state = fixture.state.clone();
        let pending = tokio::spawn(async move {
            if binary {
                resolve_python(State(state), request).await
            } else {
                resolve(State(state), request).await
            }
        });
        tokio::time::timeout(Duration::from_secs(3), receiving)
            .await
            .unwrap()
            .unwrap();
        fixture
            .state
            .auth
            .activate_for_test(SecretString::new("b".repeat(64)));
        release.send(()).unwrap();
        let response = tokio::time::timeout(Duration::from_secs(3), pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
