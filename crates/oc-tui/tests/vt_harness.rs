//! CELL-GRID STREAM test harness (self-contained lane module).
//!
//! NOT a test binary — included from the `vt_hover_sweep` /
//! `vt_temporal` test binaries via `#[path = "vt_harness.rs"] mod
//! vt_harness;`. This file is owned by the vt-stream lane; the shared
//! `tests/common` (when it lands) is NOT touched.
//!
//! # What this captures
//!
//! The existing PTY suites assert on the raw byte stream (needles in a
//! cumulative buffer). This harness answers a different question:
//! **what did the screen LOOK like, over time?** The PTY's raw ANSI
//! output is parsed into styled cell grids ("snapshots"), one per
//! output burst bounded by a quiet window, so temporal behavior
//! (spinner frames, caret blink, marquee scrolls, hover highlights)
//! becomes directly observable instead of inferred from escape
//! sequences.
//!
//! # vt100 crate vs. own emulator — the choice
//!
//! [`vt100`] 0.15 (dev-dep of oc-tui) parses the stream; the choice
//! over hand-rolling an emulator on top of `src/render/ansi.rs`:
//!
//! * VT state tracking is subtle (SGR accumulation across cells,
//!   alternate-screen enter/leave, cursor addressing, wide chars) and
//!   exactly the vt100 crate's job — its `Parser`+`Screen::cell()`
//!   API hands us char+fg+bg+mods per cell directly.
//! * It is pure Rust (`vte` + `utf8parse`, no build scripts) and
//!   compiles clean under the pinned 1.99.0 toolchain (verified).
//! * Pinned to 0.15, NOT 0.16: 0.16 requires `unicode-width ^0.2.1`,
//!   which cannot resolve against ratatui 0.29's exact `"=0.2.0"`
//!   pin; 0.15's `unicode-width 0.1` coexists in the lock already
//!   (same as `console`).
//! * `src/render/ansi.rs` is our EMITTER-side palette mapping (theme
//!   colors → SGR), not a parser; reusing it would have meant
//!   writing a second VT parser with nothing shared but the palette.
//!
//! # Snapshot policy (animated output must not coalesce)
//!
//! A snapshot is emitted per output BURST: consecutive byte chunks
//! with inter-chunk gaps ≤ [`VtStream::DEFAULT_QUIET_MS`] (80 ms)
//! merge into one burst; a gap > 80 ms closes it and records the
//! screen state the burst left behind, timestamped at the burst's
//! last byte. The run loop ticks at 100 ms and animated cells (the
//! 80 ms braille spinner, the 500 ms caret blink) re-emit on every
//! tick, so bursts arrive ≥ 100 ms apart — each animation frame lands
//! in its OWN snapshot instead of being flattened. [`VtStream`]
//! never fabricates ticks: a stream that goes silent produces no
//! snapshots.
//!
//! # Script replay (groundwork for v2-vs-ours stream diffing)
//!
//! [`VtStream::record_bursts`] returns the raw bytes of every
//! detected burst; [`VtStream::emit_script_replay`] feeds such a
//! recording back through a fresh parser on a synthetic clock (one
//! burst per `quiet+40 ms` step, so replayed bursts stay separate
//! snapshots). Intended usage (future lane): record OUR PTY output
//! for a scripted interaction, record v2's for the same script, replay
//! both, and diff the per-burst [`Grid`]s cell-by-cell — the same
//! `diff()` used by the hover sweep, applied across implementations.
//!
//! ```ignore
//! // record side (live capture in a PTY test):
//! let bursts = stream.record_bursts().to_vec();
//! std::fs::write("bursts.bin", serde_bursts(&bursts)); // (serialization is lane-local)
//! // replay side:
//! let mut replay = VtStream::new(80, 24);
//! replay.emit_script_replay(&bursts.iter().map(Vec::as_slice).collect::<Vec<_>>());
//! let grids: Vec<&Grid> = replay.snapshots().iter().map(|s| &s.grid).collect();
//! ```

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(dead_code)]
// shared harness: not every consumer uses every piece
// Terminal dimensions (u16 by VT definition), harness-clock
// instants, and style digests are bounded magnitudes by construction;
// the truncation lints add nothing here.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Grid model
// ---------------------------------------------------------------------------

/// One parsed screen cell: the visible char plus a style digest
/// (fg + bg + modifier bits packed into a u64; two cells with the same
/// digest render identically).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtCell {
    pub ch: char,
    pub style: u64,
}

/// The kind of change one cell underwent between two grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// The visible character changed (re-render, reveal, marquee shift).
    Char,
    /// Only fg/bg/modifiers changed (hover highlight, caret blink).
    Style,
}

/// One changed cell between two grids (`diff` reports in row-major
/// order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellDiff {
    pub x: usize,
    pub y: usize,
    pub kind: DiffKind,
}

/// A cell matrix snapshot: char + style digest per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    /// Row-major `[y * cols + x]`.
    pub cells: Vec<VtCell>,
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

    /// Cells that changed from `self` (the BEFORE state) to `other`
    /// (the AFTER state). Positions in `skip` (known-volatile cells —
    /// the blinking caret etc.) are excluded.
    #[must_use]
    pub fn diff(&self, other: &Self, skip: &HashSet<(usize, usize)>) -> Vec<CellDiff> {
        let mut out = Vec::new();
        for y in 0..self.rows.min(other.rows) {
            for x in 0..self.cols.min(other.cols) {
                if skip.contains(&(x, y)) {
                    continue;
                }
                let (a, b) = (self.cell(x, y), other.cell(x, y));
                if a == b {
                    continue;
                }
                out.push(CellDiff {
                    x,
                    y,
                    kind: if a.ch == b.ch { DiffKind::Style } else { DiffKind::Char },
                });
            }
        }
        out
    }

    /// The plain text of a `w×h` region at `(x, y)` (for eyeballing
    /// and locating surfaces; each row right-trimmed).
    #[must_use]
    pub fn content_rect(&self, x: usize, y: usize, w: usize, h: usize) -> String {
        let mut lines = Vec::new();
        for row in y..y.saturating_add(h).min(self.rows) {
            let mut text = String::new();
            for col in x..x.saturating_add(w).min(self.cols) {
                text.push(self.cell(col, row).ch);
            }
            lines.push(text.trim_end().to_owned());
        }
        lines.join("\n")
    }

    /// One row's text (right-trimmed).
    #[must_use]
    pub fn row_text(&self, y: usize) -> String {
        self.content_rect(0, y, self.cols, 1)
    }

    /// Every cell blank (the post-teardown / never-painted signature).
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|cell| cell.ch == ' ' && cell.style == 0)
    }
}

/// Pack fg + bg + modifier bits into one comparable digest (see
/// [`VtCell::style`]).
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

// ---------------------------------------------------------------------------
// VtStream
// ---------------------------------------------------------------------------

/// One burst's worth of parsed state.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Harness-clock ms at the burst's LAST byte (when this state was
    /// reached).
    pub t_ms: u64,
    /// The screen the burst left behind.
    pub grid: Grid,
    /// Raw bytes in the burst.
    pub burst_bytes: usize,
}

/// The cell-grid stream: raw PTY bytes in, burst-bounded grid
/// snapshots out (see the module header for the policy).
pub struct VtStream {
    parser: vt100::Parser,
    quiet_ms: u64,
    snapshots: Vec<Snapshot>,
    /// Raw bytes of the still-open burst.
    current_burst: Vec<u8>,
    /// Raw bytes of every closed burst (script-replay recordings).
    bursts: Vec<Vec<u8>>,
    last_feed_ms: Option<u64>,
    fed_bytes: usize,
}

