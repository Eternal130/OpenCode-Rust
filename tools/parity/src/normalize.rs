//! Canonical normalization for parity comparisons. The full spec, including
//! rationale and exact regexes, lives in `tools/parity/NORMALIZATION.md`;
//! this module is its executable form. Any value compared by this crate must
//! pass through [`normalize_value`] on **both** sides.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

/// Rewriting context: sandbox roots that appear inside payloads (paths like
/// `paths.tmp`) and must collapse to a stable token.
#[derive(Debug, Clone, Default)]
pub struct NormEnv {
    roots: Vec<String>,
}

/// First-seen canonical id numbering state for one normalization run
/// (spec §5.4). A *run* is one whole comparison unit on one side — one HTTP
/// body, or one collected SSE sequence. Within a run, the first distinct id
/// of a prefix seen becomes `<PREFIX_ID:1>`, the second `<PREFIX_ID:2>`, …;
/// an id seen again reuses its number, so **referential integrity survives**:
/// two positions referencing the same id get the same token, and positions
/// referencing *different* ids get different tokens.
#[derive(Debug, Clone, Default)]
pub struct IdNumbering {
    /// Matched id text (e.g. `msg_abc…`) → assigned number.
    assigned: HashMap<String, usize>,
    /// Prefix (e.g. `msg`) → highest number handed out so far.
    last: HashMap<String, usize>,
}

impl IdNumbering {
    /// A fresh, empty numbering (numbers start at 1 for each prefix).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The canonical number for `id` (whose prefix is `prefix`), assigning
    /// the next number for that prefix on first sight.
    pub fn number_for(&mut self, prefix: &str, id: &str) -> usize {
        if let Some(number) = self.assigned.get(id) {
            return *number;
        }
        let number = self.last.get(prefix).map_or(1, |last| last + 1);
        self.last.insert(prefix.to_string(), number);
        self.assigned.insert(id.to_string(), number);
        number
    }
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
///
/// Aligned to every epoch-millisecond field in the ported wire schema
/// (`crates/oc-schema/src`, cross-checked 2026-10): `time`/`created`/
/// `updatedAt`-style names plus the message `time` blocks (`completed`,
/// `streamed`/`ran`, `at`), session watermarks (`idle`, `viewed`,
/// `archived`), `started` (shell/step), stats `Range` (`from`, `to`),
/// retry scheduling (`at`, `next`), integration attempts (`expires`),
/// credentials (`expires`), projects (`active`), and web-search results
/// (`published`). Deliberately **not** masked: `seq` (aggregate log
/// position — sequence comparison orders by it), `version`, `ordinal`,
/// `attempt` (counters), and `Prompt.Mention` `start`/`end` (character
/// offsets, not timestamps).
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
    "started",
    "completed",
    "streamed",
    "ran",
    "idle",
    "viewed",
    "archived",
    "at",
    "from",
    "to",
    "next",
    "expires",
    "published",
    "active",
];
/// Field holding a version string (semver or a local dev marker).
const VERSION_KEYS: &[&str] = &["version"];
/// Field holding a machine-generated session slug (adjective-noun pair,
/// `Slug.create()` in the reference): random per run, shape-stable.
const SLUG_KEYS: &[&str] = &["slug"];
/// Fields whose string value may be a bare **project id**: a hex digest
/// of the project directory (`Hash.fast("directory:<dir>")` — 40
/// lowercase hex chars on the reference, which uses sha1; the Rust port's
/// oc-core `hash_fast` uses sha256, 64 hex chars). Sandbox project
/// directories differ per run, so these ids are per-run values; they are
/// rewritten to numbered `<PRJ_ID:n>` tokens through the shared id
/// numbering (spec §5.4 "Bare project ids"). Only exact-key matches with
/// the 40-or-64-hex shape are rewritten — event/session/message ids keep
/// flowing through the prefix content pass.
const PROJECT_ID_KEYS: &[&str] = &["projectID", "id", "previous"];

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
/// Prefixed opencode identifiers, e.g. `evt_11c556f9...` → `<EVT_ID:1>`.
/// Prefixes verified against the v2.0.24 schema (`crates/oc-schema/src/id.rs`):
/// `evt_` (event), `ses` (session), `msg_` (session message), `frm_` (form),
/// `wrk` (workspace), `per_` (permission), `psv_` (persistent-pty permission
/// saved), `cred_` (credential), `con_` (integration attempt), `sh_` (shell);
/// `usr_` / `acp_` are reserved for port-side namespaces.
static ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(evt|ses|msg|usr|frm|wrk|acp|sh|per|psv|cred|con)_[0-9A-Za-z]{8,}\b").unwrap()
});
/// ISO-8601 timestamps anywhere in text → `<ISO>`.
static ISO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?").unwrap()
});
/// Version strings under `version` keys: semver (optional `v`, pre-release)
/// or a local dev build marker.
static VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?|v?local)$").unwrap());
/// Machine-generated session slugs (`adjective-noun`, both lowercase words).
static SLUG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]+-[a-z]+$").unwrap());
/// Bare project ids: hex digests (40 = sha1/reference, 64 = sha256/Rust
/// port's `hash_fast` — the key gates the rewrite, either length is a
/// project id).
static PROJECT_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$|^[0-9a-f]{64}$").unwrap());
/// Bare sha256 hex digests (64 lowercase hex chars): content-addressed
/// hashes whose input embeds run-specific state (instruction entry hashes
/// in `session.instructions.updated`, worktree fingerprints).
static SHA256_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{64}$").unwrap());

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

