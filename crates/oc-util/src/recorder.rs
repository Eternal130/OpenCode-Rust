//! TUI session recorder (diff-ext C1) — env-gated capture of a live
//! oc-tui session into the channel-8 corpus pipeline.
//!
//! PRIVACY RULE (user iron law, 2026-11-15): recorded sessions are REAL
//! user data. The recording directory is git-ignored
//! (`tools/diff/corpus/tui/recorded-local/`, see the repo `.gitignore`
//! and the guard `tools/guards/recorded_local.py`), raw files (suffix
//! `.raw.json`) NEVER enter a commit, and reports quote aggregate
//! counts only — never recorded content.
//!
//! Gating: `TUI_RECORD_DIR=<dir>` arms the recorder; unset means the
//! whole module is a no-op. Every hook begins with an
//! [`AtomicBool::load`] (acquire) on [`ENABLED`] — a single relaxed
//! load on the disabled fast path, no allocation, no lock, no I/O
//! (zero overhead for non-recording sessions).
//!
//! Capture surface (one file per session, `<id>.raw.json` where
//! `id = rec-<timestamp>-<session8>`):
//! * `boot` + `sizes` — the boot terminal size and every resize
//!   (hooks: [`crate::app`] `App::new`/`App::resize` in oc-tui).
//! * `events` — the raw `/api/event` SSE payloads in EXACT arrival
//!   order (hook: oc-tui `data.rs` `Connection::run`, the ingest seam
//!   the diff driver's replay uses).
//! * `keys` — the `App::press` specs in dispatch order (hook: oc-tui
//!   `app.rs::press`), i.e. the channel-8 key vocabulary verbatim.
//! * `fetch` — every HTTP response the client saw: endpoint path,
//!   status and body (hook: `oc-client` `transport.rs`, the single
//!   chokepoint every generated method rides).
//!
//! Message-content redaction (default ON, `TUI_RECORD_REDACT=0` to
//! disable): string values under the message-content keys
//! (`text`/`title`/`summary`/`reasoning`/`output`) are replaced by a
//! shape-preserving placeholder — same length, same newlines, same
//! unicode category per char (`recorder::redact_value`) — so the
//! recording stays replay-true (both legs of the diff see identical
//! shapes) while the raw words never leave the machine even into the
//! local file. The compiler (`tools/diff/tui_record_compile.py`)
//! redacts again by default (idempotent) on the way into the corpus.
//!
//! Flush discipline: the whole document is rewritten on every record
//! (sessions are short; a crash mid-recording still leaves a valid
//! JSON file of everything captured so far).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Map, Value};

/// Message-content field names whose string values get the
/// shape-preserving placeholder under default redaction.
const CONTENT_KEYS: [&str; 5] = ["text", "title", "summary", "reasoning", "output"];

/// Disabled fast-path flag. Set once by [`ensure_init`] when
/// `TUI_RECORD_DIR` is present; every hook loads it first.
static ENABLED: AtomicBool = AtomicBool::new(false);

struct State {
    dir: PathBuf,
    doc: Map<String, Value>,
}

static STATE: OnceLock<Mutex<Option<State>>> = OnceLock::new();

fn state_cell() -> &'static Mutex<Option<State>> {
    STATE.get_or_init(|| Mutex::new(None))
}

/// Whether recording is armed (the disabled fast-path check).
#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// One-record time: `rec-<yyyymmdd-hhmmss>-<8 hex>` (UTC, session
/// nonce = first 8 chars of a uuid4 — the session id surrogate).
fn recording_id(now_ms: u64) -> String {
    let secs = now_ms / 1000;
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let days = secs / 86400;
    // days since 1970-01-01 -> civil date (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let nonce = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    format!("rec-{year:04}{month:02}{d:02}-{h:02}{m:02}{s:02}-{nonce}")
}

