//! `config/policy.ts` — `Config.Policy`.

use serde::{Deserialize, Serialize};

/// `Config.Policy.Effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
}

/// `Config.Policy`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Policy {
    pub action: Action,
    pub resource: String,
    pub effect: Effect,
}

/// `Config.Policy.action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    ProviderUse,
    Permission,
}
