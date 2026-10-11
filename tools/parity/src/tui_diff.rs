//! SIDE-BY-SIDE DIFFERENTIAL TUI rig (lane/diff-rt) — the user-mandated
//! parity standard: run v2.0.24's TUI and our Rust TUI in the SAME
//! project directory, apply IDENTICAL operation scripts, and compare
//! the COMPLETE cell grids after every step.
//!
//! # Architecture
//!
//! ```text
//!                    ┌─ ref server (bun, parity::reference)  ←┐
//!   ONE FakeLlm SSE  │                                        │ same project dir
//!   stub (shared)  ──┤                                        │ + same config file
//!                    │                                        │ (opencode.json)
//!                    └─ rust server (oc-server)             ←┘
//!                            ▲                                  ▲
//!                            │ --server                         │ --server
//!   ┌─ PTY 80x24 / 120x30 ───┴──────────┐   ┌─ PTY same size ──┴─────────────┐
//!   │ v2.0.24 TUI (bun, /tmp/opencode/  │   │ our TUI (target/debug/opencode)│
//!   │ ref/packages/cli, own HOME)       │   │ own HOME                       │
//!   └────────────┬──────────────────────┘   └──────────────┬─────────────────┘
//!                │ identical bytes (keys/text/mouse/resize) │
//!                ▼                                           ▼
//!        VtStream grid (vt100)                      VtStream grid (vt100)
//!                └──────────────── diff per step ────────────────┘
//!                     EXACT (char+style) and STRUCTURAL (masked)
//! ```
//!
//! * **Reference runtime**: the v2.0.24 TUI launched exactly the way
//!   `tests/p10_cross_tui.rs` launches it (`bun run --cwd <ref>/
//!   packages/cli --define OPENCODE_VERSION="2.0.24" … src/index.ts
//!   --server <url>`), on a PTY we own. The ref checkout is
//!   self-healed by [`crate::reference::ensure_reference_checkout`]
//!   (tar copy from `/opt/opencode-ref` + `bun install`).
//! * **Same project directory**: both TUI children run with the SAME
//!   cwd — one seeded sandbox project (git repo + identical files) —
//!   so project names, titles and file trees match. Each side gets
//!   its OWN home/XDG roots (they cannot share DBs) and its OWN
//!   server; ONE shared `opencode.json` points BOTH stacks at ONE
//!   shared SSE stub with the SAME model id/name.
//! * **Identical operation scripts**: JSON scripts
//!   (`tui-diff/scripts/*.json`) feed the SAME bytes with the SAME
//!   pacing to both PTYs (keys, literal text, SGR mouse events,
//!   leader chords, waits, resizes). After every step both grids are
//!   captured and compared.
//!
//! # Comparison modes (reported separately)
//!
//! * **EXACT** — cell-by-cell over the union grid: `char` + style
//!   digest (fg/bg RGB or index + modifier bits). The bar is complete
//!   identity; NO content normalization is applied in this mode.
//! * **STRUCTURAL** — model-output text may differ between runs; the
//!   structural mode re-examines every EXACT mismatch under two
//!   documented normalizations: (1) a glyph mask (every alphanumeric
//!   char → `x`; punctuation, box-drawing, whitespace stay exact) and
//!   (2) color canonicalization — indexed colors resolve through the
//!   xterm-256 palette to RGB (v2 emits 256-indices, we emit
//!   truecolor) and then compare with a ±8/channel perceptual
//!   tolerance. Modifier bits and geometry are NEVER relaxed. Per-row
//!   run digests compare text extents, margins and vertical gaps. A
//!   step whose ONLY divergences pass this filter is *text-only*
//!   (structural pass); anything else (geometry drift, attribute
//!   drift, real color drift, chrome glyph drift) stays an EXACT
//!   mismatch. Chrome therefore remains byte-exact in EXACT mode and
//!   geometry-exact in STRUCT mode: neither mask rescues position,
//!   extent, attributes or layout.
//!
//! # Normalizations (minimal — this is the complete list)
//!
//! None in EXACT mode. In STRUCTURAL mode exactly two: the
//! alphanumeric glyph mask, and color canonicalization
//! (idx → xterm-256 RGB + ±8/channel tolerance; justification in
//! [`style_equal_structural`]). Cursor position is never compared
//! (grids carry cell contents only). Any additional normalization
//! must be justified in the report.
//!
//! # Why the vt100 parser was lifted
//!
//! `crates/oc-tui/tests/vt_harness.rs` is a test-binary module
//! (`#[path]`-included); test crates are not importable from
//! `tools/parity`, so the `Grid`/`VtStream` core is re-homed here
//! (same vt100 0.15 pin, same style digest) with the burst machinery
//! trimmed to what the differential needs (live `grid_now`, raw
//! mirror, stability waits).

#![allow(unsafe_code)] // pty(7)/TIOCSCTTY are only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::scenario::ScenarioHost;

// ---------------------------------------------------------------------------
// vt grid model (lifted from crates/oc-tui/tests/vt_harness.rs — see header)
// ---------------------------------------------------------------------------

/// One parsed screen cell: visible char + style digest (fg+bg+mods).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtCell {
    pub ch: char,
    pub style: u64,
}

/// A cell-matrix snapshot: char + style digest per cell, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<VtCell>,
}

fn style_digest(cell: &vt100::Cell) -> u64 {
    let pack = |color: vt100::Color| -> u64 {
        match color {
            vt100::Color::Default => 0,
            vt100::Color::Idx(i) => (1 << 8) | u64::from(i),
            vt100::Color::Rgb(r, g, b) => {
                (2 << 24) | (u64::from(r) << 16) | (u64::from(g) << 8) | u64::from(b)
            }
        }
    };
    let mods = u64::from(u8::from(cell.bold()))
        | u64::from(u8::from(cell.italic())) << 1
        | u64::from(u8::from(cell.underline())) << 2
        | u64::from(u8::from(cell.inverse())) << 3;
    (pack(cell.fgcolor()) << 30) | (pack(cell.bgcolor()) << 4) | mods
}

impl Grid {
    fn from_parser(parser: &vt100::Parser) -> Self {
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (usize::from(rows), usize::from(cols));
        let mut cells = Vec::with_capacity(rows * cols);
        for y in 0..rows {
            for x in 0..cols {
                cells.push(screen.cell(y as u16, x as u16).map_or(
                    VtCell { ch: ' ', style: 0 },
                    |cell| VtCell {
                        ch: cell.contents().chars().next().unwrap_or(' '),
                        style: style_digest(cell),
                    },
                ));
            }
        }
        Self { cols, rows, cells }
    }

    #[must_use]
    pub fn cell(&self, x: usize, y: usize) -> VtCell {
        self.cells.get(y * self.cols + x).copied().unwrap_or(VtCell { ch: ' ', style: 0 })
    }

    /// One row's text (right-trimmed).
    #[must_use]
    pub fn row_text(&self, y: usize) -> String {
        if y >= self.rows {
            return String::new();
        }
        let text: String = (0..self.cols).map(|x| self.cell(x, y).ch).collect();
        text.trim_end().to_owned()
    }

    /// Every row's text.
    #[must_use]
    pub fn text(&self) -> Vec<String> {
        (0..self.rows).map(|y| self.row_text(y)).collect()
    }

    /// Cells that changed between two grids (volatile-cell detection).
    #[must_use]
    pub fn diff_cells(&self, other: &Self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for y in 0..self.rows.min(other.rows) {
            for x in 0..self.cols.min(other.cols) {
                if self.cell(x, y) != other.cell(x, y) {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|cell| cell.ch == ' ' && cell.style == 0)
    }
}

/// The cell-grid stream: raw PTY bytes in, live grid state out.
pub struct VtStream {
    parser: vt100::Parser,
    last_feed_ms: Option<u64>,
    fed_bytes: usize,
}

impl VtStream {
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows as u16, cols as u16, 0),
            last_feed_ms: None,
            fed_bytes: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) {
        if bytes.is_empty() {
            return;
        }
        self.parser.process(bytes);
        self.fed_bytes += bytes.len();
        self.last_feed_ms = Some(now_ms);
    }

    #[must_use]
    pub fn grid_now(&self) -> Grid {
        Grid::from_parser(&self.parser)
    }

    #[must_use]
    pub fn last_feed_ms(&self) -> Option<u64> {
        self.last_feed_ms
    }

    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.fed_bytes
    }
}

// ---------------------------------------------------------------------------
// The shared SSE stub (one process, serving both stacks)
// ---------------------------------------------------------------------------

/// The deterministic reply scripts the stub can serve. Both stacks
/// parse `delta.reasoning` / `delta.reasoning_content` from
/// OpenAI-compatible streams (verified: ref `packages/ai/src/protocols/
/// openai-chat.ts` lines 151-154, our `oc-ai/src/protocols/openai_chat.rs`
/// lines 153-156), so reasoning deltas render thought blocks in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StubScript {
    /// One text turn.
    Text,
    /// Reasoning deltas then a text turn (thought block).
    Reasoning,
    /// Reasoning deltas then a LONG text turn (scroll surface).
    ReasoningLong,
}

impl StubScript {
    fn sse(&self) -> String {
        let ev = |v: &Value| format!("data: {}\n\n", v);
        let mut out = String::new();
        let role = serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]});
        out.push_str(&ev(&role));
        match self {
            StubScript::Text | StubScript::Reasoning | StubScript::ReasoningLong => {
                if matches!(self, StubScript::Reasoning | StubScript::ReasoningLong) {
                    let r1 = serde_json::json!({"choices":[{"delta":{"reasoning":"Let me think about this carefully."}}]});
                    let r2 =
                        serde_json::json!({"choices":[{"delta":{"reasoning":" Step by step."}}]});
                    out.push_str(&ev(&r1));
                    out.push_str(&ev(&r2));
                }
                if *self == StubScript::ReasoningLong {
                    for line in 0..32 {
                        let text =
                            format!("Long reply line {} of the parity scroll fixture.", line + 1);
                        let delta = serde_json::json!({"choices":[{"delta":{"content": format!("{text}\n")}}]});
                        out.push_str(&ev(&delta));
                    }
                } else {
                    let text = if *self == StubScript::Text {
                        "Parity fake says hello."
                    } else {
                        "Parity fake says hello after thinking."
                    };
                    let delta = serde_json::json!({"choices":[{"delta":{"content":text}}]});
                    out.push_str(&ev(&delta));
                }
                let finish = serde_json::json!({
                    "choices":[{"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}
                });
                out.push_str(&ev(&finish));
            }
        }
        out.push_str("data: [DONE]\n\n");
        out
    }
}

// ---------------------------------------------------------------------------
// The attribution-keyed subagent flow (corpus3)
// ---------------------------------------------------------------------------