/// Arms the recorder from the environment (the first hook call does
/// this; later calls are no-ops). Unset/empty `TUI_RECORD_DIR` leaves
/// everything disabled forever — the complete bypass.
fn ensure_init() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let Some(dir) = std::env::var_os("TUI_RECORD_DIR").filter(|d| !d.is_empty()) else {
            return;
        };
        let redact = !matches!(std::env::var("TUI_RECORD_REDACT").as_deref(), Ok("0" | "false"));
        let now_ms = now_millis();
        let mut doc = Map::new();
        doc.insert("schema".into(), json!("tui-diff-recording/1"));
        doc.insert("id".into(), json!(recording_id(now_ms)));
        doc.insert("recorded_at".into(), json!(now_ms));
        doc.insert("app".into(), json!("oc-tui-rust"));
        doc.insert("redacted".into(), json!(redact));
        doc.insert("boot".into(), Value::Null);
        doc.insert("sizes".into(), json!([]));
        doc.insert("fetch".into(), json!({}));
        doc.insert("events".into(), json!([]));
        doc.insert("keys".into(), json!([]));
        let mut cell = state_cell().lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = std::fs::create_dir_all(&dir) {
            eprintln!("tui-record: cannot create {}: {error}", dir.display());
            return;
        }
        *cell = Some(State { dir: PathBuf::from(dir), doc });
        ENABLED.store(true, Ordering::Relaxed);
    });
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Test-only: clear the process-global recorder state so a test can
/// pin the armed/disabled path deterministically.
#[doc(hidden)]
pub fn reset_for_tests() {
    let mut cell = state_cell().lock().unwrap_or_else(|e| e.into_inner());
    *cell = None;
    ENABLED.store(false, Ordering::Relaxed);
    // re-arm the init OnceLock by replacing the cell the next init
    // fills: the OnceLock itself cannot be reset within a process, so
    // tests that need arming use `init_for_tests`.
}

/// Test-only: arm with an explicit directory + redaction flag.
#[doc(hidden)]
pub fn init_for_tests(dir: &std::path::Path, redact: bool) {
    reset_for_tests();
    let now_ms = now_millis();
    let mut doc = Map::new();
    doc.insert("schema".into(), json!("tui-diff-recording/1"));
    doc.insert("id".into(), json!(recording_id(now_ms)));
    doc.insert("recorded_at".into(), json!(now_ms));
    doc.insert("app".into(), json!("oc-tui-rust"));
    doc.insert("redacted".into(), json!(redact));
    doc.insert("boot".into(), Value::Null);
    doc.insert("sizes".into(), json!([]));
    doc.insert("fetch".into(), json!({}));
    doc.insert("events".into(), json!([]));
    doc.insert("keys".into(), json!([]));
    *state_cell().lock().unwrap_or_else(|e| e.into_inner()) =
        Some(State { dir: dir.to_owned(), doc });
    ENABLED.store(true, Ordering::Relaxed);
}

/// Rewrite the output file from the in-memory document (best effort —
/// a recording failure must never take the session down).
fn flush(state: &State) {
    let id = state.doc.get("id").and_then(Value::as_str).unwrap_or("rec");
    let path = state.dir.join(format!("{id}.raw.json"));
    if let Ok(body) = serde_json::to_string_pretty(&Value::Object(state.doc.clone())) {
        let tmp = state.dir.join(format!("{id}.raw.json.tmp"));
        if std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            eprintln!("tui-record: write failed: {}", path.display());
        }
    }
}

fn with_state(f: impl FnOnce(&mut State)) {
    if !enabled() {
        return;
    }
    let mut cell = state_cell().lock().unwrap_or_else(|e| e.into_inner());
    let Some(state) = cell.as_mut() else { return };
    f(state);
    flush(state);
}

fn redacted() -> bool {
    state_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|s| s.doc.get("redacted").and_then(Value::as_bool).unwrap_or(true))
}

/// Hook: boot with the initial terminal size (`App::new`).
pub fn boot(width: u16, height: u16) {
    ensure_init();
    with_state(|state| {
        if state.doc.get("boot").map_or(true, Value::is_null) {
            state.doc.insert("boot".into(), json!({ "width": width, "height": height }));
            state.doc.insert("width".into(), json!(width));
            state.doc.insert("height".into(), json!(height));
        }
        if let Some(Value::Array(sizes)) = state.doc.get_mut("sizes") {
            if sizes.last().and_then(|s| s.as_array()) != Some(&vec![json!(width), json!(height)]) {
                sizes.push(json!([width, height]));
            }
        }
    });
}

