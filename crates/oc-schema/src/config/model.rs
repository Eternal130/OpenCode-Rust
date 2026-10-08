//! `config/model.ts` — `Config.ModelSelection` (short string or explicit).

use serde::{Deserialize, Serialize};

use crate::id::{ModelId, ModelVariantId, ProviderId};

/// `Config.ModelSelection` — encoded as the explicit struct; the short string
/// form (`providerID/modelID[#variant]`) is accepted on deserialize, matching
/// v2's decode-only transform (encode is passthrough on the explicit shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ModelSelection {
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    pub model: ModelId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<ModelVariantId>,
}

impl ModelSelection {
    /// Parses `providerID/modelID` or `providerID/modelID#variant`
    /// (`^[^/#]+/[^#]+(?:#[^#]+)?$`).
    pub fn parse(input: &str) -> Result<Self, String> {
        let parsed = crate::model::Ref::parse(input).map_err(|error| error.to_string())?;
        Ok(Self { provider_id: parsed.provider_id, model: parsed.id, variant: parsed.variant })
    }
}

impl<'de> Deserialize<'de> for ModelSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(rename = "providerID")]
            provider_id: ProviderId,
            model: ModelId,
            #[serde(default)]
            variant: Option<ModelVariantId>,
        }
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(short) => {
                ModelSelection::parse(&short).map_err(serde::de::Error::custom)
            }
            serde_json::Value::Object(_) => {
                let raw: Raw = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(ModelSelection {
                    provider_id: raw.provider_id,
                    model: raw.model,
                    variant: raw.variant,
                })
            }
            other => Err(serde::de::Error::custom(format!(
                "expected model selection string or object, got {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_explicit_forms() {
        let from_short: ModelSelection =
            serde_json::from_str("\"anthropic/claude-opus-4-6#max\"").unwrap();
        assert_eq!(from_short.provider_id.as_str(), "anthropic");
        assert_eq!(from_short.model.as_str(), "claude-opus-4-6");
        assert_eq!(from_short.variant.as_ref().unwrap().as_str(), "max");
        // Encoded form is always the explicit struct.
        assert_eq!(
            serde_json::to_value(&from_short).unwrap(),
            serde_json::json!({
                "providerID": "anthropic",
                "model": "claude-opus-4-6",
                "variant": "max"
            })
        );
        let from_explicit: ModelSelection = serde_json::from_value(serde_json::json!({
            "providerID": "opencode",
            "model": "grok-code"
        }))
        .unwrap();
        assert_eq!(from_explicit.provider_id.as_str(), "opencode");
        assert!(serde_json::from_str::<ModelSelection>("\"bad\"").is_err());
    }
}
