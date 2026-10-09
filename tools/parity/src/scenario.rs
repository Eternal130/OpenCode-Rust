//! Scripted SSE scenario runner: boot the reference server, subscribe to
//! `/api/event` from t0, drive a scripted API flow (Basic auth), collect
//! frames until quiescence, normalize per the canonical sequence policy,
//! and golden the result. These goldens pin **reference** behavior; Phase 5
//! replays the same scripts against the Rust port and compares via
//! [`crate::sse::compare_sequences`] (or the golden capture form here,
//! [`crate::sse::compare_scenario_captures`]).
//!
//! ## Scenario model
//!
//! A scenario is a [`ScenarioSpec`] (name + collection budget) plus a list
//! of [`Step`]s executed strictly in order against one freshly spawned
//! [`crate::reference::ReferenceServer`]:
//!
//! - [`Step::request`] — one HTTP request (path and JSON body are
//!   `{capture}`-templated); the response status is checked (2xx unless
//!   pinned with [`.expect`](Step::expect)) and JSON pointers can be
//!   captured out of the response body ([`.capture`](Step::capture)).
//! - [`Step::write_file`] — create/modify a file (templated path +
//!   contents); used for config-file scenarios.
//! - [`Step::expect_frames`] — post-collection sanity floor: at least `n`
//!   data frames of the given `type` must have arrived. This runs in
//!   update mode too, so a broken scenario can never write an empty
//!   golden unnoticed.
//!
//! The driver pre-seeds two captures: `{project_dir}` (the sandbox project
//! directory, `<sandbox>/project`) and `{project_dir_q}` (percent-encoded,
//! for `?location[directory]=…` query parameters). Requests that need a
//! location use the body `location` field (POST /api/session) or the
//! `location[directory]` query (GET endpoints); session-scoped routes
//! resolve the session's own location server-side.
//!
//! ## Golden format
//!
//! `goldens/scenarios/<name>.json` stores
//! [`crate::sse::scenario_capture`]: the canonical grouping of
//! `NORMALIZATION.md` §8 with group keys dropped and groups sorted by
//! canonical serialization — a deterministic serialization of the §8.4
//! multiset. Run mode compares a fresh capture against the golden via
//! [`crate::sse::compare_scenario_captures`] (multiset semantics, first
//! divergent frame reported); `PARITY_UPDATE=1` (re)writes the golden.
//!
//! ## Deterministic model responses (no LLM keys)
//!
//! The reference's own `OPENCODE_SIMULATE` mode is **not** a turnkey fake
//! provider (see the module docs on [`FakeLlm`] for the full finding).
//! Scenarios that need a model instead seed a custom provider through the
//! project config (`<project>/opencode.json`):
//!
//! ```json
//! {
//!   "$schema": "https://opencode.ai/config.json",
//!   "providers": {
//!     "parityfake": {
//!       "package": "@opencode/ai/providers/openai-compatible",
//!       "settings": { "baseURL": "http://127.0.0.1:<port>/v1" },
//!       "models": { "fake-model": { "name": "Fake Model" } }
//!     }
//!   }
//! }
//! ```
//!
//! The `openai-compatible` package is a builtin of the reference build (no
//! npm fetch), so a config-seeded provider works fully offline. Its requests
//! go to `POST <baseURL>/chat/completions`, which [`FakeLlm`] answers with a
//! scripted `OpenAI`-compatible chat SSE stream — the whole prompt→message
//! flow then runs through the real session pipeline deterministically.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::golden;
use crate::minih;
use crate::normalize::NormEnv;
use crate::sse::{self, Frame, SseStream};

/// A server a [`ScenarioSpec`] can run against: the reference v2.0.24
/// server ([`crate::reference::ReferenceServer`]) or the Rust port
/// ([`crate::rust_server::RustServer`]) — the same scripts drive both.
pub trait ScenarioHost {
    /// Base URL, e.g. `http://127.0.0.1:54321`.
    fn url(&self) -> &str;

