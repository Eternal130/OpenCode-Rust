//! Shared helpers for the Phase-10 parity battery (`tests/p10_*.rs`).
//!
//! Every p10 test is opt-in twice: `#[ignore]`d at the binary level (so the
//! default suite never needs bun/the reference/tmux) AND gated at runtime on
//! `OC_P10=1`, mirroring the `OC_M1_TUI`/`OC_M2` pattern:
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_)'
//! ```

#![allow(dead_code)] // shared across battery files; each uses a subset

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use parity::minih;

/// Whether the Phase-10 battery is enabled (`OC_P10=1`).
pub fn enabled() -> bool {
    std::env::var("OC_P10").ok().as_deref() == Some("1")
}

/// Print the standard skip note and return `Ok(())` (test passes as SKIP).
pub fn skip(reason: &str) {
    eprintln!("skipping: {reason} (opt-in; set OC_P10=1 and see tests/p10_* docs)");
}

/// The p10 artifact directory (evidence copies land in `goldens/p10/`).
pub fn artifacts() -> PathBuf {
    let dir = PathBuf::from("/tmp/opencode/p10-artifacts");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The golden directory for the battery.
pub fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens/p10")
}

/// The pinned reference source checkout (`/opt/opencode-ref`).
pub fn ref_source() -> PathBuf {
    PathBuf::from(
        std::env::var("PARITY_REF_SOURCE").unwrap_or_else(|_| "/opt/opencode-ref".to_owned()),
    )
}

/// Locate bun for auxiliary uses (TUI launch, sqlite inspection).
pub fn bun_path() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("PARITY_BUN") {
        return Ok(PathBuf::from(path));
    }
    for candidate in [
        "/usr/local/bin/bun",
        "/root/.bun/bin/bun",
        "/home/eternal130/.bun/bin/bun",
        "/usr/bin/bun",
    ] {
        if Path::new(candidate).is_file() {
            return Ok(PathBuf::from(candidate));
        }
    }
    std::env::var("PATH")
        .ok()
        .and_then(|paths| {
            paths.split(':').find_map(|dir| {
                let candidate = PathBuf::from(dir).join("bun");
                candidate.is_file().then_some(candidate)
            })
        })
        .ok_or_else(|| anyhow!("bun not found"))
}

/// Whether the runnable reference checkout is present (or rebuildable).
/// Strict-aware: under `OC_GATE_STRICT=1` an unusable reference side
/// fails the test instead of skipping (Gate 0; see `parity::gate`).
pub fn reference_runnable() -> bool {
    !parity::reference::skip_if_no_bun()
}

// ---------------------------------------------------------------------------
// Authed raw HTTP against either server (thin sugar over parity::minih).
// ---------------------------------------------------------------------------

/// One authed JSON API call. Returns `(status, parsed-body-or-string)`.
pub async fn api(
    base: &str,
    password: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, Value)> {
    let auth = minih::basic_auth("opencode", password);
    let response = minih::request(
        base,
        Some(&auth),
        method,
        path,
        body,
        "application/json",
        Duration::from_secs(30),
    )
    .await
    .with_context(|| format!("{method} {path}"))?;
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok((response.status, value))
}

