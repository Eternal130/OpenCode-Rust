//! Canonical normalization for parity comparisons. The full spec, including
//! rationale and exact regexes, lives in `tools/parity/NORMALIZATION.md`;
//! this module is its executable form. Any value compared by this crate must
//! pass through [`normalize_value`] on **both** sides.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

/// Rewriting context: sandbox roots that appear inside payloads (paths like
/// `paths.tmp`) and must collapse to a stable token.
#[derive(Debug, Clone, Default)]
pub struct NormEnv {
    roots: Vec<String>,
}

impl NormEnv {
    /// An environment with no sandbox roots (string rules only).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a sandbox root path; occurrences in strings become `<SANDBOX>`.
    /// All roots should be registered; replacement order is longest-first.
    #[must_use]
    pub fn with_root(mut self, root: impl Into<String>) -> Self {
        self.roots.push(root.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Key sets (spec section "Key-driven rewrites")
// ---------------------------------------------------------------------------

/// Numeric field rewritten to `<PORT>` (e.g. `{"port": 50713}`).
const PORT_KEYS: &[&str] = &["port"];
/// Numeric field rewritten to `<PID>`.
const PID_KEYS: &[&str] = &["pid"];
/// Numeric field (milliseconds) rewritten to `<MS>`: jittered delays and
/// durations must not cause spurious divergence.
const MS_KEYS: &[&str] = &["duration", "elapsed", "delay", "jitter", "timeout", "took", "interval"];
/// Field holding a timestamp (number: unix epoch, or ISO-8601 string).
const TS_KEYS: &[&str] = &[
    "time",
    "timestamp",
    "created",
    "createdAt",
    "updatedAt",
    "updated",
    "modified",
    "startedAt",
    "endedAt",
    "expiresAt",
    "lastUsedAt",
    "date",
];
/// Field holding a version string (semver or a local dev marker).
const VERSION_KEYS: &[&str] = &["version"];

// ---------------------------------------------------------------------------
// String regexes (spec section "String-content passes", applied in order)
// ---------------------------------------------------------------------------

/// Whole-string bare origin, e.g. `http://127.0.0.1:50713` → `<URL>`.
static ORIGIN_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https?://(?:\[[0-9a-fA-F:]+\]|[A-Za-z0-9._~-]+)(?::[0-9]+)?/?$").unwrap()
});
/// URL authority embedded in longer text, e.g. `see http://127.0.0.1:99/x` →
/// `see http://<HOST>:<PORT>/x`.
static EMBEDDED_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(https?://)(?:\[[0-9a-fA-F:]+\]|[A-Za-z0-9._~-]+)(:[0-9]+)").unwrap()
});
/// Prefixed opencode identifiers, e.g. `evt_11c556f9...` → `<EVT_ID>`.
/// Prefixes verified against the v2.0.24 schema (event, session, message,
/// form, workspace, shell) plus `usr_`/`acp_` reserved for the port.
static ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(evt|ses|msg|usr|frm|wrk|acp|sh)_[0-9A-Za-z]{8,}\b").unwrap());
/// ISO-8601 timestamps anywhere in text → `<ISO>`.
static ISO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?").unwrap()
});
/// Version strings under `version` keys: semver (optional `v`, pre-release)
/// or a local dev build marker.
static VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?|v?local)$").unwrap());

fn rewrite_number_by_key(key: &str) -> Option<&'static str> {
    if PORT_KEYS.contains(&key) {
        Some("<PORT>")
    } else if PID_KEYS.contains(&key) {
        Some("<PID>")
    } else if MS_KEYS.contains(&key) {
        Some("<MS>")
    } else if TS_KEYS.contains(&key) {
        Some("<TS>")
    } else {
        None
    }
}

fn rewrite_string_by_key(key: &str, value: &str) -> Option<&'static str> {
    if TS_KEYS.contains(&key) && ISO_RE.is_match(value) {
        return Some("<ISO>");
    }
    if VERSION_KEYS.contains(&key) && VERSION_RE.is_match(value) {
        return Some("<VERSION>");
    }
    None
}

