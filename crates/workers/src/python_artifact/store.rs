//! Verified ciphertext upload and immutable SQLite publication.

use super::*;
use bytes::Bytes;
use futures::stream;
use open_compute_artifacts::{ARTIFACT_KEY_VERSION, ArtifactRef, ArtifactStore};
use open_compute_storage::PlatformStorage;
use open_compute_storage::worker_repository::{
    PythonPreparedArtifactRecord, VersionState, WorkerRepository,
};

/// Publish an encrypted native snapshot while fencing object GC through authority commit.
///
/// Recovery verifies and reuses an existing record, rechecking Version authority at
/// the commit fence; it never replaces bytes or installs another runtime's artifact.
/// `commit` executes the supplied synchronous authority operation under the caller's
/// admission fence. It must call that operation once, without awaiting or re-entry.
pub async fn publish_prepared_python(
    storage: &PlatformStorage,
    artifacts: &ArtifactStore,
    identity: &PreparedPythonIdentity,
    snapshot: &SecretBytes,
    now_ms: i64,
    commit: impl FnOnce(&dyn Fn() -> Result<(), PlatformError>) -> Result<(), PlatformError>,
) -> Result<PythonPreparedArtifactRecord, PlatformError> {
    let identity_digest = identity.sha256()?;
    let _gc_reservation = artifacts.reserve_version_artifact().await;
    let repository = WorkerRepository::new(storage.db());
    let source = repository.version_snapshot(
        identity.instance_id,
        identity.worker_id,
        identity.version_id,
        true,
    )?;
    if hex::encode(source.version.worker_code_sha256) != identity.worker_code_sha256
        || !source
            .version
            .main_module
            .as_deref()
            .is_some_and(|name| name.ends_with(".py"))
    {
        return Err(invalid());
    }
    if let Some(existing) = source.python_prepared {
        restore_prepared_python(&existing, identity, artifacts, storage.crypto()).await?;
        commit(&|| {
            let current = repository.version_snapshot(
                identity.instance_id,
                identity.worker_id,
                identity.version_id,
                true,
            )?;
            if current.python_prepared.as_ref() != Some(&existing) {
                return Err(invalid());
            }
            Ok(())
        })?;
        return Ok(existing);
    }
    if source.version.state != VersionState::Validating {
        return Err(invalid());
    }
    let encrypted = EncryptedPythonArtifact::seal(identity.clone(), snapshot, storage.crypto())?;
    let digest = hex::encode(Sha256::digest(encrypted.bytes()));
    let size = encrypted.bytes().len() as u64;
    let _admission = storage.reserve_mutation(size)?;
    let body = Bytes::from(encrypted.into_bytes());
    let artifact = artifacts
        .put_verified(
            stream::once(async move { Ok::<Bytes, std::io::Error>(body) }),
            &digest,
            size,
        )
        .await?;
    if artifact.sha256_hex() != digest || artifact.size() != size {
        return Err(invalid());
    }
    let record = PythonPreparedArtifactRecord {
        version_id: identity.version_id,
        prepared_identity_sha256: identity_digest,
        identity_json: serde_json::to_vec(identity).map_err(|_| invalid())?,
        artifact_sha256: *artifact.sha256_bytes(),
        artifact_size: size,
        created_at_ms: now_ms,
    };
    commit(&|| {
        repository.publish_python_prepared(identity.instance_id, identity.worker_id, &record)
    })?;
    Ok(record)
}

/// Verify the exact retained identity and ciphertext object before native snapshot restore.
pub async fn restore_prepared_python(
    record: &PythonPreparedArtifactRecord,
    expected: &PreparedPythonIdentity,
    artifacts: &ArtifactStore,
    crypto: &SecretCrypto,
) -> Result<SecretBytes, PlatformError> {
    if record.version_id != expected.version_id
        || record.prepared_identity_sha256 != expected.sha256()?
        || record.identity_json != serde_json::to_vec(expected).map_err(|_| invalid())?
        || record.artifact_size == 0
        || record.artifact_size
            > (HEADER_PREFIX_BYTES + MAX_HEADER_BYTES + MAX_PYTHON_SNAPSHOT_BYTES + 16) as u64
    {
        return Err(invalid());
    }
    let reference = ArtifactRef::new(
        ARTIFACT_KEY_VERSION,
        &hex::encode(record.artifact_sha256),
        record.artifact_size,
    )?;
    let encrypted = artifacts.open(&reference).await?;
    EncryptedPythonArtifact::open(&encrypted, expected, crypto)
}