/// Unwrap the `{"data": ...}` envelope of a successful response.
pub fn envelope(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

/// Poll an API call until the body satisfies `pred` (or fail with the tail).
#[allow(clippy::too_many_arguments)]
pub async fn wait_api<F>(
    base: &str,
    password: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
    pred: F,
    tries: usize,
    secs: u64,
) -> Result<Value>
where
    F: Fn(&Value) -> bool,
{
    let mut last = Value::Null;
    for _ in 0..tries {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        let (status, value) = api(base, password, method, path, body).await?;
        last = value.clone();
        if status == 200 && pred(&last) {
            return Ok(last);
        }
    }
    Err(anyhow!("timed out polling {method} {path}; last response: {last}"))
}

/// The id of the session present in the list but not in `known`.
pub async fn new_session_id(base: &str, password: &str, known: &[String]) -> Result<String> {
    let (status, value) = api(base, password, "GET", "/api/session", None).await?;
    anyhow::ensure!(status == 200, "session list status {status}: {value}");
    let rows = envelope(&value)
        .as_array()
        .cloned()
        .ok_or_else(|| anyhow!("session list data is not an array: {value}"))?;
    for row in &rows {
        if let Some(id) = row.get("id").and_then(Value::as_str) {
            if !known.iter().any(|known| known == id) {
                return Ok(id.to_owned());
            }
        }
    }
    Err(anyhow!("no new session in list of {} rows", rows.len()))
}

// ---------------------------------------------------------------------------
// The scripted fake LLM (tool calls need per-request scripts; the scenario
// FakeLlm is text-only). Same contract as the m2 one: scripts served in
// order, one per MAIN request (a request whose body carries a `tools`
// array); tool-less requests always get the last script.
// ---------------------------------------------------------------------------

pub struct ScriptedFakeLlm {
    port: u16,
}

impl ScriptedFakeLlm {
    pub async fn start(scripts: Vec<String>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.context("bind scripted fake llm")?;
        let port = listener.local_addr()?.port();
        let counter = Arc::new(AtomicUsize::new(0));
        tokio::spawn(async move {
            if let Err(error) = serve_fake(listener, scripts, counter).await {
                eprintln!("[p10] scripted fake llm ended: {error}");
            }
        });
        Ok(Self { port })
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

async fn serve_fake(
    listener: TcpListener,
    scripts: Vec<String>,
    counter: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let scripts = scripts.clone();
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut seen = 0;
            while seen < buffer.len() {
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    socket.read(buffer.as_mut_slice()),
                )
                .await
                {
                    Ok(Ok(0) | Err(_)) | Err(_) => break,
                    Ok(Ok(read)) => {
                        seen += read;
                        if let Some(header_end) =
                            find_subslice(&buffer[..seen], b"\r\n\r\n").map(|index| index + 4)
                        {
                            let text = String::from_utf8_lossy(&buffer[..header_end]);
                            let length = text
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("Content-Length:")
                                        .or_else(|| line.strip_prefix("content-length:"))
                                })
                                .and_then(|value| value.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            if seen >= header_end + length {
                                break;
                            }
                        }
                    }
                }
            }
            let wants_tools = find_subslice(&buffer, b"\"tools\"").is_some();
            let index =
                if wants_tools { counter.fetch_add(1, Ordering::SeqCst) } else { usize::MAX };
            let script =
                scripts.get(index).or_else(|| scripts.last()).expect("at least one script").clone();
            let response = format!(
                concat!(
                    "HTTP/1.1 200 OK\r\n",
                    "Content-Type: text/event-stream\r\n",
                    "Cache-Control: no-cache\r\n",
                    "Connection: close\r\n",
                    "\r\n",
                    "{script}",
                ),
                script = script,
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// A plain text turn (role + one content delta + stop + usage).
pub fn text_script(text: &str) -> String {
    let role = serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]}).to_string();
    let content = serde_json::json!({"choices":[{"delta":{"content":text}}]}).to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}
    })
    .to_string();
    format!("data: {role}\n\ndata: {content}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

/// A `read` tool-call turn (the engine executes it and comes back with the
/// tool result, consuming the next script).
pub fn read_tool_script(call_id: &str, path: &str) -> String {
    let arguments = serde_json::json!({"path": path}).to_string();
    let call = serde_json::json!({
        "choices":[{"delta":{"role":"assistant","tool_calls":[{
            "index":0,"id":call_id,"type":"function",
            "function":{"name":"read","arguments":arguments}
        }]}}]
    })
    .to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}
    })
    .to_string();
    format!("data: {call}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

/// The provider config pinning `parityfake/fake-model` as default.
pub fn provider_config(base: &str, second_model: bool) -> String {
    let models = if second_model {
        r#""fake-model": { "name": "Fake Model" },
        "fake-model-a": { "name": "Fake Model A" }"#
    } else {
        r#""fake-model": { "name": "Fake Model" }"#
    };
    format!(
        r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "parityfake/fake-model",
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{base}/v1" }},
      "models": {{ {models} }}
    }}
  }}
}}
"#
    )
}

