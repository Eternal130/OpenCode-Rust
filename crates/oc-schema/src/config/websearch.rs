//! `config/websearch.ts` — `ConfigWebSearch`.

use serde::{Deserialize, Serialize};

/// `ConfigWebSearch.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    /// `"random"` rotates through available providers; otherwise a provider id.
    pub provider: WebSearchProvider,
}

/// Provider selection: `"random"` or a concrete id. Hand-written serde since
/// untagged unions cannot split on a literal string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSearchProvider {
    Random,
    Id(String),
}

impl Serialize for WebSearchProvider {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            WebSearchProvider::Random => serializer.serialize_str("random"),
            WebSearchProvider::Id(id) => serializer.serialize_str(id),
        }
    }
}

impl<'de> Deserialize<'de> for WebSearchProvider {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "random" {
            Ok(WebSearchProvider::Random)
        } else {
            Ok(WebSearchProvider::Id(value))
        }
    }
}

impl schemars::JsonSchema for WebSearchProvider {
    fn schema_name() -> String {
        "ConfigWebSearch.Provider".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        serde_json::from_value(serde_json::json!({"type": "string"})).expect("static schema")
    }
}

/// `ConfigWebSearch.Selection` — `false | Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum WebSearchSelection {
    Disabled(bool),
    Info(Info),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_wire() {
        assert_eq!(
            serde_json::to_value(WebSearchProvider::Random).unwrap(),
            serde_json::json!("random")
        );
        let parsed: WebSearchProvider = serde_json::from_str("\"tavily\"").unwrap();
        assert_eq!(parsed, WebSearchProvider::Id("tavily".into()));
    }

    #[test]
    fn selection_wire() {
        let selection: WebSearchSelection =
            serde_json::from_value(serde_json::json!({"provider": "random"})).unwrap();
        assert!(matches!(selection, WebSearchSelection::Info(_)));
        let disabled: WebSearchSelection =
            serde_json::from_value(serde_json::json!(false)).unwrap();
        assert_eq!(disabled, WebSearchSelection::Disabled(false));
    }
}
