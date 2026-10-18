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
pub enum Action {
    /// v2's wire literal is `provider.use` (config/policy.ts:8); the
    /// kebab-case `provider-use` spelling stays accepted as an alias
    /// for pre-literal encoded documents (the v1 migration products
    /// and older normalize output).
    #[serde(rename = "provider.use", alias = "provider-use")]
    ProviderUse,
    #[serde(rename = "permission")]
    Permission,
}
