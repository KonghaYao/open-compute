//! Current runtime fencing and private, verified prepared-snapshot transfer.

use super::*;
use crate::python_artifact::{PreparedPythonIdentity, restore_prepared_python};
use open_compute_core::SecretBytes;
use open_compute_storage::worker_repository::VersionSnapshot;

pub(super) fn prepared_identity(
    source: &RuntimeSource,
    snapshot: &VersionSnapshot,
) -> Result<Option<PreparedPythonIdentity>, PlatformError> {
    let Some(record) = &snapshot.python_prepared else {
        return Ok(None);
    };
    let identity: PreparedPythonIdentity =
        serde_json::from_slice(&record.identity_json).map_err(|_| invariant())?;
    if serde_json::to_vec(&identity).map_err(|_| invariant())? != record.identity_json
        || record.version_id != snapshot.version.id
        || identity.instance_id != source.storage.identity().instance_id
        || identity.worker_id != snapshot.worker.id
        || identity.version_id != snapshot.version.id
        || identity.worker_code_sha256 != hex::encode(snapshot.version.worker_code_sha256)
        || identity.runtime.pin != source.python_runtime_pin
        || record.prepared_identity_sha256 != identity.sha256()?
    {
        return Err(invariant());
    }
    Ok(Some(identity))
}

impl RuntimeSource {
    /// Transfer verified snapshot bytes only for the exact current engine and retained identity.
    ///
    /// This never prepares, installs, repairs or writes an artifact. Source JSON carries only
    /// its identity digest; the binary response avoids base64 expansion of snapshot memory.
    pub async fn resolve_python_prepared(
        &self,
        key: &str,
        expected_worker_code_sha256: &str,
        scope: RuntimeScope,
        expected_prepared_sha256: &str,
    ) -> Result<SecretBytes, PlatformError> {
        let (instance, worker, version) = parse_loader_key(key)?;
        resolution::validate_expected_digest(expected_worker_code_sha256)?;
        resolution::validate_expected_digest(expected_prepared_sha256)?;
        let snapshot = WorkerRepository::new(self.storage.db()).version_snapshot(
            instance,
            worker,
            version,
            scope != RuntimeScope::Runtime,
        )?;
        resolution::validate_scope(&snapshot, scope)?;
        if hex::encode(snapshot.version.worker_code_sha256) != expected_worker_code_sha256 {
            return Err(invariant());
        }
        let identity = prepared_identity(self, &snapshot)?.ok_or_else(invariant)?;
        if hex::encode(identity.sha256()?) != expected_prepared_sha256 {
            return Err(invariant());
        }
        // Recheck source authority even when this private endpoint is called without
        // the preceding metadata fetch. A self-consistent retained record cannot
        // substitute another SDK/package inventory for the immutable upload.
        let resolved = self
            .resolve(key, expected_worker_code_sha256, scope)
            .await?;
        if resolved.python_prepared_sha256.as_deref() != Some(expected_prepared_sha256) {
            return Err(invariant());
        }
        let record = snapshot.python_prepared.as_ref().ok_or_else(invariant)?;
        restore_prepared_python(record, &identity, &self.artifacts, self.storage.crypto()).await
    }
}
