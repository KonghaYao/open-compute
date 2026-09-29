use super::{
    AiBackendConfig, AiBackendProtocol, MAX_MODEL_NAME_BYTES, MAX_REVISION_BYTES,
    validate_nonempty, validate_optional_nonempty,
};
use crate::{ErrorCode, PlatformError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Operator mapping from a public reranking alias to one dedicated backend model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiRerankingModelConfig {
    /// Operation-specific backend entry name.
    pub backend: String,
    /// Model value sent to the provider.
    pub remote_model: String,
    /// Optional immutable revision identifier actually supported by the provider.
    #[serde(default)]
    pub provider_revision: Option<String>,
}

impl AiRerankingModelConfig {
    pub(super) fn validate(
        &self,
        backends: &BTreeMap<String, AiBackendConfig>,
    ) -> Result<(), PlatformError> {
        if !backends.get(&self.backend).is_some_and(|backend| {
            matches!(
                backend.protocol,
                AiBackendProtocol::CohereRerankV2 | AiBackendProtocol::RerankV1
            )
        }) {
            return Err(PlatformError::new(
                ErrorCode::ConfigInvalid,
                "AI reranking model references an incompatible backend",
            ));
        }
        validate_nonempty(&self.remote_model, MAX_MODEL_NAME_BYTES)?;
        validate_optional_nonempty(self.provider_revision.as_deref(), MAX_REVISION_BYTES)
    }
}
