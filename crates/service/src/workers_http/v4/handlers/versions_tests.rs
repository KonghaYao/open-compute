use super::*;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use open_compute_core::{RequestId, SecretString};
use open_compute_storage::worker_repository::{
    NewVersion, NewVersionProducts, VersionContentKind, WorkerRepository,
};
use std::collections::BTreeMap;
use tower::ServiceExt as _;

fn ready_version(
    repository: WorkerRepository<'_>,
    account: open_compute_core::InstanceId,
    worker: open_compute_core::WorkerId,
    now: i64,
) -> VersionId {
    let id = VersionId::generate();
    repository
        .insert_staging_version(
            &NewVersion {
                id,
                instance_id: account,
                worker_id: worker,
                content_kind: VersionContentKind::Worker,
                artifact_sha256: Some([1; 32]),
                artifact_size: Some(1),
                artifact_schema_version: Some(1),
                main_module: Some("index.js".to_owned()),
                worker_code_sha256: [2; 32],
                compatibility_date: "2026-09-08".to_owned(),
                compatibility_flags: Vec::new(),
                resource_limits:
                    open_compute_storage::worker_repository::EffectiveResourceLimits::standard_defaults(),
                vars: BTreeMap::new(),
                secrets: BTreeMap::new(),
                request_id: RequestId::generate(),
                now_ms: now,
            },
            &NewVersionProducts::default(),
            100,
        )
        .unwrap();
    repository.begin_validation(id).unwrap();
    repository.mark_ready(id, now + 1).unwrap();
    id
}

