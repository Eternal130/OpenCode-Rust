//! Port of `model.ts` and `provider.ts`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{PositiveInt, Record, UsdPerMillionTokens};
use crate::id::{ModelId, ModelVariantId, ProviderId};
use crate::permission;
use crate::provider;

/// `Model.Ref` — `{ id, providerID, variant? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ref {
    pub id: ModelId,
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<ModelVariantId>,
}

/// Error produced by [`Ref::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Invalid model reference: {0}")]
pub struct InvalidModelRef(pub String);

impl Ref {
    /// `Model.Ref.parse` — accepts `providerID/modelID` and
    /// `providerID/modelID#variant`.
    pub fn parse(input: &str) -> Result<Self, InvalidModelRef> {
        let provider_end = input.find('/');
        let provider_end = match provider_end {
            Some(index) if index > 0 => index,
            _ => return Err(InvalidModelRef(input.to_string())),
        };
        let provider_id = &input[..provider_end];
        let variant_start = input[provider_end + 1..].find('#').map(|i| provider_end + 1 + i);
        let id = match variant_start {
            Some(start) => &input[provider_end + 1..start],
            None => &input[provider_end + 1..],
        };
        let variant = variant_start.map(|start| &input[start + 1..]);
        if id.is_empty()
            || provider_id.contains('#')
            || variant.is_some_and(|value| value.is_empty() || value.contains('#'))
        {
            return Err(InvalidModelRef(input.to_string()));
        }
        Ok(Self {
            provider_id: ProviderId::parse(provider_id),
            id: ModelId::parse(id),
            variant: variant.map(ModelVariantId::parse),
        })
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.variant {
            Some(variant) => write!(f, "{}/{}#{}", self.provider_id, self.id, variant),
            None => write!(f, "{}/{}", self.provider_id, self.id),
        }
    }
}

/// `Model.Compatibility`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Compatibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_field: Option<String>,
    /// Require every assistant message to include its reasoning field, even
    /// when empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_reasoning: Option<bool>,
    #[serde(rename = "maxTokensField", default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_field: Option<MaxTokensField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_finish_reason: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_assistant_after_tool: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache_key: Option<bool>,
}

/// `Model.MaxTokensField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    MaxCompletionTokens,
    MaxTokens,
}

/// `Model.Capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Capabilities {
    pub tools: bool,
    pub input: Vec<String>,
    pub output: Vec<String>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            tools: true,
            input: vec!["text".into(), "image".into()],
            output: vec!["text".into()],
        }
    }
}

/// `Model.Cost` — context-tier pricing per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<CostTier>,
    pub input: UsdPerMillionTokens,
    pub output: UsdPerMillionTokens,
    pub cache: CostCache,
}

/// Context-tier tag of [`Cost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CostTier {
    #[serde(rename = "type")]
    pub tier_type: CostTierType,
    pub size: i64,
}

/// Tag of [`CostTier`] — always `context` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum CostTierType {
    #[serde(rename = "context")]
    Context,
}

/// Cache pricing of [`Cost`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CostCache {
    pub read: UsdPerMillionTokens,
    pub write: UsdPerMillionTokens,
}

/// `Model.Variant`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Variant {
    pub id: ModelVariantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<provider::Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
}

/// `Model.Settings` — `compaction` plus arbitrary provider options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<provider::Compaction>,
    /// Provider packages may define arbitrary model-level options beyond
    /// The shared compaction policy.
    #[serde(flatten)]
    pub rest: Record,
}

/// `Model.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: ModelId,
    #[serde(rename = "modelID")]
    pub model_id: ModelId,
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<ProviderId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<Compatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BTreeMap<String, Value>>,
    pub capabilities: Capabilities,
    pub variants: Vec<Variant>,
    pub time: ModelTime,
    pub cost: Vec<Cost>,
    pub status: Status,
    pub enabled: bool,
    pub limit: ModelLimit,
}

/// Released timestamp of [`Info`] (epoch seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelTime {
    pub released: crate::common::Num,
}

/// Context window limits of [`Info`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelLimit {
    pub context: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    pub output: i64,
}

/// `Model.Info.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Alpha,
    Beta,
    Deprecated,
    Active,
}

impl Info {
    /// `Model.Info.default` — a permissive placeholder model entry.
    pub fn default_for(provider_id: ProviderId, id: ModelId) -> Self {
        Self {
            id: id.clone(),
            model_id: id.clone(),
            provider_id,
            canonical: None,
            family: None,
            name: id.0,
            compatibility: None,
            package: None,
            settings: None,
            headers: None,
            body: None,
            capabilities: Capabilities::default(),
            variants: vec![],
            time: ModelTime { released: crate::common::Num(0.0) },
            cost: vec![],
            status: Status::Active,
            enabled: true,
            limit: ModelLimit { context: 200_000, input: None, output: 32_000 },
        }
    }
}

/// Positive-integer re-export used by agent steps.
#[allow(dead_code)]
type _Steps = PositiveInt;

/// `permission` re-export for ruleset construction on agents.
#[allow(dead_code)]
type _Permission = permission::Ruleset;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_parse_and_wire() {
        let parsed = Ref::parse("anthropic/claude-opus-4-6#max").unwrap();
        assert_eq!(parsed.provider_id.as_str(), "anthropic");
        assert_eq!(parsed.id.as_str(), "claude-opus-4-6");
        assert_eq!(parsed.variant.as_ref().unwrap().as_str(), "max");
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::json!({
                "id": "claude-opus-4-6",
                "providerID": "anthropic",
                "variant": "max"
            })
        );
        assert_eq!(Ref::parse("anthropic").unwrap_err().0, "anthropic");
        assert!(Ref::parse("anthropic/").is_err());
        assert!(Ref::parse("a/b#").is_err());
        assert!(Ref::parse("a/b#c#d").is_err());
    }

    #[test]
    fn ref_without_variant_round_trip() {
        let parsed = Ref::parse("opencode/grok-code").unwrap();
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::json!({"id": "grok-code", "providerID": "opencode"})
        );
    }

    #[test]
    fn model_info_default() {
        let info = Info::default_for(ProviderId::anthropic(), ModelId::parse("claude"));
        assert!(info.enabled);
        assert_eq!(info.status, Status::Active);
        assert_eq!(info.limit.context, 200_000);
    }
}