/// Apply the ordered string-content passes to a single string (spec section
/// "String-content passes"). Roots are replaced literally, longest first.
#[must_use]
pub fn normalize_str(text: &str, env: &NormEnv) -> String {
    let mut out = text.to_string();
    let mut roots: Vec<&String> = env.roots.iter().collect();
    roots.sort_by_key(|root| std::cmp::Reverse(root.len()));
    for root in roots {
        if !root.is_empty() {
            out = out.replace(root.as_str(), "<SANDBOX>");
        }
    }
    out = ORIGIN_URL.replace(&out, "<URL>").into_owned();
    out = EMBEDDED_URL.replace_all(&out, "${1}<HOST>:<PORT>").into_owned();
    out = ID_RE
        .replace_all(&out, |captures: &regex::Captures| {
            let prefix = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
            format!("<{}_ID>", prefix.to_uppercase())
        })
        .into_owned();
    out = ISO_RE.replace_all(&out, "<ISO>").into_owned();
    out
}

/// Normalize a parsed JSON value in place per `NORMALIZATION.md`.
///
/// Object keys are compared unordered (`serde_json` sorts them); array order
/// is preserved and significant. Rewritten scalars become string tokens, so a
/// rewritten number intentionally changes JSON type — see the spec.
#[must_use]
pub fn normalize_value(value: &Value, env: &NormEnv) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                let normalized = match value {
                    Value::Number(number) => match rewrite_number_by_key(key) {
                        Some(token) => Value::String(token.to_string()),
                        None => Value::Number(number.clone()),
                    },
                    Value::String(text) => match rewrite_string_by_key(key, text) {
                        Some(token) => Value::String(token.to_string()),
                        None => Value::String(normalize_str(text, env)),
                    },
                    other => normalize_value(other, env),
                };
                out.insert(key.clone(), normalized);
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| normalize_value(item, env)).collect())
        }
        // Bare strings (array elements included) get the content passes; a
        // bare string has no key, so no key-driven rewrite applies.
        Value::String(text) => Value::String(normalize_str(text, env)),
        scalar => scalar.clone(),
    }
}