/// Body-substring markers the subagent flow keys on. Attribution is
/// NEVER request order — corpus2 #27 showed cadence drift (an extra
/// title-path request on our side) misaligns any ordering scheme; the
/// markers below identify each flow stage from the request body
/// itself, valid for both stacks:
///
/// * the subagent child's first user message is
///   `"<FIRST_PROMPT_PREFIX>\n<prompt>"` on BOTH stacks (v2
///   `packages/core/src/tool/plugin/subagent.ts:209-212`; our port
///   `oc-core/src/subagent/subagent.rs` `FIRST_PROMPT_PREFIX`);
/// * primary (tool-bearing) requests carry `max_completion_tokens`,
///   title-generation requests do not (corpus2 #27 pinned this split
///   on both stacks);
/// * the main continuation carries the subagent's fixed reply text as
///   the `subagent` tool result.
pub mod flow_markers {
    /// Child-session first-prompt prefix (both stacks, byte-exact).
    pub const FIRST_PROMPT_PREFIX: &str = "You are a subagent spawned by another session.";
    /// The subagent's fixed final reply (also the continuation marker).
    pub const SUBAGENT_TEXT: &str = "Subagent fixture done.";
    /// The main session's final reply.
    pub const FINAL_TEXT: &str = "Main flow final reply.";
    /// The fixed title served to title-path requests.
    pub const TITLE_TEXT: &str = "Parity Subagent Flow";
    /// The `subagent` tool-call arguments served on the main request.
    /// `general` is the built-in subagent-mode agent present on BOTH
    /// stacks (ref `packages/core/src/plugin/agent.ts:88-95`, ours in
    /// the default agent table) — config-defined agents cannot be used
    /// here because ours does not apply the config `agents` map to the
    /// agent registry yet (corpus3 finding; see the REPORT addendum).
    pub const TOOL_AGENT: &str = "general";
    pub const TOOL_DESCRIPTION: &str = "Check the fixture";
    pub const TOOL_PROMPT: &str = "PARITY-SUBAGENT-TASK inspect the seeded fixture and report.";
}

/// Wrap SSE `data:` events with the `[DONE]` terminator (byte-identical
/// framing to [`StubScript::sse`]).
fn sse_wrap(events: &[String]) -> String {
    let mut out = events.join("");
    out.push_str("data: [DONE]\n\n");
    out
}

fn sse_role() -> String {
    format!("data: {}\n\n", serde_json::json!({"choices":[{"delta":{"role":"assistant"}}]}))
}

fn sse_reasoning(text: &str) -> String {
    format!("data: {}\n\n", serde_json::json!({"choices":[{"delta":{"reasoning": text}}]}))
}

fn sse_content(text: &str) -> String {
    format!("data: {}\n\n", serde_json::json!({"choices":[{"delta":{"content": text}}]}))
}

fn sse_finish(reason: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({
            "choices":[{"delta":{},"finish_reason": reason}],
            "usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}
        })
    )
}

/// One `delta.tool_calls` entry (index-keyed accumulation, the shape
/// both openai-chat parsers consume — ref `packages/ai/src/protocols/
/// openai-chat.ts:1072+`, our `oc-ai/src/protocols/openai_chat.rs`).
fn sse_tool_call(id: &str, name: &str, arguments: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id": id,"function":{"name": name,"arguments": arguments}}
        ]}}]})
    )
}

/// The four SSE payloads of the subagent flow (corpus3). The main
/// payload answers with a `subagent` TOOL CALL (v2 tool name:
/// `packages/core/src/tool/plugin/subagent.ts:10`
/// `export const name = "subagent"`); the engine then spawns the child
/// session, whose own request (attribution: [`flow_markers::
/// FIRST_PROMPT_PREFIX`]) gets reasoning + a short reply; the main
/// continuation (tool result present) gets the final text; title-path
/// requests get a fixed short title so tab titles compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowScripts {
    pub main: String,
    pub subagent: String,
    pub continuation: String,
    pub title: String,
}

/// Build the deterministic subagent-flow payloads.
#[must_use]
pub fn subagent_flow_scripts() -> FlowScripts {
    let arguments = serde_json::json!({
        "agent": flow_markers::TOOL_AGENT,
        "description": flow_markers::TOOL_DESCRIPTION,
        "prompt": flow_markers::TOOL_PROMPT,
    })
    .to_string();
    let main = sse_wrap(&[
        sse_role(),
        sse_tool_call("call_sub_parity_1", "subagent", &arguments),
        sse_finish("tool_calls"),
    ]);
    let subagent = sse_wrap(&[
        sse_role(),
        sse_reasoning("Subagent thinking about the fixture."),
        sse_content(flow_markers::SUBAGENT_TEXT),
        sse_finish("stop"),
    ]);
    let continuation =
        sse_wrap(&[sse_role(), sse_content(flow_markers::FINAL_TEXT), sse_finish("stop")]);
    let title = sse_wrap(&[sse_role(), sse_content(flow_markers::TITLE_TEXT), sse_finish("stop")]);
    FlowScripts { main, subagent, continuation, title }
}

/// Classify one main-request body against the flow (see [`SharedStub`]).
/// Returns the SSE payload and the attribution kind for the recording.
///
/// Single-turn by design: the continuation marker (`SUBAGENT_TEXT`)
/// lives in the conversation history from the second exchange on, so
/// any later primary request would classify as a continuation. Every
/// corpus3 script submits exactly once into a fresh session (fresh
/// servers per run), and a misclassification's failure mode is benign
/// (the terminal final-text reply), so the constraint is documented
/// rather than enforced.
fn classify_flow_request(body: &[u8], flow: &FlowScripts) -> (String, &'static str) {
    let contains = |needle: &str| body.windows(needle.len()).any(|w| w == needle.as_bytes());
    if contains(flow_markers::FIRST_PROMPT_PREFIX) {
        (flow.subagent.clone(), "subagent")
    } else if contains("max_completion_tokens") {
        if contains(flow_markers::SUBAGENT_TEXT) {
            (flow.continuation.clone(), "continuation")
        } else {
            (flow.main.clone(), "main")
        }
    } else {
        (flow.title.clone(), "title")
    }
}

/// ONE OpenAI-compatible SSE stub serving BOTH stacks. The active
/// script is swapped between rig runs; every main request (body
/// carrying `"messages"`) gets the active script verbatim, so replies
/// are byte-identical on both sides and across runs. When a
/// [`FlowScripts`] is armed instead (corpus3), each main request is
/// answered by ATTRIBUTION (body-substring matching, never request
/// order — see [`flow_markers`]).
pub struct SharedStub {
    pub url: String,
    active: Arc<Mutex<String>>,
    flow: Arc<Mutex<Option<FlowScripts>>>,
    main_requests: Arc<AtomicUsize>,
    shutdown: Arc<AtomicUsize>,
    /// Request-path parity: recorded shapes of main requests (the
    /// transport/options surface — everything EXCEPT `messages`, whose
    /// text legitimately differs per stack's system prompt).
    recorded: Arc<Mutex<Vec<RecordedRequest>>>,
}

/// The transport/options shape of one main request: the sorted
/// top-level body keys minus `messages`/`tools`, the option values
/// (temperature and friends), the model id, the user-agent (the
/// only attribution hint the shared stub gets — both stacks hit it
/// from the same config), and — for flow runs — the stub's
/// body-substring attribution (`main`/`subagent`/`continuation`/
/// `title`), which makes request-count drift per flow stage visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub keys: Vec<String>,
    pub options: Vec<(String, String)>,
    pub model: String,
    pub user_agent: String,
    pub kind: String,
}

impl RecordedRequest {
    fn shape(&self) -> String {
        format!(
            "kind={} model={} keys=[{}] options={{{}}} ua={}",
            self.kind,
            self.model,
            self.keys.join(","),
            self.options.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(","),
            self.user_agent
        )
    }
}

/// Option-ish keys whose VALUES are recorded (request-path parity:
/// did each stack forward the config's per-model options?).
const OPTION_KEYS: [&str; 6] =
    ["temperature", "top_p", "top_k", "max_tokens", "reasoning_effort", "frequency_penalty"];