    /// The Basic-auth password for API requests (username is `opencode`).
    fn password(&self) -> &str;

    /// Sandbox directory backing this server (HOME, TMPDIR, logs, and the
    /// scenario project directory).
    fn sandbox(&self) -> &Path;

    /// A [`NormEnv`] preloaded with this server's sandbox roots (override
    /// to add roots).
    fn norm_env(&self) -> NormEnv {
        NormEnv::new().with_root(self.sandbox().to_string_lossy().to_string())
    }
}

impl ScenarioHost for crate::reference::ReferenceServer {
    fn url(&self) -> &str {
        self.url()
    }
    fn password(&self) -> &str {
        self.password()
    }
    fn sandbox(&self) -> &Path {
        self.sandbox()
    }
}

/// One scripted scenario: collection budget + identity. The steps are kept
/// separate (callers pass a `&[Step]`) so the spec stays data.
pub struct ScenarioSpec {
    /// Golden/stable name (`goldens/scenarios/<name>.json`).
    pub name: &'static str,
    /// One-line description stored beside the golden.
    pub description: &'static str,
    /// Scenario watermark: stop collecting after this many data frames.
    pub max_frames: usize,
    /// Quiescence window: stop when no data frame arrived for this long
    /// (must comfortably cover the reference config watcher's 100 ms
    /// debounce plus filesystem watcher latency).
    pub quiescence: Duration,
    /// Safety bound for the whole collection.
    pub overall: Duration,
}

impl ScenarioSpec {
    /// A spec with the default budget (512 frames, 3 s quiescence, 30 s
    /// overall) — generous enough for watcher debounces.
    #[must_use]
    pub fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            max_frames: 512,
            quiescence: Duration::from_millis(3000),
            overall: Duration::from_secs(30),
        }
    }
}

/// One scripted action against the server under test.
#[derive(Clone)]
pub enum Step {
    /// HTTP request. `path`/`body` are `{capture}`-templated.
    Request {
        method: &'static str,
        path: String,
        body: Option<Value>,
        /// Exact expected status; default is "any 2xx".
        expect_status: Option<u16>,
        /// `(name, json-pointer)` pairs extracted from the response body.
        captures: Vec<(&'static str, &'static str)>,
    },
    /// Create/overwrite a file (templated path and contents).
    WriteFile { path: String, contents: String },
    /// Pause between steps (milliseconds). Used to let debounced server
    /// reactions (e.g. the config watcher's 100 ms reload debounce) settle
    /// before the next step.
    SleepMs(u64),
    /// Sanity floor: at least `min` data frames of `frame_type` must have
    /// been captured (checked in update mode too).
    ExpectFrames { frame_type: &'static str, min: usize },
}

impl Step {
    /// A request step (defaults: any 2xx status, no captures).
    pub fn request(method: &'static str, path: impl Into<String>, body: Option<Value>) -> Self {
        Step::Request { method, path: path.into(), body, expect_status: None, captures: Vec::new() }
    }

    /// A request with no JSON body.
    pub fn request_nobody(method: &'static str, path: impl Into<String>) -> Self {
        Self::request(method, path, None)
    }

    /// Pin the expected HTTP status exactly.
    #[must_use]
    pub fn expect(mut self, status: u16) -> Self {
        if let Step::Request { expect_status, .. } = &mut self {
            *expect_status = Some(status);
        }
        self
    }

    /// Capture the string at `pointer` (RFC 6901) in the response body as
    /// `{name}` for later templating.
    #[must_use]
    pub fn capture(mut self, name: &'static str, pointer: &'static str) -> Self {
        if let Step::Request { captures, .. } = &mut self {
            captures.push((name, pointer));
        }
        self
    }

    /// Write `contents` to `path` (both `{capture}`-templated).
    pub fn write_file(path: impl Into<String>, contents: impl Into<String>) -> Self {
        Step::WriteFile { path: path.into(), contents: contents.into() }
    }

    /// Pause for `millis` between steps.
    #[must_use]
    pub fn sleep_ms(millis: u64) -> Self {
        Step::SleepMs(millis)
    }