fn rewrite_string_by_key(key: &str, value: &str, ids: &mut IdNumbering) -> Option<String> {
    if TS_KEYS.contains(&key) && ISO_RE.is_match(value) {
        return Some("<ISO>".to_string());
    }
    if VERSION_KEYS.contains(&key) && VERSION_RE.is_match(value) {
        return Some("<VERSION>".to_string());
    }
    if SLUG_KEYS.contains(&key) && SLUG_RE.is_match(value) {
        return Some("<SLUG>".to_string());
    }
    if PROJECT_ID_KEYS.contains(&key) && PROJECT_ID_RE.is_match(value) {
        let number = ids.number_for("prj", value);
        return Some(format!("<PRJ_ID:{number}>"));
    }
    None
}

/// Apply the ordered string-content passes to a single string (spec section
/// "String-content passes"), threading one shared [`IdNumbering`] run so id
/// tokens stay consistent across the whole comparison unit. Roots are
/// replaced literally, longest first.
#[must_use]
pub fn normalize_str_with(text: &str, env: &NormEnv, ids: &mut IdNumbering) -> String {
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
            let id = captures.get(0).map(|m| m.as_str()).unwrap_or_default();
            let number = ids.number_for(prefix, id);
            format!("<{}_ID:{}>", prefix.to_uppercase(), number)
        })
        .into_owned();
    // Whole-string sha256 digests (spec §5.7): opaque content hashes whose
    // inputs embed run-specific state.
    if SHA256_RE.is_match(&out) {
        out = "<SHA256>".to_string();
    }
    out = ISO_RE.replace_all(&out, "<ISO>").into_owned();
    out
}

/// [`normalize_str_with`] with a fresh numbering run (one call = one run; see
/// spec §5.4 — use the `_with` variant when several strings form one
/// comparison unit).
#[must_use]
pub fn normalize_str(text: &str, env: &NormEnv) -> String {
    normalize_str_with(text, env, &mut IdNumbering::new())
}