impl SharedStub {
    /// Start the stub (background thread; std::net like the vt-harness
    /// FakeLlm — no runtime entanglement with the async rig driver).
    pub fn start() -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").context("bind stub")?;
        let addr = listener.local_addr().context("stub addr")?;
        let active = Arc::new(Mutex::new(StubScript::Text.sse()));
        let flow = Arc::new(Mutex::new(None));
        let main_requests = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let thread_active = Arc::clone(&active);
        let thread_flow = Arc::clone(&flow);
        let thread_requests = Arc::clone(&main_requests);
        let thread_shutdown = Arc::clone(&shutdown);
        let thread_recorded = Arc::clone(&recorded);
        std::thread::spawn(move || {
            listener.set_nonblocking(true).expect("stub nonblocking");
            while thread_shutdown.load(Ordering::SeqCst) == 0 {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let active = Arc::clone(&thread_active);
                        let flow = Arc::clone(&thread_flow);
                        let requests = Arc::clone(&thread_requests);
                        let recorded = Arc::clone(&thread_recorded);
                        std::thread::spawn(move || {
                            stub_handle(socket, &active, &flow, &requests, &recorded)
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self { url: format!("http://{addr}"), active, flow, main_requests, shutdown, recorded })
    }

    pub fn arm(&self, script: StubScript) {
        *self.flow.lock().expect("stub flow") = None;
        *self.active.lock().expect("stub script") = script.sse();
    }

    /// Arm the attribution-keyed subagent flow (corpus3); clears the
    /// single-script arm and vice versa.
    pub fn arm_flow(&self, flow: FlowScripts) {
        *self.active.lock().expect("stub script") = String::new();
        *self.flow.lock().expect("stub flow") = Some(flow);
    }

    #[must_use]
    pub fn main_request_count(&self) -> usize {
        self.main_requests.load(Ordering::SeqCst)
    }

    /// Drain the recorded main-request shapes (the rig snapshots per
    /// script run: fresh servers per run, so nothing leaks across).
    #[must_use]
    pub fn take_recorded(&self) -> Vec<RecordedRequest> {
        std::mem::take(&mut *self.recorded.lock().expect("stub recorded"))
    }

    pub fn stop(&self) {
        self.shutdown.store(1, Ordering::SeqCst);
    }
}

fn stub_handle(
    mut socket: std::net::TcpStream,
    active: &Arc<Mutex<String>>,
    flow: &Arc<Mutex<Option<FlowScripts>>>,
    main_requests: &Arc<AtomicUsize>,
    recorded: &Arc<Mutex<Vec<RecordedRequest>>>,
) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut seen = 0;
    let header_end;
    let length;
    // Read the request head (+ body if content-length present).
    loop {
        match socket.read(&mut buffer[seen..]) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                seen += count;
                if let Some(found) = buffer[..seen].windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buffer[..found]).to_string();
                    let parsed = head
                        .lines()
                        .find_map(|line| {
                            line.split(':')
                                .next()
                                .map(|k| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|is| is.then_some(line))
                        })
                        .and_then(|line| line.split(':').nth(1))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if seen >= found + 4 + parsed {
                        header_end = found;
                        length = parsed;
                        break;
                    }
                }
            }
        }
    }
    let is_main = buffer[..seen].windows(10).any(|w| w == b"\"messages\"");
    if is_main {
        main_requests.fetch_add(1, Ordering::SeqCst);
        // Request-path parity: record the transport/options shape.
        let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let user_agent = head
            .lines()
            .find(|line| line.len() > 11 && line[..11].eq_ignore_ascii_case("user-agent:"))
            .map_or_else(|| "(none)".to_owned(), |line| line[11..].trim().to_owned());
        let body = &buffer[header_end + 4..header_end + 4 + length];
        // Flow attribution decides the SSE payload (corpus3); the
        // single-script arm is the fallback. The kind lands in the
        // recording so per-stage request-count drift is visible.
        let armed_flow = flow.lock().expect("stub flow").clone();
        let (script, kind) = match &armed_flow {
            Some(flow) => classify_flow_request(body, flow),
            None => (active.lock().expect("stub script").clone(), "main"),
        };
        let shape = serde_json::from_slice::<serde_json::Value>(body).ok().and_then(|json| {
            let object = json.as_object()?;
            let mut keys: Vec<String> = object
                .keys()
                .filter(|key| key.as_str() != "messages" && key.as_str() != "tools")
                .cloned()
                .collect();
            keys.sort();
            let options = OPTION_KEYS
                .iter()
                .filter_map(|key| {
                    object.get(*key).map(|value| ((*key).to_owned(), value.to_string()))
                })
                .collect::<Vec<_>>();
            let model = object.get("model").and_then(serde_json::Value::as_str)?.to_owned();
            Some(RecordedRequest { keys, options, model, user_agent, kind: kind.to_owned() })
        });
        if let Some(shape) = shape {
            recorded.lock().expect("stub recorded").push(shape);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n{script}"
        );
        let _ = socket.write_all(response.as_bytes());
        let _ = socket.shutdown(std::net::Shutdown::Write);
        // Drain briefly so the client sees a clean EOF after [DONE].
        let _ = socket.read(&mut buffer);
    } else {
        let body = r#"{"data":[]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// PTY child (the symmetric spawn: any program on a fresh pty)
// ---------------------------------------------------------------------------

/// A child process running on a PTY we own, with a vt100 grid mirror
/// of everything it printed and a writer for input bytes.
pub struct PtyChild {
    child: std::process::Child,
    master: File,
    vt: Arc<Mutex<VtStream>>,
    raw: Arc<Mutex<Vec<u8>>>,
    writer: File,
    start: Instant,
    _ctty_keepalive: File,
}

impl PtyChild {
    /// Spawn `program argv` with `cwd`/`env` on a fresh `cols×rows`
    /// pty. The slave becomes the child's stdio AND controlling
    /// terminal (setsid + TIOCSCTTY in pre_exec), so SIGWINCH reaches
    /// it on resize and it behaves exactly as under a real terminal.
    pub fn spawn(
        program: &Path,
        argv: &[String],
        cwd: &Path,
        env: &[(String, String)],
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        let master = unsafe {
            let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            if fd < 0 {
                return Err(anyhow!("posix_openpt failed"));
            }
            libc::grantpt(fd);
            libc::unlockpt(fd);
            File::from_raw_fd(fd)
        };
        let slave_path = unsafe {
            let name = libc::ptsname(master.as_raw_fd());
            if name.is_null() {
                return Err(anyhow!("ptsname failed"));
            }
            std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)
            .with_context(|| format!("open pty slave {slave_path}"))?;
        set_winsize(&master, cols, rows);
        let ctty = slave.try_clone().context("dup ctty fd")?;
        let ctty_fd = ctty.as_raw_fd();

        let mut command = std::process::Command::new(program);
        unsafe {
            command
                .args(argv)
                .current_dir(cwd)
                .env_clear()
                .stdin(slave.try_clone().context("stdin slave")?)
                .stdout(slave.try_clone().context("stdout slave")?)
                .stderr(slave)
                .pre_exec(move || {
                    // pre_exec runs post-fork/pre-exec: async-signal-safe calls only.
                    if libc::setsid() < 0 {
                        // Already a session leader (can happen under odd
                        // spawners); acquiring the ctty still needs a new
                        // session, so this is fatal.
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::ioctl(ctty_fd, libc::TIOCSCTTY, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
        }
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command.spawn().with_context(|| format!("spawn {}", program.display()))?;

        let vt = Arc::new(Mutex::new(VtStream::new(usize::from(cols), usize::from(rows))));
        let raw: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let reader_master = master.try_clone().context("clone pty reader")?;
        let feed_vt = Arc::clone(&vt);
        let feed_raw = Arc::clone(&raw);
        let start = Instant::now();
        {
            let thread_start = start;
            std::thread::spawn(move || {
                let mut reader = reader_master;
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            let now_ms = thread_start.elapsed().as_millis() as u64;
                            feed_vt.lock().expect("vt").feed(&chunk[..count], now_ms);
                            feed_raw.lock().expect("raw").extend_from_slice(&chunk[..count]);
                        }
                    }
                }
            });
        }
        let writer = master.try_clone().context("clone pty writer")?;
        Ok(Self { child, master, vt, raw, writer, start, _ctty_keepalive: ctty })
    }

    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        set_winsize(&self.master, cols, rows);
        // The vt mirror must track the new grid shape too.
        *self.vt.lock().expect("vt") = VtStream::new(usize::from(cols), usize::from(rows));
    }

    #[must_use]
    pub fn grid_now(&self) -> Grid {
        self.vt.lock().expect("vt").grid_now()
    }

    #[must_use]
    pub fn raw_mirror(&self) -> Vec<u8> {
        self.raw.lock().expect("raw").clone()
    }

    #[must_use]
    pub fn raw_len(&self) -> usize {
        self.raw.lock().expect("raw").len()
    }

    /// Wait until `needle` appears in bytes appended after `from`.
    #[must_use]
    pub fn wait_for_after(&self, needle: &[u8], from: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let data = self.raw.lock().expect("raw").clone();
            if data.len() > from && data[from..].windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        false
    }

    /// Wait for cell-stability: two grids `probe_ms` apart differing in
    /// at most `tolerance` cells (the caret blink is 1-2 cells).
    #[must_use]
    pub fn wait_grid_stable(&self, tolerance: usize, probe_ms: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let first = self.grid_now();
            std::thread::sleep(Duration::from_millis(probe_ms));
            let second = self.grid_now();
            if first.diff_cells(&second).len() <= tolerance {
                return true;
            }
        }
        false
    }

    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().map(|status| status.is_none()).unwrap_or(false)
    }

    /// Terminate: SIGTERM to the group (filtered allowlist), then
    /// SIGKILL the leader; close the pty.
    pub fn kill(&mut self) {
        let pid = self.child.id();
        let _ = oc_util::procutil::signal_group_filtered(
            pid,
            libc::SIGTERM,
            &["bun", "opencode", "oc-server", "node", "sh", "cargo"],
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && self.is_running() {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Closing the master hangs up the session (SIGHUP belt+braces).
        let replacement = File::open("/dev/null")
            .unwrap_or_else(|_| unsafe { File::from_raw_fd(libc::dup(libc::STDOUT_FILENO)) });
        let master = std::mem::replace(&mut self.master, replacement);
        drop(master);
    }
}

fn set_winsize(master: &File, cols: u16, rows: u16) {
    unsafe {
        let winsize = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
    }
}

/// Encode one SGR mouse event (same encoding `ScriptStep::payload`
/// uses for single mouse ops).
fn sgr_mouse(code: u32, x: u16, y: u16, final_byte: u8) -> Vec<u8> {
    format!("\x1b[<{};{};{}{}", code, x + 1, y + 1, final_byte as char).into_bytes()
}

fn side_alive(side: &mut Option<PtyChild>) -> bool {
    side.as_mut().is_some_and(PtyChild::is_running)
}

fn write_both(ref_side: &mut Option<PtyChild>, our_side: &mut Option<PtyChild>, bytes: &[u8]) {
    if let Some(side) = ref_side.as_mut() {
        side.write(bytes);
    }
    if let Some(side) = our_side.as_mut() {
        side.write(bytes);
    }
}

/// Feed a sweep step to both PTYs and return the coverage summary.
///
/// * `motion` — an SGR hover-move for EVERY grid cell, batched (the
///   mandate's allowed mode: per-move settling is unaffordable at
///   1920+ cells); the step's normal settle+capture follows.
/// * `click` — press+release for every listed cell with a per-click
///   settle probe and a per-side reaction matrix (grid changed by
///   more than the caret-blink budget of 2 cells), which is the
///   click-parity signal; cells where exactly one side reacts are
///   listed.
///
/// Skipped cells (a TUI died mid-sweep) are counted per side.
async fn run_sweep(
    sweep: &SweepOp,
    cols: u16,
    rows: u16,
    ref_side: &mut Option<PtyChild>,
    our_side: &mut Option<PtyChild>,
) -> String {
    if sweep.action == "click" {
        click_sweep(sweep, cols, rows, ref_side, our_side).await
    } else {
        motion_sweep(sweep, cols, rows, ref_side, our_side).await
    }
}

/// The click class executor (see [`run_sweep`]).
async fn click_sweep(
    sweep: &SweepOp,
    cols: u16,
    rows: u16,
    ref_side: &mut Option<PtyChild>,
    our_side: &mut Option<PtyChild>,
) -> String {
    let mut agree = 0usize;
    let mut ref_only = Vec::new();
    let mut our_only = Vec::new();
    let mut neither = 0usize;
    for &(x, y) in &sweep.cells {
        if x >= cols || y >= rows {
            continue;
        }
        let pre_ref = ref_side.as_ref().map(PtyChild::grid_now);
        let pre_our = our_side.as_ref().map(PtyChild::grid_now);
        // A real click always follows a move — and hover-gated
        // affordances (the strip's ✕) need the motion to arm.
        // The reaction matrix therefore measures the composite
        // move+press+release interaction, documented as such.
        write_both(ref_side, our_side, &sgr_mouse(35, x, y, b'M'));
        tokio::time::sleep(Duration::from_millis(60)).await;
        write_both(ref_side, our_side, &sgr_mouse(0, x, y, b'M'));
        tokio::time::sleep(Duration::from_millis(80)).await;
        write_both(ref_side, our_side, &sgr_mouse(0, x, y, b'm'));
        let budget = Duration::from_millis(sweep.settle_ms.max(120));
        if let Some(side) = ref_side.as_ref() {
            let _ = side.wait_grid_stable(2, 120, budget);
        }
        if let Some(side) = our_side.as_ref() {
            let _ = side.wait_grid_stable(2, 120, budget);
        }
        let reacted = |pre: Option<Grid>, side: &Option<PtyChild>| match (pre, side.as_ref()) {
            (Some(pre), Some(side)) => pre.diff_cells(&side.grid_now()).len() > 2,
            _ => false,
        };
        let hit_ref = reacted(pre_ref, ref_side);
        let hit_our = reacted(pre_our, our_side);
        match (hit_ref, hit_our) {
            (true, true) => agree += 1,
            (true, false) => ref_only.push(format!("{x},{y}")),
            (false, true) => our_only.push(format!("{x},{y}")),
            (false, false) => neither += 1,
        }
    }
    format!(
        "click sweep: {} clicks — reacted both={}, neither={}, REF-only=[{}] OUR-only=[{}]",
        sweep.cells.len(),
        agree,
        neither,
        ref_only.join(" "),
        our_only.join(" ")
    )
}

/// The motion-sweep executor (see [`run_sweep`]).
async fn motion_sweep(
    sweep: &SweepOp,
    cols: u16,
    rows: u16,
    ref_side: &mut Option<PtyChild>,
    our_side: &mut Option<PtyChild>,
) -> String {
    let total = usize::from(cols) * usize::from(rows);
    let mut fed_ref = 0usize;
    let mut fed_our = 0usize;
    'rows: for y in 0..rows {
        if !side_alive(ref_side) && !side_alive(our_side) {
            break 'rows;
        }
        for x in 0..cols {
            write_both(ref_side, our_side, &sgr_mouse(35, x, y, b'M'));
            if side_alive(ref_side) {
                fed_ref += 1;
            }
            if side_alive(our_side) {
                fed_our += 1;
            }
            tokio::time::sleep(Duration::from_millis(sweep.gap_ms.min(200))).await;
        }
    }
    format!(
        "motion sweep: {total} cells — fed ref={fed_ref} our={fed_our}, skipped ref={} our={}",
        total - fed_ref,
        total - fed_our
    )
}