// ---------------------------------------------------------------------------
// Known-red registry (PARITY-LOOP.md §8.5, B0-2).
// ---------------------------------------------------------------------------

/// The failure-message prefix for a mismatch already registered in
/// `goldens/p10/known-red.json` (still a FAILURE — strict mode never skips).
pub const KNOWN_RED_PREFIX: &str = "KNOWN-RED:";
/// The failure-message prefix for a mismatch with no (or expired) registry
/// entry: new reds go straight to red (§8.5).
pub const NEW_RED_PREFIX: &str = "NEW-RED:";

/// One registry entry in `goldens/p10/known-red.json`.
#[derive(Debug, Clone)]
pub struct KnownRedEntry {
    pub ledger_id: String,
    pub test: String,
    pub pattern: String,
    pub added: String,
    pub expiry_batch: String,
    pub owner: String,
}

/// Parse the checked-in registry (`goldens/p10/known-red.json`).
///
/// # Errors
/// When the registry file is missing or malformed — a broken registry is a
/// harness failure, never a silent pass.
pub fn known_red_registry() -> Result<Vec<KnownRedEntry>> {
    let path = golden_dir().join("known-red.json");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let rows = value
        .as_array()
        .ok_or_else(|| anyhow!("{} is not an array of registry entries", path.display()))?;
    let mut entries = Vec::new();
    for row in rows {
        let field = |name: &str| {
            row.get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("registry entry missing field {name:?}: {row}"))
        };
        entries.push(KnownRedEntry {
            ledger_id: field("ledger_id")?,
            test: field("test")?,
            pattern: field("pattern")?,
            added: field("added")?,
            expiry_batch: field("expiry_batch")?,
            owner: field("owner")?,
        });
    }
    Ok(entries)
}

/// Batch ordering for expiry checks (Batch 0 < 1A < 1B < 2 < 3 < 4; unknown
/// labels sort last so an unparseable expiry never silently expires).
fn batch_rank(batch: &str) -> usize {
    const ORDER: [&str; 6] = ["Batch 0", "Batch 1A", "Batch 1B", "Batch 2", "Batch 3", "Batch 4"];
    ORDER.iter().position(|known| *known == batch).unwrap_or(ORDER.len())
}

/// The current parity batch for expiry evaluation (override with
/// `PARITY_CURRENT_BATCH`); defaults to Batch 0, the batch that built this
/// skeleton.
fn current_batch() -> String {
    std::env::var("PARITY_CURRENT_BATCH").unwrap_or_else(|_| "Batch 0".to_owned())
}

/// How a ledger ID relates to the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedClass {
    /// Registered and not yet expired — failures surface as `KNOWN-RED:`.
    Known { expiry_batch: String },
    /// Missing, drifted, or expired — failures surface as `NEW-RED:` with
    /// the reason (§8.5: known-red can only shrink).
    New { reason: String },
}

/// A resolved known-red reference for one ledger ID (see [`known_red`]).
#[derive(Debug, Clone)]
pub struct KnownRed {
    /// The ledger ID the failing check is attributed to.
    pub ledger_id: String,
    /// The expiry batch the CALLER believes the entry carries; a mismatch
    /// with the registry is drift and is surfaced, not hidden.
    pub expected_expiry: String,
    /// The resolved registry state.
    pub class: RedClass,
}

