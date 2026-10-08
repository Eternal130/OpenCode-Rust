//! models.dev catalog source shapes (wire structs for `api.json`, port of
//! `packages/core/src/models-dev.ts` `SourceProvider`/`SourceModel`).
//!
//! Unknown fields are ignored; known-absent fields default. Numbers arrive
//! as JS numbers — limits/costs deserialize `f64` and convert with explicit
//! truncation helpers.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// `api.json` root: `{ [providerID]: SourceProvider }`.
pub type SourceCatalog = BTreeMap<String, SourceProvider>;

/// A provider record in `api.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceProvider {
    #[serde(default)]
    pub api: Option<String>,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    pub id: String,
    #[serde(default)]
    pub npm: String,
    #[serde(default)]
    pub models: BTreeMap<String, SourceModel>,
}

/// A model record in `api.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub attachment: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_options: Vec<SourceReasoningOption>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub tool_call: bool,
    /// `boolean | string | { field }` — resolved through
    /// [`crate::provider_catalog::compat_from_interleaved`].
    #[serde(default)]
    pub interleaved: Option<Value>,
    #[serde(default)]
    pub cost: Option<SourceCost>,
    #[serde(default)]
    pub limit: SourceLimit,
    #[serde(default)]
    pub modalities: Option<SourceModalities>,
    #[serde(default)]
    pub experimental: Option<SourceExperimental>,
    #[serde(default)]
    pub status: Option<SourceStatus>,
    #[serde(default)]
    pub provider: Option<SourceModelProvider>,
}

/// Per-model provider override (`model.provider`).
#[derive(Debug, Clone, Deserialize)]
pub struct SourceModelProvider {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
}

/// `reasoning_options` entries: `effort`, `toggle`, or `budget_tokens`
/// (v2 `Variant.Support`, source side — effort values may be null).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceReasoningOption {
    Effort {
        #[serde(default)]
        values: Option<Vec<Option<String>>>,
    },
    Toggle,
    BudgetTokens {
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
}

/// Cost record (USD per million tokens).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourceCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub tiers: Vec<SourceCostTier>,
}

/// Context-tier cost override.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceCostTier {
    pub tier: SourceCostTierTag,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// `tier: { type: "context", size }`.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceCostTierTag {
    #[serde(rename = "type")]
    pub tier_type: String,
    pub size: f64,
}

/// Context-window limits.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourceLimit {
    #[serde(default)]
    pub context: Option<f64>,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
}

/// Input/output modalities.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceModalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Experimental mode blocks.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourceExperimental {
    #[serde(default)]
    pub modes: BTreeMap<String, SourceModeOptions>,
}

/// `experimental.modes[mode]`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourceModeOptions {
    #[serde(default)]
    pub cost: Option<SourceCost>,
    #[serde(default)]
    pub provider: Option<SourceModeProvider>,
}

/// `experimental.modes[mode].provider` — per-mode request overlay.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourceModeProvider {
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
}

/// Catalog model status (`alpha | beta | deprecated`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Alpha,
    Beta,
    Deprecated,
}

/// Parse an `api.json` body into source providers.
pub fn parse_catalog(text: &str) -> Result<SourceCatalog, String> {
    serde_json::from_str::<SourceCatalog>(text)
        .map_err(|error| format!("invalid catalog JSON: {error}"))
}
