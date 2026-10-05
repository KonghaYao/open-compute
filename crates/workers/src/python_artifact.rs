//! Immutable encrypted envelope around workerd-owned Python snapshot bytes.

use open_compute_core::{ErrorCode, InstanceId, PlatformError, SecretBytes, VersionId, WorkerId};
use open_compute_storage::crypto::{MAX_PYTHON_SNAPSHOT_BYTES, SecretCrypto, SecretEnvelope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"OCPY0001";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const HEADER_PREFIX_BYTES: usize = 12;

mod store;
pub use store::{publish_prepared_python, restore_prepared_python};

/// Verified host runtime inputs shared by every Version in one instance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonRuntimePin {
    /// Exact workerd source commit from the formal runtime lock.
    pub workerd_revision: String,
    /// Verified workerd executable for the selected host target.
    pub workerd_binary_sha256: String,
    /// Ordered, non-secret process flags from the formal runtime lock.
    pub process_flags: Vec<String>,
    /// Verified uncompressed Pyodide bundle.
    pub pyodide_bundle_sha256: String,
    /// Runtime/system Worker asset manifest consumed by this executable.
    pub runtime_assets_sha256: String,
}

/// Verified host runtime and bundle-specific package inputs for one snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonRuntimeIdentity {
    /// Current host pin; SDK and package inputs remain owned by the immutable bundle.
    pub pin: PythonRuntimePin,
    /// Digest of uploaded module names, types, sizes and content, including SDK/package files.
    pub module_inventory_sha256: String,
}

/// Inputs that bind a snapshot to its immutable source, environment and runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedPythonIdentity {
    /// Current envelope/prepare format; earlier development formats are rejected.
    pub schema_version: u32,
    /// Instance that owns the encryption key and Version authority.
    pub instance_id: InstanceId,
    /// Source Worker identity.
    pub worker_id: WorkerId,
    /// Source Version, including its immutable secret revisions.
    pub version_id: VersionId,
    /// Authority descriptor digest covering modules, compatibility, limits and env.
    pub worker_code_sha256: String,
    /// Exact runtime, system assets and package inputs.
    pub runtime: PythonRuntimeIdentity,
}

impl PreparedPythonIdentity {
    /// Derive preparation inputs from a verified Python `RuntimeSource` snapshot.
    ///
    /// Uploads contain resolved files, not the original wheel archives. Their names,
    /// representations and exact content are authoritative; wheel provenance belongs
    /// to developer-toolchain qualification rather than a guessed upload field.
    pub fn from_snapshot(
        pin: PythonRuntimePin,
        snapshot: &crate::RuntimeSnapshot,
    ) -> Result<Self, PlatformError> {
        let (instance_id, worker_id, version_id) =
            crate::descriptor::parse_loader_key(&snapshot.loader_key)?;
        let main = snapshot.main_module.as_deref().ok_or_else(invalid)?;
        if !snapshot
            .modules
            .iter()
            .any(|module| module.name == main && module.module_type == crate::ModuleType::Python)
        {
            return Err(invalid());
        }
        let identity = Self {
            schema_version: 1,
            instance_id,
            worker_id,
            version_id,
            worker_code_sha256: snapshot.worker_code_sha256.clone(),
            runtime: PythonRuntimeIdentity::from_modules(pin, &snapshot.modules)?,
        };
        identity.sha256()?;
        Ok(identity)
    }