impl VtStream {
    /// Default quiet window (ms): bursts separated by more than this
    /// are distinct snapshots. 80 ms keeps the 100 ms-tick animation
    /// bursts (spinner/caret) in separate snapshots while merging the
    /// chunks of one logical frame paint.
    pub const DEFAULT_QUIET_MS: u64 = 80;

    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows as u16, cols as u16, 0),
            quiet_ms: Self::DEFAULT_QUIET_MS,
            snapshots: Vec::new(),
            current_burst: Vec::new(),
            bursts: Vec::new(),
            last_feed_ms: None,
            fed_bytes: 0,
        }
    }

    /// Feed one raw chunk that arrived at `now_ms` (harness clock).
    /// A >`quiet_ms` gap since the previous chunk closes the previous
    /// burst into a snapshot BEFORE this chunk is applied.
    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) {
        if bytes.is_empty() {
            return;
        }
        if let Some(last) = self.last_feed_ms {
            if now_ms.saturating_sub(last) > self.quiet_ms {
                self.close_burst(last);
            }
        }
        self.parser.process(bytes);
        self.current_burst.extend_from_slice(bytes);
        self.fed_bytes += bytes.len();
        self.last_feed_ms = Some(now_ms);
    }

    /// Close the open burst into a snapshot if it has been quiet long
    /// enough (call after waiting out the quiet window; also closes
    /// the trailing burst on teardown).
    pub fn settle_snapshot(&mut self, now_ms: u64) {
        if let Some(last) = self.last_feed_ms {
            if now_ms.saturating_sub(last) > self.quiet_ms {
                self.close_burst(last);
            }
        }
    }

    fn close_burst(&mut self, t_ms: u64) {
        let bytes = std::mem::take(&mut self.current_burst);
        let burst_bytes = bytes.len();
        self.bursts.push(bytes);
        self.snapshots.push(Snapshot { t_ms, grid: Grid::from_parser(&self.parser), burst_bytes });
        self.last_feed_ms = None;
    }

    #[must_use]
    pub fn snapshots(&self) -> &[Snapshot] {
        &self.snapshots
    }

    /// The CURRENT screen (fed state, settled or not).
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

    /// Raw bytes per closed burst (script-replay source; see module
    /// header for the v2-vs-ours diffing plan).
    #[must_use]
    pub fn record_bursts(&self) -> &[Vec<u8>] {
        &self.bursts
    }

    /// Feed a recorded input script (bursts from [`Self::record_bursts`])
    /// on a synthetic clock — one burst per `quiet+40 ms` step so each
    /// replayed burst lands as its own snapshot. Groundwork for
    /// character-stream differential testing; documented in the module
    /// header.
    pub fn emit_script_replay(&mut self, script: &[&[u8]]) {
        let mut t = 0_u64;
        for chunk in script {
            t += self.quiet_ms + 40;
            self.feed(chunk, t);
        }
        self.settle_snapshot(t + self.quiet_ms + 40);
    }
}

// ---------------------------------------------------------------------------
// Temporal analysis helpers
// ---------------------------------------------------------------------------

/// `(t_ms, value)` pairs where the cell's char actually changed
/// between consecutive snapshots (char timeline, transitions only).
#[must_use]
pub fn char_timeline(snaps: &[Snapshot], x: usize, y: usize) -> Vec<(u64, char)> {
    timeline(snaps, x, y, |cell| cell.ch as u64)
        .into_iter()
        .map(|(t, v)| (t, char::from_u32(v as u32).unwrap_or(' ')))
        .collect()
}

/// `(t_ms, digest)` pairs where the cell's STYLE digest changed
/// (style timeline, transitions only — the caret blink's signal).
#[must_use]
pub fn style_timeline(snaps: &[Snapshot], x: usize, y: usize) -> Vec<(u64, u64)> {
    timeline(snaps, x, y, |cell| cell.style)
}

fn timeline(
    snaps: &[Snapshot],
    x: usize,
    y: usize,
    key: impl Fn(VtCell) -> u64,
) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    let mut last: Option<u64> = None;
    for snap in snaps {
        let value = key(snap.grid.cell(x, y));
        if last != Some(value) {
            out.push((snap.t_ms, value));
            last = Some(value);
        }
    }
    out
}

/// Median of the gaps between consecutive timestamps (ms).
#[must_use]
pub fn median_gap(times: &[u64]) -> Option<u64> {
    if times.len() < 2 {
        return None;
    }
    let mut gaps: Vec<u64> = times.windows(2).map(|pair| pair[1].saturating_sub(pair[0])).collect();
    gaps.sort_unstable();
    Some(gaps[gaps.len() / 2])
}

// ---------------------------------------------------------------------------
// PTY attach (size-parameterized; the existing suites' pattern)
// ---------------------------------------------------------------------------

struct PtyGuard {
    master: File,
    saved_stdin: RawFd,
    saved_stdout: RawFd,
}

impl Drop for PtyGuard {
    fn drop(&mut self) {
        unsafe {
            libc::dup2(self.saved_stdin, libc::STDIN_FILENO);
            libc::dup2(self.saved_stdout, libc::STDOUT_FILENO);
            libc::close(self.saved_stdin);
            libc::close(self.saved_stdout);
        }
    }
}

fn attach_pty(cols: u16, rows: u16) -> PtyGuard {
    let master = unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(fd >= 0, "posix_openpt");
        libc::grantpt(fd);
        libc::unlockpt(fd);
        File::from_raw_fd(fd)
    };
    let slave_path = unsafe {
        let name = libc::ptsname(master.as_raw_fd());
        assert!(!name.is_null(), "ptsname");
        std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
    };
    let slave =
        std::fs::OpenOptions::new().read(true).write(true).open(&slave_path).expect("open slave");
    unsafe {
        let winsize = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
    }
    let saved_stdin = unsafe { libc::dup(libc::STDIN_FILENO) };
    let saved_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    unsafe {
        libc::dup2(slave.as_raw_fd(), libc::STDIN_FILENO);
        libc::dup2(slave.as_raw_fd(), libc::STDOUT_FILENO);
    }
    drop(slave);
    PtyGuard { master, saved_stdin, saved_stdout }
}

/// Sandbox state roots so the boot storage never touches the
/// developer's real tabs state (same scheme as the other PTY suites).
#[must_use]
pub fn sandbox_env(tag: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from("/tmp/opencode").join(format!(
        "oc-tui-vt-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos())
    ));
    std::fs::create_dir_all(root.join(".config")).expect("config dir");
    std::fs::create_dir_all(root.join(".local/state")).expect("state dir");
    std::env::set_var("OPENCODE_TEST_HOME", &root);
    std::env::set_var("XDG_CONFIG_HOME", root.join(".config"));
    std::env::set_var("XDG_STATE_HOME", root.join(".local/state"));
    root
}

/// Serialize the tests of one binary under plain `cargo test`
/// (nextest runs one process per test anyway): PTY attach and the
/// sandbox env vars are process-global state.
pub fn pty_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().expect("pty lock")
}

// ---------------------------------------------------------------------------
// FakeLlm: scripted server with a streaming SSE script
// ---------------------------------------------------------------------------

/// One scripted SSE event: `delay_ms` AFTER the first prompt POST is
/// observed, `body` (the bus event JSON object) goes out.
#[derive(Clone)]
pub struct ScriptEvent {
    pub delay_ms: u64,
    pub body: String,
}

/// Session row served by `session.list`.
#[derive(Clone)]
pub struct FakeSession {
    pub id: &'static str,
    pub title: &'static str,
}

/// The standard two-session history: one short title (tab 1) and one
/// long title that must truncate in the strip (marquee surface).
pub const LONG_TITLE: &str = "Long Session Title That Must Truncate In The Tab Strip";

#[must_use]
pub fn standard_sessions() -> Vec<FakeSession> {
    vec![
        FakeSession { id: "ses_alpha", title: "Alpha" },
        FakeSession { id: "ses_long", title: LONG_TITLE },
    ]
}