    /// Require at least `min` frames of `frame_type` in the capture.
    #[must_use]
    pub fn expect_frames(frame_type: &'static str, min: usize) -> Self {
        Step::ExpectFrames { frame_type, min }
    }
}

/// Everything a scenario produced.
pub struct Outcome {
    /// Captured template values (includes the pre-seeded project-dir ones).
    pub captures: BTreeMap<String, String>,
    /// Raw collected data-carrying frames (heartbeats skipped), arrival
    /// order.
    pub frames: Vec<Frame>,
    /// [`sse::scenario_capture`] of `frames` — the golden form.
    pub groups: Vec<Value>,
}

/// The scenario golden directory (`goldens/scenarios`).
#[must_use]
pub fn golden_dir() -> PathBuf {
    golden::dir().join("scenarios")
}

/// Percent-encode for a query-parameter value (RFC 3986 unreserved kept).
#[must_use]
pub fn encode_query_value(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[usize::from(byte) >> 4] as char);
                out.push(HEX[usize::from(byte) & 0x0f] as char);
            }
        }
    }
    out
}

/// Run `steps` against a freshly-subscribed SSE stream on `server`, collect
/// per the spec, and return the capture. `server` must be a live
/// [`ScenarioHost`] (see [`drive_and_golden`] for the full test shape).
#[allow(clippy::too_many_lines)]
pub async fn drive(
    server: &impl ScenarioHost,
    spec: &ScenarioSpec,
    steps: &[Step],
) -> Result<Outcome> {
    let project = project_dir(server);
    std::fs::create_dir_all(&project).with_context(|| format!("create {}", project.display()))?;

    let mut captures = BTreeMap::new();
    captures.insert("project_dir".to_string(), project.to_string_lossy().into_owned());
    captures.insert("project_dir_q".to_string(), encode_query_value(&project.to_string_lossy()));

    // Subscribe from t0 so nothing published during the script is missed.
    let mut stream = SseStream::subscribe(
        server.url(),
        server.password(),
        "/api/event",
        Duration::from_secs(10),
    )
    .await
    .context("subscribe /api/event")?;

    let auth = minih::basic_auth("opencode", server.password());
    for (index, step) in steps.iter().enumerate() {
        match step {
            Step::Request { method, path, body, expect_status, captures: wanted } => {
                let path = render(path, &captures);
                let body_text = body.as_ref().map(|b| render(&b.to_string(), &captures));
                let response = minih::request(
                    server.url(),
                    Some(&auth),
                    method,
                    &path,
                    body_text.as_deref(),
                    "application/json",
                    Duration::from_secs(30),
                )
                .await
                .with_context(|| format!("step {index}: {method} {path}"))?;
                let ok = match expect_status {
                    Some(expected) => response.status == *expected,
                    None => (200..300).contains(&response.status),
                };
                if !ok {
                    let body = String::from_utf8_lossy(&response.body);
                    return Err(anyhow!(
                        "step {index}: {method} {path} returned HTTP {} (wanted {:?}): {}",
                        response.status,
                        expect_status,
                        body.chars().take(400).collect::<String>()
                    ));
                }
                let text = String::from_utf8_lossy(&response.body).into_owned();
                if !wanted.is_empty() {
                    let value: Value = serde_json::from_str(&text)
                        .with_context(|| format!("step {index}: non-JSON response: {text}"))?;
                    for (name, pointer) in wanted {
                        let found =
                            value.pointer(pointer).and_then(Value::as_str).with_context(|| {
                                format!(
                                    "step {index}: capture {name} at {pointer} not found in {text}"
                                )
                            })?;
                        captures.insert((*name).to_string(), found.to_string());
                    }
                }
            }
            Step::WriteFile { path, contents } => {
                let path = render(path, &captures);
                let contents = render(contents, &captures);
                if let Some(parent) = std::path::Path::new(&path).parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                std::fs::write(&path, contents.as_bytes())
                    .with_context(|| format!("step {index}: write {path}"))?;
            }
            Step::SleepMs(millis) => {
                tokio::time::sleep(Duration::from_millis(*millis)).await;
            }
            Step::ExpectFrames { .. } => {} // checked post-collection
        }
    }

    let frames = stream
        .collect_scenario(spec.max_frames, spec.quiescence, spec.overall)
        .await
        .context("collect scenario frames")?;

    // Sanity floors run in update mode too: a broken scenario must fail
    // loudly instead of writing a golden of nothing.
    for step in steps {
        if let Step::ExpectFrames { frame_type, min } = step {
            let have = count_frames_of_type(&frames, frame_type);
            if have < *min {
                let mut census = std::collections::BTreeMap::new();
                for frame in &frames {
                    if let Some(found) =
                        serde_json::from_str::<Value>(&frame.data).ok().and_then(|value| {
                            value.get("type").and_then(Value::as_str).map(str::to_string)
                        })
                    {
                        *census.entry(found).or_insert(0usize) += 1;
                    }
                }
                let mut failures = Vec::new();
                for frame in &frames {
                    if let Ok(value) = serde_json::from_str::<Value>(&frame.data) {
                        if value.get("type").and_then(Value::as_str)
                            == Some("session.execution.failed")
                        {
                            failures.push(value.to_string());
                        }
                    }
                }
                return Err(anyhow!(
                    "scenario {}: expected at least {min} frame(s) of type {frame_type}, captured {have} ({} total data frames; census: {census:?}; execution failures: {failures:?})",
                    spec.name,
                    frames.len()
                ));
            }
        }
    }

    let groups = sse::scenario_capture(&frames, &server.norm_env());
    Ok(Outcome { captures, frames, groups })
}

