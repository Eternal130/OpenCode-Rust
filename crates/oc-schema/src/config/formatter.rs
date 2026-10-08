//! `config/formatter.ts` — `Config.Formatter`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `Config.Formatter.Entry`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}

/// `Config.Formatter` — `boolean | Record<string, Entry>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Formatter {
    /// Enable/disable all built-in formatters.
    All(bool),
    /// Per-formatter overrides.
    Entries(BTreeMap<String, Entry>),
}
