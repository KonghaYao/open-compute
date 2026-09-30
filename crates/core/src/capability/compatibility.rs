//! Compatibility capabilities projected from the embedded workerd catalog.

use super::{is_sha256, unique_nonempty};
use serde::{Deserialize, Serialize};

/// One compatibility implication reflected from workerd's schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityImplicationV1 {
    /// Compatibility feature field names which activate this feature.
    pub flags: Vec<String>,
    /// First compatibility date on which the implication applies.
    pub after_date: String,
}

impl CompatibilityImplicationV1 {
    fn validate(&self) -> bool {
        unique_nonempty(&self.flags) && compatibility_date_shape(&self.after_date)
    }
}

/// One compatibility feature reflected from workerd's compiled schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityFeatureV1 {
    /// Cap'n Proto field name.
    pub field: String,
    /// User input which explicitly enables the feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_flag: Option<String>,
    /// User input which explicitly disables the feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_flag: Option<String>,
    /// Date on which the feature becomes enabled by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_on_date: Option<String>,
    /// Whether the feature is enabled for every compatibility date.
    pub enabled_for_all_dates: bool,
    /// Whether workerd requires its experimental process mode for this feature.
    pub experimental: bool,
    /// Whether the feature changes the Python snapshot release identity.
    pub python_snapshot_release: bool,
    /// Reflected implication rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implied_by: Vec<CompatibilityImplicationV1>,
}

impl CompatibilityFeatureV1 {
    fn validate(&self) -> bool {
        !self.field.is_empty()
            && (self.enable_flag.is_some() || self.disable_flag.is_some())
            && self
                .enable_flag
                .as_ref()
                .is_none_or(|flag| !flag.is_empty())
            && self
                .disable_flag
                .as_ref()
                .is_none_or(|flag| !flag.is_empty())
            && self
                .default_on_date
                .as_ref()
                .is_none_or(|date| compatibility_date_shape(date))
            && self
                .implied_by
                .iter()
                .all(CompatibilityImplicationV1::validate)
    }
}

/// Tenant compatibility capability emitted by the exact embedded workerd binary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCompatibilityV1 {
    /// Runtime validation authority.
    pub validation: String,
    /// Latest date compiled into the binary.
    pub binary_maximum_date: String,
    /// Whether dates after the current UTC day are accepted.
    pub future_dates_allowed: bool,
    /// Whether the workerd process enables experimental compatibility flags.
    pub experimental_enabled: bool,
    /// Features in Cap'n Proto field ordinal order.
    pub features: Vec<CompatibilityFeatureV1>,
    /// SHA-256 of the exact workerd catalog bytes.
    pub catalog_sha256: String,
}

impl RuntimeCompatibilityV1 {
    /// Validate catalog identity and unique input flags.
    pub fn validate(&self) -> bool {
        let input_flags = self
            .features
            .iter()
            .flat_map(|feature| {
                [feature.enable_flag.as_ref(), feature.disable_flag.as_ref()]
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<Vec<_>>();
        self.validation == "workerd_code_version"
            && compatibility_date_shape(&self.binary_maximum_date)
            && !self.future_dates_allowed
            && !self.features.is_empty()
            && self.features.iter().all(CompatibilityFeatureV1::validate)
            && unique_nonempty(&input_flags)
            && is_sha256(&self.catalog_sha256)
    }
}

/// Compatibility metadata used only by platform-owned system Workers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemWorkerCompatibilityV1 {
    /// Fixed system Worker compatibility date.
    pub compatibility_date: String,
    /// Fixed system Worker compatibility flags.
    pub compatibility_flags: Vec<String>,
}

impl SystemWorkerCompatibilityV1 {
    pub(super) fn validate(&self) -> bool {
        compatibility_date_shape(&self.compatibility_date)
            && unique_nonempty(&self.compatibility_flags)
    }
}

fn compatibility_date_shape(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}
