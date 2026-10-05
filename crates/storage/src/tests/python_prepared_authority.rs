use super::*;
use crate::worker_repository::{
    NewVersionProducts, PythonPreparedArtifactRecord, VersionContentKind,
};
use open_compute_core::RequestId;

fn staging_version(
    repo: WorkerRepository<'_>,
    instance: InstanceId,
    worker: WorkerId,
    main: &str,
) -> VersionId {
    let version = VersionId::generate();
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
            compatibility_flags: if main.ends_with(".py") {
                vec!["python_workers".to_owned()]
            } else {
                Vec::new()
            },
            resource_limits: EffectiveResourceLimits::standard_defaults(),
            vars: BTreeMap::new(),
            secrets: BTreeMap::new(),
            request_id: RequestId::generate(),
            now_ms: 1,
        },
        &NewVersionProducts::default(),
        100,
    )
    .unwrap();
    version
}

fn record(
    instance: InstanceId,
    worker: WorkerId,
    version: VersionId,
    digest: u8,
) -> PythonPreparedArtifactRecord {
    PythonPreparedArtifactRecord {
        version_id: version,
        prepared_identity_sha256: [digest; 32],
        identity_json: serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "instanceId": instance,
            "workerId": worker,
            "versionId": version,
            "workerCodeSha256": "02".repeat(32),
            "runtime": {
                "pin": {
                    "workerdRevision": "a".repeat(40),
                    "workerdBinarySha256": "b".repeat(64),
                    "processFlags": ["--experimental"],
                    "pyodideBundleSha256": "c".repeat(64),
                    "runtimeAssetsSha256": "d".repeat(64)
                },
                "moduleInventorySha256": "e".repeat(64)
            }
        }))
        .unwrap(),
        artifact_sha256: [digest; 32],
        artifact_size: 2_048,
        created_at_ms: 2,
    }
}