// ---------------------------------------------------------------------------
// Operation scripts (JSON)
// ---------------------------------------------------------------------------

/// One script step. Exactly one of `keys` / `text` / `mouse` /
/// `resize` / `sweep` is set (a bare `wait_ms` step only
/// settles+captures).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptStep {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub mouse: Option<MouseOp>,
    #[serde(default)]
    pub resize: Option<ResizeOp>,
    #[serde(default)]
    pub sweep: Option<SweepOp>,
    /// Settle budget after the op (ms).
    #[serde(default = "default_step_wait")]
    pub wait_ms: u64,
}

fn default_step_wait() -> u64 {
    800
}

/// Grid-wide mouse coverage (the hover-sweep / click-sweep mandate):
/// `motion` feeds an SGR mouse-move for EVERY cell of the grid (batch
/// settle afterwards — per-move settling is unaffordable at 1920+
/// cells, the mandate explicitly allows batching); `click` feeds a
/// press+release for every listed cell with a per-click settle probe
/// and a per-side reaction matrix (did THIS side's grid change),
/// which is the click-parity signal. Skipped cells (child died
/// mid-sweep) are counted and reported.
#[derive(Debug, Clone, Deserialize)]
pub struct SweepOp {
    /// `motion` | `click`.
    pub action: String,
    /// Cells to click, `[[x, y], …]` (click mode). Motion mode always
    /// covers the whole grid.
    #[serde(default)]
    pub cells: Vec<(u16, u16)>,
    /// Gap between generated motion events (motion mode), ms.
    #[serde(default = "default_sweep_gap")]
    pub gap_ms: u64,
    /// Press→release gap and per-click settle budget (click mode), ms.
    #[serde(default = "default_click_settle")]
    pub settle_ms: u64,
}

fn default_sweep_gap() -> u64 {
    15
}
fn default_click_settle() -> u64 {
    400
}

#[derive(Debug, Clone, Deserialize)]
pub struct MouseOp {
    /// `motion` | `press` | `release`.
    pub action: String,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResizeOp {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Script {
    pub id: String,
    #[serde(default)]
    pub description: String,
    /// Which stub reply script to arm (`text` | `reasoning` |
    /// `reasoning-long` | `subagent-flow` — the attribution-keyed
    /// multi-request flow, corpus3).
    #[serde(default = "default_stub")]
    pub stub: String,
    /// Primary comparison mode for the report (`exact` | `struct`).
    /// Both modes are always computed; this only picks the headline.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Optional per-script size override `WxH`: the script runs at
    /// EXACTLY this size instead of the corpus sizes (`--sizes`
    /// remains the global knob). Needed when a feature is only
    /// observable off the default grid — e.g. the session sidebar's
    /// v2 auto breakpoint (`width − verticalTabsWidth > 120`,
    /// session-frame.tsx:107) is unreachable at 80/120 cols.
    #[serde(default)]
    pub size: Option<String>,
    pub steps: Vec<ScriptStep>,
}

fn default_stub() -> String {
    "text".to_owned()
}
fn default_mode() -> String {
    "exact".to_owned()
}

/// Parse a `WxH` size spec into `(cols, rows)` (shared by the
/// runner's `--sizes` flag and the per-script `size` override).
#[must_use]
pub fn parse_size_spec(spec: &str) -> Option<(u16, u16)> {
    let (w, h) = spec.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// Encode one key token to its terminal byte sequence.
pub fn encode_key(token: &str) -> Result<Vec<u8>> {
    let bytes = match token {
        "enter" | "return" => b"\r".to_vec(),
        "esc" | "escape" => b"\x1b".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "space" => b" ".to_vec(),
        "leader" => b"\x18".to_vec(), // ctrl+x, the v2 default leader
        "alt+up" => b"\x1b[1;3A".to_vec(),
        "alt+down" => b"\x1b[1;3B".to_vec(),
        "alt+right" => b"\x1b[1;3C".to_vec(),
        "alt+left" => b"\x1b[1;3D".to_vec(),
        other => {
            if let Some(letter) = other.strip_prefix("ctrl+") {
                let mut chars = letter.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    let lower = ch.to_ascii_lowercase();
                    if lower.is_ascii_lowercase() {
                        vec![(lower as u8) - b'a' + 1]
                    } else {
                        return Err(anyhow!("unsupported ctrl token: {other}"));
                    }
                } else {
                    return Err(anyhow!("unsupported ctrl token: {other}"));
                }
            } else if let Some(literal) = other.strip_prefix("raw:") {
                literal.to_owned().into_bytes()
            } else {
                return Err(anyhow!("unknown key token: {other}"));
            }
        }
    };
    Ok(bytes)
}

impl ScriptStep {
    /// The bytes this step feeds to both PTYs (empty for wait-only).
    pub fn payload(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for token in &self.keys {
            out.extend_from_slice(&encode_key(token)?);
        }
        if let Some(text) = &self.text {
            out.extend_from_slice(text.as_bytes());
        }
        if let Some(mouse) = &self.mouse {
            let (code, final_byte) = match mouse.action.as_str() {
                "motion" => (35, b'M'),
                "press" => (0, b'M'),
                "release" => (0, b'm'),
                other => return Err(anyhow!("unknown mouse action: {other}")),
            };
            out.extend_from_slice(
                format!("\x1b[<{};{};{}{}", code, mouse.x + 1, mouse.y + 1, final_byte as char)
                    .as_bytes(),
            );
        }
        Ok(out)
    }

    /// Human description for the report.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.keys.is_empty() {
            parts.push(format!("keys[{}]", self.keys.join(",")));
        }
        if let Some(text) = &self.text {
            parts.push(format!("text({text:?})"));
        }
        if let Some(mouse) = &self.mouse {
            parts.push(format!("mouse({} @{},{})", mouse.action, mouse.x, mouse.y));
        }
        if let Some(resize) = &self.resize {
            parts.push(format!("resize({}x{})", resize.cols, resize.rows));
        }
        if let Some(sweep) = &self.sweep {
            parts.push(match sweep.action.as_str() {
                "click" => format!("sweep(click, {} cells)", sweep.cells.len()),
                _ => "sweep(motion, whole grid)".to_owned(),
            });
        }
        if parts.is_empty() {
            "wait".to_owned()
        } else {
            parts.join(" + ")
        }
    }
}

// ---------------------------------------------------------------------------
// Diffs: EXACT and STRUCTURAL
// ---------------------------------------------------------------------------

/// One mismatching cell (EXACT mode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMiss {
    pub x: usize,
    pub y: usize,
    pub ref_cell: VtCell,
    pub our_cell: VtCell,
}

/// EXACT diff over the union grid: char + style digest per cell.
#[derive(Debug, Clone, Default)]
pub struct ExactDiff {
    pub mismatched_cells: usize,
    pub char_mismatches: usize,
    pub style_mismatches: usize,
    pub first: Option<CellMiss>,
    /// Up to 16 sample mismatches (row-major) for the report.
    pub samples: Vec<CellMiss>,
}

fn exact_diff(ref_grid: &Grid, our_grid: &Grid) -> ExactDiff {
    let mut diff = ExactDiff::default();
    for y in 0..ref_grid.rows.max(our_grid.rows) {
        for x in 0..ref_grid.cols.max(our_grid.cols) {
            let (a, b) = (ref_grid.cell(x, y), our_grid.cell(x, y));
            if a == b {
                continue;
            }
            diff.mismatched_cells += 1;
            if a.ch != b.ch {
                diff.char_mismatches += 1;
            }
            if a.style != b.style {
                diff.style_mismatches += 1;
            }
            let miss = CellMiss { x, y, ref_cell: a, our_cell: b };
            if diff.first.is_none() {
                diff.first = Some(miss.clone());
            }
            if diff.samples.len() < 16 {
                diff.samples.push(miss);
            }
        }
    }
    diff
}

/// The glyph mask: every alphanumeric char becomes `x`; everything
/// else (punctuation, box-drawing, whitespace) stays exact. Styles are
/// NOT masked — structural parity still requires identical styling.
fn mask_char(ch: char) -> char {
    if ch.is_alphanumeric() {
        'x'
    } else {
        ch
    }
}