/// Run a scenario end-to-end (drive + golden assert). Returns the outcome
/// for extra caller-side assertions.
pub async fn drive_and_golden(
    server: &impl ScenarioHost,
    spec: &ScenarioSpec,
    steps: &[Step],
) -> Result<Outcome> {
    let outcome = drive(server, spec, steps).await?;
    assert_golden(spec, &outcome.groups)?;
    Ok(outcome)
}

/// Compare (or with `PARITY_UPDATE=1`, write) the scenario golden for
/// `spec` against the captured `groups`.
pub fn assert_golden(spec: &ScenarioSpec, groups: &[Value]) -> Result<()> {
    let value = json!({
        "scenario": spec.name,
        "description": spec.description,
        "groups": groups,
    });
    let path = golden_dir().join(format!("{}.json", spec.name));
    if golden::update_mode() || !path.is_file() {
        let previous = std::fs::read_to_string(&path).ok();
        std::fs::create_dir_all(golden_dir())
            .with_context(|| format!("create {}", golden_dir().display()))?;
        let rendered = format!("{}\n", serde_json::to_string_pretty(&value)?);
        std::fs::write(&path, rendered.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        if previous.as_deref() != Some(rendered.as_str()) {
            eprintln!("[parity] scenario golden {} written/updated", path.display());
        }
        return Ok(());
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let golden_value: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let golden_groups = golden_value
        .get("groups")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow!("golden {} has no groups array", path.display()))?;
    if let Some(diff) = sse::compare_scenario_captures(&golden_groups, groups) {
        anyhow::bail!(
            "scenario golden mismatch for {} ({} golden groups vs {} captured); {diff}; golden file: {} (refresh with PARITY_UPDATE=1)",
            spec.name,
            golden_groups.len(),
            groups.len(),
            path.display()
        );
    }
    Ok(())
}

/// The sandbox project directory scenarios operate on (inside the server's
/// sandbox, so every emitted path normalizes under `<SANDBOX>`).
#[must_use]
pub fn project_dir(server: &impl ScenarioHost) -> PathBuf {
    server.sandbox().join("project")
}

/// Count data frames whose payload `type` equals `frame_type`.
fn count_frames_of_type(frames: &[Frame], frame_type: &str) -> usize {
    frames
        .iter()
        .filter(|frame| {
            serde_json::from_str::<Value>(&frame.data)
                .ok()
                .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_string))
                .is_some_and(|found| found == frame_type)
        })
        .count()
}

