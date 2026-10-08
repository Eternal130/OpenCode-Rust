//! `config/provider.ts` — `Config.Provider` and model overlays.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::Num;
use crate::id::{ModelId, ModelVariantId, ProviderId};
use crate::provider::{Compaction, Transport};

/// `Config.Provider.Settings` — known keys plus arbitrary values
/// (number | boolean | string | null | JSON).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<crate::provider::Timeout>,
    #[serde(rename = "headerTimeout", default, skip_serializing_if = "Option::is_none")]
    pub header_timeout: Option<crate::provider::Timeout>,
    #[serde(rename = "chunkTimeout", default, skip_serializing_if = "Option::is_none")]
    pub chunk_timeout: Option<crate::provider::Timeout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Option<Value>>,
}

/// `Config.Model.Settings`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    #[serde(flatten)]
    pub rest: BTreeMap<String, Option<Value>>,
}

/// `Config.Provider.Request` — header/body overlay for agents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
}

/// `Config.Model.Cost.Cache`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CostCache {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<crate::common::UsdPerMillionTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<crate::common::UsdPerMillionTokens>,
}

/// `Config.Model.Cost`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<CostTier>,
    pub input: crate::common::UsdPerMillionTokens,
    pub output: crate::common::UsdPerMillionTokens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CostCache>,
}

/// Context-tier tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CostTier {
    #[serde(rename = "type")]
    pub tier_type: CostTierType,
    pub size: i64,
}

/// Always `context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum CostTierType {
    #[serde(rename = "context")]
    Context,
}

/// `Config.Model.Limit`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Limit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<i64>,
}

/// `Config.Model.Capabilities` — partial; unset falls back to base then
/// defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Capabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<String>>,
}

/// `Config.Model` — per-model overlay.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Model {
    #[serde(rename = "modelID", default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<ModelId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<crate::model::Compatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Capabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<ModelVariant>>,
    /// A single cost entry or a list of tiered entries.
    #[serde(default, deserialize_with = "cost_one_or_many::deserialize")]
    pub cost: Option<CostList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
}

/// One-or-many `cost` value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum CostList {
    One(Box<Cost>),
    Many(Vec<Cost>),
}

/// Serde helper: `Cost | Array<Cost>`.
mod cost_one_or_many {
    use super::CostList;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<CostList>, D::Error> {
        Option::<CostList>::deserialize(deserializer)
    }
}

/// `Config.Model` variant overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelVariant {
    pub id: ModelVariantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
}

/// `Config.Provider`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Provider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<ProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<BTreeMap<String, Model>>,
}

/// `Num` re-export for cost math.
#[allow(dead_code)]
type _Num = Num;