/// Unpack one packed color to canonical RGB: indexed colors resolve
/// through the xterm-256 palette (`tui_frame::xterm256_rgb`), truecolor
/// passes through. `None` = terminal-default (unknowable RGB — only
/// equal to another default).
fn canonical_rgb(packed: u64) -> Option<(u8, u8, u8)> {
    if packed == 0 {
        return None;
    }
    if packed >> 8 == 1 {
        return Some(crate::tui_frame::xterm256_rgb((packed & 0xFF) as u8));
    }
    if packed >> 24 == 2 {
        return Some((
            ((packed >> 16) & 0xFF) as u8,
            ((packed >> 8) & 0xFF) as u8,
            (packed & 0xFF) as u8,
        ));
    }
    None
}

/// STRUCT-mode style equality — the SECOND structural normalization
/// (see the module header): attributes must match exactly; colors are
/// canonicalized (idx → RGB via the xterm-256 palette, removing the
/// emitter-encoding difference: v2 emits 256-indices, we emit
/// truecolor) and then compared with a ±8/channel perceptual
/// tolerance. Justification: without this, a whole-screen
/// encoding/value drift (idx232=#080808 vs our #0A0A0A background)
/// marks EVERY cell as a style mismatch and the text/geometry
/// classification the structural mode exists for can never trigger.
/// EXACT mode is untouched — byte identity remains the bar there.
fn style_equal_structural(a: u64, b: u64) -> bool {
    if a & 0xF != b & 0xF {
        return false; // modifier bits differ
    }
    for shift in [30, 4] {
        let (ca, cb) =
            (canonical_rgb((a >> shift) & 0x3FFF_FFFF), canonical_rgb((b >> shift) & 0x3FFF_FFFF));
        match (ca, cb) {
            (None, None) => {}
            (Some((r1, g1, b1)), Some((r2, g2, b2))) => {
                if r1.abs_diff(r2) > 8 || g1.abs_diff(g2) > 8 || b1.abs_diff(b2) > 8 {
                    return false;
                }
            }
            _ => return false, // default vs explicit color
        }
    }
    true
}

fn masked_equal(a: VtCell, b: VtCell) -> bool {
    mask_char(a.ch) == mask_char(b.ch) && style_equal_structural(a.style, b.style)
}

/// Decode a style digest to a human-readable form for the report.
#[must_use]
pub fn decode_style(style: u64) -> String {
    let unpack = |packed: u64| -> String {
        if packed == 0 {
            "default".to_owned()
        } else if packed >> 8 == 1 {
            format!("idx{}", packed & 0xFF)
        } else if packed >> 24 == 2 {
            format!(
                "#{:02X}{:02X}{:02X}",
                (packed >> 16) & 0xFF,
                (packed >> 8) & 0xFF,
                packed & 0xFF
            )
        } else {
            format!("?{packed}")
        }
    };
    let fg = unpack(style >> 30);
    let bg = unpack((style & 0x3FFF_FFF0) >> 4);
    let mut mods = Vec::new();
    if style & 1 != 0 {
        mods.push("bold");
    }
    if style & 2 != 0 {
        mods.push("italic");
    }
    if style & 4 != 0 {
        mods.push("underline");
    }
    if style & 8 != 0 {
        mods.push("inverse");
    }
    format!(
        "fg={fg} bg={bg}{}",
        if mods.is_empty() { String::new() } else { format!(" [{}]", mods.join(",")) }
    )
}

/// Per-row runs of masked text (`x` runs): `(row, [(start_col, len)])`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureDigest {
    pub rows: usize,
    pub cols: usize,
    pub runs: Vec<(usize, Vec<(usize, usize)>)>,
    /// Row indices that carry at least one run.
    pub text_rows: Vec<usize>,
}

impl StructureDigest {
    /// Vertical gaps between consecutive text rows (the thought-header
    /// ↔ content distance shows up here), plus margins per text row.
    #[must_use]
    pub fn geometry(&self) -> Vec<(usize, usize, Option<usize>)> {
        self.text_rows
            .iter()
            .map(|row| {
                let runs = &self.runs.iter().find(|(r, _)| r == row).expect("row runs").1;
                let margin = runs.first().map(|(start, _)| *start);
                let width: usize = runs.iter().map(|(_, len)| len).sum();
                (*row, width, margin)
            })
            .collect()
    }
}

fn structure_digest(grid: &Grid) -> StructureDigest {
    let mut runs = Vec::new();
    let mut text_rows = Vec::new();
    for y in 0..grid.rows {
        let mut row_runs = Vec::new();
        let mut run: Option<(usize, usize)> = None;
        for x in 0..grid.cols {
            if mask_char(grid.cell(x, y).ch) == 'x' {
                run = Some(match run {
                    Some((start, len)) => (start, len + 1),
                    None => (x, 1),
                });
            } else if let Some(entry) = run.take() {
                row_runs.push(entry);
            }
        }
        if let Some(entry) = run.take() {
            row_runs.push(entry);
        }
        if !row_runs.is_empty() {
            text_rows.push(y);
        }
        runs.push((y, row_runs));
    }
    StructureDigest { rows: grid.rows, cols: grid.cols, runs, text_rows }
}

/// STRUCTURAL analysis of an EXACT mismatch: which mismatches are
/// pure glyph-text drift (masked-equal, same geometry+style), and
/// where the run digests first diverge.
#[derive(Debug, Clone, Default)]
pub struct StructDiff {
    /// Mismatches that masked-compare equal (glyph text only).
    pub text_only_cells: usize,
    /// Mismatches that survive masking (geometry/style/chrome drift).
    pub residual_cells: usize,
    pub residual_first: Option<CellMiss>,
    /// True when every EXACT mismatch was text-only.
    pub text_only: bool,
    pub digest_ref: Option<StructureDigest>,
    pub digest_our: Option<StructureDigest>,
    /// First row where the run digests differ.
    pub digest_first_divergence: Option<usize>,
}

fn struct_diff(ref_grid: &Grid, our_grid: &Grid, exact: &ExactDiff) -> StructDiff {
    let _ = exact;
    let mut out = StructDiff::default();
    let mut text_only_cells = 0usize;
    let mut residual_cells = 0usize;
    let mut residual_first = None;
    for y in 0..ref_grid.rows.max(our_grid.rows) {
        for x in 0..ref_grid.cols.max(our_grid.cols) {
            let (a, b) = (ref_grid.cell(x, y), our_grid.cell(x, y));
            if a == b {
                continue;
            }
            if masked_equal(a, b) {
                text_only_cells += 1;
            } else {
                residual_cells += 1;
                if residual_first.is_none() {
                    residual_first = Some(CellMiss { x, y, ref_cell: a, our_cell: b });
                }
            }
        }
    }
    let digest_ref = structure_digest(ref_grid);
    let digest_our = structure_digest(our_grid);
    let digest_first_divergence = (0..digest_ref.runs.len().max(digest_our.runs.len())).find(|y| {
        let a = digest_ref.runs.get(*y).map(|(_, r)| r.clone());
        let b = digest_our.runs.get(*y).map(|(_, r)| r.clone());
        a != b
    });
    out.text_only_cells = text_only_cells;
    out.residual_cells = residual_cells;
    out.residual_first = residual_first;
    out.text_only = exact.mismatched_cells > 0 && residual_cells == 0;
    out.digest_ref = Some(digest_ref);
    out.digest_our = Some(digest_our);
    out.digest_first_divergence = digest_first_divergence;
    out
}

// ---------------------------------------------------------------------------
// Step / script outcomes
// ---------------------------------------------------------------------------

/// Classification of one step (headline; details stay in the diffs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepClass {
    Identical,
    TextOnly,
    Mismatch,
}

/// One compared step.
#[derive(Debug, Clone)]
pub struct StepOutcome {
    pub step_index: usize,
    pub label: String,
    pub op: String,
    pub settled_ref: bool,
    pub settled_our: bool,
    pub class: StepClass,
    pub exact: ExactDiff,
    pub structural: StructDiff,
    pub ref_text: Vec<String>,
    pub our_text: Vec<String>,
    /// Sweep coverage / click-parity summary (hover-sweep and
    /// click-sweep steps only; `None` for ordinary steps).
    pub coverage: Option<String>,
}

impl StepOutcome {
    #[must_use]
    pub fn class_name(&self) -> &'static str {
        match self.class {
            StepClass::Identical => "identical",
            StepClass::TextOnly => "text-only",
            StepClass::Mismatch => "MISMATCH",
        }
    }
}

/// One (script × size × run) execution.
#[derive(Debug, Clone)]
pub struct ScriptRun {
    pub script_id: String,
    pub mode: String,
    pub cols: u16,
    pub rows: u16,
    pub run: usize,
    pub steps: Vec<StepOutcome>,
    /// Driver-level failure (boot failure, spawn failure …).
    pub error: Option<String>,
    pub booted_ref: bool,
    pub booted_our: bool,
    /// Main-request shapes the stub recorded during this run
    /// (request-path parity; see `RecordedRequest`).
    pub requests: Vec<RecordedRequest>,
}

impl ScriptRun {
    #[must_use]
    pub fn mismatch_steps(&self) -> usize {
        self.steps.iter().filter(|s| s.class == StepClass::Mismatch).count()
    }
    #[must_use]
    pub fn text_only_steps(&self) -> usize {
        self.steps.iter().filter(|s| s.class == StepClass::TextOnly).count()
    }
    #[must_use]
    pub fn identical_steps(&self) -> usize {
        self.steps.iter().filter(|s| s.class == StepClass::Identical).count()
    }
}

// ---------------------------------------------------------------------------
// The rig
// ---------------------------------------------------------------------------

/// Rig knobs.
pub struct RigConfig {
    pub scripts_dir: PathBuf,
    pub out_dir: PathBuf,
    pub sizes: Vec<(u16, u16)>,
    pub runs: usize,
    pub script_filter: Option<String>,
    /// Skip the ref side entirely (driver smoke mode).
    pub no_ref: bool,
}

impl RigConfig {
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            scripts_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tui-diff/scripts"),
            out_dir: PathBuf::from("/tmp/opencode/tui-diff"),
            sizes: vec![(80, 24), (120, 30)],
            runs: 1,
            script_filter: None,
            no_ref: false,
        }
    }
}

