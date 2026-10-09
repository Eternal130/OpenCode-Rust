//! oc-client error model (v2 `ClientError` + declared-error envelope parity).
//!
//! Declared API errors keep every envelope field (v2 "spread"), expose the
//! `_tag` as `name` and resolve `message` from `body.message` with
//! `body.data.message` as fallback.

use std::collections::BTreeMap;

/// Errors produced by the oc-client transport, SSE decoder and encoders.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// A declared error status whose JSON body parsed. The full envelope is
    /// kept in [`ClientError::Api::fields`] (v2 spreads the fields onto the
    /// thrown Error), `name` mirrors `_tag`, `message` mirrors
    /// `body.message ?? body.data?.message`.
    #[error("api error {status} {name}: {message}")]
    Api { status: u16, name: String, message: String, fields: BTreeMap<String, serde_json::Value> },
    /// A status the operation does not declare (v2 `UnexpectedStatus`).
    #[error("unexpected status {status}")]
    UnexpectedStatus { status: u16, body: String },
    /// Response/SSE content-type mismatch (v2 `UnsupportedContentType`).
    #[error("unsupported content type: {0}")]
    UnsupportedContentType(String),
    /// Body was not decodable JSON / SSE frame was not valid JSON
    /// (v2 `MalformedResponse`).
    #[error("malformed response: {0}")]
    MalformedResponse(String),
    /// SSE buffering exceeded the 16 MiB cap (v2 `SseEventTooLarge`).
    #[error("sse event too large: buffered bytes exceed {0}")]
    SseEventTooLarge(usize),
    /// reqwest transport failure (v2 `Transport`).
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    /// The configured base URL cannot be parsed.
    #[error("invalid base url: {0:?}")]
    InvalidBaseUrl(String),
    /// Serializing a request body to JSON failed.
    #[error("request encoding error: {0}")]
    Encoding(String),
}

impl ClientError {
    /// `_tag` of a declared API error, if this is one.
    #[must_use]
    pub fn api_name(&self) -> Option<&str> {
        match self {
            Self::Api { name, .. } => Some(name),
            _ => None,
        }
    }

    /// HTTP status of an API or unexpected-status error.
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } | Self::UnexpectedStatus { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Spread envelope field of a declared API error (v2 parity).
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&serde_json::Value> {
        match self {
            Self::Api { fields, .. } => fields.get(key),
            _ => None,
        }
    }
}

/// Builds the declared-error variant from a parsed JSON envelope
/// (server-deep-map §6: `{"_tag": "...", ...camelCase fields}`).
pub(crate) fn declared(status: u16, body: serde_json::Value) -> ClientError {
    let mut fields = BTreeMap::new();
    if let serde_json::Value::Object(map) = body {
        for (k, v) in map {
            fields.insert(k, v);
        }
    }
    let name = fields.get("_tag").and_then(serde_json::Value::as_str).unwrap_or("Error").to_owned();
    let message = fields
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            fields
                .get("data")
                .and_then(|d| d.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default();
    ClientError::Api { status, name, message, fields }
}