/// Normalize a parsed JSON value per `NORMALIZATION.md`, threading one shared
/// [`IdNumbering`] run: id tokens are assigned first-seen per prefix in
/// encounter order (depth-first; arrays in element order, object keys in
/// sorted order — deterministic for a given value).
#[must_use]
pub fn normalize_value_with(value: &Value, env: &NormEnv, ids: &mut IdNumbering) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                let normalized = match value {
                    Value::Number(number) => match rewrite_number_by_key(key) {
                        Some(token) => Value::String(token.to_string()),
                        None => Value::Number(number.clone()),
                    },
                    Value::String(text) => match rewrite_string_by_key(key, text, ids) {
                        Some(token) => Value::String(token),
                        None => Value::String(normalize_str_with(text, env, ids)),
                    },
                    other => normalize_value_with(other, env, ids),
                };
                out.insert(key.clone(), normalized);
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| normalize_value_with(item, env, ids)).collect())
        }
        // Bare strings (array elements included) get the content passes; a
        // bare string has no key, so no key-driven rewrite applies.
        Value::String(text) => Value::String(normalize_str_with(text, env, ids)),
        scalar => scalar.clone(),
    }
}

/// [`normalize_value_with`] with a fresh numbering run: **one call = one
/// normalization run = one id-numbering scope** (spec §5.4). Callers comparing
/// multi-part units (e.g. an SSE frame sequence) must use the `_with` variant
/// with a single run for the whole unit.
#[must_use]
pub fn normalize_value(value: &Value, env: &NormEnv) -> Value {
    normalize_value_with(value, env, &mut IdNumbering::new())
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
        assert_eq!(out, "session <SES_ID:1> and <EVT_ID:1>");
    }

    #[test]
    fn rewrites_live_v2_permission_and_integration_prefixes() {
        let out = normalize_str(
            "per_01JZY0ID0123456789 psv_01JZY0ID0123456789 cred_01JZY0ID0123456789 con_01JZY0ID0123456789",
            &NormEnv::new(),
        );
        assert_eq!(out, "<PER_ID:1> <PSV_ID:1> <CRED_ID:1> <CON_ID:1>");
    }

    #[test]
    fn same_id_rewrites_to_same_token() {
        let out = normalize_str(
            "ses_01JZY0ID0123456789ABCDEFGH -> ses_01JZY0ID0123456789ABCDEFGH",
            &env(),
        );
        assert_eq!(out, "<SES_ID:1> -> <SES_ID:1>");
    }

    #[test]
    fn distinct_ids_get_first_seen_canonical_numbers() {
        // Two different msg ids referenced by two events: different tokens.
        let out = normalize_str(
            "msg_01JZY0ID0123456789AAAA vs msg_01JZY0ID0123456789BBBB",
            &NormEnv::new(),
        );
        assert_eq!(out, "<MSG_ID:1> vs <MSG_ID:2>");
        // Numbering is per prefix: a ses id after two msg ids is still 1.
        let out = normalize_str(
            "msg_01JZY0ID0123456789AAAA msg_01JZY0ID0123456789BBBB ses_01JZY0ID0123456789CCCC",
            &NormEnv::new(),
        );
        assert_eq!(out, "<MSG_ID:1> <MSG_ID:2> <SES_ID:1>");
    }

    #[test]
    fn referential_integrity_within_one_run() {
        // Same id referenced twice inside one run -> same numbered token;
        // the second distinct id -> the next number. Encounter order is
        // depth-first with object keys sorted ("a" before "b").
        let mut ids = IdNumbering::new();
        let input = json!({
            "a": "references msg_01JZY0ID0123456789AAAA",
            "b": "references msg_01JZY0ID0123456789BBBB then msg_01JZY0ID0123456789AAAA again"
        });
        let out = normalize_value_with(&input, &NormEnv::new(), &mut ids);
        assert_eq!(out["a"], json!("references <MSG_ID:1>"));
        assert_eq!(out["b"], json!("references <MSG_ID:2> then <MSG_ID:1> again"));
    }

    #[test]
    fn swapped_references_are_a_detectable_divergence() {
        // Definitions live in one array; the references under `refs` point
        // back at them. The port side swaps which message each reference
        // points at — same set of ids, wrong cross-references. First-seen
        // numbering (definitions encountered first) keeps the definition
        // tokens aligned, so the swapped reference tokens must now diverge.
        let reference = json!({
            "messages": [
                {"id": "msg_01JZY0ID0123456789AAAA"},
                {"id": "msg_01JZY0ID0123456789BBBB"}
            ],
            "refs": [
                {"to": "msg_01JZY0ID0123456789BBBB"},
                {"to": "msg_01JZY0ID0123456789AAAA"}
            ]
        });
        let port = json!({
            "messages": [
                {"id": "msg_01JZY0ID0123456789CCCC"},
                {"id": "msg_01JZY0ID0123456789DDDD"}
            ],
            "refs": [
                {"to": "msg_01JZY0ID0123456789CCCC"},
                {"to": "msg_01JZY0ID0123456789DDDD"}
            ]
        });
        let a = normalize_value(&reference, &NormEnv::new());
        let b = normalize_value(&port, &NormEnv::new());
        let divergence = crate::httpdiff::deep_diff(&a, &b)
            .expect("a swapped id reference must survive normalization as a divergence");
        assert_eq!(divergence.path, "$.refs[0].to");
        assert_eq!(divergence.left, json!("<MSG_ID:2>"));
        assert_eq!(divergence.right, json!("<MSG_ID:1>"));
    }

    #[test]
    fn correct_references_are_not_a_divergence() {
        // Mirror of the swap test: references agree structurally -> the two
        // sides normalize equal despite carrying different raw ids.
        let reference = json!({
            "messages": [
                {"id": "msg_01JZY0ID0123456789AAAA"},
                {"id": "msg_01JZY0ID0123456789BBBB"}
            ],
            "refs": [
                {"to": "msg_01JZY0ID0123456789BBBB"},
                {"to": "msg_01JZY0ID0123456789AAAA"}
            ]
        });
        let port = json!({
            "messages": [
                {"id": "msg_01JZY0ID0123456789CCCC"},
                {"id": "msg_01JZY0ID0123456789DDDD"}
            ],
            "refs": [
                {"to": "msg_01JZY0ID0123456789DDDD"},
                {"to": "msg_01JZY0ID0123456789CCCC"}
            ]
        });
        let a = normalize_value(&reference, &NormEnv::new());
        let b = normalize_value(&port, &NormEnv::new());
        assert!(crate::httpdiff::deep_diff(&a, &b).is_none());
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
    fn every_ts_key_field_is_masked() {
        // Enumerate the covered key list (single source of truth: TS_KEYS)
        // against a JSON object carrying every such field with a distinct
        // large epoch-millis value, and assert each is masked to <TS>.
        let mut input = Map::new();
        for (i, key) in TS_KEYS.iter().enumerate() {
            input.insert(
                (*key).to_string(),
                Value::from(1_700_000_000_000_i64 + i64::try_from(i).unwrap()),
            );
        }
        let out = normalize_value(&Value::Object(input), &NormEnv::new());
        for key in TS_KEYS {
            assert_eq!(out[key], json!("<TS>"), "key {key} must be masked to <TS>");
        }
    }

    #[test]
    fn non_timestamp_counters_stay_numeric() {
        // seq/version/ordinal/attempt are counters, not timestamps; seq in
        // particular drives sequence ordering and must survive normalization.
        let out = normalize_value(
            &json!({"seq": 3, "version": 1, "ordinal": 7, "attempt": 2, "count": 9}),
            &env(),
        );
        assert_eq!(out, json!({"seq": 3, "version": 1, "ordinal": 7, "attempt": 2, "count": 9}));
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
        assert_eq!(out["events"][0]["id"], json!("<EVT_ID:1>"));
        assert_eq!(out["events"][0]["type"], json!("server.connected"));
        // Distinct ids get distinct tokens (first-seen numbering).
        assert_eq!(out["events"][1]["id"], json!("<EVT_ID:2>"));
    }

    #[test]
    fn booleans_and_nulls_pass_through() {
        let input = json!({"ok": true, "nothing": null, "zero": 0});
        assert_eq!(normalize_value(&input, &env()), input);
    }

    #[test]
    fn session_slugs_collapse_to_slug_token() {
        // Session slugs are random adjective-noun pairs ("brave-comet");
        // the shape gate must not eat titles or other hyphenated text.
        let out = normalize_value(
            &json!({"slug": "brave-comet", "title": "brave-comet", "name": "e2e-check"}),
            &env(),
        );
        assert_eq!(out["slug"], json!("<SLUG>"));
        assert_eq!(out["title"], json!("brave-comet"));
        assert_eq!(out["name"], json!("e2e-check"));
        // Multi-word or capitalized values stay literal (not the generator).
        let out = normalize_value(&json!({"slug": "Not-Generated"}), &env());
        assert_eq!(out["slug"], json!("Not-Generated"));
    }

    #[test]
    fn bare_project_ids_get_numbered_tokens() {
        // Project ids are sha1-hex of the project directory — per-run values
        // under sandbox paths. Same id → same token, different ids →
        // different tokens (referential integrity, like prefixed ids).
        let a = "3da5b7c9f01e2a6d8c4b9e7f5a3d1c2b4e6f8a0d";
        let b = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c";
        let out = normalize_value(
            &json!({
                "projectID": a,
                "nested": {"id": a, "previous": b},
                "global": "global"
            }),
            &env(),
        );
        assert_eq!(out["projectID"], json!("<PRJ_ID:1>"));
        assert_eq!(out["nested"]["id"], json!("<PRJ_ID:1>"));
        assert_eq!(out["nested"]["previous"], json!("<PRJ_ID:2>"));
        // Non-hash values under the same keys stay literal (project ids may
        // also be the fixed "global" project).
        assert_eq!(out["global"], json!("global"));
    }

    #[test]
    fn short_hex_strings_are_not_project_ids() {
        // 39/41 chars, uppercase, or prefixed ids must not be mistaken for
        // bare project ids.
        let out = normalize_value(
            &json!({"id": "3da5b7c9f01e2a6d8c4b9e7f5a3d1c2b4e6f8a0", "projectID": "ABCDEF0123456789ABCDEF0123456789ABCDEF01"}),
            &env(),
        );
        assert_eq!(out["id"], json!("3da5b7c9f01e2a6d8c4b9e7f5a3d1c2b4e6f8a0"));
        assert_eq!(out["projectID"], json!("ABCDEF0123456789ABCDEF0123456789ABCDEF01"));
    }

    #[test]
    fn whole_string_sha256_digests_collapse() {
        // Instruction-entry hashes (session.instructions.updated `delta`
        // maps) are sha256 digests of content that embeds run-specific
        // values (paths, dates) — the digest differs per run even for the
        // "same" logical instruction. Whole-string 64-hex values collapse
        // to <SHA256>; embedded digests inside longer text stay literal.
        let digest = "3a6c896ca4366410e84edccbf179835337f464e16493a692c18b854a528ce2e1";
        let out = normalize_value(
            &json!({
                "delta": { "core/environment": digest, "core/date": digest },
                "note": format!("see {digest} inline"),
                "text": "Parity fake says hello."
            }),
            &env(),
        );
        assert_eq!(out["delta"]["core/environment"], json!("<SHA256>"));
        assert_eq!(out["delta"]["core/date"], json!("<SHA256>"));
        assert_eq!(out["note"], json!(format!("see {digest} inline")));
        assert_eq!(out["text"], json!("Parity fake says hello."));
        // 63/65 chars and uppercase stay literal.
        let out = normalize_value(&json!({"a": &digest[..63], "b": format!("{digest}f")}), &env());
        assert_eq!(out["a"], json!(&digest[..63]));
        assert_eq!(out["b"], json!(format!("{digest}f")));
    }
}
