use super::*;
use open_compute_core::{PlatformError, ResourceId};
use open_compute_storage::durable_objects::DurableObjectRepository;
use open_compute_storage::resources::{
    ReserveResourceCreate, ResourceCreateReservation, ResourceRepository,
};

#[tokio::test]
async fn preparation_reads_only_a_validating_python_candidates_exact_private_environment() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        PlatformStorage::bootstrap(
            &storage_config(&directory.path().join("data")),
            &SystemClock,
        )
        .unwrap(),
    );
    let instance = storage.identity().instance_id;
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(instance, "prepare-env", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let resources = ResourceRepository::new(storage.db());
    let namespace_id = ResourceId::generate();
    let ResourceCreateReservation::Reserved(namespace) = resources
        .reserve_create(
            &ReserveResourceCreate {
                instance_id: instance,
                kind: BindingKind::DoNamespace,
                name: "prepare-counter",
                idempotency_key: "prepare-counter",
                fingerprint_key_id: "test-key",
                request_fingerprint: &[8; 32],
                resource_id: namespace_id,
                driver_schema_version:
                    open_compute_storage::durable_objects::DO_NAMESPACE_SCHEMA_VERSION,
                request_id: RequestId::generate(),
                now_ms: 2,
                expires_at_ms: 10_000,
            },
            100,
        )
        .unwrap()
    else {
        panic!("namespace reservation");
    };
    DurableObjectRepository::new(&storage)
        .ensure_namespace(&namespace, worker, "Counter")
        .unwrap();
    resources.mark_ready(namespace_id, 3).unwrap();
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let source = RuntimeSource::new(
        storage.clone(),
        artifacts.clone(),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    let observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = observed.clone();
    let validator_source = source.clone();
    let validator = move |candidate: ValidationCandidate| {
        let source = validator_source.clone();
        let observed = captured.clone();
        async move {
            let key = loader_key(
                candidate.instance_id,
                candidate.worker_id,
                candidate.version_id,
            );
            let digest = hex::encode(candidate.worker_code_sha256);
            let validation = source
                .resolve(&key, &digest, RuntimeScope::Validation)
                .await
                .unwrap();
            assert!(validation.secrets.is_empty());
            assert!(validation.bindings[0].durable_object_identity.is_none());
            assert!(
                source
                    .resolve(&key, &digest, RuntimeScope::Probe)
                    .await
                    .unwrap()
                    .secrets
                    .is_empty()
            );
            assert_eq!(
                source
                    .resolve(&key, &digest, RuntimeScope::Runtime)
                    .await
                    .unwrap_err()
                    .code(),
                ErrorCode::VersionNotReady
            );
            let prepared = source
                .resolve(&key, &digest, RuntimeScope::Preparation)
                .await;
            if validation.main_module.as_deref() == Some("main.py") {
                let prepared = prepared.unwrap();
                assert_eq!(
                    prepared.secrets["API_TOKEN"].expose(),
                    "private-prepare-value"
                );
                assert_eq!(prepared.vars["MODE"], serde_json::json!("production"));
                assert_eq!(prepared.modules[0].module_type, ModuleType::Python);
                assert_eq!(prepared.loader_key, key);
                assert_eq!(prepared.worker_code_sha256, digest);
                assert!(!format!("{prepared:?}").contains("private-prepare-value"));
                let payload = RuntimeSource::internal_payload(&prepared).unwrap();
                let wire: serde_json::Value = serde_json::from_slice(payload.expose()).unwrap();
                assert_eq!(wire["env"]["API_TOKEN"], "private-prepare-value");
                assert!(prepared.observability.is_none());
                let identity = prepared.bindings[0]
                    .durable_object_identity
                    .as_ref()
                    .unwrap();
                assert_eq!(identity.namespace_prefix.len(), 16);
                assert!(!identity.namespace_name_key.expose().is_empty());
                assert_eq!(
                    validation.bindings[0].descriptor,
                    prepared.bindings[0].descriptor
                );
                assert_eq!(
                    source
                        .resolve(&key, &"0".repeat(64), RuntimeScope::Preparation)
                        .await
                        .unwrap_err()
                        .code(),
                    ErrorCode::VersionInvariantViolation
                );
            } else {
                assert_eq!(
                    prepared.unwrap_err().code(),
                    ErrorCode::VersionInvariantViolation
                );
            }
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(PlatformError::new(
                ErrorCode::BundleRuntimeInvalid,
                "preparation fixture stops admission",
            ))
        }
    };
    let controller = VersionController::new(
        &storage,
        artifacts,
        Arc::new(validator),
        BundleLimits::default(),
    );
    for (key, python) in [("python", true), ("javascript", false)] {
        let mut request = version_request(instance, worker, key, "private-prepare-value");
        request.deployment_source = None;
        request.bindings.insert(
            "COUNTER".to_owned(),
            VersionBindingInput {
                kind: BindingKind::DoNamespace,
                id: namespace_id,
                permissions: open_compute_core::CanonicalPermissions::default(),
                config: open_compute_core::CanonicalBindingConfig::default(),
            },
        );
        if python {
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
            request.content = VersionContent::Worker {
                bundle: bundle.into_bytes().into(),
                assets: None,
            };
        }
        assert_eq!(
            controller.create_version(request).await.unwrap_err().code(),
            ErrorCode::BundleRuntimeInvalid
        );
    }
    assert_eq!(observed.load(std::sync::atomic::Ordering::SeqCst), 2);
    let mut ready_request = version_request(instance, worker, "ready-js", "private-prepare-value");
    ready_request.deployment_source = None;
    VersionController::new(
        &storage,
        artifact_store(&mock),
        Arc::new(AcceptAllValidator),
        BundleLimits::default(),
    )
    .create_version(ready_request)
    .await
    .unwrap();
    let versions = repository.list_versions(instance, worker).unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(
        versions
            .iter()
            .filter(|version| version.state == VersionState::Rejected)
            .count(),
        2
    );
    for version in versions {
        assert_eq!(
            source
                .resolve(
                    &loader_key(instance, worker, version.id),
                    &hex::encode(version.worker_code_sha256),
                    RuntimeScope::Preparation
                )
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionNotReady
        );
    }
    assert_eq!(
        repository
            .get_worker(instance, worker)
            .unwrap()
            .active_version_id,
        None
    );
}