/// Resolve `ledger_id` against `goldens/p10/known-red.json`.
///
/// Gate tests wrap their fallible run in this so a registered failure keeps
/// failing — with the message prefixed `KNOWN-RED:<ledger-id>:` — while a
/// missing/expired/drifted registration prefixes `NEW-RED:` instead (§8.5:
/// a skip is never a pass, and known-red may only shrink).
#[must_use]
pub fn known_red(ledger_id: &str, expiry_batch: &str) -> KnownRed {
    let class = match known_red_registry()
        .ok()
        .and_then(|entries| entries.into_iter().find(|entry| entry.ledger_id == ledger_id))
    {
        Some(entry) => {
            if entry.expiry_batch != expiry_batch {
                RedClass::New {
                    reason: format!(
                        "registry expiry_batch {:?} != caller-expected {:?}",
                        entry.expiry_batch, expiry_batch
                    ),
                }
            } else if batch_rank(&entry.expiry_batch) < batch_rank(&current_batch()) {
                RedClass::New {
                    reason: format!("registry entry expired at {:?}", entry.expiry_batch),
                }
            } else {
                RedClass::Known { expiry_batch: entry.expiry_batch }
            }
        }
        None => RedClass::New {
            reason: "no entry for this ledger id in goldens/p10/known-red.json".to_owned(),
        },
    };
    KnownRed { ledger_id: ledger_id.to_owned(), expected_expiry: expiry_batch.to_owned(), class }
}

impl KnownRed {
    /// Prefix a failure per the registry state. The result is STILL an
    /// `Err` — categorization changes the message, never the outcome.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)] // consumed for the {:#} chain
    pub fn categorize(&self, error: anyhow::Error) -> anyhow::Error {
        let detail = format!("{error:#}");
        match &self.class {
            RedClass::Known { expiry_batch } => anyhow!(
                "{KNOWN_RED_PREFIX}{}: {detail}\n(known red: registered in \
                 goldens/p10/known-red.json with expiry {expiry_batch}; this is still a \
                 failure — strict mode never skips; see PARITY-LOOP.md §8.5)",
                self.ledger_id
            ),
            RedClass::New { reason } => anyhow!(
                "{NEW_RED_PREFIX}{}: {detail}\n({reason}; triage via the parity ledger \
                 before registering — known-red may only shrink; see PARITY-LOOP.md §8.5)",
                self.ledger_id
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// bun:sqlite inspection (the battery already requires bun; reuse it to read
// the shared database without linking a sqlite driver into the harness).
// ---------------------------------------------------------------------------

/// Dump `{migrations: [{id, time_completed}], sessions: [{id, parent_id,
/// agent, model}]}` from an opencode database via `bun:sqlite`.
///
/// The database (plus any `-wal`/`-shm` siblings — opencode runs WAL) is
/// first copied into the artifact directory and the copy is inspected: the
/// servers are stopped at dump time, and a private copy keeps the evidence
/// file untouched for the next boot.
pub fn bun_db_dump(bun: &Path, db: &Path) -> Result<Value> {
    let scratch = artifacts().join(format!(
        "dbcopy-{}-{}.db",
        db.file_name().and_then(|n| n.to_str()).unwrap_or("db").replace('-', "_"),
        std::process::id()
    ));
    std::fs::copy(db, &scratch).with_context(|| format!("copy {}", db.display()))?;
    let stem = scratch.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    for suffix in ["-wal", "-shm"] {
        let side = db.with_file_name(format!(
            "{}{suffix}",
            db.file_name().and_then(|n| n.to_str()).unwrap_or_default()
        ));
        if side.is_file() {
            let _ = std::fs::copy(&side, scratch.with_file_name(format!("{stem}{suffix}")));
        }
    }
    let script = r#"
import { Database } from "bun:sqlite";
const db = new Database(process.argv[process.argv.length - 1]);
const migrations = db
  .query("SELECT id, time_completed FROM migration ORDER BY id")
  .all();
const sessions = db
  .query("SELECT id, parent_id, agent, model FROM session_v2 ORDER BY time_created, id")
  .all();
console.log(JSON.stringify({ migrations, sessions }));
"#;
    let output = Command::new(bun)
        .arg("-e")
        .arg(script)
        .arg(&scratch)
        .output()
        .with_context(|| format!("bun -e sqlite dump on {}", scratch.display()))?;
    anyhow::ensure!(
        output.status.success(),
        "bun sqlite dump failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).with_context(|| format!("parse bun dump: {text}"))
}
