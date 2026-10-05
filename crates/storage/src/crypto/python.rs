//! Deployment-bound encryption for dedicated Python snapshots.

use super::*;

/// Maximum unencrypted dedicated snapshot accepted by the platform.
pub const MAX_PYTHON_SNAPSHOT_BYTES: usize = 128 * 1024 * 1024;

impl SecretCrypto {
    /// Seal a Python snapshot under an independent deployment-artifact AAD domain.
    pub fn seal_python_snapshot(
        &self,
        plaintext: &SecretBytes,
        instance: InstanceId,
        worker: WorkerId,
        version: VersionId,
        prepared_identity: &[u8; 32],
    ) -> Result<SecretEnvelope, PlatformError> {
        if plaintext.expose().len() < 16 || plaintext.expose().len() > MAX_PYTHON_SNAPSHOT_BYTES {
            return Err(snapshot_error());
        }
        let mut nonce = [0; NONCE_LEN];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| snapshot_error())?;
        let aad = snapshot_aad(instance, worker, version, prepared_identity);
        let ciphertext = self
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext.expose(),
                    aad: &aad,
                },
            )
            .map_err(|_| snapshot_error())?;
        Ok(SecretEnvelope {
            version: SecretEnvelope::CURRENT_VERSION,
            key_id: self.key_id.clone(),
            algorithm: ALGORITHM.to_owned(),
            nonce: nonce.to_vec(),
            ciphertext,
        })
    }

    /// Open only a snapshot sealed for this exact Version and prepared identity.
    pub fn open_python_snapshot(
        &self,
        envelope: &SecretEnvelope,
        instance: InstanceId,
        worker: WorkerId,
        version: VersionId,
        prepared_identity: &[u8; 32],
    ) -> Result<SecretBytes, PlatformError> {
        if envelope.version != SecretEnvelope::CURRENT_VERSION
            || envelope.algorithm != ALGORITHM
            || envelope.key_id != self.key_id
            || envelope.nonce.len() != NONCE_LEN
            || envelope.ciphertext.len() < 32
            || envelope.ciphertext.len() > MAX_PYTHON_SNAPSHOT_BYTES + 16
        {
            return Err(snapshot_error());
        }
        let aad = snapshot_aad(instance, worker, version, prepared_identity);
        let plaintext = self
            .cipher
            .decrypt(
                XNonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| snapshot_error())?;
        Ok(SecretBytes::new(plaintext))
    }
}

fn snapshot_aad(
    instance: InstanceId,
    worker: WorkerId,
    version: VersionId,
    identity: &[u8; 32],
) -> Vec<u8> {
    // UUIDs and the digest have fixed lengths; the leading domain cannot collide
    // with environment-secret AAD, whose first bytes are its schema.
    let mut aad = b"open-compute/python-prepared/v1\0".to_vec();
    aad.extend_from_slice(instance.as_uuid().as_bytes());
    aad.extend_from_slice(worker.as_uuid().as_bytes());
    aad.extend_from_slice(version.as_uuid().as_bytes());
    aad.extend_from_slice(identity);
    aad
}

fn snapshot_error() -> PlatformError {
    PlatformError::new(
        ErrorCode::VersionInvariantViolation,
        "dedicated Python snapshot encryption validation failed",
    )
}

#[cfg(test)]
#[path = "python_tests.rs"]
mod tests;