pub fn load_scripts(dir: &Path, filter: &Option<String>) -> Result<Vec<Script>> {
    let mut scripts = Vec::new();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("read scripts dir {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    paths.sort();
    for path in paths {
        if let Some(filter) = filter {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            // Exact stem, exact path, or stem PREFIX (corpus3: the
            // three `subagent-*` scripts run under `--filter subagent`).
            if stem != filter && !stem.starts_with(filter) && path.to_string_lossy() != *filter {
                continue;
            }
        }
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let script: Script =
            serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
        scripts.push(script);
    }
    if scripts.is_empty() {
        return Err(anyhow!("no scripts found in {}", dir.display()));
    }
    Ok(scripts)
}

/// Seed the shared sandbox project: identical files + a git repo, so
/// project names, titles and file trees match on both sides. The
/// corpus3 subagent flow targets the BUILT-IN `general` subagent agent
/// (present on both stacks), so no agent config is seeded — ours does
/// not apply the config `agents` map to the agent registry yet (the
/// corpus3 REPORT addendum registers that gap from a live probe).
fn seed_project(project: &Path, stub_url: &str) -> Result<()> {
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(project.join("README.md"), "parity diff project\n\nSeeded fixture.\n")?;
    std::fs::write(
        project.join("src").join("main.rs"),
        "fn main() {\n    println!(\"parity fixture\");\n}\n",
    )?;
    std::fs::write(
        project.join("notes.md"),
        "notes for the diff rig\n- seeded\n- identical on both sides\n",
    )?;
    // The ONE config both stacks read (same provider, same model id).
    // The model-level `body` overlay is v2's sanctioned way to force a
    // wire field (config ModelOverlays.body merges into the provider
    // request body) — request-path parity checks whether each stack
    // forwards it (`temperature: 0` must appear in the stub-recorded
    // request shape).
    let config = format!(
        r#"{{
  "$schema": "https://opencode.ai/config.json",
  "model": "parityfake/fake-model",
  "providers": {{
    "parityfake": {{
      "package": "@opencode/ai/providers/openai-compatible",
      "settings": {{ "baseURL": "{stub_url}/v1" }},
      "models": {{ "fake-model": {{ "name": "Fake Model", "body": {{ "temperature": 0 }} }} }}
    }}
  }}
}}
"#
    );
    std::fs::write(project.join("opencode.json"), config)?;
    // The ref TUI (bun) resolves its JSX/tsconfig from the PROCESS
    // CWD: with cwd = our sandbox project it finds no tsconfig and
    // defaults to react-jsx (module-not-found, observed live). A
    // self-contained solid-js tsconfig in the project root restores
    // the checkout's transform settings; our side ignores it. Both
    // sides see the identical project.
    std::fs::write(
        project.join("tsconfig.json"),
        r#"{
  "$schema": "https://json.schemastore.org/tsconfig",
  "compilerOptions": {
    "jsx": "preserve",
    "jsxImportSource": "@opentui/solid",
    "lib": ["ESNext", "DOM", "DOM.Iterable", "DOM.AsyncIterable"],
    "module": "esnext",
    "moduleResolution": "bundler",
    "target": "esnext",
    "types": ["bun"]
  }
}
"#,
    )?;
    // git repo (optional: recorded in the report when unavailable).
    let git =
        std::process::Command::new("git").args(["init", "--quiet"]).current_dir(project).output();
    match git {
        Ok(output) if output.status.success() => {
            let _ = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.email=parity@opencode.ai",
                    "-c",
                    "user.name=Parity Rig",
                    "add",
                    "-A",
                ])
                .current_dir(project)
                .output();
            let _ = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.email=parity@opencode.ai",
                    "-c",
                    "user.name=Parity Rig",
                    "commit",
                    "--quiet",
                    "-m",
                    "seed",
                ])
                .current_dir(project)
                .output();
        }
        _ => eprintln!("[tui-diff] git unavailable; project seeded without a repo"),
    }
    Ok(())
}

fn home_env(home: &Path, password: &str, config: &Path) -> Vec<(String, String)> {
    vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("HOME".to_owned(), home.display().to_string()),
        ("OPENCODE_TEST_HOME".to_owned(), home.display().to_string()),
        ("XDG_DATA_HOME".to_owned(), home.join(".local/share").display().to_string()),
        ("XDG_STATE_HOME".to_owned(), home.join(".local/state").display().to_string()),
        ("XDG_CONFIG_HOME".to_owned(), home.join(".config").display().to_string()),
        ("XDG_CACHE_HOME".to_owned(), home.join(".cache").display().to_string()),
        ("TMPDIR".to_owned(), home.join("tmp").display().to_string()),
        ("OPENCODE_PASSWORD".to_owned(), password.to_owned()),
        ("OPENCODE_CONFIG".to_owned(), config.display().to_string()),
        ("PATH".to_owned(), "/root/.bun/bin:/usr/local/bin:/usr/bin:/bin".to_owned()),
        // Determinism: no locale surprises in either stack.
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("LC_ALL".to_owned(), "C.UTF-8".to_owned()),
    ]
}

/// The alternate-screen leave sequence — the TUI exit footprint.
pub const ALT_SCREEN_LEAVE: &[u8] = b"\x1b[?1049l";

/// The reference checkout root (env override like the rest of the
/// parity harness; default `/tmp/opencode/ref`).
pub fn ref_checkout_root() -> PathBuf {
    std::env::var("PARITY_REF_ROOT")
        .map_or_else(|_| PathBuf::from("/tmp/opencode/ref"), PathBuf::from)
}

