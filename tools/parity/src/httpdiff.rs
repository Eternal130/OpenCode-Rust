//! Same-request-vs-two-bases comparison: run a request against reference and
//! port base URLs, normalize both bodies, deep-diff, report the first
//! divergence path.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;

use crate::minih;
use crate::normalize::{normalize_value, NormEnv};

/// First divergence between two normalized values, as a JSON path like
/// `$.urls[0]` or `$.paths.tmp`.
#[derive(Debug)]
pub struct Divergence {
    pub path: String,
    pub left: Value,
    pub right: Value,
}

impl std::fmt::Display for Divergence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "divergence at {}: reference={} port={}", self.path, self.left, self.right)
    }
}

fn diff_at(path: String, left: &Value, right: &Value) -> Divergence {
    Divergence { path, left: left.clone(), right: right.clone() }
}

/// Depth-first first divergence. Objects compare unordered (key order is not
/// semantic), arrays compare order-sensitively, scalars compare exactly
/// (integer vs float distinction included). Paths look like `$.urls[0]`.
#[must_use]
pub fn deep_diff(left: &Value, right: &Value) -> Option<Divergence> {
    deep_diff_at("$", left, right)
}

fn deep_diff_at(path: &str, left: &Value, right: &Value) -> Option<Divergence> {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                let child = format!("{path}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(l), Some(r)) => {
                        if let Some(found) = deep_diff_at(&child, l, r) {
                            return Some(found);
                        }
                    }
                    (l, r) => {
                        // Missing keys render as null on the absent side.
                        return Some(diff_at(
                            child,
                            l.unwrap_or(&Value::Null),
                            r.unwrap_or(&Value::Null),
                        ));
                    }
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(diff_at(path.to_string(), left, right));
            }
            for (index, (l, r)) in a.iter().zip(b.iter()).enumerate() {
                let child = format!("{path}[{index}]");
                if let Some(found) = deep_diff_at(&child, l, r) {
                    return Some(found);
                }
            }
            None
        }
        (a, b) if a == b => None,
        _ => Some(diff_at(path.to_string(), left, right)),
    }
}

/// One side of a comparison: normalized body plus HTTP status.
pub struct EndpointResponse {
    pub status: u16,
    pub value: Value,
}

/// One side of a two-base comparison.
pub struct Side<'a> {
    /// Base URL, e.g. `http://127.0.0.1:54321`.
    pub base: &'a str,
    /// Basic-auth password (username is always `opencode`).
    pub password: &'a str,
    /// Normalization environment for this side's payloads.
    pub env: &'a NormEnv,
}

/// Result of [`compare_requests`].
pub struct Comparison {
    pub reference: EndpointResponse,
    pub port: EndpointResponse,
    pub divergence: Option<Divergence>,
}

/// Fetch `path` from `base`, parsing the body as JSON (non-JSON bodies become
/// a `Value::String`, so error pages still diff).
pub async fn get_json(
    base: &str,
    password: &str,
    path: &str,
    timeout: Duration,
) -> Result<EndpointResponse> {
    let auth = minih::basic_auth("opencode", password);
    let response =
        minih::request(base, Some(&auth), "GET", path, None, "application/json", timeout).await?;
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok(EndpointResponse { status: response.status, value })
}

/// Run the identical request against two sides, normalize both bodies with
/// their respective [`NormEnv`]s, and deep-diff. Status codes must match for
/// the comparison to be considered divergence-free.
pub async fn compare_requests(
    reference: Side<'_>,
    port: Side<'_>,
    method: &str,
    path: &str,
    body: Option<&Value>,
    timeout: Duration,
) -> Result<Comparison> {
    let body_text = body.map(std::string::ToString::to_string);
    let auth_a = minih::basic_auth("opencode", reference.password);
    let auth_b = minih::basic_auth("opencode", port.password);
    let (a, b) = tokio::try_join!(
        minih::request(
            reference.base,
            Some(&auth_a),
            method,
            path,
            body_text.as_deref(),
            "application/json",
            timeout
        ),
        minih::request(
            port.base,
            Some(&auth_b),
            method,
            path,
            body_text.as_deref(),
            "application/json",
            timeout
        ),
    )?;
    let parse = |raw: &[u8]| -> Value {
        let text = String::from_utf8_lossy(raw).into_owned();
        serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))
    };
    let response_a = EndpointResponse {
        status: a.status,
        value: normalize_value(&parse(&a.body), reference.env),
    };
    let response_b =
        EndpointResponse { status: b.status, value: normalize_value(&parse(&b.body), port.env) };
    let divergence = if response_a.status == response_b.status {
        deep_diff(&response_a.value, &response_b.value)
    } else {
        Some(Divergence {
            path: "$status".to_string(),
            left: Value::from(response_a.status),
            right: Value::from(response_b.status),
        })
    };
    Ok(Comparison { reference: response_a, port: response_b, divergence })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identical_values_have_no_divergence() {
        let value = json!({"a": [1, 2, {"b": true}], "c": "text"});
        assert!(deep_diff(&value, &value).is_none());
    }

    #[test]
    fn object_key_order_is_not_a_divergence() {
        let left = json!({"a": 1, "b": 2});
        let right = json!({"b": 2, "a": 1});
        assert!(deep_diff(&left, &right).is_none());
    }

    #[test]
    fn reports_first_divergence_path() {
        let left = json!({"a": 1, "urls": ["http://x:1", "http://x:2"], "deep": {"x": [0, {"k": "same"}, 2]}});
        let right = json!({"a": 1, "urls": ["http://x:1", "http://x:9"], "deep": {"x": [0, {"k": "same"}, 2]}});
        let diff = deep_diff(&left, &right).unwrap();
        assert_eq!(diff.path, "$.urls[1]");
        assert_eq!(diff.right, json!("http://x:9"));
    }

    #[test]
    fn reports_missing_keys_and_array_length() {
        let diff = deep_diff(&json!({"a": {"b": 1}}), &json!({"a": {}})).unwrap();
        assert_eq!(diff.path, "$.a.b");
        let diff = deep_diff(&json!({"list": [1, 2]}), &json!({"list": [1]})).unwrap();
        assert_eq!(diff.path, "$.list");
    }
}
