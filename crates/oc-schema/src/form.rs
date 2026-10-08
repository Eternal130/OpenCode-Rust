//! Port of `form.ts`.

use serde::{Deserialize, Serialize};

use crate::common::{NonNegativeInt, Record};
use crate::id::FormId;

/// `Form.Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Option_ {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `Form.When` — one visibility condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct When {
    pub key: String,
    pub op: WhenOp,
    pub value: WhenValue,
}

/// `Form.When.op`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WhenOp {
    Eq,
    Neq,
}

/// `Form.When.value` — scalar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum WhenValue {
    String(String),
    Number(f64),
    Bool(bool),
}

/// `Form.Field` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Field {
    /// Free-form text.
    String {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Vec<When>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<StringFormat>,
        #[serde(rename = "minLength", default, skip_serializing_if = "Option::is_none")]
        min_length: Option<NonNegativeInt>,
        #[serde(rename = "maxLength", default, skip_serializing_if = "Option::is_none")]
        max_length: Option<NonNegativeInt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<Vec<Option_>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom: Option<bool>,
    },
    /// Floating-point number.
    Number {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Vec<When>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    /// Integer number.
    Integer {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Vec<When>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    /// Boolean toggle.
    Boolean {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Vec<When>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
    /// Multi-select from options.
    Multiselect {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hidden: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        when: Option<Vec<When>>,
        options: Vec<Option_>,
        #[serde(rename = "minItems", default, skip_serializing_if = "Option::is_none")]
        min_items: Option<NonNegativeInt>,
        #[serde(rename = "maxItems", default, skip_serializing_if = "Option::is_none")]
        max_items: Option<NonNegativeInt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Vec<String>>,
    },
    /// External URL step.
    External {
        key: String,
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

/// `Form.StringField.format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StringFormat {
    Email,
    Uri,
    Date,
    DateTime,
}

/// `Form.Fields` — non-empty list of fields.
pub type Fields = Vec<Field>;

/// `Form.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: FormId,
    /// Session owner; plain string because MCP elicitation uses the
    /// `"global"` sentinel.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    pub fields: Fields,
}

/// `Form.Value` — scalar or string list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum FormValue {
    String(String),
    Number(f64),
    Bool(bool),
    Strings(Vec<String>),
}

/// `Form.Answer`.
pub type Answer = std::collections::BTreeMap<String, FormValue>;

/// `Form.State` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum State {
    /// Awaiting a reply.
    Pending,
    /// Answered.
    Answered { answer: Answer },
    /// Cancelled; `message` explains why nobody answered.
    Cancelled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

/// `Form.Detail`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Detail {
    pub id: FormId,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    pub fields: Fields,
    pub state: State,
}

/// `Form.Reply`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Reply {
    pub answer: Answer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_field_wire() {
        let value = serde_json::json!({
            "type": "string",
            "key": "api_key",
            "title": "API key",
            "format": "uri",
            "required": true
        });
        let parsed: Field = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Field::String { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn state_wire() {
        let value = serde_json::json!({"status": "answered", "answer": {"key": "v"}});
        let parsed: State = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }
}
