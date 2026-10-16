//! Media protocol request bodies (port of the `MediaProtocol` body half of
//! `route/media-protocol.ts:23-53`).
//!
//! A media body is JSON, multipart form data, or raw bytes, each optionally
//! carrying query parameters appended to the endpoint URL before the
//! route and caller `http.query` overlays. Multipart forms are represented
//! as ordered fields (TS `FormData` is insertion-ordered); the transport
//! layer encodes them.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::schema::options::{merge_json_records, JsonRecord};
use crate::schema::AiError;

/// Array values become repeated parameters (`keyterm=a&keyterm=b`)
/// (media-protocol.ts:24).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query(pub BTreeMap<String, QueryValue>);

/// One query parameter: a scalar or a repeated list.
#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    One(String),
    Many(Vec<String>),
}

impl Query {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), QueryValue::One(value.into()));
    }
}

/// The request body a media protocol lowers to (media-protocol.ts:27-34).
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Json {
        value: JsonRecord,
        /// Appended to the endpoint URL before route/caller `http.query`.
        query: Option<Query>,
    },
    Multipart(Form),
    Binary {
        value: Vec<u8>,
        content_type: String,
        query: Option<Query>,
    },
}

/// JS-number parity for wire bodies: TS `JSON.stringify` renders integral
/// numbers without a decimal point, so `11.0` must cross the wire as `11`
/// (see `protocols::shared::number`, same policy applied deeply here).
#[must_use]
pub fn normalize_numbers(value: &Value) -> Value {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64() {
                if float.is_finite()
                    && float.fract() == 0.0
                    && float.abs() < 9_007_199_254_740_992.0
                {
                    Value::from(float as i64)
                } else {
                    value.clone()
                }
            } else {
                value.clone()
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_numbers).collect()),
        Value::Object(fields) => Value::Object(
            fields.iter().map(|(key, value)| (key.clone(), normalize_numbers(value))).collect(),
        ),
        _ => value.clone(),
    }
}

impl Body {
    /// `MediaProtocol.json` (media-protocol.ts:37-41).
    #[must_use]
    pub fn json(value: JsonRecord) -> Self {
        Self::Json {
            value: normalize_numbers(&Value::Object(value))
                .as_object()
                .cloned()
                .unwrap_or_default(),
            query: None,
        }
    }

    /// `MediaProtocol.json(body, query)`.
    #[must_use]
    pub fn json_with_query(value: JsonRecord, query: Query) -> Self {
        Self::Json {
            value: normalize_numbers(&Value::Object(value))
                .as_object()
                .cloned()
                .unwrap_or_default(),
            query: Some(query),
        }
    }

    /// `MediaProtocol.multipart` (media-protocol.ts:42).
    #[must_use]
    pub fn multipart(value: Form) -> Self {
        Self::Multipart(value)
    }

    /// `MediaProtocol.binary` (media-protocol.ts:43-47).
    #[must_use]
    pub fn binary(value: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self::Binary { value, content_type: content_type.into(), query: None }
    }

    /// `MediaProtocol.binary(body, type, query)`.
    #[must_use]
    pub fn binary_with_query(
        value: Vec<u8>,
        content_type: impl Into<String>,
        query: Query,
    ) -> Self {
        Self::Binary { value, content_type: content_type.into(), query: Some(query) }
    }

    /// The `output_format` a JSON body carries, for protocols whose
    /// responses omit it (openai-images `requestedFormat`).
    #[must_use]
    pub fn json_field(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Json { value, .. } => value.get(key),
            _ => None,
        }
    }
}

/// Ordered multipart form representation. TS `FormData` preserves insertion
/// order; providers with positional fields (BFL `input_image_2`…) rely on
/// named fields only, so a `Vec` of pairs is faithful.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Form {
    pub fields: Vec<(String, FormValue)>,
}

/// One multipart part.
#[derive(Debug, Clone, PartialEq)]
pub enum FormValue {
    /// A text field: strings pass as-is, other values as JSON
    /// (`MediaInput.appendFields`, media-input.ts:78-93).
    Field(String),
    /// A file part (`MediaInput.blob`) with its filename and media type.
    File { filename: String, media_type: String, data: Vec<u8> },
}

impl Form {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append_field(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.fields.push((key.into(), FormValue::Field(value.into())));
    }

