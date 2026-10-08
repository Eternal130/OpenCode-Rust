//! `config/warming.ts` — `Config.Warming`.

use serde::{Deserialize, Serialize};

/// `Config.Warming.Info`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    /// Prompt sent for keep-alive requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Idle time between keep-alive requests (default "4 minutes").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    /// Time after the last active request to keep a session warm (default
    /// "30 minutes").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
}

/// `Config.Warming` — `boolean | Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Warming {
    All(bool),
    Info(Info),
}
