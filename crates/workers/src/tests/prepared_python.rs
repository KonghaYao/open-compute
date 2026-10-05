use super::*;
use crate::python_artifact::{
    PreparedPythonIdentity, PythonRuntimeIdentity, restore_prepared_python,
};
use open_compute_core::{PlatformError, SecretBytes};
use open_compute_storage::worker_repository::{NewVersion, NewVersionProducts, VersionContentKind};

async fn publish_prepared_python(
    storage: &PlatformStorage,
    artifacts: &ArtifactStore,
    identity: &PreparedPythonIdentity,
    snapshot: &SecretBytes,
    now_ms: i64,
) -> Result<open_compute_storage::worker_repository::PythonPreparedArtifactRecord, PlatformError> {
    python_artifact::publish_prepared_python(
        storage,
        artifacts,
        identity,
        snapshot,
        now_ms,
        |publish| publish(),
    )
    .await
}

fn create_candidate(
    storage: &PlatformStorage,
    worker: WorkerId,
    main: &str,
) -> PreparedPythonIdentity {
    let version = VersionId::generate();
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    repo.insert_staging_version(
        &NewVersion {
            id: version,
            instance_id: instance,
            worker_id: worker,
            content_kind: VersionContentKind::Worker,
            artifact_sha256: Some([1; 32]),
            artifact_size: Some(100),
            artifact_schema_version: Some(1),
            main_module: Some(main.to_owned()),
            worker_code_sha256: [2; 32],
            compatibility_date: "2026-09-08".to_owned(),
            compatibility_flags: Vec::new(),
            resource_limits:
                open_compute_storage::worker_repository::EffectiveResourceLimits::standard_defaults(
                ),
            vars: BTreeMap::new(),
            secrets: BTreeMap::new(),
            request_id: RequestId::generate(),
            now_ms: 1,
        },
        &NewVersionProducts::default(),
        100,
    )
    .unwrap();
    repo.begin_validation(version).unwrap();
    PreparedPythonIdentity {
        schema_version: 1,
        instance_id: instance,
        worker_id: worker,
        version_id: version,
        worker_code_sha256: "02".repeat(32),
        runtime: PythonRuntimeIdentity {
            pin: python_artifact::PythonRuntimePin {
                workerd_revision: "a".repeat(40),
                workerd_binary_sha256: "b".repeat(64),
                process_flags: vec!["--experimental".to_owned()],
                pyodide_bundle_sha256: "c".repeat(64),
                runtime_assets_sha256: "d".repeat(64),
            },
            module_inventory_sha256: "e".repeat(64),
        },
    }
}