#[tokio::test]
async fn beta_delete_tombstones_only_non_active_versions_and_replays() {
    let (_temp, _mock, state, account, storage) =
        crate::tests::initialized_worker_http_fixture().await;
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(account, "versions", RequestId::generate(), 1, 100)
        .unwrap()
        .0;
    let historical = ready_version(repository, account, worker.id, 2);
    let active = ready_version(repository, account, worker.id, 4);
    repository
        .promote(account, worker.id, active, None, RequestId::generate(), 6)
        .unwrap();
    let authority = crate::cloudflare_v4::accounts::V4InstanceContext::new(account, 1);
    let worker_tag = authority.public_worker_tag(worker.id);
    let worker_path = format!(
        "/client/v4/accounts/{}/workers/workers/versions",
        authority.public_id()
    );
    let prefix = format!(
        "/client/v4/accounts/{}/workers/workers/versions/versions/",
        authority.public_id()
    );
    let app = crate::http::admin_router(
        state
            .with_platform_storage(storage.clone())
            .with_v4_tokens(
                SecretString::new("deployer-token"),
                SecretString::new("read-token"),
            )
            .with_v4_instance_context(authority),
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(&worker_path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{worker_path}?unexpected=true"))
                    .header(header::AUTHORIZATION, "Bearer read-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(worker_path.replace("/versions", "/missing"))
                    .header(header::AUTHORIZATION, "Bearer read-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(worker_path)
                .header(header::AUTHORIZATION, "Bearer read-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["result"]["id"], worker_tag);
    assert_eq!(body["result"]["subdomain"]["enabled"], false);
    let send = |version: VersionId| {
        Request::builder()
            .method("DELETE")
            .uri(format!("{prefix}{version}"))
            .header(header::AUTHORIZATION, "Bearer deployer-token")
            .body(Body::empty())
            .unwrap()
    };
    for request in [
        Request::builder()
            .method("DELETE")
            .uri(format!("{prefix}{historical}"))
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("DELETE")
            .uri(format!("{prefix}{historical}?unexpected=true"))
            .header(header::AUTHORIZATION, "Bearer deployer-token")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("DELETE")
            .uri(format!("{prefix}{historical}"))
            .header(header::AUTHORIZATION, "Bearer deployer-token")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("DELETE")
            .uri(format!("{prefix}{historical}"))
            .header(header::AUTHORIZATION, "Bearer deployer-token")
            .body(Body::from("x"))
            .unwrap(),
    ] {
        assert!(
            app.clone()
                .oneshot(request)
                .await
                .unwrap()
                .status()
                .is_client_error()
        );
    }
    for requested in ["bad", "deadbeef"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("{prefix}{requested}"))
                    .header(header::AUTHORIZATION, "Bearer deployer-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
    assert_eq!(
        app.clone().oneshot(send(active)).await.unwrap().status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(send(historical))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(send(historical)).await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        repository
            .list_versions(account, worker.id)
            .unwrap()
            .into_iter()
            .find(|version| version.id == historical)
            .unwrap()
            .state,
        VersionState::Tombstoned
    );
    assert_eq!(
        resolve_version(repository, account, worker.id, "latest")
            .unwrap()
            .id,
        active
    );
    let historical_text = historical.to_string();
    let active_text = active.to_string();
    assert_eq!(
        resolve_version(repository, account, worker.id, &historical_text[..8]).unwrap_err(),
        V4Error::Conflict
    );
    let unique_prefix_len = (historical_text
        .bytes()
        .zip(active_text.bytes())
        .position(|(historical, active)| historical != active)
        .unwrap()
        + 1)
    .max(8);
    assert!(unique_prefix_len < historical_text.len());
    assert_eq!(
        resolve_version(
            repository,
            account,
            worker.id,
            &historical_text[..unique_prefix_len],
        )
        .unwrap()
        .id,
        historical
    );
}
#[tokio::test]
async fn beta_version_reads_use_current_shapes_and_verified_immutable_modules() {
    use base64::Engine as _;
    use open_compute_workers::{
        CanonicalBundle, CreateVersionRequest, ModuleInput, ModuleType, RuntimeValidator,
        ValidationCandidate, VersionBundle, VersionContent, VersionController,
        VersionRuntimeFeatures,
    };
    use sha2::Digest as _;
    use std::sync::Arc;
    let (_temp, _mock, state, account, storage) =
        crate::tests::initialized_worker_http_fixture().await;
    let api = state.worker_api().unwrap();
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(account, "current-versions", RequestId::generate(), 1, 100)
        .unwrap()
        .0;
    let bundle = CanonicalBundle::build(
        "index.js",
        vec![ModuleInput {
            name: "index.js".to_owned(),
            module_type: ModuleType::EsModule,
            bytes: b"export default {}".to_vec(),
        }],
        api.bundle_limits,
    )
    .unwrap();
    let validator: Arc<dyn RuntimeValidator> = Arc::new(|_: ValidationCandidate| async { Ok(()) });
    let mut request = CreateVersionRequest {
        instance_id: account,
        worker_id: worker.id,
        idempotency_key: "read-version".to_owned(),
        content: VersionContent::Worker {
            bundle: VersionBundle::Bytes(bundle.into_bytes()),
            assets: None,
        },
        vars: BTreeMap::new(),
        secrets: BTreeMap::from([("TOKEN".to_owned(), SecretString::new("read-private-secret"))]),
        bindings: BTreeMap::new(),
        services: BTreeMap::new(),
        runtime_features: VersionRuntimeFeatures {
            compatibility_date: "2026-09-08".to_owned(),
            ..Default::default()
        },
        queue_consumers: Vec::new(),
        crons: Vec::new(),
        deployment_source: None,
        observability: None,
        request_id: RequestId::generate(),
        now_ms: 2,
    };
    let outcome = VersionController::new(
        &storage,
        api.artifacts.clone(),
        validator.clone(),
        api.bundle_limits,
    )
    .create_version(request.clone())
    .await
    .unwrap();
    let CreateVersionOutcome::Applied(outcome) = outcome else {
        panic!("unexpected replay")
    };
    let id = outcome.version.id;
    let assets_worker = repository
        .create_worker(account, "assets-version", RequestId::generate(), 3, 100)
        .unwrap()
        .0;
    let bytes = b"asset".to_vec();
    let digest = hex::encode(sha2::Sha256::digest(&bytes));
    api.artifacts
        .put_verified(
            futures::stream::once(async move { Ok::<bytes::Bytes, std::io::Error>(bytes.into()) }),
            &digest,
            5,
        )
        .await
        .unwrap();
    request.worker_id = assets_worker.id;
    request.idempotency_key = "read-assets-version".to_owned();
    request.secrets.clear();
    request.content = VersionContent::AssetsOnly {
        assets: open_compute_workers::VersionAssets {
            manifest: open_compute_workers::AssetManifestV1 {
                schema_version: 1,
                entries: vec![open_compute_workers::AssetEntryV1 {
                    path: "/asset.txt".to_owned(),
                    sha256: digest,
                    size: 5,
                    content_type: "text/plain".to_owned(),
                }],
            },
            routing: open_compute_workers::AssetRoutingConfigV1 {
                schema_version: 1,
                binding: None,
                run_worker_first: open_compute_workers::RunWorkerFirst::All(false),
                html_handling: Default::default(),
                not_found_handling: open_compute_workers::NotFoundHandling::None,
                headers: Vec::new(),
                redirects: Vec::new(),
            },
        },
    };
    let CreateVersionOutcome::Applied(assets) = VersionController::new(
        &storage,
        api.artifacts.clone(),
        validator,
        api.bundle_limits,
    )
    .create_version(request)
    .await
    .unwrap() else {
        panic!("unexpected assets replay")
    };
    let assets_snapshot = repository
        .version_snapshot(account, assets_worker.id, assets.version.id, false)
        .unwrap();
    assert!(
        beta_version_item(
            api,
            &crate::cloudflare_v4::accounts::V4InstanceContext::new(account, 1),
            &assets_snapshot
        )
        .unwrap()
        .get("main_module")
        .is_none()
    );
    assert_eq!(
        beta_version_modules(api, &assets_snapshot.version)
            .await
            .unwrap(),
        serde_json::json!([])
    );
    let mut corrupt = outcome.version.clone();
    corrupt.artifact_size = None;
    assert_eq!(
        beta_version_modules(api, &corrupt).await.unwrap_err(),
        V4Error::IntegrityFailure
    );

    let deleted = ready_version(repository, account, worker.id, 4);
    repository
        .tombstone_version(account, worker.id, deleted, RequestId::generate(), 6)
        .unwrap();
    let authority = crate::cloudflare_v4::accounts::V4InstanceContext::new(account, 1);
    let foreign = crate::cloudflare_v4::accounts::V4InstanceContext::new(
        open_compute_core::InstanceId::generate(),
        1,
    );
    let path = format!(
        "/client/v4/accounts/{}/workers/workers/{}/versions",
        authority.public_id(),
        authority.public_worker_tag(worker.id)
    );
    let foreign_path = path.replace(authority.public_id(), foreign.public_id());
    let app = crate::http::admin_router(
        state
            .with_v4_tokens(
                SecretString::new("deployer-token"),
                SecretString::new("read-token"),
            )
            .with_v4_instance_context(authority),
    );
    for (uri, status) in [
        (path.clone(), StatusCode::OK),
        (format!("{path}?page=2&per_page=1"), StatusCode::OK),
        (format!("{path}?page=0"), StatusCode::BAD_REQUEST),
        (format!("{path}?deployable=false"), StatusCode::BAD_REQUEST),
        (format!("{path}/{id}"), StatusCode::OK),
        (format!("{path}/latest?include=modules"), StatusCode::OK),
        (
            format!("{path}/{id}?include=invalid"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("{path}/{id}?include=modules&include=modules"),
            StatusCode::BAD_REQUEST,
        ),
        (format!("{path}/{deleted}"), StatusCode::NOT_FOUND),
        (format!("{path}/invalid"), StatusCode::BAD_REQUEST),
        (foreign_path, StatusCode::NOT_FOUND),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .header(header::AUTHORIZATION, "Bearer read-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{uri}");
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("read-private-secret"));
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        if uri == path {
            assert_eq!(body["result"].as_array().unwrap().len(), 1);
            assert_eq!(body["result"][0]["id"], id.to_string());
            assert_eq!(body["result_info"]["total_count"], 1);
        }
        if uri.ends_with("latest?include=modules") {
            assert_eq!(body["result"]["id"], id.to_string());
            assert_eq!(body["result"]["source"], "open-compute");
            assert_eq!(body["result"]["main_module"], "index.js");
            assert_eq!(
                body["result"]["bindings"][0],
                serde_json::json!({"name":"TOKEN","type":"secret_text"})
            );
            assert_eq!(
                body["result"]["modules"][0]["content_base64"],
                base64::engine::general_purpose::STANDARD.encode(b"export default {}")
            );
            assert_eq!(
                body["result"]["modules"][0]["content_type"],
                "application/javascript+module"
            );
        }
    }
    assert_eq!(
        app.oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