    pub fn append_file(
        &mut self,
        key: impl Into<String>,
        filename: impl Into<String>,
        media_type: impl Into<String>,
        data: Vec<u8>,
    ) {
        self.fields.push((
            key.into(),
            FormValue::File { filename: filename.into(), media_type: media_type.into(), data },
        ));
    }

    /// The text fields as a JSON record (golden-test view).
    #[must_use]
    pub fn fields_json(&self) -> JsonRecord {
        let mut record = JsonRecord::new();
        for (key, value) in &self.fields {
            if let FormValue::Field(text) = value {
                let parsed: Value =
                    serde_json::from_str(text).unwrap_or(Value::String(text.clone()));
                record.insert(key.clone(), parsed);
            }
        }
        record
    }

    /// The file parts as `(key, filename, mediaType)` triples (golden-test view).
    #[must_use]
    pub fn files(&self) -> Vec<(&str, &str, &str)> {
        self.fields
            .iter()
            .filter_map(|(key, value)| match value {
                FormValue::File { filename, media_type, .. } => {
                    Some((key.as_str(), filename.as_str(), media_type.as_str()))
                }
                _ => None,
            })
            .collect()
    }
}

/// Merge a JSON record from optional parts (`mergeJsonRecords` semantics:
/// deep merge with later items winning, `undefined`/`None` skipped).
#[must_use]
pub fn merge_records(items: &[Option<&JsonRecord>]) -> Option<JsonRecord> {
    merge_json_records(items)
}

/// A record from literal key/value pairs (test + lowering convenience).
/// `None` — and JSON `null`, the TS `undefined` equivalent — skip the
/// entry so bodies never carry null-valued keys the TS encoder omits.
#[must_use]
pub fn record<const N: usize>(entries: [(&str, Option<Value>); N]) -> Option<JsonRecord> {
    let mut out = JsonRecord::new();
    let mut any = false;
    for (key, value) in entries {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            out.insert(key.to_string(), value);
            any = true;
        }
    }
    if any {
        Some(out)
    } else {
        None
    }
}

/// `Some(value)` as JSON, `None` as JSON null — the optional-field lowering
/// idiom for wire bodies (TS spreads `undefined` to the same effect).
#[must_use]
pub fn json_opt<T: serde::Serialize>(value: Option<T>) -> Value {
    match value {
        Some(value) => serde_json::to_value(value).unwrap_or(Value::Null),
        None => Value::Null,
    }
}

/// `true` as JSON or JSON null — the opt-in boolean lowering idiom
/// (route/media.ts:419-423: protocols reject opt-in booleans with
/// `=== true` instead of listing them unsupported).
#[must_use]
pub fn json_flag(condition: bool) -> Value {
    if condition {
        Value::Bool(true)
    } else {
        Value::Null
    }
}

/// The invalid-base64 error `MediaInput.inlineBytes` reports
/// (media-input.ts:9-16).
pub fn invalid_base64(route: &str) -> AiError {
    crate::protocols::shared::invalid_request(format!("{route} media contains invalid base64 data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_records_deep_with_later_winning() {
        let a = serde_json::from_value::<JsonRecord>(json!({"x": {"y": 1, "z": 2}, "keep": true}))
            .unwrap();
        let b = serde_json::from_value::<JsonRecord>(json!({"x": {"y": 3}})).unwrap();
        let merged = merge_records(&[Some(&a), Some(&b)]).unwrap();
        assert_eq!(merged["x"]["y"], json!(3));
        assert_eq!(merged["x"]["z"], json!(2));
        assert_eq!(merged["keep"], json!(true));
        assert!(merge_records(&[None, None]).is_none());
    }

    #[test]
    fn record_skips_none_entries() {
        let merged = record([("a", Some(json!(1))), ("b", None)]).unwrap();
        assert_eq!(merged.len(), 1);
        assert!(record([("a", None)]).is_none());
    }

    #[test]
    fn form_fields_and_files_view() {
        let mut form = Form::new();
        form.append_field("model", "x");
        form.append_field("nums", json!([1, 2]).to_string());
        form.append_file("file", "audio.mp3", "audio/mpeg", vec![1, 2, 3]);
        assert_eq!(form.fields_json()["model"], json!("x"));
        assert_eq!(form.fields_json()["nums"], json!([1, 2]));
        assert_eq!(form.files(), vec![("file", "audio.mp3", "audio/mpeg")]);
    }
}