/// Replace every `{capture}` token with its captured value.
fn render(template: &str, captures: &BTreeMap<String, String>) -> String {
    let mut out = template.to_string();
    for (key, value) in captures {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

/// A minimal deterministic OpenAI-compatible chat server on loopback for
/// the no-key prompt scenario. Every `POST */chat/completions` is answered
/// with the same scripted SSE stream (role delta, two content deltas,
/// `stop` finish with fixed usage, `[DONE]`), then the connection closes —
/// byte-identical across runs, so the whole prompt→message flow is
/// deterministic.
///
/// ## Why not `OPENCODE_SIMULATE`? (finding)
///
/// The reference's simulation mode (`packages/simulation`, enabled with
/// `OPENCODE_SIMULATE=1`, see `packages/cli/src/server-process.ts:103`)
/// replaces **all** outbound HTTP with a simulated route table:
///
/// - Without `OPENCODE_DRIVE` it answers only
///   `https://models.opencode.ai/api.json` (with `{}`) and denies every
///   other destination — hermetic networking, **no** LLM answering. It
///   would actually *break* a config-seeded local provider (localhost is
///   not in the route table).
/// - With `OPENCODE_DRIVE=1` (or a manifest under
///   `$XDG_STATE_HOME/opencode-drive/instances/<name>.json`) it
///   additionally registers a driver-answered `OpenAI` endpoint at
///   `https://api.openai.com/v1/chat/completions`: a control server
///   listens on `ws://127.0.0.1:40950` and an external controller speaks a
///   JSON-RPC websocket protocol (`simulation.handshake` → `llm.attach` →
///   receive `llm.request` → answer `llm.chunk` / `llm.finish`). Fully
///   deterministic in principle, but it needs a hand-rolled websocket
///   client in the harness plus a config-seeded `openai` provider — strictly
///   more machinery than this plain-HTTP fake for the same guarantee.
///
/// So: documented, not used. The config-seeded `openai-compatible`
/// provider + this fake achieves the deterministic prompt→message flow
/// with zero extra protocol code.
pub struct FakeLlm {
    port: u16,
}

impl FakeLlm {
    /// Start the fake on a free loopback port.
    pub async fn start() -> Result<Self> {
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.context("bind fake llm")?;
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Err(error) = serve(listener).await {
                eprintln!("[parity] fake llm server ended: {error}");
            }
        });
        Ok(Self { port })
    }

    /// Base URL for the provider config `settings.baseURL`.
    #[must_use]
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The ready-made provider config document pointing at this fake.
    #[must_use]
    pub fn provider_config(&self) -> String {
        format!(
            r#"{{
  "$schema": "https://opencode.ai/config.json",
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{}/v1" }},
      "models": {{ "fake-model": {{ "name": "Fake Model" }} }}
    }}
  }}
}}
"#,
            self.base()
        )
    }
}

async fn serve(listener: tokio::net::TcpListener) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const RESPONSE: &str = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: close\r\n",
        "\r\n",
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Parity fake says \"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello.\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":7,\"total_tokens\":18}}\n\n",
        "data: [DONE]\n\n",
    );

    loop {
        let (mut socket, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => return Err(error.into()),
        };
        // Serve every connection with the same script (a fresh task keeps
        // the accept loop free for concurrent requests).
        tokio::spawn(async move {
            let mut buffer = [0u8; 8192];
            // Read the request head plus body (requests are small and
            // content-length framed); a short read budget avoids hanging on
            // a silent peer.
            let mut seen = 0;
            while seen < buffer.len() {
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    socket.read(&mut buffer[seen..]),
                )
                .await
                {
                    Ok(Ok(0) | Err(_)) | Err(_) => break,
                    Ok(Ok(n)) => {
                        seen += n;
                        if let Some(head_end) = find_head_end(&buffer[..seen]) {
                            let body_needed = content_length(&buffer[..head_end]);
                            if seen >= head_end + body_needed {
                                break;
                            }
                        }
                    }
                }
            }
            let _ = socket.write_all(RESPONSE.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
    }
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n").map(|position| position + 4)
}

fn content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head);
    text.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The canonical scenario scripts
