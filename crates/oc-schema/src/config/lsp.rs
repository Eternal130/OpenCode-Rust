//! `config/lsp.ts` — `Config.LSP`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `Config.LSP.Disabled` — `{disabled: true}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Disabled {
    pub disabled: bool,
}

/// `Config.LSP.Server`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Server {
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initialization: Option<BTreeMap<String, Value>>,
}

/// `Config.LSP.Entry` — disabled marker or server config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Entry {
    Disabled(Disabled),
    Server(Server),
}

/// `Config.LSP` — `boolean | Record<string, Entry>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Lsp {
    /// Enable/disable all built-in servers.
    All(bool),
    /// Per-server overrides.
    Entries(BTreeMap<String, Entry>),
}