#[test]
fn python_prepared_publication_is_scoped_immutable_and_survives_restart() {
    let (_directory, root) = unique_root();
    let config = storage_config(&root);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    let worker = repo
        .create_worker(instance, "python-worker", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let other = repo
        .create_worker(instance, "other-worker", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let version = staging_version(repo, instance, worker, "main.py");
    let artifact = record(instance, worker, version, 7);
    assert!(
        repo.publish_python_prepared(instance, worker, &artifact)
            .is_err()
    );
    repo.begin_validation(version).unwrap();
    assert!(
        repo.version_snapshot(instance, worker, version, true)
            .unwrap()
            .python_prepared
            .is_none()
    );
    assert!(repo.mark_ready(version, 2).is_err());
    assert_eq!(
        repo.get_version(instance, worker, version).unwrap().state,
        VersionState::Validating
    );
    assert!(
        repo.publish_python_prepared(instance, other, &artifact)
            .is_err()
    );
    assert!(
        repo.publish_python_prepared(InstanceId::generate(), worker, &artifact)
            .is_err()
    );
    for (field, value) in [
        ("schemaVersion", serde_json::json!(0)),
        ("instanceId", serde_json::json!(InstanceId::generate())),
        ("workerId", serde_json::json!(other)),
        ("versionId", serde_json::json!(VersionId::generate())),
        ("workerCodeSha256", serde_json::json!("00".repeat(32))),
    ] {
        let mut invalid = artifact.clone();
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&invalid.identity_json).unwrap();
        metadata[field] = value;
        invalid.identity_json = serde_json::to_vec(&metadata).unwrap();
        assert!(
            repo.publish_python_prepared(instance, worker, &invalid)
                .is_err()
        );
    }
    for json in [
        Vec::new(),
        b"not json".to_vec(),
        b"{}".to_vec(),
        vec![b' '; 65_537],
    ] {
        let invalid = PythonPreparedArtifactRecord {
            identity_json: json,
            ..artifact.clone()
        };
        assert!(
            repo.publish_python_prepared(instance, worker, &invalid)
                .is_err()
        );
    }
    for size in [0, 134_283_293, u64::MAX] {
        let invalid = PythonPreparedArtifactRecord {
            artifact_size: size,
            ..artifact.clone()
        };
        assert!(
            repo.publish_python_prepared(instance, worker, &invalid)
                .is_err()
        );
    }
    let js = staging_version(repo, instance, worker, "main.js");
    repo.begin_validation(js).unwrap();
    assert!(
        repo.publish_python_prepared(instance, worker, &record(instance, worker, js, 8))
            .is_err()
    );
    repo.mark_ready(js, 3).unwrap();
    repo.publish_python_prepared(instance, worker, &artifact)
        .unwrap();
    assert!(
        repo.publish_python_prepared(instance, worker, &artifact)
            .is_err()
    );
    assert!(
        repo.publish_python_prepared(instance, worker, &record(instance, worker, version, 9))
            .is_err()
    );
    for sql in [
        "UPDATE version_python_prepared SET artifact_size=10",
        "DELETE FROM version_python_prepared",
    ] {
        assert!(
            storage
                .db()
                .with_immediate(|tx| tx
                    .execute(sql, [])
                    .map_err(|_| crate::worker_repository::invariant()))
                .is_err()
        );
    }
    repo.mark_ready(version, 4).unwrap();
    assert!(!format!("{artifact:?}").contains("runtime"));
    drop(storage);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repo = WorkerRepository::new(storage.db());
    assert_eq!(
        repo.version_snapshot(instance, worker, version, false)
            .unwrap()
            .python_prepared,
        Some(artifact)
    );
    assert_eq!(
        repo.referenced_artifacts().unwrap(),
        vec![([1; 32], 100), ([7; 32], 2_048)]
    );
    let inventory = crate::inspect::inspect_snapshot_immutable_references(
        &root.join("control.sqlite"),
        5_000,
        "system/",
    )
    .unwrap();
    assert_eq!(inventory.len(), 2);
    assert!(
        inventory
            .iter()
            .any(|entry| entry.role == "version_artifact"
                && entry.sha256 == "07".repeat(32)
                && entry.size == 2_048)
    );
}

#[test]
fn python_prepared_gc_retention_and_delete_recovery_follow_version_authority() {
    let (_directory, root) = unique_root();
    let config = storage_config(&root);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let instance = storage.identity().instance_id;
    let repo = WorkerRepository::new(storage.db());
    let request = RequestId::generate();
    let worker = repo
        .create_worker(instance, "python-retention", request, 1, 100)
        .unwrap()
        .0
        .id;
    let first = staging_version(repo, instance, worker, "main.py");
    let second = staging_version(repo, instance, worker, "main.py");
    for (version, digest) in [(first, 7), (second, 8)] {
        repo.begin_validation(version).unwrap();
        repo.publish_python_prepared(instance, worker, &record(instance, worker, version, digest))
            .unwrap();
        repo.mark_ready(version, 3).unwrap();
    }
    repo.promote(instance, worker, first, None, request, 4)
        .unwrap();
    repo.promote(instance, worker, second, Some(first), request, 5)
        .unwrap();
    repo.promote(instance, worker, first, Some(second), request, 6)
        .unwrap();
    assert_eq!(
        repo.version_snapshot(instance, worker, first, false)
            .unwrap()
            .python_prepared
            .unwrap()
            .artifact_sha256,
        [7; 32]
    );
    assert_eq!(
        repo.begin_version_delete(instance, worker, first)
            .unwrap_err()
            .code(),
        ErrorCode::VersionActive
    );
    repo.add_version_referrer(second, "control_idempotency", "held", 6)
        .unwrap();
    assert_eq!(
        repo.begin_version_delete(instance, worker, second)
            .unwrap_err()
            .code(),
        ErrorCode::VersionReferenced
    );
    repo.remove_version_referrer(second, "control_idempotency", "held")
        .unwrap();
    repo.begin_version_delete(instance, worker, second).unwrap();
    assert_eq!(repo.referenced_artifacts().unwrap().len(), 3);
    drop(storage);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repo = WorkerRepository::new(storage.db());
    assert_eq!(repo.recover_deleting_versions(request, 10, 64).unwrap(), 1);
    assert_eq!(
        repo.referenced_artifacts().unwrap(),
        vec![([1; 32], 100), ([7; 32], 2_048)]
    );
    let rejected = staging_version(repo, instance, worker, "main.py");
    repo.begin_validation(rejected).unwrap();
    repo.publish_python_prepared(instance, worker, &record(instance, worker, rejected, 9))
        .unwrap();
    repo.mark_rejected(
        rejected,
        VersionState::Validating,
        ErrorCode::BundleRuntimeInvalid,
        11,
    )
    .unwrap();
    assert!(
        repo.referenced_artifacts()
            .unwrap()
            .contains(&([9; 32], 2_048))
    );
    repo.tombstone_version(instance, worker, rejected, request, 12)
        .unwrap();
    assert!(
        !repo
            .referenced_artifacts()
            .unwrap()
            .contains(&([9; 32], 2_048))
    );
    assert_eq!(
        repo.get_version(instance, worker, second).unwrap().state,
        VersionState::Tombstoned
    );
}

#[test]
fn published_control_migrations_are_unchanged_when_python_authority_is_added() {
    let (_directory, root) = unique_root();
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut connection = Connection::open(root.join("control.sqlite")).unwrap();
    crate::schema_migrations::migrate_to_for_test(
        &mut connection,
        crate::schema_migrations::DatabaseKind::Control,
        13,
    );
    let history = |connection: &Connection| {
        connection.prepare("SELECT version, checksum FROM refinery_schema_history WHERE version<=13 ORDER BY version").unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).unwrap()
            .collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    let published = history(&connection);
    assert_eq!(published.len(), 13);
    drop(connection);
    fs::set_permissions(
        root.join("control.sqlite"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let storage = PlatformStorage::bootstrap(&storage_config(&root), &SystemClock).unwrap();
    storage
        .db()
        .with_read(|connection| {
            assert_eq!(history(connection), published);
            Ok(())
        })
        .unwrap();
    assert!(
        storage
            .db()
            .table_sql("version_python_prepared")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        crate::migrations::inspect_schema(storage.db()).unwrap(),
        crate::migrations::current_schema_version()
    );
}
