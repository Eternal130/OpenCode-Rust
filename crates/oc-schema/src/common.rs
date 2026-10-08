//! Small shared schemas: `money.ts`, `token-usage.ts`, `llm.ts`,
//! `session-error.ts`, and the `RelativePath`/`AbsolutePath` brands from
//! `schema.ts`.

use std::collections::BTreeMap;
use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `RelativePath` — branded string.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct RelativePath(pub String);

impl RelativePath {
    /// Wraps a path string.
    pub fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `AbsolutePath` — branded string.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct AbsolutePath(pub String);

impl AbsolutePath {
    /// Wraps a path string.
    pub fn parse(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A JSON number that round-trips with JavaScript semantics: whole values
/// serialize as integers (`500`, not `500.0`), matching v2's `Schema.Finite`
/// output. Use for any field whose v2 type is a plain number.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Num(pub f64);

impl Serialize for Num {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = self.0;
        if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
            if value < 0.0 {
                serializer.serialize_i64(value as i64)
            } else {
                serializer.serialize_u64(value as u64)
            }
        } else {
            serializer.serialize_f64(value)
        }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let number = serde_json::Number::deserialize(deserializer)?;
        match number.as_f64() {
            Some(value) => Ok(Num(value)),
            None => Err(serde::de::Error::custom("expected a JSON number")),
        }
    }
}

impl schemars::JsonSchema for Num {
    fn schema_name() -> String {
        "Number".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        serde_json::from_value(serde_json::json!({"type": "number"})).expect("static number schema")
    }
}

impl From<f64> for Num {
    fn from(value: f64) -> Self {
        Num(value)
    }
}

impl From<Num> for f64 {
    fn from(value: Num) -> Self {
        value.0
    }
}

/// `Money.USD` — finite float, transparent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct Usd(pub Num);

impl Usd {
    /// `Money.USD.zero`.
    pub fn zero() -> Self {
        Self(Num(0.0))
    }
}

/// `Money.USDPerMillionTokens` — finite float, transparent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct UsdPerMillionTokens(pub Num);

impl UsdPerMillionTokens {
    /// `Money.USDPerMillionTokens.zero`.
    pub fn zero() -> Self {
        Self(Num(0.0))
    }
}

impl Add for Usd {
    type Output = Usd;
    fn add(self, rhs: Usd) -> Usd {
        Usd(Num(self.0 .0 + rhs.0 .0))
    }
}

impl AddAssign for Usd {
    fn add_assign(&mut self, rhs: Usd) {
        self.0 .0 += rhs.0 .0;
    }
}

/// `TokenUsage.Info`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: Num,
    pub output: Num,
    pub reasoning: Num,
    pub cache: TokenUsageCache,
}

/// Cache breakdown of [`TokenUsage`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageCache {
    pub read: Num,
    pub write: Num,
}

impl TokenUsage {
    /// `TokenUsage.total`.
    pub fn total(&self) -> f64 {
        self.input.0 + self.output.0 + self.reasoning.0 + self.cache.read.0 + self.cache.write.0
    }
}

/// `LLM.FinishReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
    Unknown,
}

/// `Session.StructuredError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<SessionErrorResponse>,
}

/// The `response` field of [`SessionError`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionErrorResponse {
    pub body: String,
}

/// JSON object map used for `Record<string, unknown>` fields (provider state,
/// message metadata, tool metadata, …).
pub type Record = BTreeMap<String, Value>;

/// `NonNegativeInt` — integer >= 0.
pub type NonNegativeInt = u64;

/// `PositiveInt` — integer > 0.
pub type PositiveInt = u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_total() {
        let usage = TokenUsage {
            input: Num(100.0),
            output: Num(20.0),
            reasoning: Num(5.0),
            cache: TokenUsageCache { read: Num(10.0), write: Num(15.0) },
        };
        assert_eq!(usage.total(), 150.0);
    }

    #[test]
    fn finish_reason_wire_names() {
        assert_eq!(serde_json::to_string(&FinishReason::Stop).unwrap(), "\"stop\"");
        assert_eq!(serde_json::to_string(&FinishReason::ToolCalls).unwrap(), "\"tool-calls\"");
        assert_eq!(
            serde_json::to_string(&FinishReason::ContentFilter).unwrap(),
            "\"content-filter\""
        );
        let back: FinishReason = serde_json::from_str("\"length\"").unwrap();
        assert_eq!(back, FinishReason::Length);
    }

    #[test]
    fn session_error_wire() {
        let error = SessionError {
            error_type: "APIError".to_string(),
            message: "boom".to_string(),
            status: Some(429),
            response: None,
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json, serde_json::json!({"type": "APIError", "message": "boom", "status": 429}));
        let back: SessionError = serde_json::from_value(json).unwrap();
        assert_eq!(back, error);
    }

    #[test]
    fn usd_transparent() {
        assert_eq!(serde_json::to_string(&Usd(Num(0.00123))).unwrap(), "0.00123");
        assert_eq!(serde_json::to_string(&Usd(Num(500.0))).unwrap(), "500");
        let back: Usd = serde_json::from_str("1.5").unwrap();
        assert_eq!(back, Usd(Num(1.5)));
    }
}