/// Hook: terminal resize (`App::resize`) — records the size change.
pub fn resize(width: u16, height: u16) {
    ensure_init();
    if !enabled() {
        return;
    }
    with_state(|state| {
        if let Some(Value::Array(sizes)) = state.doc.get_mut("sizes") {
            if sizes.last().and_then(|s| s.as_array()) != Some(&vec![json!(width), json!(height)]) {
                sizes.push(json!([width, height]));
            }
        }
    });
}

/// Hook: one raw `/api/event` SSE payload, in arrival order (the
/// ingest seam — `Connection::run` in oc-tui `data.rs`).
pub fn event(value: &Value) {
    ensure_init();
    if !enabled() {
        return;
    }
    let recorded = if redacted() { redact_value(value) } else { value.clone() };
    with_state(|state| {
        if let Some(Value::Array(events)) = state.doc.get_mut("events") {
            events.push(recorded);
        }
    });
}

/// Hook: one `App::press` key spec, in dispatch order.
pub fn key(spec: &str) {
    ensure_init();
    if !enabled() {
        return;
    }
    with_state(|state| {
        if let Some(Value::Array(keys)) = state.doc.get_mut("keys") {
            keys.push(json!(spec));
        }
    });
}

/// Hook: one client fetch response (endpoint, status, decoded body).
/// The path is the RENDERED request path (query-less); the body is the
/// JSON the server returned, message-content-redacted by default.
pub fn fetch(method: &str, path: &str, status: u16, body: &Value) {
    ensure_init();
    if !enabled() {
        return;
    }
    let body = if redacted() { redact_value(body) } else { body.clone() };
    with_state(|state| {
        let Some(Value::Object(fetch)) = state.doc.get_mut("fetch") else { return };
        let entry = fetch
            .entry(path.to_owned())
            .or_insert_with(|| json!({ "method": method, "status": status, "body": null }));
        if let Some(object) = entry.as_object_mut() {
            object.insert("method".into(), json!(method));
            object.insert("status".into(), json!(status));
            object.insert("body".into(), body);
        }
    });
}

/// Hook: the SSE stream opened (`/api/event` subscribe).
pub fn sse_open(path: &str, status: u16) {
    ensure_init();
    if !enabled() {
        return;
    }
    with_state(|state| {
        let Some(Value::Object(fetch)) = state.doc.get_mut("fetch") else { return };
        fetch.entry(path.to_owned()).or_insert_with(
            || json!({ "method": "GET", "status": status, "body": { "sse": true } }),
        );
    });
}

/// Shape-preserving message-content placeholder for one char:
/// newlines/spaces/tabs kept, letters→`x`, digits→`9`, other
/// punctuation/symbols→`-` (unicode category coarse-preserving).
fn redact_char(ch: char) -> char {
    if ch.is_whitespace() {
        return ch;
    }
    if ch.is_alphabetic() {
        // a unicode-letter placeholder keeps idempotence ('α' is
        // alphabetic itself) and the letter category for non-ascii.
        return if ch.is_ascii() { 'x' } else { 'α' };
    }
    if ch.is_numeric() {
        return '9';
    }
    '-'
}

/// Replace a string with its same-shape placeholder (length,
/// newlines and unicode letter/digit/other category preserved).
#[must_use]
pub fn redact_str(text: &str) -> String {
    text.chars().map(redact_char).collect()
}

/// Recursively redact every string under a message-content context
/// (the value of a `text`/`title`/`summary`/`reasoning`/`output` key,
/// including arrays of strings — tool outputs).
fn redact_content(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact_str(text)),
        Value::Array(items) => Value::Array(items.iter().map(redact_content).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), redact_content(v))).collect())
        }
        other => other.clone(),
    }
}