/// The streaming script: on the first prompt POST, a user message is
/// enqueued, a step starts, and ONE reasoning delta creates a running
/// "Thinking" row — then the stream HOLDS (no `step.ended`), so the
/// transcript's loading indicator keeps animating off the clock for
/// as long as the test observes.
#[must_use]
pub fn streaming_thinking_script(session: &str, text: &str) -> Vec<ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    vec![
        ScriptEvent {
            delay_ms: 30,
            body: format!(
                r#"{{"id":"evt_inbox","created":{now_ms},"type":"session.inbox.enqueued","data":{{"sessionID":"{session}","inboxID":"msg_user_1","item":{{"type":"user","payload":{{"text":"{text}"}}}}}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 60,
            body: format!(
                r#"{{"id":"evt_step","created":{now_ms},"type":"session.step.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","agent":"build","model":{{"providerID":"anthropic","id":"claude-sonnet"}}}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 80,
            body: format!(
                r#"{{"id":"evt_r1","created":{now_ms},"type":"session.reasoning.delta","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","ordinal":0,"delta":"Pondering"}}}}"#
            ),
        },
        ScriptEvent {
            delay_ms: 900,
            body: format!(
                r#"{{"id":"evt_r2","created":{now_ms},"type":"session.reasoning.delta","data":{{"sessionID":"{session}","assistantMessageID":"msg_asst_1","ordinal":0,"delta":" still"}}}}"#
            ),
        },
    ]
}

/// The persistent-terminal fixture's shared state (TUI-4's PTY proof):
/// one scripted terminal `pty_fix` whose snapshot checkpoint and WS
/// replay paint fixed text, plus every inbound WS binary frame the
/// TUI sends (the interaction-frame roundtrip evidence).
pub struct TerminalFixture {
    pub armed: AtomicBool,
    /// POSTs to the session-terminal create endpoint.
    pub created: AtomicUsize,
    /// Completed WS attach handshakes.
    pub ws_attached: AtomicUsize,
    /// Inbound WS binary frames (interaction frames), in order.
    pub input_frames: Mutex<Vec<Vec<u8>>>,
}

impl TerminalFixture {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(false),
            created: AtomicUsize::new(0),
            ws_attached: AtomicUsize::new(0),
            input_frames: Mutex::new(Vec::new()),
        }
    }
}

pub struct FakeLlm {
    pub url: String,
    shutdown: Arc<AtomicBool>,
    pub prompt_posts: Arc<AtomicUsize>,
    /// Whether `/api/vcs` answers (opt-in — lane/if's footer-label
    /// proof): the default OFF keeps the vcs lookup failing, so the
    /// location label stays absent for the suites whose baselines
    /// predate it (the hover sweeps assert off-strip cells do not
    /// respond).
    pub vcs: Arc<AtomicBool>,
    /// Raw `session.list` rows (opt-in — the TUI-5/6/7 dialog proofs
    /// need richer `SessionInfo` wire rows than the title-only fixture:
    /// time.updated, parentID, directory, projectID). When set, the
    /// rows REPLACE the fixture pair in the list body.
    pub session_rows: Arc<Mutex<Option<Vec<String>>>>,
    /// The `/api/project` body (opt-in — TUI-5/6's projects registry).
    pub projects: Arc<Mutex<Option<String>>>,
    /// The persistent-pty fixture family (TUI-4).
    pub terminals: Arc<TerminalFixture>,
    /// Whether `/api/mcp` answers (opt-in — SURF-1's sidebar MCP
    /// proof): three servers (2 connected + 1 failed) exercise the
    /// collapse toggle AND the error dot.
    pub mcp: Arc<AtomicBool>,
    /// Whether `/api/info` + `/api/pair` answer (opt-in — SURF-2's
    /// `DialogPair` proof).
    pub pair: Arc<AtomicBool>,
    /// Whether `/api/integration` answers (opt-in — lane FINAL's
    /// connect-flow proof): one key-method integration.
    pub integrations: Arc<AtomicBool>,
    /// Whether `/api/skill` answers (opt-in — TUI-8's `DialogSkill`
    /// proof): two skills.
    pub skills: Arc<AtomicBool>,
    /// TUI-14 — every `POST /api/session/:id/shell` body (the
    /// shell-mode submit's server-side evidence).
    pub shell_posts: Arc<Mutex<Vec<String>>>,
    /// TUI-15 — every `POST /api/session/:id/synthetic` body (the
    /// editor-context submit's server-side evidence).
    pub synthetic_posts: Arc<Mutex<Vec<String>>>,
    /// TUI-17 — every `POST /api/session/:id/interrupt` path (the
    /// per-child interrupt's server-side evidence).
    pub interrupt_posts: Arc<Mutex<Vec<String>>>,
    /// SSE script played once, after the first prompt POST.
    script: Arc<Mutex<Option<Vec<ScriptEvent>>>>,
    /// Lane E2's boot stretch: every request handled before this
    /// deadline sleeps until it passes before its first response
    /// byte — a deterministic "slow server at boot" for the
    /// `StartupLoading` temporal gate (the deadline stays armed but
    /// is inert once passed — later requests pass straight through).
    boot_stretch_until: Arc<Mutex<Option<std::time::Instant>>>,
}