// ---------------------------------------------------------------------------

/// The canonical scenario scripts (the five goldens under
/// `goldens/scenarios/`). Shared by the reference replay
/// (`tests/scenarios.rs`) and the Rust-port replay
/// (`tests/rust_scenarios.rs`) so both sides run byte-identical steps.
pub mod scripts {
    use super::{FakeLlm, ScenarioSpec, Step};
    use serde_json::json;

    /// One named scenario: its spec and steps.
    pub struct Script {
        pub spec: ScenarioSpec,
        pub steps: Vec<Step>,
    }

    /// The canonical script for `name`, plus the [`FakeLlm`] it needs
    /// (`prompt_fake_llm`).
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines, clippy::must_use_candidate)]
    pub fn build(name: &str, fake: Option<&FakeLlm>) -> Option<Script> {
        let script = match name {
            "session_lifecycle" => Script {
                spec: ScenarioSpec::new(
                    "session_lifecycle",
                    "POST /api/session (create) -> PATCH rename -> GET list -> DELETE; session lifecycle frames",
                ),
                steps: vec![
                    Step::request(
                        "POST",
                        "/api/session",
                        Some(json!({
                            "title": "parity lifecycle",
                            "location": { "directory": "{project_dir}" }
                        })),
                    )
                    .capture("session_id", "/data/id"),
                    Step::request(
                        "PATCH",
                        "/api/session/{session_id}",
                        Some(json!({ "title": "parity renamed" })),
                    ),
                    Step::request_nobody("GET", "/api/session"),
                    Step::request_nobody("DELETE", "/api/session/{session_id}").expect(204),
                    Step::expect_frames("session.created", 1),
                    Step::expect_frames("session.renamed", 1),
                    Step::expect_frames("session.deleted", 1),
                ],
            },
            "config_touch" => Script {
                spec: ScenarioSpec::new(
                    "config_touch",
                    "boot project location -> create opencode.json -> modify it; config.updated frames",
                ),
                steps: vec![
                    Step::request_nobody("GET", "/api/location?location[directory]={project_dir_q}"),
                    Step::write_file(
                        "{project_dir}/opencode.json",
                        "{\n  \"username\": \"parity-one\"\n}\n",
                    ),
                    // Let the watcher's 100 ms reload debounce fire (and the
                    // reload's config.updated land) before the second write,
                    // so each modification gets its own reload.
                    Step::sleep_ms(800),
                    Step::write_file(
                        "{project_dir}/opencode.json",
                        "{\n  \"username\": \"parity-two\"\n}\n",
                    ),
                    Step::expect_frames("config.updated", 2),
                ],
            },
            "permission_saved" => Script {
                spec: ScenarioSpec::new(
                    "permission_saved",
                    "ask permission (save) -> reply always -> GET /api/permission/saved -> DELETE row",
                ),
                steps: vec![
                    Step::request(
                        "POST",
                        "/api/session",
                        Some(json!({
                            "title": "parity permission",
                            "location": { "directory": "{project_dir}" }
                        })),
                    )
                    .capture("session_id", "/data/id"),
                    // Warm-up ask (see the reference quirk note in
                    // tests/scenarios.rs): frame-less either way on the
                    // reference (cold Permission service short-circuits to
                    // deny) and on the port (warm service rules allow the
                    // read) — it makes the real ask below deterministic.
                    Step::request(
                        "POST",
                        "/api/session/{session_id}/permission",
                        Some(json!({ "action": "read", "resources": ["/etc/parity-warmup"] })),
                    ),
                    Step::request(
                        "POST",
                        "/api/session/{session_id}/permission",
                        Some(json!({
                            "action": "external_directory",
                            "resources": ["/etc/parity-readonly"],
                            "save": ["/etc/parity-readonly"]
                        })),
                    )
                    .capture("request_id", "/data/id"),
                    Step::request(
                        "POST",
                        "/api/session/{session_id}/permission/{request_id}/reply",
                        Some(json!({ "decision": "always" })),
                    )
                    .expect(204),
                    Step::request_nobody(
                        "GET",
                        "/api/permission/saved?location[directory]={project_dir_q}",
                    )
                    .capture("saved_id", "/data/0/id"),
                    Step::request_nobody("DELETE", "/api/permission/saved/{saved_id}").expect(204),
                    Step::expect_frames("permission.asked", 1),
                    Step::expect_frames("permission.replied", 1),
                ],
            },
            "location_reload" => Script {
                spec: ScenarioSpec::new(
                    "location_reload",
                    "boot project location -> POST /api/location/reload; location.shutdown frames",
                ),
                steps: vec![
                    Step::request_nobody("GET", "/api/location?location[directory]={project_dir_q}"),
                    Step::request_nobody("POST", "/api/location/reload").expect(204),
                    Step::expect_frames("location.shutdown", 1),
                ],
            },
            "prompt_fake_llm" => {
                let fake = fake?;
                Script {
                    spec: ScenarioSpec::new(
                        "prompt_fake_llm",
                        "config-seeded openai-compatible provider -> fake local LLM -> prompt/message flow frames",
                    ),
                    steps: vec![
                        Step::write_file("{project_dir}/opencode.json", fake.provider_config()),
                        Step::request(
                            "POST",
                            "/api/session",
                            Some(json!({
                                "title": "parity prompt",
                                "location": { "directory": "{project_dir}" },
                                "model": { "providerID": "parityfake", "id": "fake-model" }
                            })),
                        )
                        .capture("session_id", "/data/id"),
                        Step::request(
                            "POST",
                            "/api/session/{session_id}/prompt",
                            Some(json!({ "text": "Say hello." })),
                        ),
                        Step::expect_frames("session.text.delta", 1),
                    ],
                }
            }
            _ => return None,
        };
        Some(script)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_capture_tokens() {
        let mut captures = BTreeMap::new();
        captures.insert("session_id".to_string(), "ses_01JZ".to_string());
        captures.insert("n".to_string(), "7".to_string());
        assert_eq!(
            render("/api/session/{session_id}/form/{n}", &captures),
            "/api/session/ses_01JZ/form/7"
        );
        assert_eq!(render("{\"title\":\"{n} steps\"}", &captures), "{\"title\":\"7 steps\"}");
        // Unknown tokens stay untouched.
        assert_eq!(render("{nope}", &captures), "{nope}");
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode_query_value("/tmp/a b"), "%2Ftmp%2Fa%20b");
        assert_eq!(encode_query_value("plain-1.2~x"), "plain-1.2~x");
    }

    #[test]
    fn counts_frames_of_type() {
        let frames = vec![
            Frame {
                event: None,
                data: r#"{"id":"evt_x","type":"session.created","data":{}}"#.into(),
                comments: vec![],
            },
            Frame {
                event: None,
                data: r#"{"id":"evt_y","type":"session.renamed","data":{}}"#.into(),
                comments: vec![],
            },
            Frame { event: None, data: "not json".into(), comments: vec![] },
        ];
        assert_eq!(count_frames_of_type(&frames, "session.created"), 1);
        assert_eq!(count_frames_of_type(&frames, "session.deleted"), 0);
        assert_eq!(count_frames_of_type(&frames, "not json"), 0);
    }

    #[test]
    fn parses_content_length_from_fake_llm_requests() {
        let head = b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 42\r\n\r\n";
        assert_eq!(content_length(head), 42);
        assert_eq!(find_head_end(head), Some(head.len()));
        assert_eq!(content_length(b"GET / HTTP/1.1\r\n\r\n"), 0);
    }
}