/// Parse `text` as JSON and normalize it. Non-JSON input is an error (use
/// [`normalize_str`] for plain text).
pub fn normalize_json(text: &str, env: &NormEnv) -> anyhow::Result<Value> {
    let parsed: Value = serde_json::from_str(text)?;
    Ok(normalize_value(&parsed, env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env() -> NormEnv {
        NormEnv::new().with_root("/tmp/opencode/parity-sandbox/run1")
    }

    #[test]
    fn rewrites_known_id_prefixes() {
        let out = normalize_str(
            "session ses_01JZY0ID0123456789ABCDEFGH and evt_11c556f9e001H4T7m94mksITaJ",
            &env(),
        );
        assert_eq!(out, "session <SES_ID> and <EVT_ID>");
    }

    #[test]
    fn same_id_rewrites_to_same_token() {
        let out = normalize_str(
            "ses_01JZY0ID0123456789ABCDEFGH -> ses_01JZY0ID0123456789ABCDEFGH",
            &env(),
        );
        assert_eq!(out, "<SES_ID> -> <SES_ID>");
    }

    #[test]
    fn does_not_touch_lookalikes() {
        // Too short after the prefix, or wrong prefix.
        let out = normalize_str("ses_123 keep", &NormEnv::new());
        assert_eq!(out, "ses_123 keep");
        let out = normalize_str("xyz_0123456789ABCDEF", &NormEnv::new());
        assert_eq!(out, "xyz_0123456789ABCDEF");
    }

    #[test]
    fn rewrites_origin_urls_and_embedded_authorities() {
        assert_eq!(normalize_str("http://127.0.0.1:50713", &env()), "<URL>");
        assert_eq!(normalize_str("http://127.0.0.1:50713/", &env()), "<URL>");
        assert_eq!(
            normalize_str("server at http://127.0.0.1:50713/api/info today", &env()),
            "server at http://<HOST>:<PORT>/api/info today"
        );
        // Embedded URL without a port is left alone (spec: only authority with
        // an explicit port is rewritten).
        assert_eq!(
            normalize_str("go to http://example.com/docs", &env()),
            "go to http://example.com/docs"
        );
    }

    #[test]
    fn rewrites_iso_timestamps_anywhere() {
        assert_eq!(normalize_str("at 2026-10-08T16:30:42.123Z ok", &env()), "at <ISO> ok");
        assert_eq!(normalize_str("at 2026-10-08T16:30:42+02:00 ok", &env()), "at <ISO> ok");
        assert_eq!(normalize_str("at 2026-10-08T16:30:42 ok", &env()), "at <ISO> ok");
    }

    #[test]
    fn rewrites_sandbox_roots_longest_first() {
        let env = NormEnv::new()
            .with_root("/tmp/opencode/parity-sandbox")
            .with_root("/tmp/opencode/parity-sandbox/run1");
        assert_eq!(
            normalize_str("/tmp/opencode/parity-sandbox/run1/tmp/opencode", &env),
            "<SANDBOX>/tmp/opencode"
        );
        assert_eq!(
            normalize_str("/tmp/opencode/parity-sandbox/other/x", &env),
            "<SANDBOX>/other/x"
        );
    }

    #[test]
    fn key_driven_number_rewrites() {
        let out = normalize_value(
            &json!({"port": 50713, "pid": 4242, "duration": 1200, "delay": 40, "created": 1_760_000_000_000_i64, "count": 3}),
            &env(),
        );
        assert_eq!(
            out,
            json!({"port": "<PORT>", "pid": "<PID>", "duration": "<MS>", "delay": "<MS>", "created": "<TS>", "count": 3})
        );
    }

    #[test]
    fn key_driven_string_rewrites() {
        let out = normalize_value(
            &json!({"version": "2.0.24", "createdAt": "2026-10-08T16:30:42Z", "name": "2026-10-08T16:30:42Z"}),
            &env(),
        );
        assert_eq!(out, json!({"version": "<VERSION>", "createdAt": "<ISO>", "name": "<ISO>"}));
        // Dev fallbacks also collapse.
        let out = normalize_value(&json!({"version": "local"}), &env());
        assert_eq!(out, json!({"version": "<VERSION>"}));
        // Non-version strings under `version` stay literal.
        let out = normalize_value(&json!({"version": "custom-build-thing"}), &env());
        assert_eq!(out, json!({"version": "custom-build-thing"}));
    }

    #[test]
    fn recursion_preserves_array_order_and_nested_keys() {
        let input = json!({
            "urls": ["http://127.0.0.1:50713"],
            "paths": {"tmp": "/tmp/opencode/parity-sandbox/run1/tmp/opencode"},
            "events": [
                {"id": "evt_11c556f9e001H4T7m94mksITaJ", "type": "server.connected", "data": {}},
                {"id": "evt_11c556f9e001ZZZZZZZZZZZZZZZ", "type": "server.connected", "data": {}}
            ]
        });
        let out = normalize_value(&input, &env());
        assert_eq!(out["urls"][0], json!("<URL>"));
        assert_eq!(out["paths"]["tmp"], json!("<SANDBOX>/tmp/opencode"));
        assert_eq!(out["events"][0]["id"], json!("<EVT_ID>"));
        assert_eq!(out["events"][0]["type"], json!("server.connected"));
        assert_eq!(out["events"][0]["id"], out["events"][1]["id"]);
    }

    #[test]
    fn booleans_and_nulls_pass_through() {
        let input = json!({"ok": true, "nothing": null, "zero": 0});
        assert_eq!(normalize_value(&input, &env()), input);
    }
}