/// Run ONE script once at ONE size. Spawns fresh servers + TUIs,
/// replays the steps, compares after each step, tears everything down.
pub async fn run_script_once(
    script: &Script,
    cols: u16,
    rows: u16,
    run: usize,
    stub: &SharedStub,
    cfg: &RigConfig,
    bun: &Path,
    cli: &Path,
) -> ScriptRun {
    let mut outcome = ScriptRun {
        script_id: script.id.clone(),
        mode: script.mode.clone(),
        cols,
        rows,
        run,
        steps: Vec::new(),
        error: None,
        booted_ref: false,
        booted_our: false,
        requests: Vec::new(),
    };
    // Request-path parity: start from a clean recording window.
    let _ = stub.take_recorded();
    let tag = format!("{}-{}x{}-r{}", script.id, cols, rows, run);
    let root = cfg.out_dir.join(tag);
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    let ref_home = root.join("ref-home");
    let our_home = root.join("our-home");
    for (path, dirs) in [
        (&ref_home, [".config", ".local/share", ".local/state", ".cache", "tmp"]),
        (&our_home, [".config", ".local/share", ".local/state", ".cache", "tmp"]),
    ] {
        for dir in dirs {
            let _ = std::fs::create_dir_all(path.join(dir));
        }
    }
    if let Err(error) = seed_project(&project, &stub.url) {
        outcome.error = Some(format!("seed project: {error}"));
        return outcome;
    }
    match script.stub.as_str() {
        "reasoning" => stub.arm(StubScript::Reasoning),
        "reasoning-long" => stub.arm(StubScript::ReasoningLong),
        "subagent-flow" => stub.arm_flow(subagent_flow_scripts()),
        _ => stub.arm(StubScript::Text),
    }

    // Servers: fresh per run so restored tabs/sessions never leak
    // across scripts (determinism requirement).
    let config_path = project.join("opencode.json");
    let config_env = config_path.display().to_string();
    let mut ref_server = None;
    if !cfg.no_ref {
        match crate::reference::spawn_reference(crate::reference::ReferenceSpawn {
            extra_env: vec![("OPENCODE_CONFIG", config_env.as_str())],
            seed_files: Vec::new(),
        })
        .await
        {
            Ok(server) => ref_server = Some(server),
            Err(error) => {
                outcome.error = Some(format!("ref server: {error}"));
                return outcome;
            }
        }
    }
    let mut our_server = match crate::rust_server::spawn_rust(crate::rust_server::RustSpawn {
        extra_env: vec![("OPENCODE_CONFIG", config_env.as_str())],
        seed_files: Vec::new(),
    })
    .await
    {
        Ok(server) => Some(server),
        Err(error) => {
            outcome.error = Some(format!("rust server: {error}"));
            if let Some(server) = ref_server.take() {
                let _ = server.stop().await;
            }
            return outcome;
        }
    };

    // TUIs: identical env shape, own homes, same cwd (the project).
    // The ref TUI is launched like p10_cross_tui, EXCEPT `bun run
    // --cwd` is avoided: `--cwd` reparents the process cwd to
    // packages/cli (observed live), which would break the
    // same-project-directory invariant. Plain `bun <abs>/src/index.ts`
    // keeps cwd = the project while resolving imports from the
    // checkout's node_modules normally.
    let ref_entry = ref_checkout_root().join("packages/cli/src/index.ts");
    let ref_preload =
        ref_checkout_root().join("packages/cli/node_modules/@opentui/solid/scripts/preload.js");
    let ref_url = ref_server.as_ref().map(|s| s.url().to_owned()).unwrap_or_default();
    let our_url = our_server.as_ref().map(|s| s.url().to_owned()).unwrap_or_default();
    let ref_argv = vec![
        "--preload".to_owned(),
        ref_preload.display().to_string(),
        "--define".to_owned(),
        "OPENCODE_VERSION=\"2.0.24\"".to_owned(),
        "--define".to_owned(),
        "OPENCODE_CHANNEL=\"stable\"".to_owned(),
        "--define".to_owned(),
        "OPENCODE_ARTIFACT=\"cli\"".to_owned(),
        ref_entry.display().to_string(),
        "--server".to_owned(),
        ref_url,
    ];
    let ref_side = (!cfg.no_ref)
        .then(|| {
            let server = ref_server.as_ref().unwrap();
            PtyChild::spawn(
                bun,
                &ref_argv,
                &project,
                &home_env(&ref_home, server.password(), &config_path),
                cols,
                rows,
            )
        })
        .transpose();
    let our_side = our_server.as_ref().and_then(|server| {
        PtyChild::spawn(
            cli,
            &["--server".to_owned(), our_url],
            &project,
            &home_env(&our_home, server.password(), &config_path),
            cols,
            rows,
        )
        .ok()
    });

    let (mut ref_side, mut our_side): (Option<PtyChild>, Option<PtyChild>) =
        match (ref_side, our_side) {
            (ref_side, Some(our_side)) => (ref_side.ok().flatten(), Some(our_side)),
            (ref_side, our_side) => {
                outcome.error = Some(format!(
                    "spawn TUIs (ref: ok={}, ours: ok={})",
                    ref_side.is_ok(),
                    our_side.is_some()
                ));
                if let Ok(Some(mut side)) = ref_side {
                    side.kill();
                }
                if let Some(mut side) = our_side {
                    side.kill();
                }
                if let Some(server) = ref_server.take() {
                    let _ = server.stop().await;
                }
                if let Some(server) = our_server.take() {
                    let _ = server.stop().await;
                }
                return outcome;
            }
        };

    // Boot marker: wait for non-blank grids on both sides (bounded).
    let boot_deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < boot_deadline {
        let ref_painted =
            ref_side.as_ref().is_some_and(|s| s.grid_now().cells.iter().any(|c| c.ch != ' '));
        let our_painted =
            our_side.as_ref().is_some_and(|s| s.grid_now().cells.iter().any(|c| c.ch != ' '));
        if ref_painted && our_painted {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    outcome.booted_ref =
        ref_side.as_ref().is_some_and(|s| s.grid_now().cells.iter().any(|c| c.ch != ' '));
    outcome.booted_our =
        our_side.as_ref().is_some_and(|s| s.grid_now().cells.iter().any(|c| c.ch != ' '));
    if let Some(side) = ref_side.as_ref() {
        if !outcome.booted_ref {
            dump_side_failure(&root, side, "ref");
        }
    }
    if let Some(side) = our_side.as_ref() {
        if !outcome.booted_our {
            dump_side_failure(&root, side, "our");
        }
    }

    // Steps: identical bytes, same pacing, both PTYs; compare after.
    for (index, step) in script.steps.iter().enumerate() {
        let payload = step.payload().unwrap_or_default();
        let coverage = if let Some(sweep) = step.sweep.as_ref() {
            // Sweep steps feed the PTYs themselves (per-cell captures
            // for clicks); the final settle+capture below closes them.
            Some(run_sweep(sweep, cols, rows, &mut ref_side, &mut our_side).await)
        } else {
            None
        };
        if let Some(side) = ref_side.as_mut() {
            if !payload.is_empty() {
                side.write(&payload);
            }
            if let Some(resize) = &step.resize {
                side.resize(resize.cols, resize.rows);
            }
        }
        if let Some(side) = our_side.as_mut() {
            if !payload.is_empty() {
                side.write(&payload);
            }
            if let Some(resize) = &step.resize {
                side.resize(resize.cols, resize.rows);
            }
        }
        let budget = Duration::from_millis(step.wait_ms.min(20_000));
        tokio::time::sleep(budget).await;
        let settled_ref = ref_side
            .as_ref()
            .is_some_and(|side| side.wait_grid_stable(2, 250, Duration::from_secs(5)));
        let settled_our = our_side
            .as_ref()
            .is_some_and(|side| side.wait_grid_stable(2, 250, Duration::from_secs(5)));
        // One extra blink-phase beat, then capture.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let our_grid = our_side.as_ref().map(|side| side.grid_now()).unwrap_or_else(|| Grid {
            cols: usize::from(cols),
            rows: usize::from(rows),
            cells: vec![VtCell { ch: ' ', style: 0 }; usize::from(cols) * usize::from(rows)],
        });
        let ref_grid = match ref_side.as_ref() {
            Some(side) => side.grid_now(),
            None => our_grid.clone(),
        };
        let exact = exact_diff(&ref_grid, &our_grid);
        let structural = struct_diff(&ref_grid, &our_grid, &exact);
        let class = if exact.mismatched_cells == 0 {
            StepClass::Identical
        } else if structural.text_only {
            StepClass::TextOnly
        } else {
            StepClass::Mismatch
        };
        outcome.steps.push(StepOutcome {
            step_index: index,
            label: step.label.clone().unwrap_or_default(),
            op: step.describe(),
            settled_ref: settled_ref && !cfg.no_ref,
            settled_our,
            class,
            exact,
            structural,
            ref_text: ref_grid.text(),
            our_text: our_grid.text(),
            coverage,
        });
        // Grid artifacts for mismatching steps (decision-grade dumps).
        if class != StepClass::Identical {
            let prefix = root.join(format!("s{:02}", index));
            let _ =
                std::fs::write(prefix.with_extension("ref.txt"), ref_grid.text().join("\n") + "\n");
            let _ =
                std::fs::write(prefix.with_extension("our.txt"), our_grid.text().join("\n") + "\n");
        }
    }

    // Exit chord if the script did not send one: leader+q.
    let exits = script.steps.iter().any(|step| {
        step.keys.iter().any(|k| k == "leader" || k == "ctrl+x")
            && step.keys.iter().any(|k| k == "q" || k == "ctrl+c" || k == "ctrl+d")
    });
    if !exits {
        if let Some(side) = ref_side.as_mut() {
            side.write(b"\x18q");
        }
        if let Some(side) = our_side.as_mut() {
            side.write(b"\x18q");
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    if let Some(side) = ref_side.as_ref() {
        let _ = side.wait_for_after(ALT_SCREEN_LEAVE, side.raw_len(), Duration::from_secs(5));
    }
    if let Some(side) = our_side.as_ref() {
        let _ = side.wait_for_after(ALT_SCREEN_LEAVE, side.raw_len(), Duration::from_secs(5));
    }
    if std::env::var_os("PARITY_DUMP_RAW").is_some() {
        if let Some(side) = ref_side.as_ref() {
            let _ = std::fs::write(root.join("ref.raw.ansi"), side.raw_mirror());
        }
        if let Some(side) = our_side.as_ref() {
            let _ = std::fs::write(root.join("our.raw.ansi"), side.raw_mirror());
        }
    }
    if std::env::var_os("PARITY_DUMP_STYLES").is_some() {
        // Per-cell style digests (row-major, space-separated) for the
        // final grids — S0 emission analysis.
        let dump = |name: &str, side: Option<&PtyChild>| {
            if let Some(side) = side {
                let grid = side.grid_now();
                let text = (0..grid.rows)
                    .map(|y| {
                        (0..grid.cols)
                            .map(|x| format!("{:x}", grid.cells[y * grid.cols + x].style))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = std::fs::write(root.join(format!("{name}.styles.txt")), text + "\n");
            }
        };
        dump("ref", ref_side.as_ref());
        dump("our", our_side.as_ref());
    }
    if let Some(side) = ref_side.as_mut() {
        side.kill();
    }
    if let Some(side) = our_side.as_mut() {
        side.kill();
    }
    if let Some(server) = ref_server.take() {
        let _ = server.stop().await;
    }
    if let Some(server) = our_server.take() {
        let _ = server.stop().await;
    }
    // Request-path parity: everything the stub saw during this run.
    outcome.requests = stub.take_recorded();
    outcome
}

fn dump_side_failure(root: &Path, side: &PtyChild, name: &str) {
    let raw = side.raw_mirror();
    let printable: String = raw
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b'\n' || *b == b'\r' || *b == b'\t' {
                (*b) as char
            } else {
                '·'
            }
        })
        .collect();
    let _ = std::fs::write(root.join(format!("{name}-boot-failure.txt")), printable);
}

// ---------------------------------------------------------------------------
// Report rendering
// ---------------------------------------------------------------------------

/// The full corpus report.
#[derive(Debug, Clone, Default)]
pub struct CorpusReport {
    pub runs: Vec<ScriptRun>,
}

impl CorpusReport {
    pub fn push(&mut self, run: ScriptRun) {
        self.runs.push(run);
    }

    /// Cross-run determinism split for one (script,size,step): present
    /// in every run vs flaky.
    #[must_use]
    pub fn deterministic_mismatches(&self) -> Vec<(String, u16, u16, usize)> {
        let mut keys = Vec::new();
        let mut counts: std::collections::HashMap<(String, u16, u16, usize), usize> =
            std::collections::HashMap::new();
        let mut totals: std::collections::HashMap<(String, u16, u16), usize> =
            std::collections::HashMap::new();
        for run in &self.runs {
            if run.error.is_some() {
                continue;
            }
            *totals.entry((run.script_id.clone(), run.cols, run.rows)).or_default() += 1;
            for step in &run.steps {
                if step.class == StepClass::Mismatch {
                    *counts
                        .entry((run.script_id.clone(), run.cols, run.rows, step.step_index))
                        .or_default() += 1;
                }
            }
        }
        for ((script, cols, rows, step), count) in counts {
            let total = totals.get(&(script.clone(), cols, rows)).copied().unwrap_or(0);
            if count == total && total > 1 {
                keys.push((script, cols, rows, step));
            }
        }
        keys.sort();
        keys
    }

    /// Render the human-readable report.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# TUI side-by-side differential — corpus report\n\n");
        out.push_str("Comparison bar: COMPLETE cell-grid identity (char + style digest)\n");
        out.push_str("after every step, v2.0.24 TUI vs Rust TUI, same project dir, same\n");
        out.push_str("config/stub, identical input bytes. Structural mode re-examines\n");
        out.push_str("mismatches under the alphanumeric glyph mask (geometry+style must\n");
        out.push_str("still match); chrome is never masked.\n\n");
        // Corpus table.
        out.push_str("## Corpus results\n\n");
        out.push_str("| script | size | run | steps | identical | text-only | mismatch | boot r/o | error |\n");
        out.push_str("|---|---|---|---|---|---|---|---|---|\n");
        for run in &self.runs {
            out.push_str(&format!(
                "| {} | {}x{} | {} | {} | {} | {} | {} | {}/{} | {} |\n",
                run.script_id,
                run.cols,
                run.rows,
                run.run,
                run.steps.len(),
                run.identical_steps(),
                run.text_only_steps(),
                run.mismatch_steps(),
                run.booted_ref,
                run.booted_our,
                run.error.as_deref().unwrap_or("")
            ));
        }
        // Determinism split.
        let deterministic = self.deterministic_mismatches();
        if self.runs.iter().any(|r| r.run > 0) || self.runs.len() > 1 {
            out.push_str("\n## Determinism split (across runs)\n\n");
            out.push_str(&format!(
                "Mismatches present in EVERY run (deterministic — fix-wave backlog):\n{:?}\n\n",
                deterministic
                    .iter()
                    .map(|(s, c, r, i)| format!("{s}@{c}x{r}#{i}"))
                    .collect::<Vec<_>>()
            ));
        }
        // Request-path parity (stub-recorded main requests).
        out.push_str("\n## Request-path parity (stub-recorded main requests)\n\n");
        out.push_str("Transport/options shape per request: attribution kind (flow runs:\n");
        out.push_str("`main`/`subagent`/`continuation`/`title`, body-substring keyed),\n");
        out.push_str("top-level body keys minus `messages`/`tools` (system-prompt text\n");
        out.push_str("legitimately differs), option values, model id, user-agent. Both\n");
        out.push_str("stacks read ONE config whose model carries a `body` overlay with\n");
        out.push_str("`temperature: 0` — a missing option in a shape means that stack\n");
        out.push_str("dropped it.\n\n");
        let mut any_requests = false;
        for run in &self.runs {
            if run.requests.is_empty() {
                continue;
            }
            any_requests = true;
            let mut shapes: Vec<(String, usize)> = Vec::new();
            for request in &run.requests {
                let shape = request.shape();
                match shapes.iter_mut().find(|(s, _)| *s == shape) {
                    Some((_, count)) => *count += 1,
                    None => shapes.push((shape, 1)),
                }
            }
            shapes.sort();
            out.push_str(&format!(
                "### {} @ {}x{} run {} — {} main requests, {} distinct shape(s)\n",
                run.script_id,
                run.cols,
                run.rows,
                run.run,
                run.requests.len(),
                shapes.len()
            ));
            for (shape, count) in shapes {
                out.push_str(&format!("- x{count}: {shape}\n"));
            }
            out.push('\n');
        }
        if !any_requests {
            out.push_str("(no main requests recorded — no submit steps ran)\n\n");
        }
        // Per-mismatch detail.
        out.push_str("\n## Mismatch detail\n\n");
        for run in &self.runs {
            if run.error.is_some() {
                out.push_str(&format!(
                    "### {} @ {}x{} (run {}) — DRIVER ERROR\n\n```\n{}\n```\n\n",
                    run.script_id,
                    run.cols,
                    run.rows,
                    run.run,
                    run.error.as_deref().unwrap_or("")
                ));
                continue;
            }
            for step in &run.steps {
                if step.class == StepClass::Identical {
                    continue;
                }
                let first = step.exact.first.as_ref();
                out.push_str(&format!(
                    "### {} @ {}x{} run {} step {} — {} ({})\n\n",
                    run.script_id,
                    run.cols,
                    run.rows,
                    run.run,
                    step.step_index,
                    step.class_name(),
                    if step.label.is_empty() { &step.op } else { &step.label },
                ));
                out.push_str(&format!("op: {}\n", step.op));
                if let Some(coverage) = &step.coverage {
                    out.push_str(&format!("coverage: {coverage}\n"));
                }
                out.push_str(&format!(
                    "settled: ref={} our={} | mismatches: {} (char {} / style {}), text-only {}\n",
                    step.settled_ref,
                    step.settled_our,
                    step.exact.mismatched_cells,
                    step.exact.char_mismatches,
                    step.exact.style_mismatches,
                    step.structural.text_only_cells,
                ));
                if let Some(first) = first {
                    out.push_str(&format!(
                        "first diff: ({}, {}) ref={:?} our={:?} styles ref[{}] our[{}]\n",
                        first.x,
                        first.y,
                        first.ref_cell.ch,
                        first.our_cell.ch,
                        decode_style(first.ref_cell.style),
                        decode_style(first.our_cell.style)
                    ));
                    let y = first.y;
                    out.push_str(&format!(
                        "ref row {y}: `{}`\n",
                        step.ref_text.get(y).map(String::as_str).unwrap_or("")
                    ));
                    out.push_str(&format!(
                        "our row {y}: `{}`\n",
                        step.our_text.get(y).map(String::as_str).unwrap_or("")
                    ));
                    if step.class == StepClass::Mismatch {
                        // Extra context rows around the first diff.
                        for dy in 1..=2 {
                            let ry = y + dy;
                            if ry < step.ref_text.len() || ry < step.our_text.len() {
                                out.push_str(&format!(
                                    "ref row {ry}: `{}`\nour row {ry}: `{}`\n",
                                    step.ref_text.get(ry).map(String::as_str).unwrap_or(""),
                                    step.our_text.get(ry).map(String::as_str).unwrap_or("")
                                ));
                            }
                        }
                    }
                }
                if step.class == StepClass::Mismatch {
                    if let Some(diverge) = step.structural.digest_first_divergence {
                        out.push_str(&format!(
                            "structure: first run-digest divergence at row {diverge} (masked-text runs/margins differ)\n"
                        ));
                        if let (Some(ref_digest), Some(our_digest)) =
                            (&step.structural.digest_ref, &step.structural.digest_our)
                        {
                            out.push_str(&format!(
                                "structure: ref text-rows={} our text-rows={}\n",
                                ref_digest.text_rows.len(),
                                our_digest.text_rows.len()
                            ));
                        }
                    }
                    out.push_str("cell samples (x,y): ref→our\n");
                    for miss in step.exact.samples.iter().take(8) {
                        out.push_str(&format!(
                            "  ({},{}) {:?}→{:?} ref[{}] our[{}]\n",
                            miss.x,
                            miss.y,
                            miss.ref_cell.ch,
                            miss.our_cell.ch,
                            decode_style(miss.ref_cell.style),
                            decode_style(miss.our_cell.style)
                        ));
                    }
                }
                out.push('\n');
            }
        }
        out
    }
}

/// Locate bun (same search order as the parity harness).
pub fn find_bun() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PARITY_BUN") {
        return Some(PathBuf::from(path));
    }
    for candidate in
        ["/root/.bun/bin/bun", "/usr/local/bin/bun", "/usr/bin/bun", "/opt/homebrew/bin/bun"]
    {
        if Path::new(candidate).is_file() {
            return Some(PathBuf::from(candidate));
        }
    }
    std::env::var("PATH").ok().and_then(|paths| {
        paths.split(':').find_map(|dir| {
            let candidate = PathBuf::from(dir).join("bun");
            candidate.is_file().then_some(candidate)
        })
    })
}

