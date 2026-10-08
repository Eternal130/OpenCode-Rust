//! `config/reference.ts` — `Config.Reference`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `Config.Reference.Git`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Git {
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// `Config.Reference.Local`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Local {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

/// `Config.Reference.Entry` — path string or explicit shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Entry {
    /// Bare path or spec string.
    Path(String),
    /// Git repository.
    Git(Git),
    /// Local directory.
    Local(Local),
}

/// `Config.Reference`.
pub type Reference = BTreeMap<String, Entry>;