/// Recursively redact string values under the message-content keys.
#[must_use]
pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, val) in map {
                if CONTENT_KEYS.contains(&key.as_str()) {
                    out.insert(key.clone(), redact_content(val));
                } else {
                    out.insert(key.clone(), redact_value(val));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_is_zero_cost_bypass() {
        let _env = crate::test_support::lock_env();
        // No TUI_RECORD_DIR in this process (the test binary never sets
        // it): every hook must be a no-op and `enabled()` stays false.
        super::reset_for_tests();
        // (ensure_init already ran via the OnceLock in an armed
        // process would flip ENABLED; here we assert the bypass
        // directly: hooks on a disarmed recorder do nothing.)
        ENABLED.store(false, Ordering::Relaxed);
        assert!(!enabled());
        boot(80, 24);
        event(&serde_json::json!({"type": "session.updated"}));
        key("return");
        resize(120, 30);
        fetch("GET", "/api/x", 200, &serde_json::json!({"ok": true}));
        // No state was created: still disabled, no panic, nothing wrote.
        assert!(!enabled());
    }

    #[test]
    fn records_boot_events_keys_sizes_fetch_shape() {
        let _env = crate::test_support::lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        init_for_tests(dir.path(), true);
        assert!(enabled());
        boot(80, 24);
        event(&serde_json::json!({
            "id": "evt_c", "created": 0, "type": "server.connected", "data": {}
        }));
        event(&serde_json::json!({
            "id": "evt_1", "created": 1, "type": "session.updated",
            "data": { "sessionID": "ses_1", "info": { "title": "hello" } }
        }));
        key("return");
        key("x");
        resize(120, 30);
        fetch("GET", "/api/config", 200, &serde_json::json!([]));
        sse_open("/api/event", 200);

        let state = state_cell().lock().unwrap_or_else(|e| e.into_inner());
        let state = state.as_ref().expect("state");
        let doc = &state.doc;
        assert_eq!(doc["schema"], "tui-diff-recording/1");
        assert_eq!(doc["boot"], json!({"width": 80, "height": 24}));
        assert_eq!(doc["width"], 80);
        assert_eq!(doc["height"], 24);
        // sizes: boot size once, then the resize (no duplicate entries)
        assert_eq!(doc["sizes"], json!([[80, 24], [120, 30]]));
        // events in arrival order, server.connected included
        assert_eq!(doc["events"].as_array().map(Vec::len), Some(2));
        assert_eq!(doc["events"][0]["type"], "server.connected");
        assert_eq!(doc["events"][1]["type"], "session.updated");
        // keys in dispatch order
        assert_eq!(doc["keys"], json!(["return", "x"]));
        // fetch map: endpoint + status + body
        assert_eq!(doc["fetch"]["/api/config"]["status"], 200);
        assert_eq!(doc["fetch"]["/api/config"]["body"], json!([]));
        assert_eq!(doc["fetch"]["/api/event"]["body"], json!({"sse": true}));
        // the file exists on disk with the .raw.json suffix
        let id = doc["id"].as_str().expect("id").to_owned();
        let file = dir.path().join(format!("{id}.raw.json"));
        let body = std::fs::read_to_string(&file).expect("raw file flushed");
        let parsed: Value = serde_json::from_str(&body).expect("valid json");
        assert_eq!(parsed["schema"], "tui-diff-recording/1");
    }

    #[test]
    fn redaction_preserves_shape_and_is_idempotent() {
        let _env = crate::test_support::lock_env();
        let text = "Hello 世界!\nline2\t 42";
        let once = redact_str(text);
        assert_eq!(once.chars().count(), text.chars().count());
        assert_eq!(once.matches('\n').count(), text.matches('\n').count());
        assert_eq!(once.matches('\t').count(), text.matches('\t').count());
        // unicode letter category preserved as the wide placeholder
        assert!(once.contains('α'));
        assert_eq!(redact_str(&once), once, "idempotent");

        let value = json!({
            "type": "session.text.ended",
            "data": { "sessionID": "ses_1", "text": "secret words\nsecond", "ordinal": 0 },
            "nested": { "output": ["a", "b"] }
        });
        let redacted = redact_value(&value);
        assert_eq!(redacted["data"]["sessionID"], "ses_1", "non-content keys untouched");
        assert_eq!(redacted["data"]["text"], redact_str("secret words\nsecond"));
        assert_ne!(redacted["data"]["text"], json!("secret words\nsecond"));
        assert_eq!(redacted["nested"]["output"][0], redact_str("a"));
        assert_eq!(redacted_value_again(&redacted), redacted, "value redaction idempotent");
    }

    fn redacted_value_again(value: &Value) -> Value {
        redact_value(value)
    }
}
