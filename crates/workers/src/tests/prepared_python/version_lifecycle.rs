//! Source and secret revision fencing across immutable deployment rollback and restart.

use super::*;
use open_compute_storage::worker_repository::DeploymentSource;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn source_and_secret_rotation_restore_the_original_prepared_identity_after_rollback() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp/prepared-python-run");
    fs::create_dir_all(&base).unwrap();
    let evidence = recovery::Evidence(Some(
        tempfile::Builder::new()
            .prefix("versions-")
            .tempdir_in(fs::canonicalize(base).unwrap())
            .unwrap(),
    ));
    let directory = evidence.0.as_ref().unwrap();
    let config = storage_config(&directory.path().join("data"));
    let storage = Arc::new(PlatformStorage::bootstrap(&config, &SystemClock).unwrap());
    let instance = storage.identity().instance_id;
    let worker = WorkerRepository::new(storage.db())
        .create_worker(
            instance,
            "prepared-version-lifecycle",
            RequestId::generate(),
            1,
            100,
        )
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
    let preparations = Arc::new(AtomicUsize::new(0));
    let validator = {
        let source = source.clone();
        let storage = storage.clone();
        let artifacts = artifacts.clone();
        let preparations = preparations.clone();
        move |candidate: ValidationCandidate| {
            let source = source.clone();
            let storage = storage.clone();
            let artifacts = artifacts.clone();
            let preparations = preparations.clone();
            async move {
                let key = loader_key(
                    candidate.instance_id,
                    candidate.worker_id,
                    candidate.version_id,
                );
                let digest = hex::encode(candidate.worker_code_sha256);
                let input = source
                    .resolve(&key, &digest, RuntimeScope::Preparation)
                    .await?;
                assert!(input.python_prepared_sha256.is_none());
                let identity = PreparedPythonIdentity::from_snapshot(python_runtime_pin(), &input)?;
                // This authority regression supplies opaque fixture bytes; it does not emulate workerd.
                let snapshot = SecretBytes::new(
                    format!("prepared:{digest}:{}", input.secrets["API_TOKEN"].expose())
                        .into_bytes(),
                );
                publish_prepared_python(&storage, &artifacts, &identity, &snapshot, 20_000).await?;
                preparations.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    };
    let controller = VersionController::new(
        &storage,
        artifacts.clone(),
        Arc::new(validator),
        BundleLimits::default(),
    );
    let inputs = [
        ("message = 1\n", "original-private-token"),
        ("message = 2\n", "original-private-token"),
        ("message = 2\n", "rotated-private-token"),
    ];
    let mut versions = Vec::new();
    let mut identities = Vec::new();
    let mut records = Vec::new();
    for (index, (code, secret)) in inputs.iter().enumerate() {
        let mut request = version_request(instance, worker, &format!("revision-{index}"), secret);
        request.deployment_source = None;
        request.content = VersionContent::Worker {
            bundle: CanonicalBundle::build(
                "main.py",
                vec![ModuleInput {
                    name: "main.py".to_owned(),
                    module_type: ModuleType::Python,
                    bytes: code.as_bytes().to_vec(),
                }],
                BundleLimits::default(),
            )
            .unwrap()
            .into_bytes()
            .into(),
            assets: None,
        };
        let CreateVersionOutcome::Applied(result) =
            controller.create_version(request.clone()).await.unwrap()
        else {
            panic!("first upload must create a version")
        };
        assert_eq!(result.version.state, VersionState::Ready);
        assert!(result.deployment.is_none());
        assert!(matches!(
            controller.create_version(request).await.unwrap(),
            CreateVersionOutcome::Replay(_)
        ));
        assert_eq!(preparations.load(Ordering::SeqCst), index + 1);
        let snapshot = WorkerRepository::new(storage.db())
            .version_snapshot(instance, worker, result.version.id, false)
            .unwrap();
        let record = snapshot.python_prepared.unwrap();
        let identity: PreparedPythonIdentity =
            serde_json::from_slice(&record.identity_json).unwrap();
        identities.push(identity);
        records.push(record);
        versions.push(result.version);
    }
    assert_ne!(
        identities[0].runtime.module_inventory_sha256,
        identities[1].runtime.module_inventory_sha256
    );
    assert_eq!(
        identities[1].runtime.module_inventory_sha256,
        identities[2].runtime.module_inventory_sha256
    );
    assert_ne!(
        versions[0].worker_code_sha256,
        versions[1].worker_code_sha256
    );
    assert_ne!(
        versions[1].worker_code_sha256,
        versions[2].worker_code_sha256
    );
    for pair in identities.windows(2) {
        assert_ne!(pair[0].sha256().unwrap(), pair[1].sha256().unwrap());
    }
    let repository = WorkerRepository::new(storage.db());
    let previous_secret = repository
        .version_snapshot(instance, worker, versions[1].id, false)
        .unwrap()
        .secrets
        .remove("API_TOKEN")
        .unwrap();
    let rotated_secret = repository
        .version_snapshot(instance, worker, versions[2].id, false)
        .unwrap()
        .secrets
        .remove("API_TOKEN")
        .unwrap();
    assert_ne!(previous_secret.revision_id, rotated_secret.revision_id);
    assert_ne!(previous_secret.envelope, rotated_secret.envelope);
    assert_eq!(
        mock.object_count(),
        5,
        "two source artifacts and three encrypted snapshots"
    );
    for (digest, size) in repository.referenced_artifacts().unwrap() {
        let reference = ArtifactRef::new(1, &hex::encode(digest), size).unwrap();
        let bytes = artifacts.open(&reference).await.unwrap();
        for (_, secret) in inputs {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes())
            );
        }
    }
    let mut active = repository.get_worker(instance, worker).unwrap();
    let mut deployments = Vec::new();
    for (step, index) in [0, 1, 2, 0].into_iter().enumerate() {
        let (next, deployment) = repository
            .create_deployment_checked(
                instance,
                worker,
                versions[index].id,
                active.active_version_id,
                Some(active.route_generation),
                if step == 3 {
                    DeploymentSource::Rollback
                } else {
                    DeploymentSource::VersionsApi
                },
                &BTreeMap::new(),
                None,
                RequestId::generate(),
                30_000 + step as i64,
                StartupId::generate(),
            )
            .unwrap();
        assert_eq!(next.active_version_id, Some(versions[index].id));
        assert_eq!(next.route_generation, active.route_generation + 1);
        assert_eq!(next.active_deployment_id, Some(deployment.id));
        assert_eq!(deployment.version_id, versions[index].id);
        let key = loader_key(instance, worker, versions[index].id);
        let digest = hex::encode(versions[index].worker_code_sha256);
        let prepared = hex::encode(records[index].prepared_identity_sha256);
        let resolved = source
            .resolve(&key, &digest, RuntimeScope::Runtime)
            .await
            .unwrap();
        assert_eq!(resolved.secrets["API_TOKEN"].expose(), inputs[index].1);
        assert_eq!(resolved.route_generation, next.route_generation);
        assert_eq!(
            resolved.python_prepared_sha256.as_deref(),
            Some(prepared.as_str())
        );
        let restored = source
            .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
            .await
            .unwrap();
        assert_eq!(
            restored.expose(),
            format!("prepared:{digest}:{}", inputs[index].1).as_bytes()
        );
        let other = hex::encode(records[(index + 1) % records.len()].prepared_identity_sha256);
        assert_eq!(
            source
                .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &other)
                .await
                .unwrap_err()
                .code(),
            ErrorCode::VersionInvariantViolation
        );
        assert_eq!(repository.get_worker(instance, worker).unwrap(), next);
        deployments.push(deployment);
        active = next;
    }
    assert_eq!(deployments[3].source, DeploymentSource::Rollback);
    assert_ne!(deployments[0].id, deployments[3].id);
    assert_eq!(preparations.load(Ordering::SeqCst), 3);
    drop(controller);
    drop(source);
    drop(storage);
    let storage = Arc::new(PlatformStorage::bootstrap(&config, &SystemClock).unwrap());
    assert_eq!(
        WorkerRepository::new(storage.db())
            .get_worker(instance, worker)
            .unwrap(),
        active
    );
    let source = RuntimeSource::new(
        storage.clone(),
        artifacts.clone(),
        BundleLimits::default(),
        python_runtime_pin(),
    )
    .unwrap();
    for (index, version) in versions.iter().enumerate() {
        let key = loader_key(instance, worker, version.id);
        let digest = hex::encode(version.worker_code_sha256);
        let prepared = hex::encode(records[index].prepared_identity_sha256);
        let resolved = source
            .resolve(&key, &digest, RuntimeScope::Runtime)
            .await
            .unwrap();
        assert_eq!(resolved.secrets["API_TOKEN"].expose(), inputs[index].1);
        assert_eq!(
            source
                .resolve_python_prepared(&key, &digest, RuntimeScope::Runtime, &prepared)
                .await
                .unwrap()
                .expose(),
            format!("prepared:{digest}:{}", inputs[index].1).as_bytes()
        );
        assert_eq!(
            WorkerRepository::new(storage.db())
                .version_snapshot(instance, worker, version.id, false)
                .unwrap()
                .python_prepared
                .as_ref(),
            Some(&records[index])
        );
    }
    assert_eq!(preparations.load(Ordering::SeqCst), 3);
    assert_eq!(mock.object_count(), 5);
}
