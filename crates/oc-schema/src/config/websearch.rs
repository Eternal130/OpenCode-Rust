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

/// `ConfigWebSearch.Selection` — `false | Info` (ref websearch.ts:16
/// `Schema.Union([Schema.Literal(false), Info])`; `true` is a schema
/// error, G-160①).
#[derive(Debug, Clone, PartialEq)]
pub enum WebSearchSelection {
    Disabled,
    Info(Info),
}

impl Serialize for WebSearchSelection {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            WebSearchSelection::Disabled => serializer.serialize_bool(false),
            WebSearchSelection::Info(info) => info.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for WebSearchSelection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value == serde_json::Value::Bool(false) {
            return Ok(WebSearchSelection::Disabled);
        }
        if value.is_object() {
            if let Ok(info) = serde_json::from_value::<Info>(value) {
                return Ok(WebSearchSelection::Info(info));
            }
        }
        Err(serde::de::Error::custom(
            "invalid `websearch` value: expected a websearch config object or the literal `false`",
        ))
    }
}

impl schemars::JsonSchema for WebSearchSelection {
    fn schema_name() -> String {
        "ConfigWebSearch.Selection".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, Schema, SchemaObject, SubschemaValidation};
        let info: Schema = gen.subschema_for::<Info>().into();
        let literal_false = Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::Boolean.into()),
            ..Default::default()
        });
        Schema::Object(SchemaObject {
            subschemas: Some(Box::new(SubschemaValidation {
                one_of: Some(vec![info, literal_false]),
                ..Default::default()
            })),
            ..Default::default()
        })
    }
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
        assert_eq!(disabled, WebSearchSelection::Disabled);
    }

    /// G-160①: `true` is a schema error (ref union only admits literal `false`).
    #[test]
    fn selection_rejects_true() {
        let err =
            serde_json::from_value::<WebSearchSelection>(serde_json::json!(true)).unwrap_err();
        assert!(err.to_string().contains("literal `false`"), "unexpected error: {err}");
    }
}