    /// Validate the current model and derive its domain-separated identity.
    pub fn sha256(&self) -> Result<[u8; 32], PlatformError> {
        self.runtime.pin.validate()?;
        if self.schema_version != 1 {
            return Err(invalid());
        }
        for digest in [
            &self.worker_code_sha256,
            &self.runtime.module_inventory_sha256,
        ] {
            if !is_hex(digest, 64) {
                return Err(invalid());
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"open-compute/python-prepared-identity/v1\0");
        hash.update(serde_json::to_vec(self).map_err(|_| invalid())?);
        Ok(hash.finalize().into())
    }
}

impl PythonRuntimeIdentity {
    pub(crate) fn from_modules(
        pin: PythonRuntimePin,
        modules: &[crate::RuntimeModule],
    ) -> Result<Self, PlatformError> {
        pin.validate()?;
        // RuntimeSource preserves the canonical bundle order. Reject accidental
        // duplicate/reordered inventories rather than repairing them here.
        if modules.is_empty() || modules.windows(2).any(|pair| pair[0].name >= pair[1].name) {
            return Err(invalid());
        }
        let inventory: Vec<_> = modules
            .iter()
            .map(|module| {
                (
                    &module.name,
                    module.module_type,
                    module.bytes.len(),
                    hex::encode(Sha256::digest(&module.bytes)),
                )
            })
            .collect();
        let mut hash = Sha256::new();
        hash.update(b"open-compute/python-module-inventory/v1\0");
        hash.update(serde_json::to_vec(&inventory).map_err(|_| invalid())?);
        Ok(Self {
            pin,
            module_inventory_sha256: hex::encode(hash.finalize()),
        })
    }
}

impl PythonRuntimePin {
    /// Reject incomplete or noncanonical engine identity before preparation or restore.
    pub fn validate(&self) -> Result<(), PlatformError> {
        if !is_hex(&self.workerd_revision, 40)
            || self.process_flags.is_empty()
            || self.process_flags.len() > 16
            || self.process_flags.iter().enumerate().any(|(index, flag)| {
                !flag.starts_with("--")
                    || flag.len() < 3
                    || flag.len() > 128
                    || flag.contains('=')
                    || !flag.bytes().all(|byte| byte.is_ascii_graphic())
                    || self.process_flags[..index].contains(flag)
            })
            || [
                &self.workerd_binary_sha256,
                &self.pyodide_bundle_sha256,
                &self.runtime_assets_sha256,
            ]
            .iter()
            .any(|digest| !is_hex(digest, 64))
        {
            return Err(invalid());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Header {
    identity: PreparedPythonIdentity,
    snapshot_sha256: String,
    snapshot_size: usize,
    // Encode metadata as JSON and append ciphertext as raw bytes, avoiding a
    // sixfold allocation for serde's numeric byte-array encoding.
    envelope: SecretEnvelope,
}

/// Serialized ciphertext suitable for the existing content-addressed artifact store.
pub struct EncryptedPythonArtifact {
    bytes: Vec<u8>,
}

impl std::fmt::Debug for EncryptedPythonArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EncryptedPythonArtifact")
            .field("size", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl EncryptedPythonArtifact {
    /// Encrypt workerd-produced bytes without interpreting its snapshot format.
    pub fn seal(
        identity: PreparedPythonIdentity,
        snapshot: &SecretBytes,
        crypto: &SecretCrypto,
    ) -> Result<Self, PlatformError> {
        let prepared_digest = identity.sha256()?;
        let mut envelope = crypto.seal_python_snapshot(
            snapshot,
            identity.instance_id,
            identity.worker_id,
            identity.version_id,
            &prepared_digest,
        )?;
        let ciphertext = std::mem::take(&mut envelope.ciphertext);
        let header = Header {
            identity,
            snapshot_sha256: hex::encode(Sha256::digest(snapshot.expose())),
            snapshot_size: snapshot.expose().len(),
            envelope,
        };
        let header = serde_json::to_vec(&header).map_err(|_| invalid())?;
        if header.len() > MAX_HEADER_BYTES {
            return Err(invalid());
        }
        let mut bytes = Vec::with_capacity(HEADER_PREFIX_BYTES + header.len() + ciphertext.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(
            &(u32::try_from(header.len()).map_err(|_| invalid())?).to_be_bytes(),
        );
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&ciphertext);
        Ok(Self { bytes })
    }

    /// Ciphertext only; safe to persist as an immutable content-addressed object.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Transfer the ciphertext container to the artifact store without copying it.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Verify framing, metadata and AEAD before admitting bytes for native restore.
    pub fn open(
        bytes: &[u8],
        expected: &PreparedPythonIdentity,
        crypto: &SecretCrypto,
    ) -> Result<SecretBytes, PlatformError> {
        let prepared_digest = expected.sha256()?;
        if bytes.len() < HEADER_PREFIX_BYTES
            || bytes.len() > HEADER_PREFIX_BYTES + MAX_HEADER_BYTES + MAX_PYTHON_SNAPSHOT_BYTES + 16
            || &bytes[..8] != MAGIC
        {
            return Err(invalid());
        }
        let header_size =
            u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| invalid())?) as usize;
        if header_size == 0 || header_size > MAX_HEADER_BYTES {
            return Err(invalid());
        }
        let header_end = HEADER_PREFIX_BYTES + header_size;
        let header_bytes = bytes
            .get(HEADER_PREFIX_BYTES..header_end)
            .ok_or_else(invalid)?;
        let mut header: Header = serde_json::from_slice(header_bytes).map_err(|_| invalid())?;
        if &header.identity != expected
            || !is_hex(&header.snapshot_sha256, 64)
            || header.snapshot_size < 16
            || header.snapshot_size > MAX_PYTHON_SNAPSHOT_BYTES
            || !header.envelope.ciphertext.is_empty()
            || serde_json::to_vec(&header).map_err(|_| invalid())? != header_bytes
        {
            return Err(invalid());
        }
        let ciphertext = bytes.get(header_end..).ok_or_else(invalid)?;
        if ciphertext.len() != header.snapshot_size + 16 {
            return Err(invalid());
        }
        header.envelope.ciphertext = ciphertext.to_vec();
        let snapshot = crypto.open_python_snapshot(
            &header.envelope,
            expected.instance_id,
            expected.worker_id,
            expected.version_id,
            &prepared_digest,
        )?;
        if hex::encode(Sha256::digest(snapshot.expose())) != header.snapshot_sha256 {
            return Err(invalid());
        }
        Ok(snapshot)
    }
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn invalid() -> PlatformError {
    PlatformError::new(
        ErrorCode::VersionInvariantViolation,
        "dedicated Python artifact verification failed",
    )
}

#[cfg(test)]
#[path = "python_artifact_tests.rs"]
mod tests;