#[tokio::test]
async fn prepared_python_publishes_ciphertext_and_reuses_exact_authority_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let config = storage_config(&directory.path().join("data"));
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    let worker = repo
        .create_worker(instance, "python-publish", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let identity = create_candidate(&storage, worker, "main.py");
    let snapshot = SecretBytes::new(b"secret in native snapshot bytes".to_vec());
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let published = publish_prepared_python(&storage, &artifacts, &identity, &snapshot, 2)
        .await
        .unwrap();
    let reference = ArtifactRef::new(
        1,
        &hex::encode(published.artifact_sha256),
        published.artifact_size,
    )
    .unwrap();
    let ciphertext = artifacts.open(&reference).await.unwrap();
    assert!(
        !ciphertext
            .windows(snapshot.expose().len())
            .any(|part| part == snapshot.expose())
    );
    assert_eq!(
        repo.version_snapshot(instance, worker, identity.version_id, true)
            .unwrap()
            .python_prepared,
        Some(published.clone())
    );
    assert_eq!(
        repo.referenced_artifacts().unwrap(),
        vec![
            ([1; 32], 100),
            (published.artifact_sha256, published.artifact_size)
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
    repo.mark_ready(identity.version_id, 3).unwrap();
    drop(storage);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let reused = publish_prepared_python(
        &storage,
        &artifacts,
        &identity,
        &SecretBytes::new(b"different newly prepared bytes".to_vec()),
        4,
    )
    .await
    .unwrap();
    assert_eq!(reused, published);
    assert_eq!(mock.object_count(), 1);
    assert_eq!(
        restore_prepared_python(&reused, &identity, &artifacts, storage.crypto())
            .await
            .unwrap()
            .expose(),
        snapshot.expose()
    );
    let mut changed_runtime = identity.clone();
    changed_runtime.runtime.pin.workerd_revision = "f".repeat(40);
    assert!(
        publish_prepared_python(&storage, &artifacts, &changed_runtime, &snapshot, 5)
            .await
            .is_err()
    );
    assert_eq!(mock.object_count(), 1);
    mock.corrupt_body(&reference.physical_key("system/"));
    assert!(
        restore_prepared_python(&reused, &identity, &artifacts, storage.crypto())
            .await
            .is_err()
    );
    assert!(
        publish_prepared_python(&storage, &artifacts, &identity, &snapshot, 6)
            .await
            .is_err()
    );
    assert_eq!(mock.object_count(), 1);
    assert_eq!(
        WorkerRepository::new(storage.db())
            .get_version(instance, worker, identity.version_id)
            .unwrap()
            .state,
        VersionState::Ready
    );
}

#[tokio::test]
async fn prepared_python_refuses_wrong_source_metadata_scope_and_invalid_results() {
    let directory = tempfile::tempdir().unwrap();
    let storage = PlatformStorage::bootstrap(
        &storage_config(&directory.path().join("data")),
        &SystemClock,
    )
    .unwrap();
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    let worker = repo
        .create_worker(instance, "python-invalid", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let identity = create_candidate(&storage, worker, "main.py");
    let js = create_candidate(&storage, worker, "main.js");
    let snapshot = SecretBytes::new(b"native snapshot bytes".to_vec());
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    assert!(
        publish_prepared_python(&storage, &artifacts, &js, &snapshot, 2)
            .await
            .is_err()
    );
    let mut wrong = identity.clone();
    wrong.worker_code_sha256 = "00".repeat(32);
    assert!(
        publish_prepared_python(&storage, &artifacts, &wrong, &snapshot, 2)
            .await
            .is_err()
    );
    wrong = identity.clone();
    wrong.instance_id = InstanceId::generate();
    assert!(
        publish_prepared_python(&storage, &artifacts, &wrong, &snapshot, 2)
            .await
            .is_err()
    );
    assert!(
        publish_prepared_python(
            &storage,
            &artifacts,
            &identity,
            &SecretBytes::new(Vec::new()),
            2
        )
        .await
        .is_err()
    );
    assert_eq!(mock.object_count(), 0);
    assert!(
        repo.version_snapshot(instance, worker, identity.version_id, true)
            .unwrap()
            .python_prepared
            .is_none()
    );
    mock.set_fault(open_compute_artifacts::Fault::Permission);
    assert!(
        publish_prepared_python(&storage, &artifacts, &identity, &snapshot, 2)
            .await
            .is_err()
    );
    assert!(
        repo.version_snapshot(instance, worker, identity.version_id, true)
            .unwrap()
            .python_prepared
            .is_none()
    );
    assert_eq!(mock.object_count(), 0);
    mock.set_fault(open_compute_artifacts::Fault::None);
    let published = publish_prepared_python(&storage, &artifacts, &identity, &snapshot, 3)
        .await
        .unwrap();
    for invalid in [
        open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            version_id: VersionId::generate(),
            ..published.clone()
        },
        open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            prepared_identity_sha256: [0; 32],
            ..published.clone()
        },
        open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            identity_json: b"{}".to_vec(),
            ..published.clone()
        },
        open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            artifact_size: 0,
            ..published.clone()
        },
        open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            artifact_size: u64::MAX,
            ..published.clone()
        },
    ] {
        assert!(
            restore_prepared_python(&invalid, &identity, &artifacts, storage.crypto())
                .await
                .is_err()
        );
    }
    let key = SecretBytes::new(vec![7; 32]);
    let wrong_key = open_compute_storage::crypto::SecretCrypto::new(
        &key,
        &hex::encode(sha2::Sha256::digest(key.expose())),
    )
    .unwrap();
    assert!(
        restore_prepared_python(&published, &identity, &artifacts, &wrong_key)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn runtime_source_restores_only_current_prepared_identity_and_retains_it_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let config = storage_config(&directory.path().join("data"));
    let storage = Arc::new(PlatformStorage::bootstrap(&config, &SystemClock).unwrap());
    let instance = storage.identity().instance_id;
    let worker = WorkerRepository::new(storage.db())
        .create_worker(instance, "python-restore", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let source = RuntimeSource::new(
        storage.clone(),
        artifacts.clone(),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    let validator = {
        let source = source.clone();
        let storage = storage.clone();
        let artifacts = artifacts.clone();
        move |candidate: ValidationCandidate| {
            let source = source.clone();
            let storage = storage.clone();
            let artifacts = artifacts.clone();
            async move {
                let key = loader_key(
                    candidate.instance_id,
                    candidate.worker_id,
                    candidate.version_id,
                );
                let digest = hex::encode(candidate.worker_code_sha256);
                let initial = source
                    .resolve(&key, &digest, RuntimeScope::Preparation)
                    .await
                    .unwrap();
                assert!(initial.python_prepared_sha256.is_none());
                assert_eq!(initial.secrets["API_TOKEN"].expose(), "secret-in-snapshot");
                assert_eq!(
                    source
                        .resolve_python_prepared(
                            &key,
                            &digest,
                            RuntimeScope::Validation,
                            &"0".repeat(64)
                        )
                        .await
                        .unwrap_err()
                        .code(),
                    ErrorCode::VersionInvariantViolation
                );
                let identity =
                    PreparedPythonIdentity::from_snapshot(python_runtime_pin(), &initial).unwrap();
                let expected = identity.sha256().unwrap();
                // The uploaded SDK/package layout is data driven, including data and JS.
                for index in 0..initial.modules.len() {
                    for change in 0..3 {
                        let mut changed = initial.clone();
                        let module = &mut changed.modules[index];
                        match change {
                            0 => module.bytes.push(1),
                            1 => module.module_type = ModuleType::Text,
                            _ => module.name.push('z'),
                        }
                        if module.module_type == ModuleType::Python && change == 2 {
                            changed.main_module = Some(module.name.clone());
                        }
                        let next =
                            PreparedPythonIdentity::from_snapshot(python_runtime_pin(), &changed);
                        if let Ok(next) = next {
                            assert_ne!(next.sha256().unwrap(), expected);
                        } else {
                            assert_eq!(
                                index, 0,
                                "only changing the main may invalidate Python admission"
                            );
                        }
                    }
                }
                for change in 0..6 {
                    let mut invalid = initial.clone();
                    match change {
                        0 => invalid.modules.clear(),
                        1 => invalid.modules.push(invalid.modules[0].clone()),
                        2 => invalid.modules.reverse(),
                        3 => invalid.main_module = None,
                        4 => invalid.loader_key = "invalid".to_owned(),
                        _ => invalid.worker_code_sha256 = "invalid".to_owned(),
                    }
                    assert!(
                        PreparedPythonIdentity::from_snapshot(python_runtime_pin(), &invalid)
                            .is_err()
                    );
                }
                publish_prepared_python(
                    &storage,
                    &artifacts,
                    &identity,
                    &SecretBytes::new(b"native secret-in-snapshot".to_vec()),
                    2,
                )
                .await
                .unwrap();
                let prepared = hex::encode(identity.sha256().unwrap());
                for scope in [
                    RuntimeScope::Validation,
                    RuntimeScope::Probe,
                    RuntimeScope::Preparation,
                ] {
                    let snapshot = source.resolve(&key, &digest, scope).await.unwrap();
                    assert_eq!(
                        snapshot.python_prepared_sha256.as_deref(),
                        Some(prepared.as_str())
                    );
                    assert_eq!(
                        source
                            .resolve_python_prepared(&key, &digest, scope, &prepared)
                            .await
                            .unwrap()
                            .expose(),
                        b"native secret-in-snapshot"
                    );
                }
                assert_eq!(
                    source
                        .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
                        .await
                        .unwrap_err()
                        .code(),
                    ErrorCode::VersionNotReady
                );
                Ok(())
            }
        }
    };
    let mut request = version_request(instance, worker, "source-python", "secret-in-snapshot");
    request.deployment_source = None;
    request.content = VersionContent::Worker {
        bundle: CanonicalBundle::build(
            "main.py",
            vec![
                ModuleInput {
                    name: "main.py".to_owned(),
                    module_type: ModuleType::Python,
                    bytes: b"pass\n".to_vec(),
                },
                ModuleInput {
                    name: "python_modules/example/data.json".to_owned(),
                    module_type: ModuleType::Data,
                    bytes: br#"{"value": 1}"#.to_vec(),
                },
                ModuleInput {
                    name: "python_modules/workers/workflows.js".to_owned(),
                    module_type: ModuleType::EsModule,
                    bytes: b"export const value = 1;".to_vec(),
                },
            ],
            BundleLimits::default(),
        )
        .unwrap()
        .into_bytes()
        .into(),
        assets: None,
    };
    let controller = VersionController::new(
        &storage,
        artifacts.clone(),
        Arc::new(validator),
        BundleLimits::default(),
    );
    let result = controller.create_version(request).await.unwrap();
    let CreateVersionOutcome::Applied(result) = result else {
        panic!("unexpected replay")
    };
    let key = loader_key(instance, worker, result.version.id);
    let digest = hex::encode(result.version.worker_code_sha256);
    let snapshot = source
        .resolve(&key, &digest, RuntimeScope::Runtime)
        .await
        .unwrap();
    let prepared = snapshot.python_prepared_sha256.unwrap();
    let payload = RuntimeSource::internal_payload(
        &source
            .resolve(&key, &digest, RuntimeScope::Runtime)
            .await
            .unwrap(),
    )
    .unwrap();
    let wire: serde_json::Value = serde_json::from_slice(payload.expose()).unwrap();
    assert_eq!(wire["pythonPreparedSha256"], prepared);
    assert!(wire.get("pythonSnapshot").is_none());
    assert!(
        !payload
            .expose()
            .windows(b"native secret-in-snapshot".len())
            .any(|part| part == b"native secret-in-snapshot")
    );
    assert_eq!(
        source
            .resolve_python_prepared(&key, &digest, RuntimeScope::Preparation, &prepared)
            .await
            .unwrap_err()
            .code(),
        ErrorCode::VersionNotReady
    );
    for (code, prepared) in [
        ("0".repeat(64), prepared.clone()),
        (digest.clone(), "0".repeat(64)),
        ("X".repeat(64), prepared.clone()),
        (digest.clone(), "x".to_owned()),
    ] {
        assert_eq!(
            source
                .resolve_python_prepared(&key, &code, RuntimeScope::Runtime, &prepared)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
    }
    for index in 0..5 {
        let mut pin = python_runtime_pin();
        match index {
            0 => pin.workerd_revision = "f".repeat(40),
            1 => pin.workerd_binary_sha256 = "f".repeat(64),
            2 => pin.process_flags.push("--no-autogates".to_owned()),
            3 => pin.pyodide_bundle_sha256 = "f".repeat(64),
            _ => pin.runtime_assets_sha256 = "f".repeat(64),
        }
        let changed = RuntimeSource::new(
            storage.clone(),
            artifacts.clone(),
            BundleLimits::default(),
            pin,
        )
        .unwrap();
        assert_eq!(
            changed
                .resolve(&key, &digest, RuntimeScope::Runtime)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
        assert_eq!(
            changed
                .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
    }
    let mut invalid_pin = python_runtime_pin();
    invalid_pin.workerd_revision.clear();
    assert!(
        RuntimeSource::new(
            storage.clone(),
            artifacts.clone(),
            BundleLimits::default(),
            invalid_pin
        )
        .is_err()
    );
    drop(controller);
    drop(source);
    drop(storage);
    let storage = Arc::new(PlatformStorage::bootstrap(&config, &SystemClock).unwrap());
    let source = RuntimeSource::new(
        storage.clone(),
        artifacts.clone(),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    assert_eq!(
        source
            .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
            .await
            .unwrap()
            .expose(),
        b"native secret-in-snapshot"
    );
    assert_eq!(mock.object_count(), 2);
    let record = WorkerRepository::new(storage.db())
        .version_snapshot(instance, worker, result.version.id, false)
        .unwrap()
        .python_prepared
        .unwrap();
    let reference = ArtifactRef::new(
        1,
        &hex::encode(record.artifact_sha256),
        record.artifact_size,
    )
    .unwrap();
    mock.corrupt_body(&reference.physical_key("system/"));
    assert_eq!(
        source
            .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
            .await
            .unwrap_err()
            .code(),
        ErrorCode::ArtifactIntegrityError
    );
    assert_eq!(mock.object_count(), 2);
    assert_eq!(
        WorkerRepository::new(storage.db())
            .get_version(instance, worker, result.version.id)
            .unwrap()
            .state,
        VersionState::Ready
    );
}

#[tokio::test]
async fn runtime_source_refuses_noncanonical_or_unvalidated_prepared_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        PlatformStorage::bootstrap(
            &storage_config(&directory.path().join("data")),
            &SystemClock,
        )
        .unwrap(),
    );
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    let worker = repo
        .create_worker(instance, "python-metadata", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let mock = MockS3::spawn("open-compute").await;
    let source = RuntimeSource::new(
        storage.clone(),
        artifact_store(&mock),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    for index in 0..4 {
        let identity = create_candidate(&storage, worker, "main.py");
        let mut value = serde_json::to_value(&identity).unwrap();
        let metadata = match index {
            0 => {
                value["unknown"] = serde_json::json!(true);
                serde_json::to_vec(&value).unwrap()
            }
            1 => serde_json::to_vec_pretty(&identity).unwrap(),
            2 => {
                value["runtime"]["moduleInventorySha256"] = serde_json::json!("invalid");
                serde_json::to_vec(&value).unwrap()
            }
            _ => serde_json::to_vec(&identity).unwrap(),
        };
        let mut record = open_compute_storage::worker_repository::PythonPreparedArtifactRecord {
            version_id: identity.version_id,
            prepared_identity_sha256: identity.sha256().unwrap(),
            identity_json: metadata,
            artifact_sha256: [4; 32],
            artifact_size: 100,
            created_at_ms: 2,
        };
        if index == 3 {
            record.prepared_identity_sha256 = [0; 32];
        }
        repo.publish_python_prepared(instance, worker, &record)
            .unwrap();
        assert_eq!(
            source
                .resolve_python_prepared(
                    &loader_key(instance, worker, identity.version_id),
                    &identity.worker_code_sha256,
                    RuntimeScope::Validation,
                    &hex::encode(record.prepared_identity_sha256)
                )
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
    }
    assert!(
        mock.recorded().is_empty(),
        "invalid metadata reached object storage"
    );
}

#[tokio::test]
async fn prepared_inventory_must_match_the_verified_upload_before_metadata_or_binary_restore() {
    let directory = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        PlatformStorage::bootstrap(
            &storage_config(&directory.path().join("data")),
            &SystemClock,
        )
        .unwrap(),
    );
    let instance = storage.identity().instance_id;
    let worker = WorkerRepository::new(storage.db())
        .create_worker(instance, "python-inventory", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let source = RuntimeSource::new(
        storage.clone(),
        artifacts.clone(),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    let validator = {
        let source = source.clone();
        let storage = storage.clone();
        let artifacts = artifacts.clone();
        move |candidate: ValidationCandidate| {
            let source = source.clone();
            let storage = storage.clone();
            let artifacts = artifacts.clone();
            async move {
                let key = loader_key(
                    candidate.instance_id,
                    candidate.worker_id,
                    candidate.version_id,
                );
                let digest = hex::encode(candidate.worker_code_sha256);
                let snapshot = source
                    .resolve(&key, &digest, RuntimeScope::Preparation)
                    .await?;
                assert!(PythonRuntimeIdentity::from_modules(python_runtime_pin(), &[]).is_err());
                let mut identity =
                    PreparedPythonIdentity::from_snapshot(python_runtime_pin(), &snapshot)?;
                identity.runtime.module_inventory_sha256 = "0".repeat(64);
                // Deliberately inject a self-consistent encrypted record with wrong inputs.
                publish_prepared_python(
                    &storage,
                    &artifacts,
                    &identity,
                    &SecretBytes::new(b"unrelated native snapshot".to_vec()),
                    2,
                )
                .await?;
                let prepared = hex::encode(identity.sha256()?);
                assert_eq!(
                    source
                        .resolve(&key, &digest, RuntimeScope::Validation)
                        .await
                        .unwrap_err()
                        .code(),
                    ErrorCode::VersionInvariantViolation
                );
                assert_eq!(
                    source
                        .resolve_python_prepared(&key, &digest, RuntimeScope::Validation, &prepared)
                        .await
                        .unwrap_err()
                        .code(),
                    ErrorCode::VersionInvariantViolation
                );
                Err(PlatformError::new(
                    ErrorCode::BundleRuntimeInvalid,
                    "wrong prepared inventory",
                ))
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
    let mut request = version_request(instance, worker, "inventory-rejection", "private-value");
    request.deployment_source = None;
    request.content = VersionContent::Worker {
        bundle: bundle.into_bytes().into(),
        assets: None,
    };
    let result = VersionController::new(
        &storage,
        artifacts,
        Arc::new(validator),
        BundleLimits::default(),
    )
    .create_version(request)
    .await;
    assert_eq!(result.unwrap_err().code(), ErrorCode::BundleRuntimeInvalid);
    assert_eq!(
        WorkerRepository::new(storage.db())
            .get_worker(instance, worker)
            .unwrap()
            .active_version_id,
        None
    );
    assert_eq!(
        mock.object_count(),
        2,
        "rejected source and encrypted evidence remain immutable"
    );
}

#[tokio::test]
async fn prepared_publication_fences_the_sql_commit_after_upload_and_retained_restore() {
    let directory = tempfile::tempdir().unwrap();
    let storage = PlatformStorage::bootstrap(
        &storage_config(&directory.path().join("data")),
        &SystemClock,
    )
    .unwrap();
    let instance = storage.identity().instance_id;
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(
            instance,
            "fenced-publication",
            RequestId::generate(),
            1,
            100,
        )
        .unwrap()
        .0
        .id;
    let identity = create_candidate(&storage, worker, "main.py");
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let bytes = SecretBytes::new(b"native secret snapshot".to_vec());
    let error = python_artifact::publish_prepared_python(
        &storage,
        &artifacts,
        &identity,
        &bytes,
        2,
        |_| {
            assert_eq!(
                mock.object_count(),
                1,
                "the fence follows immutable upload verification"
            );
            Err(PlatformError::new(
                ErrorCode::RuntimeUnavailable,
                "generation rotated",
            ))
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::RuntimeUnavailable);
    assert!(
        repository
            .version_snapshot(instance, worker, identity.version_id, true)
            .unwrap()
            .python_prepared
            .is_none()
    );
    assert_eq!(
        repository
            .get_version(instance, worker, identity.version_id)
            .unwrap()
            .state,
        VersionState::Validating
    );
    let published = publish_prepared_python(&storage, &artifacts, &identity, &bytes, 3)
        .await
        .unwrap();
    assert_eq!(mock.object_count(), 2);
    let error = python_artifact::publish_prepared_python(
        &storage,
        &artifacts,
        &identity,
        &bytes,
        4,
        |_| {
            Err(PlatformError::new(
                ErrorCode::RuntimeUnavailable,
                "generation rotated",
            ))
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::RuntimeUnavailable);
    assert_eq!(
        mock.object_count(),
        2,
        "retained recovery does not upload or prepare again"
    );
    assert_eq!(
        repository
            .version_snapshot(instance, worker, identity.version_id, true)
            .unwrap()
            .python_prepared,
        Some(published)
    );
}

mod binding_lifecycle;
mod publication;
mod recovery;
mod version_lifecycle;