/// Run the whole corpus (all scripts × sizes × runs).
pub async fn run_corpus(cfg: &RigConfig) -> Result<CorpusReport> {
    let scripts = load_scripts(&cfg.scripts_dir, &cfg.script_filter)?;
    let stub = SharedStub::start()?;
    let bun = if cfg.no_ref {
        PathBuf::from("/bin/true")
    } else {
        find_bun().ok_or_else(|| anyhow!("bun not found (set PARITY_BUN)"))?
    };
    if !cfg.no_ref {
        crate::reference::ensure_reference_checkout()?;
    }
    let cli = crate::rust_server::cli_binary()?;
    let mut report = CorpusReport::default();
    for script in &scripts {
        // Per-script size override (e.g. sidebar-wide @160x36); a
        // malformed spec is a loud corpus error, never a silent
        // fallback to the default sizes.
        let script_sizes = match script.size.as_deref() {
            Some(spec) => vec![parse_size_spec(spec)
                .ok_or_else(|| anyhow!("script {}: bad size spec {spec:?}", script.id))?],
            None => cfg.sizes.clone(),
        };
        for &(cols, rows) in &script_sizes {
            for run in 0..cfg.runs {
                eprintln!("[tui-diff] {} @ {cols}x{rows} run {run}", script.id);
                let started = Instant::now();
                let outcome =
                    run_script_once(script, cols, rows, run, &stub, cfg, &bun, &cli).await;
                eprintln!(
                    "[tui-diff]   done in {:.1}s — {} steps, {} mismatch, {} text-only ({})",
                    started.elapsed().as_secs_f32(),
                    outcome.steps.len(),
                    outcome.mismatch_steps(),
                    outcome.text_only_steps(),
                    outcome.error.as_deref().unwrap_or("ok")
                );
                report.push(outcome);
            }
        }
    }
    stub.stop();
    Ok(report)
}

// ---------------------------------------------------------------------------
// Tests (flow stub: attribution + payload shapes)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic main-request body with the given text inside
    /// `messages` (only the raw bytes matter for attribution).
    fn body_with(text: &str, primary: bool) -> Vec<u8> {
        let max = if primary { "\"max_completion_tokens\":1024," } else { "" };
        format!(
            r#"{{"model":"fake-model",{max}"messages":[{{"role":"user","content":"{text}"}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn flow_classifies_by_body_attribution_not_order() {
        let flow = subagent_flow_scripts();
        // The subagent child's request: first-prompt prefix wins even
        // though it is also a primary (tools-bearing) request.
        let child = body_with(
            &format!("{}\n{}", flow_markers::FIRST_PROMPT_PREFIX, flow_markers::TOOL_PROMPT),
            true,
        );
        let (payload, kind) = classify_flow_request(&child, &flow);
        assert_eq!(kind, "subagent");
        assert!(payload.contains(flow_markers::SUBAGENT_TEXT));

        // Main request: primary family, no tool result yet.
        let (payload, kind) =
            classify_flow_request(&body_with("PARITY-SUBAGENT-FLOW go", true), &flow);
        assert_eq!(kind, "main");
        assert!(payload.contains("\"subagent\""));

        // Continuation: primary family carrying the subagent's reply
        // as the tool result.
        let (payload, kind) =
            classify_flow_request(&body_with(flow_markers::SUBAGENT_TEXT, true), &flow);
        assert_eq!(kind, "continuation");
        assert!(payload.contains(flow_markers::FINAL_TEXT));

        // Title-path request: no `max_completion_tokens` (corpus2 #27
        // pinned this split on both stacks).
        let (payload, kind) =
            classify_flow_request(&body_with("PARITY-SUBAGENT-FLOW go", false), &flow);
        assert_eq!(kind, "title");
        assert!(payload.contains(flow_markers::TITLE_TEXT));
    }

    #[test]
    fn flow_main_payload_is_a_subagent_tool_call() {
        let flow = subagent_flow_scripts();
        // One index-keyed tool_calls delta with the v2 tool name
        // (subagent.ts:10 `export const name = "subagent"`), the
        // configured researcher agent, and JSON arguments.
        assert!(flow.main.contains("\"tool_calls\":[{"));
        assert!(flow.main.contains("\"name\":\"subagent\""));
        assert!(flow.main.contains(flow_markers::TOOL_AGENT));
        assert!(flow.main.contains(flow_markers::TOOL_DESCRIPTION));
        assert!(flow.main.contains(flow_markers::TOOL_PROMPT));
        assert!(flow.main.contains("\"finish_reason\":\"tool_calls\""));
        assert!(flow.main.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn flow_subagent_payload_carries_reasoning_and_text() {
        let flow = subagent_flow_scripts();
        assert!(flow.subagent.contains("\"reasoning\""));
        assert!(flow.subagent.contains(flow_markers::SUBAGENT_TEXT));
        assert!(flow.subagent.contains("\"finish_reason\":\"stop\""));
    }

    #[test]
    fn arm_and_arm_flow_are_mutually_exclusive() {
        let stub = SharedStub::start().expect("stub");
        stub.arm_flow(subagent_flow_scripts());
        assert!(stub.flow.lock().expect("flow").is_some());
        stub.arm(StubScript::Text);
        assert!(stub.flow.lock().expect("flow").is_none());
        stub.stop();
    }

    #[test]
    fn flow_targets_the_builtin_general_subagent() {
        // Both stacks ship `general` with mode=subagent (ref
        // plugin/agent.ts:88-95; our default agent table) — the tool
        // call must not name a config-defined agent (ours does not
        // apply the config `agents` map to the registry yet). The
        // arguments arrive JSON-escaped inside the delta payload.
        let flow = subagent_flow_scripts();
        let escaped_agent = format!("\\\"{}\\\"", flow_markers::TOOL_AGENT);
        assert!(flow.main.contains(&escaped_agent));
        assert!(!flow.main.contains("researcher"));
    }
}
