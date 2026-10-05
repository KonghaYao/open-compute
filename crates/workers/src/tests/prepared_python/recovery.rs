//! Crash, disk-admission and object-GC recovery for immutable Python preparation.

use super::*;
use std::io::{Read as _, Write as _};
use std::os::unix::process::ExitStatusExt as _;
use tokio::io::AsyncBufReadExt as _;

pub(super) struct Evidence(pub(super) Option<tempfile::TempDir>);

impl Drop for Evidence {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Some(directory) = self.0.take()
        {
            let path = directory.keep();
            let failed = path.parent().unwrap().join("failed");
            fs::create_dir_all(&failed).unwrap();
            let destination = failed.join(path.file_name().unwrap());
            if destination.exists() {
                eprintln!("retained Python preparation evidence: {}", path.display());
            } else {
                fs::rename(&path, &destination).unwrap();
                eprintln!(
                    "retained Python preparation evidence: {}",
                    destination.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn prepared_publication_recovers_abandoned_upload_and_disk_pressure_without_changing_active()
{
    if let Ok(input) = std::env::var("OPEN_COMPUTE_PREPARED_CRASH_INPUT") {
        let (root, endpoint, identity): (std::path::PathBuf, String, PreparedPythonIdentity) =
            serde_json::from_str(&input).unwrap();
        let storage = PlatformStorage::bootstrap(&storage_config(&root), &SystemClock).unwrap();
        let config = s3_config(&endpoint);
        let credentials = resolve_s3_credentials_with(
            &config,
            &MapEnv::new()
                .with("S3_ACCESS_KEY_ID", "AKIAEXAMPLEKEYID01")
                .with(
                    "S3_SECRET_ACCESS_KEY",
                    "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                ),
        )
        .unwrap();
        let artifacts = ArtifactStore::new(
            ObjectBackend::connect_s3(&config, &credentials, 32 * 1024 * 1024).unwrap(),
        );
        python_artifact::publish_prepared_python(
            &storage,
            &artifacts,
            &identity,
            &SecretBytes::new(b"native snapshot secret bytes".to_vec()),
            5,
            |_| {
                println!("PREPARED_UPLOAD_BEFORE_SQL");
                std::io::stdout().flush().unwrap();
                let mut byte = [0];
                let _ = std::io::stdin().read_exact(&mut byte);
                panic!("parent must kill the fixture at the upload boundary");
            },
        )
        .await
        .unwrap();
        panic!("unreachable publication return");
    }
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp/prepared-python-run");
    fs::create_dir_all(&base).unwrap();
    let base = fs::canonicalize(base).unwrap();
    let evidence = Evidence(Some(
        tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(&base)
            .unwrap(),
    ));
    let directory = evidence.0.as_ref().unwrap();
    let config = storage_config(&directory.path().join("data"));
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let instance = storage.identity().instance_id;
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(instance, "prepared-recovery", RequestId::generate(), 1, 100)
        .unwrap()
        .0
        .id;
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let snapshot = SecretBytes::new(b"native snapshot secret bytes".to_vec());
    let original = create_candidate(&storage, worker, "main.py");
    let retained = publish_prepared_python(&storage, &artifacts, &original, &snapshot, 2)
        .await
        .unwrap();
    repository.mark_ready(original.version_id, 3).unwrap();
    let active = repository
        .promote(
            instance,
            worker,
            original.version_id,
            None,
            RequestId::generate(),
            4,
        )
        .unwrap();
    let candidate = create_candidate(&storage, worker, "main.py");
    drop(storage);
    let output = fs::File::create(directory.path().join("crash-child.stderr")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::prepared_python::recovery::prepared_publication_recovers_abandoned_upload_and_disk_pressure_without_changing_active", "--nocapture", "--test-threads=1"])
        .env("OPEN_COMPUTE_PREPARED_CRASH_INPUT", serde_json::to_string(&(config.path.clone(), mock.endpoint.clone(), candidate.clone())).unwrap())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(output)
        .kill_on_drop(true)
        .spawn().unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let reached = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if line.contains("PREPARED_UPLOAD_BEFORE_SQL") {
                return true;
            }
        }
        false
    })
    .await;
    if child.try_wait().unwrap().is_none() {
        child.start_kill().unwrap();
    }
    let status = child.wait().await.unwrap();
    assert_eq!(status.signal(), Some(9));
    assert!(
        matches!(reached, Ok(true)),
        "child upload boundary: {reached:?}"
    );
    assert_eq!(
        mock.object_count(),
        2,
        "verified upload precedes SQL commit"
    );

    // Reopen the same SQLite authority and key with disk admission deliberately closed.
    let mut pressured = config.clone();
    pressured.free_space_soft_bytes = u64::MAX;
    pressured.free_space_hard_bytes = u64::MAX - 1;
    let storage = PlatformStorage::bootstrap(&pressured, &SystemClock).unwrap();
    let repository = WorkerRepository::new(storage.db());
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert!(
        repository
            .version_snapshot(instance, worker, candidate.version_id, true)
            .unwrap()
            .python_prepared
            .is_none()
    );
    assert!(repository.mark_ready(candidate.version_id, 6).is_err());
    mock.clear_recorded();
    let error = publish_prepared_python(&storage, &artifacts, &candidate, &snapshot, 6)
        .await
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::StoragePressure);
    assert!(
        mock.recorded().is_empty(),
        "disk rejection must precede S3 mutation"
    );
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert_eq!(
        restore_prepared_python(&retained, &original, &artifacts, storage.crypto())
            .await
            .unwrap()
            .expose(),
        snapshot.expose()
    );
    drop(storage);

    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repository = WorkerRepository::new(storage.db());
    let published = publish_prepared_python(&storage, &artifacts, &candidate, &snapshot, 7)
        .await
        .unwrap();
    assert_eq!(mock.object_count(), 3);
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert_eq!(
        repository
            .get_version(instance, worker, candidate.version_id)
            .unwrap()
            .state,
        VersionState::Validating
    );
    let fence = artifacts.fence_version_gc().await;
    let referenced: HashSet<_> = repository
        .referenced_artifacts()
        .unwrap()
        .into_iter()
        .map(|(digest, size)| ArtifactRef::new(1, &hex::encode(digest), size).unwrap())
        .collect();
    assert_eq!(
        artifacts
            .gc_unreferenced(
                &fence,
                &referenced,
                SystemTime::now() + Duration::from_secs(60)
            )
            .await
            .unwrap(),
        1
    );
    drop(fence);
    assert_eq!(
        mock.object_count(),
        2,
        "only the abandoned ciphertext is collected"
    );
    for (record, identity) in [(&retained, &original), (&published, &candidate)] {
        assert_eq!(
            restore_prepared_python(record, identity, &artifacts, storage.crypto())
                .await
                .unwrap()
                .expose(),
            snapshot.expose()
        );
    }
    repository.mark_ready(candidate.version_id, 8).unwrap();
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    drop(storage);
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repository = WorkerRepository::new(storage.db());
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert_eq!(
        repository
            .version_snapshot(instance, worker, candidate.version_id, false)
            .unwrap()
            .python_prepared,
        Some(published.clone())
    );
    assert_eq!(
        publish_prepared_python(
            &storage,
            &artifacts,
            &candidate,
            &SecretBytes::new(b"must not replace retained bytes".to_vec()),
            9
        )
        .await
        .unwrap(),
        published
    );
    assert_eq!(mock.object_count(), 2);
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
}

#[tokio::test]
async fn prepared_publication_after_sql_crash_reuses_committed_identity_without_new_upload() {
    if let Ok(input) = std::env::var("OPEN_COMPUTE_PREPARED_COMMITTED_INPUT") {
        let (root, endpoint, identity): (std::path::PathBuf, String, PreparedPythonIdentity) =
            serde_json::from_str(&input).unwrap();
        let storage = PlatformStorage::bootstrap(&storage_config(&root), &SystemClock).unwrap();
        let config = s3_config(&endpoint);
        let credentials = resolve_s3_credentials_with(
            &config,
            &MapEnv::new()
                .with("S3_ACCESS_KEY_ID", "AKIAEXAMPLEKEYID01")
                .with(
                    "S3_SECRET_ACCESS_KEY",
                    "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                ),
        )
        .unwrap();
        let artifacts = ArtifactStore::new(
            ObjectBackend::connect_s3(&config, &credentials, 32 * 1024 * 1024).unwrap(),
        );
        python_artifact::publish_prepared_python(
            &storage,
            &artifacts,
            &identity,
            &SecretBytes::new(b"committed native snapshot secret bytes".to_vec()),
            5,
            |publish| {
                publish()?;
                // SQLite has committed, but publication has not returned to the
                // validator and the Version must still be validating.
                println!("PREPARED_SQL_COMMITTED_BEFORE_READY");
                std::io::stdout().flush().unwrap();
                let mut byte = [0];
                let _ = std::io::stdin().read_exact(&mut byte);
                panic!("parent must kill the fixture after the SQL commit");
            },
        )
        .await
        .unwrap();
        panic!("unreachable publication return");
    }

    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp/prepared-python-run");
    fs::create_dir_all(&base).unwrap();
    let evidence = Evidence(Some(
        tempfile::Builder::new()
            .prefix("commit-crash-")
            .tempdir_in(fs::canonicalize(base).unwrap())
            .unwrap(),
    ));
    let directory = evidence.0.as_ref().unwrap();
    let config = storage_config(&directory.path().join("data"));
    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let instance = storage.identity().instance_id;
    let repository = WorkerRepository::new(storage.db());
    let worker = repository
        .create_worker(
            instance,
            "prepared-commit-crash",
            RequestId::generate(),
            1,
            100,
        )
        .unwrap()
        .0
        .id;
    let mock = MockS3::spawn("open-compute").await;
    let artifacts = artifact_store(&mock);
    let original = create_candidate(&storage, worker, "main.py");
    let retained = publish_prepared_python(
        &storage,
        &artifacts,
        &original,
        &SecretBytes::new(b"active native snapshot bytes".to_vec()),
        2,
    )
    .await
    .unwrap();
    repository.mark_ready(original.version_id, 3).unwrap();
    let active = repository
        .promote(
            instance,
            worker,
            original.version_id,
            None,
            RequestId::generate(),
            4,
        )
        .unwrap();
    let candidate = create_candidate(&storage, worker, "main.py");
    drop(storage);

    let output = fs::File::create(directory.path().join("crash-child.stderr")).unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::prepared_python::recovery::prepared_publication_after_sql_crash_reuses_committed_identity_without_new_upload",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(
            "OPEN_COMPUTE_PREPARED_COMMITTED_INPUT",
            serde_json::to_string(&(
                config.path.clone(),
                mock.endpoint.clone(),
                candidate.clone(),
            ))
            .unwrap(),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(output)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let reached = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if line.contains("PREPARED_SQL_COMMITTED_BEFORE_READY") {
                return true;
            }
        }
        false
    })
    .await;
    if child.try_wait().unwrap().is_none() {
        child.start_kill().unwrap();
    }
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.signal(), Some(9));
    assert!(
        matches!(reached, Ok(true)),
        "SQL commit boundary: {reached:?}"
    );
    assert_eq!(mock.object_count(), 2);

    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repository = WorkerRepository::new(storage.db());
    let pending = repository
        .version_snapshot(instance, worker, candidate.version_id, true)
        .unwrap();
    assert_eq!(pending.version.state, VersionState::Validating);
    let committed = pending.python_prepared.unwrap();
    assert_eq!(committed.created_at_ms, 5);
    assert_eq!(
        committed.prepared_identity_sha256,
        candidate.sha256().unwrap()
    );
    assert_eq!(
        committed.identity_json,
        serde_json::to_vec(&candidate).unwrap()
    );
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    let reference = ArtifactRef::new(
        1,
        &hex::encode(committed.artifact_sha256),
        committed.artifact_size,
    )
    .unwrap();
    let snapshot = b"committed native snapshot secret bytes";
    let ciphertext = artifacts.open(&reference).await.unwrap();
    assert!(
        !ciphertext
            .windows(snapshot.len())
            .any(|part| part == snapshot)
    );
    assert_eq!(
        restore_prepared_python(&committed, &candidate, &artifacts, storage.crypto())
            .await
            .unwrap()
            .expose(),
        snapshot
    );

    // A validating Version's committed object is live, including before recovery
    // finishes validation. GC must not treat it like an abandoned upload.
    let fence = artifacts.fence_version_gc().await;
    let referenced: HashSet<_> = repository
        .referenced_artifacts()
        .unwrap()
        .into_iter()
        .map(|(digest, size)| ArtifactRef::new(1, &hex::encode(digest), size).unwrap())
        .collect();
    assert!(referenced.contains(&reference));
    assert_eq!(
        artifacts
            .gc_unreferenced(
                &fence,
                &referenced,
                SystemTime::now() + Duration::from_secs(60),
            )
            .await
            .unwrap(),
        0
    );
    drop(fence);
    mock.clear_recorded();
    assert_eq!(
        publish_prepared_python(
            &storage,
            &artifacts,
            &candidate,
            &SecretBytes::new(b"must not replace committed native bytes".to_vec()),
            6,
        )
        .await
        .unwrap(),
        committed
    );
    assert!(
        !mock
            .recorded()
            .iter()
            .any(|request| request.method == "PUT")
    );
    repository.mark_ready(candidate.version_id, 7).unwrap();
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert_eq!(mock.object_count(), 2);
    drop(storage);

    let storage = PlatformStorage::bootstrap(&config, &SystemClock).unwrap();
    let repository = WorkerRepository::new(storage.db());
    let ready = repository
        .version_snapshot(instance, worker, candidate.version_id, false)
        .unwrap();
    assert_eq!(ready.version.state, VersionState::Ready);
    assert_eq!(ready.python_prepared, Some(committed.clone()));
    assert_eq!(repository.get_worker(instance, worker).unwrap(), active);
    assert_eq!(
        restore_prepared_python(&committed, &candidate, &artifacts, storage.crypto())
            .await
            .unwrap()
            .expose(),
        snapshot
    );
    assert_eq!(
        restore_prepared_python(&retained, &original, &artifacts, storage.crypto())
            .await
            .unwrap()
            .expose(),
        b"active native snapshot bytes"
    );
}