impl FakeLlm {
    #[must_use]
    pub fn spawn(sessions: Vec<FakeSession>) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let shutdown = Arc::new(AtomicBool::new(false));
        let prompt_posts = Arc::new(AtomicUsize::new(0));
        let vcs = Arc::new(AtomicBool::new(false));
        let mcp = Arc::new(AtomicBool::new(false));
        let pair = Arc::new(AtomicBool::new(false));
        let integrations = Arc::new(AtomicBool::new(false));
        let skills = Arc::new(AtomicBool::new(false));
        let shell_posts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let synthetic_posts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let interrupt_posts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let session_rows: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
        let projects: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let terminals = Arc::new(TerminalFixture::new());
        let script: Arc<Mutex<Option<Vec<ScriptEvent>>>> = Arc::new(Mutex::new(None));
        let boot_stretch_until: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(None));
        let flag = Arc::clone(&shutdown);
        let shared_script = Arc::clone(&script);
        let shared_boot_stretch = Arc::clone(&boot_stretch_until);
        let shared_posts = Arc::clone(&prompt_posts);
        let shared_vcs = Arc::clone(&vcs);
        let shared_mcp = Arc::clone(&mcp);
        let shared_pair = Arc::clone(&pair);
        let shared_integrations = Arc::clone(&integrations);
        let shared_skills = Arc::clone(&skills);
        let shared_shell_posts = Arc::clone(&shell_posts);
        let shared_synthetic_posts = Arc::clone(&synthetic_posts);
        let shared_interrupt_posts = Arc::clone(&interrupt_posts);
        let shared_rows = Arc::clone(&session_rows);
        let shared_projects = Arc::clone(&projects);
        let shared_terminals = Arc::clone(&terminals);
        std::thread::spawn(move || {
            listener.set_nonblocking(true).expect("nonblocking accept");
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((socket, _)) => {
                        let sessions = sessions.clone();
                        let script = Arc::clone(&shared_script);
                        let boot_stretch = Arc::clone(&shared_boot_stretch);
                        let posts = Arc::clone(&shared_posts);
                        let vcs = Arc::clone(&shared_vcs);
                        let mcp = Arc::clone(&shared_mcp);
                        let pair = Arc::clone(&shared_pair);
                        let integrations = Arc::clone(&shared_integrations);
                        let skills = Arc::clone(&shared_skills);
                        let shell_posts = Arc::clone(&shared_shell_posts);
                        let synthetic_posts = Arc::clone(&shared_synthetic_posts);
                        let interrupt_posts = Arc::clone(&shared_interrupt_posts);
                        let rows = Arc::clone(&shared_rows);
                        let projects = Arc::clone(&shared_projects);
                        let terminals = Arc::clone(&shared_terminals);
                        std::thread::spawn(move || {
                            handle(
                                socket,
                                &sessions,
                                &script,
                                &boot_stretch,
                                &posts,
                                &vcs,
                                &mcp,
                                &pair,
                                &integrations,
                                &skills,
                                &shell_posts,
                                &synthetic_posts,
                                &interrupt_posts,
                                &rows,
                                &projects,
                                &terminals,
                            );
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url: format!("http://{addr}"),
            shutdown,
            prompt_posts,
            vcs,
            mcp,
            pair,
            integrations,
            skills,
            shell_posts,
            synthetic_posts,
            interrupt_posts,
            session_rows,
            projects,
            terminals,
            script,
            boot_stretch_until,
        }
    }

    /// Serve raw `session.list` wire rows (TUI-5/6/7's richer
    /// `SessionInfo` fixtures — timestamps, parents, directories,
    /// projectIDs). When set they REPLACE the fixture pair.
    pub fn arm_session_rows(&self, rows: Vec<String>) {
        *self.session_rows.lock().expect("rows") = Some(rows);
    }

    /// Serve the `/api/project` list body (TUI-5/6's registry).
    pub fn arm_projects(&self, body: String) {
        *self.projects.lock().expect("projects") = Some(body);
    }

    /// Serve the persistent-terminal family (TUI-4): the
    /// capabilities-bearing `/api/info`, the session-terminal REST
    /// endpoints, and the WS attach with scripted output.
    pub fn arm_terminals(&self) {
        self.terminals.armed.store(true, Ordering::SeqCst);
    }

    /// Answer `/api/vcs` (the footer location label's boot lookup —
    /// `{cwd}:{branch}` paints once it resolves).
    pub fn arm_vcs(&self) {
        self.vcs.store(true, Ordering::SeqCst);
    }

    /// Answer `/api/mcp` with the three-server fixture (SURF-1).
    pub fn arm_mcp(&self) {
        self.mcp.store(true, Ordering::SeqCst);
    }

    /// Answer `/api/info` + `/api/pair` (SURF-2's `DialogPair` proof).
    pub fn arm_pair(&self) {
        self.pair.store(true, Ordering::SeqCst);
    }

    /// Answer the integration routes (lane FINAL's connect-flow
    /// proof): `integration.list`/`get` + `connect.key` (204).
    pub fn arm_integrations(&self) {
        self.integrations.store(true, Ordering::SeqCst);
    }

    /// Answer `/api/skill` with the two-skill fixture (TUI-8's
    /// `DialogSkill` proof).
    pub fn arm_skills(&self) {
        self.skills.store(true, Ordering::SeqCst);
    }

    /// Arm the streaming script (played on the SSE feed after the
    /// first prompt POST). No script = a quiet held-open stream.
    pub fn arm_script(&self, script: Vec<ScriptEvent>) {
        *self.script.lock().expect("script") = Some(script);
    }

    /// Stretch boot (Lane E2's `StartupLoading` gate): every request
    /// handled before `ms` after this call holds its response until
    /// the deadline passes — the boot's server round-trips all land
    /// late, so a boot-window overlay crosses its display thresholds
    /// deterministically instead of riding boot-jitter luck.
    pub fn arm_boot_stretch(&self, ms: u64) {
        *self.boot_stretch_until.lock().expect("boot stretch") =
            Some(std::time::Instant::now() + Duration::from_millis(ms));
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

#[allow(clippy::too_many_lines)] // one arm per scripted endpoint
#[allow(clippy::too_many_arguments)] // one flag per scripted endpoint family
fn handle(
    mut socket: std::net::TcpStream,
    sessions: &[FakeSession],
    script: &Arc<Mutex<Option<Vec<ScriptEvent>>>>,
    boot_stretch_until: &Arc<Mutex<Option<std::time::Instant>>>,
    prompt_posts: &Arc<AtomicUsize>,
    vcs: &Arc<AtomicBool>,
    mcp: &Arc<AtomicBool>,
    pair: &Arc<AtomicBool>,
    integrations: &Arc<AtomicBool>,
    skills: &Arc<AtomicBool>,
    shell_posts: &Arc<Mutex<Vec<String>>>,
    synthetic_posts: &Arc<Mutex<Vec<String>>>,
    interrupt_posts: &Arc<Mutex<Vec<String>>>,
    session_rows: &Arc<Mutex<Option<Vec<String>>>>,
    projects: &Arc<Mutex<Option<String>>>,
    terminals: &Arc<TerminalFixture>,
) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 8192];
    let mut head = String::new();
    // Block until the full request head arrives (a pooled connection
    // may be accepted before the client sends its request bytes).
    loop {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                head.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if head.contains("\r\n\r\n") {
                    break;
                }
            }
        }
    }
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    // Lane E2's boot stretch: hold this response until the armed
    // deadline passes (the Option stays Some but is inert once the
    // deadline is past — later requests pass straight through).
    if let Some(deadline) = *boot_stretch_until.lock().expect("boot stretch") {
        let now = std::time::Instant::now();
        if now < deadline {
            std::thread::sleep(deadline - now);
        }
    }
    let path = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
    let method = head.split_whitespace().next().unwrap_or_default().to_uppercase();
    // TUI-TD15's knock-on: the startup prompt's resolved model
    // commits through `session.switchModel` BEFORE the prompt POST
    // (`commitModel`, prompt/index.tsx:1283-1294) — the endpoint's
    // contract is 204 No Content, and the catch-all 200+`{}` would
    // fail the empty-expectation and swallow the prompt POST.
    if method == "POST" && path.starts_with("/api/session/") && path.ends_with("/model") {
        let response = b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let _ = socket.write_all(response);
        return;
    }
    // TUI-14 — the shell-mode submit endpoint: record the request
    // BODY (the command), answer 204 No Content (the server contract
    // for `session.shell`).
    if method == "POST" && path.starts_with("/api/session/") && path.ends_with("/shell") {
        let content_length = head
            .split("\r\n")
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .or_else(|| line.strip_prefix("Content-Length:"))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body_bytes = Vec::new();
        // The head read may have swallowed body bytes already (the
        // first socket read can carry head + body together).
        let split = head.find("\r\n\r\n").map_or(0, |at| at + 4);
        body_bytes.extend_from_slice(&head.as_bytes()[split.min(head.len())..]);
        while body_bytes.len() < content_length {
            let mut chunk = [0u8; 1024];
            match socket.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => body_bytes.extend_from_slice(&chunk[..count]),
            }
        }
        shell_posts
            .lock()
            .expect("shell posts")
            .push(format!("{path} {}", String::from_utf8_lossy(&body_bytes)));
        let response = b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let _ = socket.write_all(response);
        return;
    }
    // TUI-15 — the editor-context submit (`session.synthetic`): record
    // the request BODY, answer the inbox row shape.
    if method == "POST" && path.starts_with("/api/session/") && path.ends_with("/synthetic") {
        let content_length = head
            .split("\r\n")
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .or_else(|| line.strip_prefix("Content-Length:"))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body_bytes = Vec::new();
        // The head read may have swallowed body bytes already (the
        // first socket read can carry head + body together).
        let split = head.find("\r\n\r\n").map_or(0, |at| at + 4);
        body_bytes.extend_from_slice(&head.as_bytes()[split.min(head.len())..]);
        while body_bytes.len() < content_length {
            let mut chunk = [0u8; 1024];
            match socket.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => body_bytes.extend_from_slice(&chunk[..count]),
            }
        }
        synthetic_posts
            .lock()
            .expect("synthetic posts")
            .push(format!("{path} {}", String::from_utf8_lossy(&body_bytes)));
        let body = r#"{"data":{"id":"synthetic_1","type":"synthetic","time":{"created":0}}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes());
        return;
    }
    // TUI-17 — the per-child interrupt (`v2.session.interrupt`): record
    // the path (the query `resume` rides along), answer 204 No Content.
    if method == "POST"
        && path.split('?').next().unwrap_or_default().starts_with("/api/session/")
        && path.split('?').next().unwrap_or_default().ends_with("/interrupt")
    {
        interrupt_posts
            .lock()
            .expect("interrupt posts")
            .push(path.split('?').next().unwrap_or_default().to_owned());
        let response = b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let _ = socket.write_all(response);
        return;
    }
    if std::env::var("DBG_FAKE_LOG").is_ok() || std::env::var("DBG_FAKE_LOG2").is_ok() {
        eprintln!("DBG fake: {method} {path}");
    }
    // -- TUI-4: the persistent-pty family ---------------------------------
    //
    // The WS attach endpoint upgrades BEFORE any JSON response; the
    // REST endpoints answer below through the shared body walk.
    let bare_path = path.split('?').next().unwrap_or_default().to_owned();
    if terminals.armed.load(Ordering::SeqCst)
        && method == "GET"
        && bare_path.starts_with("/api/experimental/persistent-pty/")
        && bare_path.ends_with("/connect")
    {
        let pty_id = bare_path
            .trim_start_matches("/api/experimental/persistent-pty/")
            .trim_end_matches("/connect")
            .to_owned();
        ws_attach(socket, &head, terminals, &pty_id);
        return;
    }
    if terminals.armed.load(Ordering::SeqCst)
        && (path.starts_with("/api/experimental/persistent-pty/")
            || path.starts_with("/api/experimental/session/"))
    {
        if let Some(response) = pty_rest(&method, &path, terminals) {
            let _ = socket.write_all(response.as_bytes());
            return;
        }
    }
    if path.starts_with("/api/event") {
        // The SSE connection must stay open for the WHOLE session: a
        // disconnect would make the app reconnect → resync → rebuild
        // the transcript trees from message.list, wiping the running
        // "Thinking" row the temporal tests observe. So: blocking
        // reads (no timeout) once the head is read; the thread dies
        // with the test process when the peer closes.
        socket.set_read_timeout(None).ok();
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        // The no-prompt quiet-hold window: generous — a loaded box
        // can stretch the boot → dialog → type → submit walk well
        // past a tight deadline, and parking early would silently
        // drop the armed script (the flapping-reconnect races make
        // each new SSE thread restart this window).
        let deadline = std::time::Instant::now() + Duration::from_secs(55);
        while prompt_posts.load(Ordering::SeqCst) == 0 {
            if std::time::Instant::now() > deadline {
                // No prompt arrived: hold the stream quietly (drain
                // until the peer closes).
                let mut sink = [0u8; 256];
                loop {
                    match socket.read(&mut sink) {
                        Ok(0) | Err(_) => return,
                        Ok(_) => {}
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let script = script.lock().expect("script").clone().unwrap_or_default();
        let mut anchor = std::time::Instant::now();
        for event in &script {
            let target = anchor + Duration::from_millis(event.delay_ms);
            while std::time::Instant::now() < target {
                std::thread::sleep(
                    Duration::from_millis(5).min(target - std::time::Instant::now()),
                );
            }
            anchor = target; // schedule is absolute from the prompt POST
            let encoded = serde_json_json_string(&event.body);
            let frame = format!("data: {encoded}\n\n");
            if socket.write_all(frame.as_bytes()).is_err() {
                return;
            }
        }
        // Script done: park until the peer closes.
        let mut sink = [0u8; 256];
        loop {
            match socket.read(&mut sink) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    }
    if path.ends_with("/prompt") {
        prompt_posts.fetch_add(1, Ordering::SeqCst);
    }
    // integration.connect.key (lane FINAL): a 204 No Content — the
    // client's execute_empty contract.
    if integrations.load(Ordering::SeqCst)
        && method == "POST"
        && path.starts_with("/api/integration/")
        && path.ends_with("/connect/key")
    {
        let response = b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        let _ = socket.write_all(response);
        return;
    }
    let body = if path.split('?').next().unwrap_or("") == "/api/vcs" && vcs.load(Ordering::SeqCst) {
        // vcs.get (lane/if: the footer location label's boot lookup —
        // the phantom-click PTY proof needs the `{cwd}:{branch}` label
        // to paint, so the stub resolves a branch; OPT-IN because the
        // label's hover lift is a real cell response the hover sweeps
        // would flag).
        r#"{"data":{"branch":{"current":"main","default":"main"}},"location":{"directory":"/tmp"}}"#
            .to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/vcs/base" && vcs.load(Ordering::SeqCst)
    {
        // vcs.base (SURF-2's diff viewer): no base reported.
        r#"{"data":null,"location":{"directory":"/tmp"}}"#.to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/vcs/diff" && vcs.load(Ordering::SeqCst)
    {
        // vcs.diff (SURF-2's diff viewer): one modified file fixture.
        r#"{"data":[{"file":"src/lib.rs","additions":1,"deletions":1,"patch":"--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n fn one() {\n-    let old = 1;\n+    let new = 1;\n }\n","status":"modified"}],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/info"
        && (pair.load(Ordering::SeqCst) || terminals.armed.load(Ordering::SeqCst))
    {
        // server.info (SURF-2's DialogPair / TUI-4's capability
        // probe): one loopback URL; the terminals fixture additionally
        // declares the persistentPty capability (session-terminals.
        // tsx:66-70's availability gate).
        let capabilities = if terminals.armed.load(Ordering::SeqCst) {
            r#","capabilities":{"persistentPty":true}"#
        } else {
            ""
        };
        format!(r#"{{"paths":{{"tmp":"/tmp"}},"pid":1,"urls":["http://127.0.0.1:4096"],"version":"2.0.0"{capabilities}}}"#)
            .to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/model" {
        // model.list (TUI-TD15's `--prompt` readiness gate reads it —
        // the startup prompt only submits once the catalog loads and
        // a model resolves; the catch-all `{}` would fail the parse
        // and hold the gate forever). One parityfake model, matching
        // the route_server fixture's catalog.
        concat!(
            r#"{"data":[{"providerID":"parityfake","modelID":"fake-model","id":"fake-model","#,
            r#""name":"fake-model","status":"active","enabled":true,"cost":[],"variants":[],"#,
            r#""capabilities":{"input":["text"],"output":["text"],"tools":false},"#,
            r#""limit":{"context":1000,"output":100},"time":{"released":1.0}}],"#,
            r#""location":{"directory":"/tmp"}}"#
        )
        .to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/project"
        && projects.lock().expect("projects").is_some()
    {
        // project.list (TUI-5/6's projects registry).
        projects.lock().expect("projects").clone().unwrap_or_default()
    } else if method == "GET" && path.split('?').next().unwrap_or("") == "/api/session" {
        // session.list: the armed raw rows when present (the richer
        // SessionInfo fixtures), else the title-only pair.
        let rows: Vec<String> = session_rows
            .lock()
            .expect("rows")
            .clone()
            .unwrap_or_else(|| {
                sessions
                    .iter()
                    .map(|session| {
                        format!(
                            r#"{{"id":"{}","title":"{}","time":{{"created":0,"updated":0}},"location":{{"directory":"/tmp"}},"projectID":"p","cost":0,"tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}"#,
                            session.id, session.title
                        )
                    })
                    .collect()
            });
        format!(r#"{{"cursor":{},"data":[{}]}}"#, "{}", rows.join(","))
    } else if path.split('?').next().unwrap_or("") == "/api/pair" && pair.load(Ordering::SeqCst) {
        // server.pair (SURF-2's DialogPair): a 10-minute code.
        r#"{"code":"pair-code-123","expires_in":600}"#.to_owned()
    } else if path.split('?').next().unwrap_or("") == "/api/mcp" && mcp.load(Ordering::SeqCst) {
        // mcp.list (SURF-1: the sidebar MCP section rows) — two
        // connected servers + one failed.
        r#"{"data":[{"name":"github","status":{"status":"connected"}},{"name":"filesystem","status":{"status":"connected"}},{"name":"broken","status":{"error":"connect ECONNREFUSED","status":"failed"}}],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if skills.load(Ordering::SeqCst) && path.split('?').next().unwrap_or("") == "/api/skill"
    {
        // skill.list (TUI-8: DialogSkill's rows — the padded name
        // column + the description).
        r#"{"data":[{"id":"greet","name":"greet","description":"Greet the user warmly","content":"greet skill body","path":".opencode/skill/greet.md"},{"id":"lint","name":"lint","description":"Lint the project","content":"lint skill body","path":".opencode/skill/lint.md"}],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if integrations.load(Ordering::SeqCst)
        && path.split('?').next().unwrap_or("") == "/api/integration"
    {
        // integration.list (lane FINAL): one key-method provider.
        r#"{"data":[{"id":"anthropic","name":"Anthropic","methods":[{"type":"key","label":"API key"}],"connections":[]}],"location":{"directory":"/tmp"}}"#.to_owned()
    } else if integrations.load(Ordering::SeqCst)
        && path.split('?').next().unwrap_or("") == "/api/integration/anthropic"
    {
        // integration.get (the connected toast's name lookup).
        r#"{"data":{"id":"anthropic","name":"Anthropic","methods":[{"type":"key","label":"API key"}],"connections":[]},"location":{"directory":"/tmp"}}"#.to_owned()
    } else if path.starts_with("/api/session/") && path.ends_with("/prompt") {
        // session.prompt response: decodable Envelope<SessionInboxUser>
        // (a decode failure would toast SendFailed and pollute the
        // temporal observations; `payload.text` is required).
        r#"{"data":{"delivery":"queue","id":"msg_user_1","payload":{"text":"count slowly"},"sessionID":"ses_alpha","time":{"created":0},"type":"user"}}"#.to_owned()
    } else if path.starts_with("/api/session/") {
        // message.list for the opened tabs: empty transcript.
        r#"{"cursor":{},"data":[]}"#.to_owned()
    } else if method == "POST" && path == "/api/session" {
        // session.create (the home submission's first hop — dogfood3's
        // --prompt E2E): a single-session envelope, NOT the list shape
        // (a decode failure would fail the whole send chain).
        r#"{"data":{"id":"ses_alpha","title":"New session","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}"#.to_owned()
    } else if path.starts_with("/api/session") {
        let rows: Vec<String> = sessions
            .iter()
            .map(|session| {
                format!(
                    r#"{{"id":"{}","title":"{}","time":{{"created":0,"updated":0}},"location":{{"directory":"/tmp"}},"projectID":"p","cost":0,"tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}"#,
                    session.id, session.title
                )
            })
            .collect();
        format!(r#"{{"cursor":{},"data":[{}]}}"#, "{}", rows.join(","))
    } else {
        "{}".to_owned()
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes());
}

/// Encode `body` as a JSON string literal (the SSE double-decode
/// layer: `data:` carries a JSON-encoded string whose contents are
/// the bus event JSON).
fn serde_json_json_string(body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 2);
    out.push('"');
    for ch in body.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// TUI-4 — the scripted persistent-pty endpoints + the test-side WS
// ---------------------------------------------------------------------------

/// One scripted terminal's `PersistentPty.Info` wire row.
fn pty_info_json(id: &str) -> String {
    format!(
        concat!(
            r#"{{"args":[],"command":"/bin/sh","cwd":"/tmp","foregroundProcess":null,"id":"{id}","#,
            r#""output":{{"head":0,"tail":0}},"pid":4242,"sessionID":"ses_alpha","#,
            r#""size":{{"cols":40,"rows":24}},"status":"running","title":"Terminal"}}"#
        ),
        id = id
    )
}

/// The REST endpoints of the persistent-pty family (the WS attach is
/// upgraded separately in [`ws_attach`]): session-terminal list /
/// create, snapshot, connect-token.
fn pty_rest(method: &str, path: &str, terminals: &TerminalFixture) -> Option<String> {
    let bare = path.split('?').next().unwrap_or_default();
    let envelope = |data: &str| {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{data}",
            data.len()
        )
    };
    if method == "GET" && bare == "/api/experimental/session/ses_alpha/terminal" {
        return Some(envelope(&format!(r#"{{"data":[{}]}}"#, pty_info_json("pty_fix"))));
    }
    if method == "POST" && bare == "/api/experimental/session/ses_alpha/terminal" {
        terminals.created.fetch_add(1, Ordering::SeqCst);
        return Some(envelope(&format!(r#"{{"data":{}}}"#, pty_info_json("pty_new"))));
    }
    if method == "GET"
        && bare.starts_with("/api/experimental/persistent-pty/")
        && bare.ends_with("/snapshot")
    {
        // The checkpoint paints fixed text through the pane's VT
        // emulator (base64 of "CHECKPOINT-OUT ").
        let body = format!(
            r#"{{"data":{{"checkpoint":"Q0hFQ0tQT0lOVC1PVVQg","cursor":{{"x":16,"y":0}},"info":{},"text":"CHECKPOINT-OUT "}}}}"#,
            pty_info_json("pty_fix")
        );
        return Some(envelope(&body));
    }
    if method == "POST" && bare.ends_with("/connect-token") {
        return Some(envelope(r#"{"data":{"ticket":"tick","expires_in":600}}"#));
    }
    None
}

/// The test-side RFC 6455 attach: accept the upgrade, send the
/// scripted attach/replay frames, then record every inbound binary
/// interaction frame (the TUI's typed input + resize controls).
fn ws_attach(
    mut socket: std::net::TcpStream,
    head: &str,
    terminals: &TerminalFixture,
    pty_id: &str,
) {
    let _ = pty_id;
    let Some(key) = head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim().eq_ignore_ascii_case("sec-websocket-key").then(|| value.trim().to_owned())
    }) else {
        return;
    };
    let accept = b64(&sha1(key.as_bytes(), b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"));
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: Upgrade\r\nsec-websocket-accept: {accept}\r\n\r\n"
    );
    if socket.write_all(response.as_bytes()).is_err() {
        return;
    }
    terminals.ws_attached.fetch_add(1, Ordering::SeqCst);
    // The attach protocol (handlers/persistent_pty.rs §3): attached →
    // replay binary → replay_complete; then one LIVE output frame.
    socket.set_read_timeout(Some(Duration::from_millis(20))).ok();
    let attached = format!(
        concat!(
            r#"{{"type":"attached","attachmentID":"fix-attach","inputProtocol":1,"info":{info},"role":"controller","generation":1,"replay":{{"requestedOffset":0,"availableOffset":0,"endOffset":0,"truncated":false}}}}"#
        ),
        info = pty_info_json(pty_id)
    );
    if ws_send(&mut socket, 0x1, attached.as_bytes()).is_err() {
        return;
    }
    if ws_send(&mut socket, 0x2, b"REPLAY-OUT ").is_err() {
        return;
    }
    if ws_send(&mut socket, 0x1, br#"{"type":"replay_complete","endOffset":0}"#).is_err() {
        return;
    }
    if ws_send(&mut socket, 0x2, b"LIVE-OUT").is_err() {
        return;
    }
    // Drain inbound frames until the PEER closes (a read timeout is a
    // quiet socket, not an end — the attach stays open for the test's
    // lifetime); record every binary frame (the TUI's interaction
    // frames).
    let mut buffer = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        match socket.read(&mut buffer) {
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            // EOF (the peer closed) and hard errors both end the pump.
            Ok(0) | Err(_) => return,
            Ok(count) => {
                pending.extend_from_slice(&buffer[..count]);
                while let Some((opcode, payload, consumed)) = ws_frame(&pending) {
                    pending.drain(..consumed);
                    if opcode == 0x2 {
                        terminals.input_frames.lock().expect("frames").push(payload);
                    }
                    if opcode == 0x8 {
                        let _ = ws_send(&mut socket, 0x8, &[0x03, 0xe8]);
                        return;
                    }
                }
            }
        }
    }
}

/// One outbound WS frame (server→client: unmasked).
fn ws_send(socket: &mut std::net::TcpStream, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut frame = vec![0x80 | opcode];
    if payload.len() < 126 {
        frame.push(u8::try_from(payload.len()).unwrap_or(126));
    } else {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(payload);
    socket.write_all(&frame)
}

/// Decode one inbound WS frame (client→server: masked).
#[must_use]
fn ws_frame(bytes: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    if bytes.len() < 2 {
        return None;
    }
    let opcode = bytes[0] & 0x0f;
    let masked = bytes[1] & 0x80 != 0;
    let mut length = usize::from(bytes[1] & 0x7f);
    let mut at = 2usize;
    if length == 126 {
        if bytes.len() < at + 2 {
            return None;
        }
        length = usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
        at += 2;
    } else if length == 127 {
        if bytes.len() < at + 8 {
            return None;
        }
        let mut wide = [0u8; 8];
        wide.copy_from_slice(&bytes[at..at + 8]);
        length = usize::try_from(u64::from_be_bytes(wide)).unwrap_or(0);
        at += 8;
    }
    let mask = if masked {
        if bytes.len() < at + 4 {
            return None;
        }
        let mask: [u8; 4] = bytes[at..at + 4].try_into().ok()?;
        at += 4;
        mask
    } else {
        [0u8; 4]
    };
    if bytes.len() < at + length {
        return None;
    }
    let payload = bytes[at..at + length]
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ mask[index % 4])
        .collect();
    Some((opcode, payload, at + length))
}

/// SHA-1 (test-only, for the WS accept key).
#[must_use]
fn sha1(data: &[u8], suffix: &[u8]) -> [u8; 20] {
    let mut message = data.to_vec();
    message.extend_from_slice(suffix);
    let bit_length = (message.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut words = [0u32; 80];
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    for chunk in message.chunks(64) {
        for (index, word) in chunk.chunks(4).enumerate() {
            words[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..80 {
            let value = words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16];
            words[index] = value.rotate_left(1);
        }
        // The standard's working variables (FIPS 180-4 §6.2.2):
        // `wa`..`we` rotate through the round schedule.
        let [ha, hb, hc, hd, he] = state;
        let (mut wa, mut wb, mut wc, mut wd, mut we) = (ha, hb, hc, hd, he);
        for (index, word) in words.iter().enumerate() {
            let (round, k) = match index {
                0..=19 => ((wb & wc) | ((!wb) & wd), 0x5A82_7999),
                20..=39 => (wb ^ wc ^ wd, 0x6ED9_EBA1),
                40..=59 => ((wb & wc) | (wb & wd) | (wc & wd), 0x8F1B_BCDC),
                _ => (wb ^ wc ^ wd, 0xCA62_C1D6),
            };
            let temp = wa
                .rotate_left(5)
                .wrapping_add(round)
                .wrapping_add(we)
                .wrapping_add(k)
                .wrapping_add(*word);
            we = wd;
            wd = wc;
            wc = wb.rotate_left(30);
            wb = wa;
            wa = temp;
        }
        state[0] = state[0].wrapping_add(wa);
        state[1] = state[1].wrapping_add(wb);
        state[2] = state[2].wrapping_add(wc);
        state[3] = state[3].wrapping_add(wd);
        state[4] = state[4].wrapping_add(we);
    }
    let mut digest = [0u8; 20];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// Standard base64 (test-only, for the WS accept key + checkpoints).
#[must_use]
fn b64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        // One 6-bit group per output char; group i is real while it
        // overlaps the chunk's n bytes: ceil(n*8/6) chars + padding.
        let valid = (chunk.len() * 8).div_ceil(6);
        for (index, shift) in [18u32, 12, 6, 0].iter().enumerate() {
            if index < valid {
                let slot = usize::try_from((group >> shift) & 0x3f).unwrap_or(0);
                out.push(char::from(ALPHABET[slot]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
// ---------------------------------------------------------------------------
// Live harness: PTY + reader thread feeding the VtStream + driver ops
// ---------------------------------------------------------------------------

/// A clonable handle onto the live PTY capture: everything a driver
/// thread needs (write side, both mirrors, the clock) without owning
/// the fd-restoring guard (that stays on the test's main thread).
pub struct HarnessHandle {
    pub vt: Arc<Mutex<VtStream>>,
    raw: Arc<Mutex<Vec<u8>>>,
    writer: File,
    start: std::time::Instant,
}

impl HarnessHandle {
    /// Resize the harness PTY (SCAN-EF2: E1 resize cell + B5 event
    /// scripts) — real TIOCSWINSZ on the master, so the app observes
    /// SIGWINCH exactly as a terminal resize delivers it.
    pub fn resize_pty(&self, cols: u16, rows: u16) {
        unsafe {
            let winsize = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
            libc::ioctl(self.writer.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        }
    }

    /// Harness clock (ms since attach).
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
    }

    /// SGR 1006 mouse-move (button 35 = motion with no button) at
    /// 0-based cell `(x, y)`; wire coords are 1-based, so +1 on both.
    pub fn sgr_motion(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<35;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 left-button PRESS at 0-based cell `(x, y)` (button
    /// 0; wire coords 1-based).
    pub fn sgr_press(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 left-button RELEASE at 0-based cell `(x, y)` — the
    /// `onMouseUp` trigger (v2's pointer surfaces act on release).
    pub fn sgr_release(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 wheel UP at 0-based cell `(x, y)` (button 64) —
    /// crossterm decodes it as `MouseEventKind::ScrollUp`.
    pub fn sgr_scroll_up(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<64;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 wheel DOWN at 0-based cell `(x, y)` (button 65) —
    /// `MouseEventKind::ScrollDown`.
    pub fn sgr_scroll_down(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<65;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 MIDDLE-button PRESS at 0-based cell `(x, y)` (button
    /// 1 — lane KM's tab close-on-press).
    pub fn sgr_middle_press(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<1;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 RIGHT-button PRESS at 0-based cell `(x, y)` (button
    /// 2 — lane KM's tab context menu).
    pub fn sgr_right_press(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<2;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// SGR 1006 left-button DRAG at 0-based cell `(x, y)` (button 32
    /// — motion with the left button held, lane KM's tab reorder).
    pub fn sgr_drag(&mut self, x: u16, y: u16) {
        self.write(format!("\x1b[<32;{};{}M", x + 1, y + 1).as_bytes());
    }

    /// Bytes fed so far (the cumulative mirror length).
    #[must_use]
    pub fn raw_len(&self) -> usize {
        self.raw.lock().expect("raw").len()
    }

    /// A copy of the raw byte mirror (debug dumps).
    #[must_use]
    pub fn raw_mirror(&self) -> Vec<u8> {
        self.raw.lock().expect("raw").clone()
    }

    /// Wait until `needle` appears in bytes appended after `from`.
    #[must_use]
    pub fn wait_for_after(&self, needle: &[u8], from: usize, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let data = self.raw.lock().expect("raw").clone();
            if data.len() > from && data[from..].windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        false
    }

    /// Wait for `needle` anywhere in the stream.
    #[must_use]
    pub fn wait_for(&self, needle: &[u8], timeout: Duration) -> bool {
        self.wait_for_after(needle, 0, timeout)
    }

    /// Wait until the last fed byte is older than `gap_ms` (a quiet
    /// screen), or `timeout` elapses.
    #[must_use]
    pub fn wait_quiet(&self, gap_ms: u64, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let quiet = self
                .vt
                .lock()
                .expect("vt")
                .last_feed_ms()
                .is_some_and(|last| self.now_ms().saturating_sub(last) > gap_ms);
            if quiet {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Current screen state (post-feed, settled or not).
    #[must_use]
    pub fn grid_now(&self) -> Grid {
        self.vt.lock().expect("vt").grid_now()
    }

    /// Flush the open burst into a snapshot if the quiet window has
    /// passed (idempotent).
    pub fn settle_snapshot(&self) {
        let now = self.now_ms();
        self.vt.lock().expect("vt").settle_snapshot(now);
    }

    /// Total bytes fed into the parser so far.
    #[must_use]
    pub fn fed_bytes(&self) -> usize {
        self.vt.lock().expect("vt").total_bytes()
    }

    /// Wait until the screen is CELL-stable: two grids `probe_ms`
    /// apart differ in at most `tolerance` cells (the caret blink —
    /// byte-quiet is impossible: the run loop re-emits ratatui's
    /// unconditional hide-cursor sequence every 100 ms tick, a
    /// recorded finding of the idle-noise guard).
    #[must_use]
    pub fn wait_grid_stable(&self, tolerance: usize, probe_ms: u64, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let first = self.grid_now();
            std::thread::sleep(Duration::from_millis(probe_ms));
            let second = self.grid_now();
            if first.diff(&second, &HashSet::new()).len() <= tolerance {
                return true;
            }
        }
        false
    }

    /// SA-F4's instrument repair — the BEFORE-SETTLE sampler: poll one
    /// region's STYLE digests while a transient is still in flight
    /// (no settle wait, no burst-closing). The settle-first collectors
    /// (`collect_snapshots`, `capture_baseline`) structurally miss
    /// time-driven effects — the scan's shimmer leg sampled after
    /// typing settled and saw 0 style deltas across 60 ref frames;
    /// this sampler observes the churn DURING the effect. `tick` runs
    /// between polls (whatever keeps the transient live — typing, a
    /// held turn, an armed pending state).
    pub fn style_series_during(
        &self,
        window_ms: u64,
        poll_ms: u64,
        region: (usize, usize, usize, usize),
        tick: impl FnMut(),
    ) -> Vec<(u64, Vec<u64>)> {
        let (x0, y0, width, height) = region;
        let mut tick = tick;
        let start = self.now_ms();
        let mut series = Vec::new();
        while self.now_ms() - start < window_ms {
            tick();
            let grid = self.grid_now();
            let mut styles = Vec::with_capacity(width * height);
            for y in y0..y0.saturating_add(height) {
                for x in x0..x0.saturating_add(width) {
                    if x < grid.cols && y < grid.rows {
                        styles.push(grid.cell(x, y).style);
                    }
                }
            }
            series.push((self.now_ms(), styles));
            std::thread::sleep(Duration::from_millis(poll_ms));
        }
        series
    }

    /// A settled baseline plus the KNOWN-VOLATILE cells: sampled as
    /// the union of pairwise diffs over three grids spanning ≥650 ms
    /// — the 500 ms-phase caret blink ALWAYS flips phase across a
    /// 650 ms span, so the caret is caught deterministically; any
    /// other clock-driven cell is caught generically. Sweep diffs
    /// skip these so animation cannot masquerade as hover change.
    #[must_use]
    pub fn capture_baseline(&self) -> (Grid, HashSet<(usize, usize)>) {
        // Best-effort settle: the three samples below absorb residual
        // motion through the volatile-cell union either way.
        let _settled = self.wait_grid_stable(2, 300, Duration::from_secs(5));
        let first = self.grid_now();
        std::thread::sleep(Duration::from_millis(325));
        let middle = self.grid_now();
        std::thread::sleep(Duration::from_millis(325));
        let last = self.grid_now();
        let mut volatile = HashSet::new();
        for (a, b) in [(&first, &middle), (&middle, &last), (&first, &last)] {
            volatile.extend(a.diff(b, &HashSet::new()).into_iter().map(|d| (d.x, d.y)));
        }
        (last, volatile)
    }
}

/// The live capture: the PTY guard (fd restore on drop) + the shared
/// [`HarnessHandle`] the driver threads use.
pub struct LiveHarness {
    handle: HarnessHandle,
    _guard: PtyGuard,
}

impl LiveHarness {
    /// Attach the PTY at `cols×rows` and start the reader thread that
    /// feeds both the raw mirror and the [`VtStream`] (harness-clock
    /// timestamps = ms since attach).
    #[must_use]
    pub fn attach(cols: u16, rows: u16) -> Self {
        let guard = attach_pty(cols, rows);
        let vt = Arc::new(Mutex::new(VtStream::new(usize::from(cols), usize::from(rows))));
        let raw: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let reader_master = guard.master.try_clone().expect("clone read");
        let feed_vt = Arc::clone(&vt);
        let feed_raw = Arc::clone(&raw);
        let start = std::time::Instant::now();
        {
            let thread_start = start;
            std::thread::spawn(move || {
                let mut reader = reader_master;
                let mut chunk = [0u8; 4096];
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
        let writer = guard.master.try_clone().expect("clone keys");
        Self { handle: HarnessHandle { vt, raw, writer, start }, _guard: guard }
    }

    /// A driver-thread copy of the handle (dup'd writer fd; the same
    /// mirrors and clock).
    #[must_use]
    pub fn handle(&self) -> HarnessHandle {
        HarnessHandle {
            vt: Arc::clone(&self.handle.vt),
            raw: Arc::clone(&self.handle.raw),
            writer: self.handle.writer.try_clone().expect("clone driver writer"),
            start: self.handle.start,
        }
    }
}

// ---------------------------------------------------------------------------
// Run-loop driver
// ---------------------------------------------------------------------------

/// Run the real TUI loop against `url` at `cols×rows` until it exits
/// (asserts the loop itself did not error and exited within `limit`).
pub fn run_tui(url: &str, cols: u16, rows: u16, limit: Duration) {
    run_tui_boot(url, cols, rows, limit, oc_tui::app::BootArgs::default());
}

/// [`run_tui`] with explicit boot args (dogfood3: `--prompt` seeds +
/// auto-submits the first turn, `--auto` arms permission
/// autoaccept — the flags' live-loop semantics need the real boot
/// path, not a post-boot store poke).
pub fn run_tui_boot(url: &str, cols: u16, rows: u16, limit: Duration, boot: oc_tui::app::BootArgs) {
    let client = oc_client::Client::new(url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let finished = runtime.block_on(async {
        tokio::time::timeout(limit, oc_tui::app::run_with_size(client, Some((cols, rows)), boot))
            .await
    });
    match finished {
        Ok(Ok(())) => {}
        Ok(Err(error)) => panic!("run_with_size failed: {error}"),
        Err(elapsed) => panic!("the TUI did not exit in time: {elapsed}"),
    }
}

/// The alternate-screen leave sequence — the app's exit footprint in
/// the raw stream (used to prove the app was ALIVE through the whole
/// interaction: a mid-run crash tears down early and this never
/// arrives after the final chord).
pub const ALT_SCREEN_LEAVE: &[u8] = b"\x1b[?1049l";

/// The home wordmark's opening block (boot-settled marker).
pub const WORDMARK: &[u8] = "█▀▀█".as_bytes();

/// Open the FIRST history session as a tab: `<leader>l`, wait for the
/// dialog row, Enter.
pub fn open_first_session(h: &mut HarnessHandle, needle: &[u8]) -> Result<(), String> {
    let from = h.raw_len();
    h.write(b"\x18l");
    if !h.wait_for_after(needle, from, Duration::from_secs(5)) {
        return Err(format!("the session dialog never listed {needle:?}"));
    }
    h.write(b"\r");
    Ok(())
}

/// Open the SECOND history session as a second tab: `<leader>l`,
/// wait for the dialog, `ArrowDown`, `Enter`.
pub fn open_second_session(h: &mut HarnessHandle, needle: &[u8]) -> Result<(), String> {
    let from = h.raw_len();
    h.write(b"\x18l");
    if !h.wait_for_after(needle, from, Duration::from_secs(5)) {
        return Err(format!("the session dialog never listed {needle:?}"));
    }
    h.write(b"\x1b[B\r");
    Ok(())
}

/// The standard exit chord (CI-hardened, TUI-15's wedge): clear the
/// stackable UI states FIRST — each can otherwise eat one of the two
/// ctrl+c presses and leave the app looping until its 90 s cap (the
/// runner watchdog fires first):
/// * `escape` ×2 — the topmost dialogs/toasts (the error panel's
///   F11 gate, a stacked select, an open autocomplete);
/// * `ctrl+u` — a DIRTY draft would keep `app.exit` disabled
///   (keybind.ts:131 gates ctrl+c/ctrl+d on the empty prompt), and a
///   send failure RESTORES the draft (`restoreEntry`), so the chord
///   may arrive with text still in the box;
/// * then ctrl+c ×2 (`app.exit` — the first press still absorbs any
///   survivor state, the second exits) and `<leader>q` as a harmless
///   trailing chord (`session.queued_prompts` on an empty queue).
pub fn exit_chord(h: &mut HarnessHandle) {
    h.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(150));
    h.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(150));
    h.write(b"\x15");
    std::thread::sleep(Duration::from_millis(150));
    h.write(b"\x03");
    std::thread::sleep(Duration::from_millis(200));
    h.write(b"\x03");
    std::thread::sleep(Duration::from_millis(200));
    h.write(b"\x18q");
}

/// Prove the app is still alive: the exit chord must produce the
/// alternate-screen leave sequence after `from`.
pub fn assert_exit_ack(h: &mut HarnessHandle, from: usize) -> Result<(), String> {
    exit_chord(h);
    if !h.wait_for_after(ALT_SCREEN_LEAVE, from, Duration::from_secs(5)) {
        return Err("the app did not tear down on the exit chord (crashed mid-run?)".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod ws_probe_tests {
    use super::*;

    #[test]
    fn rfc6455_accept_vector() {
        let digest = sha1(b"dGhlIHNhbXBsZSBub25jZQ==", b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
        assert_eq!(
            hex(&digest),
            "b37a4f2cc0624f1690f64606cf385945b2bec4ea",
            "sha1 matches the RFC vector"
        );
        assert_eq!(b64(&digest), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    fn hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}
