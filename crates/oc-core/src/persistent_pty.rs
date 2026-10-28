//! Persistent PTY — port of `persistent-pty/index.ts` (core-runner-map
//! §6.1) as an **in-process state machine**, plus the R-09 cross-restart
//! handover lifecycle (`daemon.ts`).
//!
//! v2 runs the terminals in an out-of-process daemon (`opencode-pty`) so
//! they survive CLI restarts, with handoff tickets and per-instance state
//! directories. **The daemon process boundary is deferred to the Phase 9
//! plugin-runtime wave** (see `spike-plugin-runtime.md`); this module
//! implements the full client-facing state machine now:
//!
//! # Cross-restart handover (R-09, v2 citations)
//!
//! v2's pieces, all inside the v2 repo (the daemon binary itself is an
//! external artifact):
//!
//! - **Runtime directory + registration**: each daemon instance owns
//!   `<root>/<id>` with a `service.json` registration
//!   `{instance_id, pid, protocol, socket, token}` and a held
//!   `service.lock` (`daemon.ts:33-39`, `165-166`, `433-444`).
//! - **`prepare_handoff`**: the stopping server asks its (live) daemon
//!   for a single-use `{ticket, expires_at}` (`daemon.ts:356-380`,
//!   server handler `persistent-pty.ts:62-66`), published to the
//!   `<service-file>.pty-handoff` sidecar BEFORE the old process stops
//!   (`pty-handoff.ts:17-43`), then injected into the replacement's env
//!   as `OPENCODE_PTY_HANDOFF` (`pty-handoff.ts:68-78`), decoded at
//!   server boot (`server-process.ts:46-57`).
//! - **Adoption ladder** (`daemon.ts:403-428`): expiry check → discover
//!   the registration → protocol check → instance check → claim with the
//!   ticket. ANY failure logs "Discarding PTY restart handoff;
//!   persistent terminals will start fresh", best-effort shuts the found
//!   daemon down, and abandons (fresh directory, `daemon.ts:287-292`).
//!
//! The port maps those semantics onto the in-process machine (there is
//! no daemon socket to claim): the **handoff snapshot** `state.json`
//! (terminal infos + replay buffers, written only by [`PersistentPty::handoff`])
//! plays the role of the live daemon — its presence is the difference
//! between a clean restart chain and a crash/kill:
//!
//! - `handoff()` writes `state.json` + mints the ticket
//!   ([`PersistentPty::handoff`]); the HTTP surface and the sidecar/env
//!   plumbing already exist (`oc-server` lifecycle.rs `pty_handoff_*`).
//! - [`PersistentPty::adopt`] runs the ladder: `Expired` →
//!   `Unreachable` (no registration) → `Protocol` → `Changed`
//!   (instance mismatch) → `Owned` (previous owner still holds the
//!   lock — the Phase-9 boundary: no socket to transfer ownership
//!   through) → `InvalidTicket`/`NoHandoffState` (died without a clean
//!   handoff — v2's kill/refresh path when the daemon died:
//!   `pty-handoff.ts:35-37` shuts terminals down and the next server
//!   starts fresh).
//! - On success the machine re-attaches to the inherited directory
//!   (migrating `service.lock`/`service.json` inode-preservingly when
//!   the root moved, `daemon.ts:435-444`), restores every terminal with
//!   its pre-restart replay window (attach at a cursor replays
//!   pre-restart bytes) marked `exited` — the child processes
//!   themselves died with the old server, which is exactly what v2
//!   does when the daemon died.
//!
//! The module otherwise implements:
//!
//! - ops `list` / `create` / `write` / `resize` / `control` / `input` /
//!   `read_rows` (`read(sessionID, lines?)`) / `snapshot` / `terminate`
//!   (`remove`) / `shutdown`;
//! - ids `pty_persistent_<n>`;
//! - **attach** replay-buffer semantics: `requestedOffset` /
//!   `availableOffset` / `endOffset` / `truncated` / `data`, roles
//!   `controller` / `observer` (+ `takeover`), generations per attach;
//! - `current[sessionID]` selection — controller activity selects a
//!   terminal, observer reads and pane visibility do not;
//! - `snapshot {info, text, checkpoint, cursor}` — the checkpoint
//!   bytes are the §7 v1 format (`wire::encode_checkpoint`): magic
//!   `OCPTYCHK`, version 1, flags, cols/rows, cursor x/y, generation,
//!   length-prefixed scrollback text and a CRC32-IEEE trailer — the
//!   same bytes the out-of-process daemon serves (one machine, one
//!   serializer);
//! - durable-ish events `persistent-pty.added` / `persistent-pty.removed`
//!   publish on the bus; exited visible terminals auto-remove.

// Parity surface: the async client API mirrors the out-of-process daemon
// protocol (Phase 9 plugin-runtime wave); several ops are synchronous
// in-process today.
#![allow(clippy::unused_async)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oc_schema::id::SessionId;
use oc_util::determinism::{AscendingIdGen, Clock, IdGen, SystemClock, SystemRng};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize};
use tokio::sync::mpsc;

use crate::bus::{Bus, PublishOptions};
use crate::pty::PtyId;

/// The frontend (design §2.2): the handlers' op surface over the
/// in-process machine (`OC_PTY_DAEMON=0`) or the daemon transport.
pub mod frontend;
/// The daemon CLIENT transport (design §2.2, §5.2, §8): discover /
/// spawn-if-absent / owner connection / one-shots / subscribe.
pub mod transport;
/// The daemon wire format shared by the `oc-pty` binary (which hosts
/// this machine) and the in-server client transport (design §2.2, §4).
pub mod wire;

/// Attach role (`daemon.ts Role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Controller,
    Observer,
}

/// `PersistentPty.CreateInput` (schema/persistent-pty.ts:26-33).
#[derive(Debug, Clone)]
pub struct CreateInput {
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
    pub env: std::collections::BTreeMap<String, String>,
    pub size: Option<(u16, u16)>,
}

/// `PersistentPty.Info` — the pty info plus session grouping, foreground
/// process, size and output window.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub id: PtyId,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: crate::pty::Status,
    pub pid: u32,
    pub exit_code: Option<i32>,
    pub session_id: SessionId,
    pub foreground_process: Option<String>,
    pub size: (u16, u16),
    /// Absolute output window `[head, tail)` (bytes).
    pub output: (usize, usize),
}

/// The wire shape of `PersistentPty.Info` (schema/src/persistent-pty.ts:13-19):
/// camelCase keys, `size: {cols, rows}` and `output: {head, tail}` objects,
/// `exitCode` only once exited. The internal struct keeps `snake_case`
/// names and pair fields, so events (and only events) serialize through
/// this mapper.
#[must_use]
pub fn terminal_wire_json(info: &Info) -> serde_json::Value {
    let mut terminal = serde_json::json!({
        "id": info.id.0,
        "title": info.title,
        "command": info.command,
        "args": info.args,
        "cwd": info.cwd,
        "status": match info.status {
            crate::pty::Status::Running => "running",
            crate::pty::Status::Exited => "exited",
        },
        "pid": info.pid,
        "sessionID": info.session_id.as_str(),
        "foregroundProcess": info.foreground_process,
        "size": { "cols": info.size.0, "rows": info.size.1 },
        "output": { "head": info.output.0, "tail": info.output.1 },
    });
    if let Some(exit_code) = info.exit_code {
        terminal["exitCode"] = serde_json::json!(exit_code);
    }
    terminal
}

/// Wire conversion into the typed `PersistentPty.Info` schema — must
/// serialize identically to [`terminal_wire_json`] (proved by the
/// `persistent_pty_terminal_wire_no_drift` test).
fn terminal_schema_info(info: &Info) -> oc_schema::persistent_pty::Info {
    oc_schema::persistent_pty::Info {
        base: oc_schema::pty::Info {
            id: info.id.0.clone(),
            title: info.title.clone(),
            command: info.command.clone(),
            args: info.args.clone(),
            cwd: info.cwd.clone(),
            status: match info.status {
                crate::pty::Status::Running => oc_schema::pty::Status::Running,
                crate::pty::Status::Exited => oc_schema::pty::Status::Exited,
            },
            pid: u64::from(info.pid),
            exit_code: info.exit_code.map(|code| u64::try_from(code).unwrap_or(0)),
        },
        session_id: info.session_id.clone(),
        foreground_process: info.foreground_process.clone(),
        size: oc_schema::persistent_pty::Size { cols: info.size.0, rows: info.size.1 },
        output: oc_schema::persistent_pty::Output {
            head: info.output.0 as u64,
            tail: info.output.1 as u64,
        },
    }
}

/// `PersistentPty.Snapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub info: Info,
    /// The visible screen rows joined by newlines.
    pub text: String,
    /// Terminal-emulator checkpoint — the §7 v1 format (magic
    /// `OCPTYCHK`, version 1, big-endian cols/rows/cursor/generation,
    /// length-prefixed scrollback, CRC32-IEEE trailer); produced by
    /// `wire::encode_checkpoint` — identical bytes in-process and
    /// over the daemon socket.
    pub checkpoint: Vec<u8>,
    pub cursor: (usize, usize),
}

/// `PersistentPty.Handoff` — daemon handoff ticket (R-09; the wire
/// shape `schema/persistent-pty.ts` `Handoff`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    pub directory: String,
    pub instance_id: String,
    pub ticket: String,
    pub expires_at: u64,
}

/// The registration written to `<runtime-dir>/service.json`
/// (`daemon.ts:33-39` — `socket` is empty until the Phase-9 daemon
/// boundary; `token` is its future wire auth).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Registration {
    pub instance_id: String,
    pub pid: u32,
    pub protocol: u64,
    pub socket: String,
    pub token: String,
}

/// The handoff snapshot written to `<runtime-dir>/state.json` by
/// [`PersistentPty::handoff`] and consumed by
/// [`PersistentPty::adopt`] — the in-process stand-in for v2's live
/// daemon: its presence is what distinguishes a clean restart chain
/// from a crash/kill.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandoffState {
    pub instance_id: String,
    pub ticket: String,
    pub expires_at: u64,
    pub terminals: Vec<TerminalSnapshot>,
}

/// One terminal's handoff-snapshot entry: the info plus the replay
/// window (buffer + absolute cursor) and the plain-text screen rows the
/// in-process machine tracks.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerminalSnapshot {
    pub info: Info,
    /// Base64 replay buffer.
    pub buffer: String,
    /// Absolute byte offset of `buffer[0]`.
    pub buffer_cursor: usize,
    /// Visible screen rows (`read`/`snapshot` restore).
    pub rows: Vec<String>,
    /// Screen cursor.
    pub cursor: (usize, usize),
    /// Controller generation at snapshot time.
    pub generation: u64,
}

/// Why an inherited handoff was discarded
/// (`daemon.ts:403-428` — every failure path lands on the same "start
/// fresh" behavior).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct AdoptionError(pub &'static str);

impl AdoptionError {
    /// `daemon.ts:406-407` — the ticket expired.
    #[must_use]
    pub const fn expired() -> Self {
        Self("PTY restart handoff expired")
    }
    /// `daemon.ts:203-205` — no readable registration where the
    /// handoff points.
    #[must_use]
    pub const fn unreachable() -> Self {
        Self("PTY restart owner unreachable")
    }
    /// `daemon.ts:209-216` — protocol mismatch.
    #[must_use]
    pub const fn protocol() -> Self {
        Self("opencode-pty protocol mismatch")
    }
    /// `daemon.ts:411-412` — the registered instance changed.
    #[must_use]
    pub const fn changed() -> Self {
        Self("PTY restart daemon changed")
    }
    /// `daemon.ts:415` — the previous owner still holds the lock; the
    /// in-process machine has no socket to transfer ownership through
    /// (the Phase-9 boundary).
    #[must_use]
    pub const fn owned() -> Self {
        Self("PTY restart owner is still running")
    }
    /// The ticket does not match the handoff snapshot.
    #[must_use]
    pub const fn invalid_ticket() -> Self {
        Self("PTY restart handoff ticket rejected")
    }
    /// The previous owner died without a clean handoff — v2's
    /// daemon-died path (kill/refresh: `pty-handoff.ts:35-37`).
    #[must_use]
    pub const fn no_state() -> Self {
        Self("PTY restart owner died without a handoff")
    }
}

/// The outcome of [`PersistentPty::adopt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// Re-attached to the inherited directory; `restored` terminals
    /// carry their pre-restart replay windows (children are gone —
    /// marked exited, v2 daemon-died semantics).
    Adopted { restored: usize, directory: String },
    /// Discarded with the v2 warning; the machine keeps/creates a
    /// fresh runtime identity.
    Discarded { reason: AdoptionError },
}

/// `PersistentPty.ReadResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadResult {
    pub pty_id: PtyId,
    pub title: String,
    pub cwd: String,
    pub foreground_process: Option<String>,
    pub screen: Screen,
}

/// `ReadResult.screen`.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub text: String,
    pub cols: u16,
    pub rows: u16,
    pub cursor: (usize, usize),
}

/// Attach replay-buffer metadata (`Attachment.replay`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    pub requested_offset: usize,
    pub available_offset: usize,
    pub end_offset: usize,
    pub truncated: bool,
}

/// `attach` input.
pub struct AttachInput<'a> {
    pub cursor: usize,
    pub attachment_id: &'a str,
    pub role: Role,
    pub takeover: bool,
    pub on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
    pub on_end: Box<dyn FnOnce() + Send + Sync>,
}

/// `attach` result.
pub struct Attachment {
    pub info: Info,
    pub role: Role,
    pub generation: u64,
    pub replay: Replay,
    pub data: Vec<u8>,
    inner: Arc<Inner>,
    id: PtyId,
    token: u64,
}

impl Attachment {
    /// Starts live delivery after replay applied.
    pub fn activate(&self) {
        self.inner.activate(&self.id, self.token);
    }

    /// Stops delivery.
    pub fn detach(&self) {
        self.inner.detach(&self.id, self.token);
    }
}

/// `StreamEvent` — the attach stream push events.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// Live output bytes.
    Output(Vec<u8>),
    /// Terminal resized (daemon.ts:107-113): the post-resize emulator
    /// checkpoint rides along so clients can fast-forward instead of
    /// replaying from scratch.
    Resized { cols: u16, rows: u16, generation: u64, checkpoint: Vec<u8> },
    /// The terminal exited.
    Exited { exit_code: Option<i32> },
    /// A controller took over and the generation bumped (design §12
    /// Q3, the APPROVED machine delta): existing subscribers learn the
    /// new controller attachment + generation so stale checkpoints can
    /// be invalidated. `attachment_id` is `None` when the controller
    /// was cleared without a replacement.
    ControllerChanged { attachment_id: Option<String>, generation: u64 },
}

/// `PersistentPty.UnavailableError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("PersistentPty.UnavailableError: {message}")]
pub struct UnavailableError {
    pub message: String,
}

/// `PersistentPty.NotFoundError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("PersistentPty.NotFoundError: {pty_id}")]
pub struct NotFoundError {
    pub pty_id: PtyId,
}

/// Every op's error shape (`PersistentPty.UnavailableError` /
/// `NotFoundError` — the handlers map onto 503/404 by message prefix).
pub type PubResult<T> = Result<T, UnavailableError>;

struct Live {
    info: Info,
    /// Absolute-byte ring window (like the non-persistent buffer).
    buffer: Vec<u8>,
    buffer_cursor: usize,
    writer: Option<Box<dyn std::io::Write + Send>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    killer: Option<Box<dyn portable_pty::ChildKiller + Send>>,
    /// VT terminal emulator (RW-02): real screen tracking over the
    /// output stream — CSI/OSC sequences, cursor movement, erase,
    /// alt-screen and resize handled by `vt100` (the same crate the
    /// TUI pane uses). Rows/cursor derive on demand.
    screen: vt100::Parser,
    generation: u64,
    controller: Option<String>,
    subscribers: HashMap<u64, Subscriber>,
    ended: HashMap<u64, Subscriber>,
}

struct Subscriber {
    on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
    on_end: Box<dyn FnOnce() + Send + Sync>,
    active: bool,
    detached: bool,
    pending: Vec<StreamEvent>,
    ended: bool,
}

const OUTPUT_LIMIT: usize = 1024 * 1024 * 8;

impl Live {
    /// A fresh VT grid at `size` (RW-02), with `history` bytes replayed
    /// through it (restore path: the retained replay buffer IS the
    /// screen's history — deterministic reconstruction).
    fn new_screen(size: (u16, u16), history: &[u8]) -> vt100::Parser {
        let mut parser = vt100::Parser::new(size.1.max(1), size.0.max(1), 0);
        parser.process(history);
        parser
    }

    /// The visible screen rows as plain text, trailing blanks trimmed
    /// (`read`/`snapshot`/checkpoints all read this).
    fn screen_rows(&self) -> Vec<String> {
        vt_rows(&self.screen)
    }

    /// The screen cursor `(x, y)`.
    fn screen_cursor(&self) -> (usize, usize) {
        vt_cursor(&self.screen)
    }
}

/// The visible rows of a VT grid as plain text, trailing blanks
/// trimmed (RW-02; free so tests can drive raw streams).
fn vt_rows(parser: &vt100::Parser) -> Vec<String> {
    let screen = parser.screen();
    let (_, cols) = screen.size();
    screen.rows(0, cols).map(|row| row.trim_end().to_owned()).collect()
}

/// The cursor `(x, y)` of a VT grid.
fn vt_cursor(parser: &vt100::Parser) -> (usize, usize) {
    let (y, x) = parser.screen().cursor_position();
    (usize::from(x), usize::from(y))
}

/// The daemon-protocol generation this machine speaks
/// (`daemon.ts:8` `ProtocolVersion`; bumped on wire-format changes).
/// Version 7 mirrors v2 (design §4.2): registrations written by earlier
/// betas (protocol `1`) are discarded — terminals are best-effort
/// across upgrades.
pub const PTY_PROTOCOL_VERSION: u64 = 7;

/// Handoff ticket TTL (the sidecar default when a daemon hands back
/// null, `pty-handoff.ts:61` — 30s).
const HANDOFF_TTL_MS: u64 = 30_000;

/// Directory/file names inside the runtime root (`daemon.ts`:
/// `service.json`, `service.lock`, `433-444`).
const SERVICE_JSON: &str = "service.json";
const SERVICE_LOCK: &str = "service.lock";
const STATE_JSON: &str = "state.json";

/// A held runtime directory: `<root>/<id>` with an exclusively-locked
/// `service.lock` and a written `service.json` registration. The lock
/// releases when the owning process dies — the liveness signal of
/// v2's daemon (which pings its socket instead, `daemon.ts:217-226`).
struct RuntimeScope {
    root: PathBuf,
    id: String,
    #[allow(dead_code)] // held for the lock; dropped only with the machine
    lock: std::fs::File,
}

impl RuntimeScope {
    fn directory(&self) -> PathBuf {
        self.root.join(&self.id)
    }
}

struct Inner {
    terminals: Mutex<HashMap<PtyId, Live>>,
    /// Controller activity selects a terminal per session.
    current: Mutex<HashMap<SessionId, PtyId>>,
    next_number: Mutex<u64>,
    next_token: Mutex<u64>,
    #[allow(dead_code)] // instance-scoped id source reserved for the Phase 9 daemon handoff
    ids: Arc<dyn IdGen>,
    /// The server bus — `None` when the machine is hosted out-of-process
    /// (the `oc-pty` daemon injects [`Inner::event_sink`] instead,
    /// design §3.1).
    bus: Option<Bus>,
    /// Host-injected publication sink (design §3.1): when present, the
    /// machine's bus publications route here instead of the bus. The
    /// daemon host serializes each publication to its owner connection
    /// as a `bus_event` frame; the in-process host leaves it `None` and
    /// publishes onto the real bus (byte-identical behavior — R3).
    event_sink: Option<EventSink>,
    clock: Arc<dyn Clock>,
    /// The runtime directory this machine owns (`None` until
    /// [`PersistentPty::set_runtime_dir`] — tests/embedders may skip
    /// the handover surface entirely).
    runtime: Mutex<Option<RuntimeScope>>,
}

/// The host-injected publication sink (design §3.1): `(event type,
/// payload)` — the payload is the verbatim bus `data` object.
pub type EventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// The persistent PTY state machine (`PersistentPty.Service`).
#[derive(Clone)]
pub struct PersistentPty {
    inner: Arc<Inner>,
    /// Default program (`ShellSelect.environment(bin)`).
    default_program: Arc<dyn Fn() -> Option<String> + Send + Sync>,
}

impl PersistentPty {
    /// Assembles the in-process machine over a bus.
    #[must_use]
    pub fn new(bus: Bus) -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        Self {
            inner: Arc::new(Inner {
                terminals: Mutex::new(HashMap::new()),
                current: Mutex::new(HashMap::new()),
                next_number: Mutex::new(0),
                next_token: Mutex::new(0),
                ids,
                bus: Some(bus),
                event_sink: None,
                clock: Arc::new(SystemClock),
                runtime: Mutex::new(None),
            }),
            default_program: Arc::new(|| None),
        }
    }

    /// Assembles the machine for an out-of-process host (design §3.1):
    /// bus publications route through `event_sink` instead of a bus.
    /// The `oc-pty` daemon serializes each `(type, payload)` to its
    /// owner connection as a `bus_event` frame; the client layer
    /// republishes them onto the server bus.
    #[must_use]
    pub fn hosted(event_sink: EventSink) -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        Self {
            inner: Arc::new(Inner {
                terminals: Mutex::new(HashMap::new()),
                current: Mutex::new(HashMap::new()),
                next_number: Mutex::new(0),
                next_token: Mutex::new(0),
                ids,
                bus: None,
                event_sink: Some(event_sink),
                clock: Arc::new(SystemClock),
                runtime: Mutex::new(None),
            }),
            default_program: Arc::new(|| None),
        }
    }

    /// Overrides the clock (handoff expiry determinism in tests).
    #[must_use]
    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        Self {
            inner: Arc::new(Inner {
                terminals: Mutex::new(HashMap::new()),
                current: Mutex::new(HashMap::new()),
                next_number: Mutex::new(0),
                next_token: Mutex::new(0),
                ids,
                bus: self.inner.bus.clone(),
                event_sink: self.inner.event_sink.clone(),
                clock,
                runtime: Mutex::new(None),
            }),
            default_program: self.default_program.clone(),
        }
    }

    /// Claims a runtime directory under `root` for this machine:
    /// `<root>/<fresh-id>` with a locked `service.lock` and the
    /// `service.json` registration (`daemon.ts:165-166`). A directory
    /// whose lock a dead predecessor left is skipped by construction
    /// (fresh id); a lock held by a LIVE peer is the
    /// unreachable-daemon case — one retry in a fresh directory, then
    /// an error (`daemon.ts:275-285` `start()`).
    pub fn set_runtime_dir(&self, root: &Path) -> Result<(), String> {
        let mut last: Option<String> = None;
        for _ in 0..2 {
            match claim_runtime_dir(root, None) {
                Ok(scope) => {
                    *self.inner.runtime.lock() = Some(scope);
                    return Ok(());
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| "could not claim a PTY runtime directory".to_owned()))
    }

    /// The runtime directory this machine owns (diagnostics/tests).
    #[must_use]
    pub fn runtime_dir(&self) -> Option<String> {
        self.inner
            .runtime
            .lock()
            .as_ref()
            .map(|scope| scope.directory().to_string_lossy().into_owned())
    }

    /// Claims a NAMED runtime directory `<root>/<id>` — the daemon
    /// host's claim (design §5.1: `oc-pty daemon --runtime-dir ROOT
    /// --name ID`), as opposed to [`PersistentPty::set_runtime_dir`]'s
    /// fresh-random identity. A lock held by a live predecessor is the
    /// unreachable-daemon case: the caller exits non-zero and the
    /// client retries in a fresh directory (§8.2). `.`/`..` are
    /// rejected with the empty and `/`-containing names — they would
    /// claim the ROOT itself (or its parent), never a child directory.
    pub fn bind_runtime_dir(&self, root: &Path, id: &str) -> Result<(), String> {
        if id.is_empty() || id.contains('/') || matches!(id, "." | "..") {
            return Err(format!("invalid PTY runtime directory name: {id}"));
        }
        let scope = claim_runtime_dir(root, Some(id))?;
        *self.inner.runtime.lock() = Some(scope);
        Ok(())
    }

    /// The registration this machine last wrote (`service.json`).
    #[must_use]
    pub fn registration(&self) -> Option<Registration> {
        self.inner.runtime.lock().as_ref().and_then(|scope| read_registration(&scope.directory()))
    }

    /// Rewrites the registration with the daemon's bound socket path
    /// (`service.json` carries `socket` for discovery — `daemon.ts:38`).
    pub fn set_registration_socket(&self, socket: &str) -> Result<(), String> {
        let directory = self
            .inner
            .runtime
            .lock()
            .as_ref()
            .map(RuntimeScope::directory)
            .ok_or_else(|| "no PTY runtime directory claimed".to_owned())?;
        let mut registration = read_registration(&directory)
            .ok_or_else(|| "PTY registration unreadable".to_owned())?;
        socket.clone_into(&mut registration.socket);
        write_json_atomic(&directory.join(SERVICE_JSON), &registration)
            .map_err(|error| format!("writing PTY registration: {error}"))
    }

    /// Re-anchors the runtime scope to `directory` (design §6 arm A,
    /// gate finding 6): after a client-side inode-preserving migration
    /// of `service.lock` + `service.json` into a new root, the live
    /// daemon's write paths (`state.json`, registration rewrites) must
    /// follow. The held lock rides its inode through the rename, so
    /// only the paths change. `daemon.sock` is NOT migrated (the
    /// daemon remembers its bound socket for exit cleanup). Returns
    /// whether the directory actually changed.
    #[must_use]
    pub fn reanchor_runtime_dir(&self, directory: &Path) -> bool {
        let Some((root, id)) = directory
            .parent()
            .zip(directory.file_name().map(|name| name.to_string_lossy().into_owned()))
        else {
            return false;
        };
        let mut runtime = self.inner.runtime.lock();
        let Some(scope) = runtime.as_mut() else { return false };
        if scope.directory() == directory {
            return false;
        }
        scope.root = root.to_path_buf();
        scope.id = id;
        true
    }

    /// The `own` claim's selection reset (design §6 arm A, gate ruling
    /// 2): a new owner starts a fresh selection epoch — `current[]` is
    /// cleared (v2's replacement has an empty map, `index.ts:140`) and
    /// every terminal's `controller` field is dropped (dead
    /// `attachment_ids` would otherwise suppress resize generation bumps,
    /// `persistent_pty.rs` resize). Generation counters are NOT reset —
    /// they stay monotonic forever (§7 carries generation).
    pub fn reset_selection_epoch(&self) {
        self.inner.current.lock().clear();
        let mut terminals = self.inner.terminals.lock();
        for live in terminals.values_mut() {
            live.controller = None;
        }
    }

    /// Restores terminals from a handoff snapshot (design §2.2/§6 arm
    /// B): replay windows intact, children gone (each terminal marked
    /// `exited` — v2's daemon-death outcome). Returns the number of
    /// restored terminals. Shared by the in-process adoption arm and
    /// the daemon's `restore` op (owner-only, §4.3).
    #[must_use]
    pub fn restore(&self, state: &HandoffState) -> usize {
        self.inner.restore(state)
    }

    /// Removes the handoff state file — the kill/refresh invalidation
    /// and the consumption half of a successful `own` claim (design
    /// §4.3: the daemon arm consumes the ticket in memory AND removes
    /// `state.json`). Public for the daemon host.
    pub fn invalidate_handoff_state(&self) {
        self.inner.invalidate_handoff_state();
    }

    /// The process-boundary seam: releases the runtime lock — exactly
    /// what process death does to `service.lock` (the fd closes; the
    /// kernel drops the flock). Used by the cross-restart tests to
    /// model a server that stopped; the in-process terminals left
    /// behind are unreachable afterwards, like real orphans.
    pub fn release_runtime(&self) {
        *self.inner.runtime.lock() = None;
    }

    /// Overrides the default program seam.
    #[must_use]
    pub fn with_default_program(
        mut self,
        program: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    ) -> Self {
        self.default_program = program;
        self
    }

    fn next_terminal_id(&self) -> PtyId {
        let mut number = self.inner.next_number.lock();
        *number += 1;
        PtyId(format!("pty_persistent_{number}"))
    }

    async fn publish(&self, type_: &str, data: serde_json::Value) {
        if let Some(sink) = &self.inner.event_sink {
            // Host-injected route (design §3.1): the daemon serializes
            // the publication to its owner connection; payloads stay
            // byte-identical to the bus route.
            sink(type_, data);
            return;
        }
        if let Some(bus) = &self.inner.bus {
            let definition = crate::bus::registry::by_type(type_)
                .cloned()
                .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(type_));
            let _ = bus.publish(&definition, data, PublishOptions::default()).await;
        }
    }

    /// `list` (persistent-pty/index.ts:142-149).
    pub fn list(&self, session_id: Option<&SessionId>) -> PubResult<Vec<Info>> {
        Ok(self
            .inner
            .terminals
            .lock()
            .values()
            .map(|live| live.info.clone())
            .filter(|info| session_id.is_none_or(|session| &info.session_id == session))
            .collect())
    }

    /// `get`.
    pub fn get(&self, id: &PtyId) -> PubResult<Info> {
        self.inner
            .terminals
            .lock()
            .get(id)
            .map(|live| live.info.clone())
            .ok_or_else(|| NotFoundError { pty_id: id.clone() }.into_unavailable())
    }

    /// `create` (index.ts:157-180): spawn the terminal, publish
    /// `persistent-pty.added`.
    #[allow(clippy::too_many_lines)] // spawn + wire + publish, mirrors index.ts create
    pub async fn create(&self, session_id: &SessionId, input: CreateInput) -> PubResult<Info> {
        let id = self.next_terminal_id();
        let program =
            input.command.clone().or_else(|| (self.default_program)()).ok_or_else(|| {
                UnavailableError { message: "no terminal program configured".into() }
            })?;
        let cwd = input.cwd.clone().unwrap_or_else(|| "/".to_string());
        let (cols, rows) = input.size.unwrap_or((80, 24));
        let mut builder = CommandBuilder::new(&program);
        builder.args(&input.args);
        builder.cwd(&cwd);
        for (key, value) in &input.env {
            builder.env(key, value);
        }
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| UnavailableError { message: error.to_string() })?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| UnavailableError { message: error.to_string() })?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| UnavailableError { message: error.to_string() })?;
        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|error| UnavailableError { message: error.to_string() })?;
        drop(pair.slave);
        let killer = child.clone_killer();
        let pid = child.process_id().unwrap_or(0);

        let info = Info {
            id: id.clone(),
            title: input.title,
            command: program,
            args: input.args,
            cwd,
            status: crate::pty::Status::Running,
            pid,
            exit_code: None,
            session_id: session_id.clone(),
            foreground_process: None,
            size: (cols, rows),
            output: (0, 0),
        };
        let live = Live {
            info: info.clone(),
            buffer: Vec::new(),
            buffer_cursor: 0,
            writer: Some(writer),
            master: Some(pair.master),
            killer: Some(killer),
            screen: Live::new_screen((cols, rows), &[]),
            generation: 0,
            controller: None,
            subscribers: HashMap::new(),
            ended: HashMap::new(),
        };
        self.inner.terminals.lock().insert(id.clone(), live);

        let (events_tx, mut events_rx) =
            mpsc::unbounded_channel::<(Vec<u8>, Option<Option<i32>>)>();
        {
            let reader = reader;
            let data_tx = events_tx.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut reader = reader;
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if data_tx.send((chunk[..read].to_vec(), None)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        let exit_tx = events_tx.clone();
        std::thread::spawn(move || {
            let code =
                child.wait().ok().map(|status| i32::try_from(status.exit_code()).unwrap_or(0));
            let _ = exit_tx.send((Vec::new(), Some(code)));
        });

        let inner = Arc::clone(&self.inner);
        let service = self.clone();
        let id_for_exit = id.clone();
        let session_for_exit = session_id.clone();
        tokio::spawn(async move {
            while let Some((chunk, exit)) = events_rx.recv().await {
                match exit {
                    None => inner.on_output(&id_for_exit, &chunk),
                    Some(code) => {
                        inner.mark_exited(&id_for_exit, code);
                        service
                            .publish(
                                "persistent-pty.removed",
                                serde_json::to_value(oc_schema::persistent_pty::Removed {
                                    session_id: session_for_exit.clone(),
                                    pty_id: id_for_exit.0.clone(),
                                })
                                .unwrap_or_default(),
                            )
                            .await;
                        break;
                    }
                }
            }
        });

        self.publish(
            "persistent-pty.added",
            serde_json::to_value(oc_schema::persistent_pty::Added {
                session_id: session_id.clone(),
                terminal: terminal_schema_info(&info),
            })
            .unwrap_or_default(),
        )
        .await;
        Ok(info)
    }

    /// `write` (index.ts:182-192).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn write(
        &self,
        id: &PtyId,
        data: &str,
        attachment_id: Option<&str>,
    ) -> PubResult<()> {
        self.get(id)?;
        self.inner.write(id, data.as_bytes(), attachment_id);
        Ok(())
    }

    /// `resize` (index.ts:194-211): controller activity selects the
    /// terminal for `read`.
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn resize(
        &self,
        id: &PtyId,
        cols: u16,
        rows: u16,
        attachment_id: Option<&str>,
    ) -> PubResult<()> {
        let info = self.get(id)?;
        self.inner.resize(id, cols, rows, attachment_id);
        self.inner.current.lock().insert(info.session_id, id.clone());
        Ok(())
    }

    /// `control` (index.ts:213-230).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn control(
        &self,
        id: &PtyId,
        attachment_id: &str,
        cols: u16,
        rows: u16,
    ) -> PubResult<()> {
        let info = self.get(id)?;
        self.inner.resize(id, cols, rows, Some(attachment_id));
        self.inner.current.lock().insert(info.session_id, id.clone());
        Ok(())
    }

    /// `input` (index.ts:232-251).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn input(
        &self,
        id: &PtyId,
        attachment_id: &str,
        cols: u16,
        rows: u16,
        data: &[u8],
    ) -> PubResult<()> {
        let info = self.get(id)?;
        self.inner.resize(id, cols, rows, Some(attachment_id));
        self.inner.write(id, data, Some(attachment_id));
        self.inner.current.lock().insert(info.session_id, id.clone());
        Ok(())
    }

    /// `snapshot` (index.ts:253-263). The checkpoint bytes are the
    /// versioned daemon wire format (design §7, R6): magic
    /// `OCPTYCHK`, rows/cursor/size/generation + scrollback + CRC32 —
    /// the in-process backend produces the identical bytes the daemon
    /// serves (same machine code path).
    pub fn snapshot(&self, id: &PtyId) -> PubResult<Snapshot> {
        let terminals = self.inner.terminals.lock();
        let live = terminals
            .get(id)
            .ok_or_else(|| NotFoundError { pty_id: id.clone() }.into_unavailable())?;
        let text = live.screen_rows().join("\n");
        let cursor = live.screen_cursor();
        let checkpoint = wire::encode_checkpoint(&wire::CheckpointV1 {
            cols: live.info.size.0,
            rows: live.info.size.1,
            cursor_x: u32::try_from(cursor.0).unwrap_or(u32::MAX),
            cursor_y: u32::try_from(cursor.1).unwrap_or(u32::MAX),
            generation: live.generation,
            scrollback: text.clone(),
        });
        Ok(Snapshot { info: live.info.clone(), text, checkpoint, cursor })
    }

    /// `read(sessionID, lines?)` (index.ts:265-291): the current terminal
    /// for the session; `None` when nothing is selected or the selection
    /// went stale. `lines` bounds the returned rows (1..=65535).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn read(
        &self,
        session_id: &SessionId,
        lines: Option<u32>,
    ) -> PubResult<Option<ReadResult>> {
        if let Some(lines) = lines {
            if !(1..=65535).contains(&lines) {
                return Err(UnavailableError {
                    message: "lines must be an integer between 1 and 65535".to_string(),
                });
            }
        }
        let Some(id) = self.inner.current.lock().get(session_id).cloned() else {
            return Ok(None);
        };
        let shadow = {
            let terminals = self.inner.terminals.lock();
            terminals.get(&id).map(|live| LiveShadow {
                info: live.info.clone(),
                rows: live.screen_rows(),
                cursor: live.screen_cursor(),
            })
        };
        let Some(live) = shadow else {
            // Stale selection: drop it (index.ts:272-274).
            self.inner.current.lock().remove(session_id);
            return Ok(None);
        };
        if live.info.session_id != *session_id {
            if self.inner.current.lock().get(session_id) == Some(&id) {
                self.inner.current.lock().remove(session_id);
            }
            return Ok(None);
        }
        let rows = live.rows;
        let text = match lines {
            Some(lines) => {
                rows.iter().rev().take(lines as usize).rev().cloned().collect::<Vec<_>>().join("\n")
            }
            None => rows.join("\n"),
        };
        Ok(Some(ReadResult {
            pty_id: live.info.id,
            title: live.info.title,
            cwd: live.info.cwd,
            foreground_process: live.info.foreground_process,
            screen: Screen {
                text,
                cols: live.info.size.0,
                rows: live.info.size.1,
                cursor: live.cursor,
            },
        }))
    }

    /// `remove` (`terminate`) (index.ts:293-300).
    pub async fn remove(&self, id: &PtyId) -> PubResult<()> {
        let info = self.get(id)?;
        self.inner.terminate(id);
        if self.inner.current.lock().get(&info.session_id) == Some(id) {
            self.inner.current.lock().remove(&info.session_id);
        }
        self.publish(
            "persistent-pty.removed",
            serde_json::to_value(oc_schema::persistent_pty::Removed {
                session_id: info.session_id.clone(),
                pty_id: id.0.clone(),
            })
            .unwrap_or_default(),
        )
        .await;
        Ok(())
    }

    /// `shutdown` (index.ts:302-307) — plus the R-09 kill semantics:
    /// any pending handoff state is invalidated (v2's daemon exits, so
    /// a later claim against it fails — `pty-handoff.ts:35-37`).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn shutdown(&self) -> PubResult<()> {
        let ids: Vec<PtyId> = self.inner.terminals.lock().keys().cloned().collect();
        for id in ids {
            self.inner.terminate(&id);
        }
        self.inner.current.lock().clear();
        self.inner.invalidate_handoff_state();
        Ok(())
    }

    /// `handoff` — the `prepare_handoff` port (`daemon.ts:356-380`):
    /// snapshots every terminal (info + replay window + screen) into
    /// `<runtime-dir>/state.json` and mints a single-use ticket.
    /// `None` when no runtime directory is configured (tests/embedders
    /// without the handover surface — the daemon absent, like v2's
    /// transport before it first connects).
    pub fn handoff(&self) -> PubResult<Option<Handoff>> {
        let scope = {
            let runtime = self.inner.runtime.lock();
            let Some(scope) = runtime.as_ref() else { return Ok(None) };
            (scope.root.clone(), scope.id.clone())
        };
        let (root, id) = scope;
        let directory = root.join(&id);
        let ticket = new_random_token();
        let expires_at = now_plus(self.inner.clock.as_ref(), HANDOFF_TTL_MS);
        let terminals: Vec<TerminalSnapshot> = self
            .inner
            .terminals
            .lock()
            .values()
            .map(|live| TerminalSnapshot {
                info: live.info.clone(),
                buffer: base64_encode(&live.buffer),
                buffer_cursor: live.buffer_cursor,
                rows: live.screen_rows(),
                cursor: live.screen_cursor(),
                generation: live.generation,
            })
            .collect();
        let state = HandoffState {
            instance_id: self.inner.registration_instance_id(),
            ticket: ticket.clone(),
            expires_at,
            terminals,
        };
        write_json_atomic(&directory.join(STATE_JSON), &state)
            .map_err(|error| UnavailableError { message: error.to_string() })?;
        Ok(Some(Handoff {
            directory: directory.to_string_lossy().into_owned(),
            instance_id: state.instance_id,
            ticket,
            expires_at,
        }))
    }

    /// Adopts an inherited handoff — the `daemon.ts:403-428` ladder on
    /// the in-process machine. Every discard path warns exactly like
    /// v2 and leaves the machine on a fresh identity; the success path
    /// re-attaches to the inherited directory (migrating the lock +
    /// registration inode-preservingly when the root moved,
    /// `daemon.ts:435-444`) and restores the terminals with their
    /// pre-restart replay windows, marked exited (the children died
    /// with the old server — v2's daemon-died outcome).
    #[must_use]
    pub fn adopt(&self, inherited: &Handoff) -> Adoption {
        match self.inner.try_adopt(inherited) {
            Ok(adoption) => adoption,
            Err(reason) => {
                // daemon.ts:419-427 — terminals are best-effort; the
                // restart itself always succeeds.
                eprintln!(
                    "Discarding PTY restart handoff; persistent terminals will start fresh ({reason})"
                );
                Adoption::Discarded { reason }
            }
        }
    }

    /// `attach` (index.ts:326-349): replay buffer semantics; controllers
    /// select the terminal; `takeover` bumps the generation.
    pub fn attach(&self, id: &PtyId, input: AttachInput<'_>) -> PubResult<Attachment> {
        self.get(id)?;
        let token = {
            let mut next = self.inner.next_token.lock();
            *next += 1;
            *next
        };
        let (info, generation, replay, data) = {
            let mut terminals = self.inner.terminals.lock();
            let live = terminals
                .get_mut(id)
                .ok_or_else(|| NotFoundError { pty_id: id.clone() }.into_unavailable())?;
            if input.role == Role::Controller
                && (input.takeover || live.controller.as_deref() != Some(input.attachment_id))
            {
                live.generation += 1;
                live.controller = Some(input.attachment_id.to_string());
                // §12 Q3 (the approved machine delta): existing
                // subscribers learn the new controller attachment +
                // generation; the attaching subscriber receives the
                // generation in its `attached` frame instead.
                let event = StreamEvent::ControllerChanged {
                    attachment_id: Some(input.attachment_id.to_string()),
                    generation: live.generation,
                };
                for subscriber in live.subscribers.values_mut() {
                    if subscriber.active {
                        (subscriber.on_event)(event.clone());
                    } else {
                        subscriber.pending.push(event.clone());
                    }
                }
            }
            let generation = live.generation;
            live.subscribers.insert(
                token,
                Subscriber {
                    on_event: input.on_event,
                    on_end: input.on_end,
                    active: false,
                    detached: false,
                    pending: Vec::new(),
                    ended: false,
                },
            );
            // Replay from the requested cursor within the retained window.
            let requested = input.cursor;
            let available = live.buffer_cursor;
            let end = available + live.buffer.len();
            let offset = requested.saturating_sub(available);
            let truncated = requested < available;
            let data = live.buffer.get(offset..).map(<[u8]>::to_vec).unwrap_or_default();
            let replay = Replay {
                requested_offset: requested,
                available_offset: available,
                end_offset: end,
                truncated,
            };
            (live.info.clone(), generation, replay, data)
        };
        if input.role == Role::Controller {
            self.inner.current.lock().insert(info.session_id.clone(), id.clone());
        }
        Ok(Attachment {
            info,
            role: input.role,
            generation,
            replay,
            data,
            inner: Arc::clone(&self.inner),
            id: id.clone(),
            token,
        })
    }
}

trait NotFoundToUnavailable {
    fn into_unavailable(self) -> UnavailableError;
}

impl NotFoundToUnavailable for NotFoundError {
    fn into_unavailable(self) -> UnavailableError {
        UnavailableError { message: self.to_string() }
    }
}

impl Inner {
    /// The registration's instance id (or a fresh one when no runtime
    /// directory is claimed — handoff snapshots are then not written).
    fn registration_instance_id(&self) -> String {
        if let Some(scope) = self.runtime.lock().as_ref() {
            if let Some(registration) = read_registration(&scope.directory()) {
                return registration.instance_id;
            }
        }
        new_random_token()
    }

    /// Removes the handoff state — the kill/refresh invalidation
    /// (a stopped machine must not be adoptable).
    fn invalidate_handoff_state(&self) {
        if let Some(scope) = self.runtime.lock().as_ref() {
            let _ = std::fs::remove_file(scope.directory().join(STATE_JSON));
        }
    }

    /// The adoption ladder (`daemon.ts:403-428`); see the module docs.
    fn try_adopt(&self, inherited: &Handoff) -> Result<Adoption, AdoptionError> {
        // 1. Expiry (`daemon.ts:406-407`).
        let now = u64::try_from(self.clock.unix_ms().max(0)).unwrap_or(0);
        if inherited.expires_at <= now {
            return Err(AdoptionError::expired());
        }
        // 2. Discover the registration (`daemon.ts:201-226`).
        let directory = PathBuf::from(&inherited.directory);
        let registration = read_registration(&directory).ok_or_else(AdoptionError::unreachable)?;
        // 3. Protocol (`daemon.ts:209-216`).
        if registration.protocol != PTY_PROTOCOL_VERSION {
            return Err(AdoptionError::protocol());
        }
        // 4. Instance (`daemon.ts:411-412`).
        if registration.instance_id != inherited.instance_id {
            return Err(AdoptionError::changed());
        }
        // 5. The handoff snapshot — the in-process stand-in for the
        //    live daemon's `prepare_handoff` ticket. Absent ⇒ the old
        //    owner died without handing off (v2: daemon died —
        //    kill/refresh); mismatched ticket ⇒ the claim is rejected.
        let state_path = directory.join(STATE_JSON);
        let state: HandoffState = read_json(&state_path).ok_or_else(AdoptionError::no_state)?;
        if state.instance_id != inherited.instance_id || state.ticket != inherited.ticket {
            return Err(AdoptionError::invalid_ticket());
        }
        // 6. Ownership: take the lock. Held ⇒ the previous owner is
        //    still running (v2 claims through the daemon socket; the
        //    in-process machine cannot — Phase 9).
        let lock = take_lock(&directory.join(SERVICE_LOCK)).map_err(|()| AdoptionError::owned())?;

        // Migrate into this machine's root when the handoff points
        // elsewhere (`daemon.ts:413-414`, `435-444`): renames keep the
        // lock's inode, so the lock just taken survives the move.
        let id = directory.file_name().map_or_else(
            || id_from_instance(&registration.instance_id),
            |name| name.to_string_lossy().into_owned(),
        );
        let root = self.runtime.lock().as_ref().map_or_else(
            || directory.parent().map_or_else(|| directory.clone(), Path::to_path_buf),
            |scope| scope.root.clone(),
        );
        let target = root.join(&id);
        if target != directory && migrate_runtime(&directory, &target).is_err() {
            drop(lock);
            return Err(AdoptionError::unreachable());
        }

        // Restore the terminals: replay windows intact, children gone
        // (exited — v2's daemon-died outcome; `Live` without io
        // handles rejects writes and serves replay reads). The loop is
        // the extracted `Machine::restore` (design §2.2/§6 arm B):
        // shared by this in-process adoption arm and the daemon's
        // owner-only `restore` op.
        let restored = self.restore(&state);

        // The claim consumes the snapshot (single-use ticket; a second
        // contender finds no state and starts fresh — the
        // `pty-handoff.ts:32-34` concurrent-prepare tolerance).
        let _ = std::fs::remove_file(target.join(STATE_JSON));
        // Re-point the registration at the new owner (the pid now
        // holding the terminals; the instance id stays — the identity
        // chains across restarts like v2's surviving daemon).
        let claimed = Registration {
            instance_id: registration.instance_id,
            pid: std::process::id(),
            protocol: PTY_PROTOCOL_VERSION,
            socket: String::new(),
            token: registration.token,
        };
        let _ = write_json_atomic(&target.join(SERVICE_JSON), &claimed);
        // The inherited directory is the live identity now; the fresh
        // scope claimed at boot (if any) is released with its lock.
        *self.runtime.lock() = Some(RuntimeScope { root, id, lock });
        Ok(Adoption::Adopted { restored, directory: target.to_string_lossy().into_owned() })
    }

    /// The extracted snapshot-restore loop (design §2.2): replay
    /// windows intact, children gone (every terminal marked `exited`),
    /// id counter continued past the restored ids. Shared by the
    /// in-process adoption ladder and the daemon's `restore` op.
    fn restore(&self, state: &HandoffState) -> usize {
        let mut terminals = self.terminals.lock();
        let mut next_number = self.next_number.lock();
        for snapshot in &state.terminals {
            let mut info = snapshot.info.clone();
            if info.status == crate::pty::Status::Running {
                info.status = crate::pty::Status::Exited;
            }
            let buffer = base64_decode(&snapshot.buffer).unwrap_or_default();
            info.output = (snapshot.buffer_cursor, snapshot.buffer_cursor + buffer.len());
            if let Some(number) = info.id.0.rsplit('_').next().and_then(|n| n.parse::<u64>().ok()) {
                *next_number = (*next_number).max(number);
            }
            // RW-02: the VT grid is reconstructed by replaying the
            // retained buffer through a fresh parser (the buffer IS
            // the screen's history) — the stored `rows`/`cursor` stay
            // the `state.json` snapshot shape for wire compatibility.
            let screen = Live::new_screen(info.size, &buffer);
            terminals.insert(
                info.id.clone(),
                Live {
                    info,
                    buffer,
                    buffer_cursor: snapshot.buffer_cursor,
                    writer: None,
                    master: None,
                    killer: None,
                    screen,
                    generation: snapshot.generation,
                    controller: None,
                    subscribers: HashMap::new(),
                    ended: HashMap::new(),
                },
            );
        }
        state.terminals.len()
    }

    fn on_output(&self, id: &PtyId, chunk: &[u8]) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        live.info.output.1 += chunk.len();
        // RW-02: real terminal emulation — the vt100 parser handles
        // CSI/OSC escape sequences, cursor movement, erase ops, the
        // alternate screen and line wrapping (replaces the naive
        // \n/\r/0x08 byte model; the daemon host shares this code).
        live.screen.process(chunk);
        live.buffer.extend_from_slice(chunk);
        if live.buffer.len() > OUTPUT_LIMIT {
            let excess = live.buffer.len() - OUTPUT_LIMIT;
            live.buffer.drain(..excess);
            live.buffer_cursor += excess;
            // Trimming by a byte count can split a multi-byte UTF-8
            // codepoint (and any escape sequence in flight); back up to
            // a char boundary so replay never emits invalid UTF-8
            // (ref trims on its string buffer — pty.ts:219-221).
            let mut extra = 0;
            while extra < live.buffer.len() && live.buffer[extra] & 0b1100_0000 == 0b1000_0000 {
                extra += 1;
            }
            if extra > 0 {
                if extra == live.buffer.len() {
                    // Degenerate: only a partial codepoint survives. Drop
                    // it entirely rather than replay invalid bytes.
                    live.buffer_cursor += extra;
                    live.buffer.clear();
                } else {
                    live.buffer.drain(..extra);
                    live.buffer_cursor += extra;
                }
            }
        }
        let event = StreamEvent::Output(chunk.to_vec());
        for subscriber in live.subscribers.values_mut() {
            if subscriber.active {
                (subscriber.on_event)(event.clone());
            } else {
                subscriber.pending.push(event.clone());
            }
        }
    }

    fn resize(&self, id: &PtyId, cols: u16, rows: u16, attachment: Option<&str>) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        if let Some(master) = live.master.as_mut() {
            let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
        live.info.size = (cols, rows);
        live.screen.set_size(rows.max(1), cols.max(1));
        if let Some(attachment) = attachment {
            if live.controller.as_deref() == Some(attachment) {
                live.generation += 1;
            }
        }
        // The resized event carries the post-resize checkpoint (the
        // versioned §7 format — same bytes `snapshot` records) and the
        // generation (v2 daemon.ts:81-87,
        // handlers/persistent-pty.ts:141-145).
        let cursor = live.screen_cursor();
        let checkpoint = wire::encode_checkpoint(&wire::CheckpointV1 {
            cols,
            rows,
            cursor_x: u32::try_from(cursor.0).unwrap_or(u32::MAX),
            cursor_y: u32::try_from(cursor.1).unwrap_or(u32::MAX),
            generation: live.generation,
            scrollback: live.screen_rows().join("\n"),
        });
        let event = StreamEvent::Resized { cols, rows, generation: live.generation, checkpoint };
        for subscriber in live.subscribers.values_mut() {
            if subscriber.active {
                (subscriber.on_event)(event.clone());
            } else {
                subscriber.pending.push(event.clone());
            }
        }
    }

    fn write(&self, id: &PtyId, data: &[u8], _attachment: Option<&str>) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        if live.info.status != crate::pty::Status::Running {
            return;
        }
        if let Some(writer) = live.writer.as_mut() {
            use std::io::Write;
            let _ = writer.write_all(data);
            let _ = writer.flush();
        }
    }

    fn mark_exited(&self, id: &PtyId, exit_code: Option<i32>) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        if live.info.status == crate::pty::Status::Exited {
            return;
        }
        live.info.status = crate::pty::Status::Exited;
        live.info.exit_code = exit_code;
        live.writer = None;
        live.master = None;
        if let Some(mut killer) = live.killer.take() {
            let _ = killer.kill();
        }
        let event = StreamEvent::Exited { exit_code };
        let mut ended = std::mem::take(&mut live.ended);
        for (token, mut subscriber) in live.subscribers.drain() {
            if subscriber.active {
                (subscriber.on_event)(event.clone());
                let on_end = std::mem::replace(&mut subscriber.on_end, Box::new(|| {}));
                on_end();
            } else {
                subscriber.pending.push(event.clone());
                subscriber.ended = true;
                ended.insert(token, subscriber);
            }
        }
        live.ended = ended;
    }

    fn terminate(&self, id: &PtyId) {
        let mut terminals = self.terminals.lock();
        if let Some(mut live) = terminals.remove(id) {
            if let Some(mut killer) = live.killer.take() {
                let _ = killer.kill();
            }
            live.writer = None;
            live.master = None;
            for (_, mut subscriber) in live.subscribers.drain() {
                if subscriber.active {
                    let on_end = std::mem::replace(&mut subscriber.on_end, Box::new(|| {}));
                    on_end();
                }
            }
            for (_, mut subscriber) in live.ended.drain() {
                if !subscriber.active {
                    subscriber.ended = true;
                    subscriber.deliver_pending();
                }
            }
        }
    }

    fn activate(&self, id: &PtyId, token: u64) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        if let Some(subscriber) = live.subscribers.get_mut(&token) {
            if !subscriber.active && !subscriber.detached {
                subscriber.deliver_pending();
            }
            return;
        }
        if let Some(subscriber) = live.ended.get_mut(&token) {
            if !subscriber.active && !subscriber.detached {
                subscriber.deliver_pending();
            }
        }
    }

    fn detach(&self, id: &PtyId, token: u64) {
        let mut terminals = self.terminals.lock();
        if let Some(live) = terminals.get_mut(id) {
            if let Some(mut subscriber) = live.subscribers.remove(&token) {
                subscriber.detached = true;
                subscriber.pending.clear();
            }
            live.ended.remove(&token);
        }
    }
}

impl Subscriber {
    fn deliver_pending(&mut self) {
        self.active = true;
        let pending = std::mem::take(&mut self.pending);
        for event in pending {
            (self.on_event)(event);
        }
        if self.ended {
            let on_end = std::mem::replace(&mut self.on_end, Box::new(|| ()));
            on_end();
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime-directory helpers (the `daemon.ts` filesystem contract)
// ---------------------------------------------------------------------------

/// Claims a fresh `<root>/<id>`: exclusive non-blocking `flock` on
/// `service.lock` (released automatically when the process dies) plus
/// the `service.json` registration (`daemon.ts:165-166`, `33-39`).
/// `id` is a fresh random token for the in-process claim and the
/// daemon-provided name for [`PersistentPty::bind_runtime_dir`].
fn claim_runtime_dir(root: &Path, id: Option<&str>) -> Result<RuntimeScope, String> {
    let id = id.map_or_else(new_random_token, std::borrow::ToOwned::to_owned);
    let directory = root.join(&id);
    std::fs::create_dir_all(&directory)
        .and_then(|()| set_private(&directory))
        .map_err(|error| format!("creating PTY runtime directory: {error}"))?;
    let lock = take_lock(&directory.join(SERVICE_LOCK))
        .map_err(|()| format!("locking PTY runtime directory {}", directory.display()))?;
    let registration = Registration {
        instance_id: new_random_token(),
        pid: std::process::id(),
        protocol: PTY_PROTOCOL_VERSION,
        socket: String::new(),
        token: new_random_token(),
    };
    write_json_atomic(&directory.join(SERVICE_JSON), &registration)
        .map_err(|error| format!("writing PTY registration: {error}"))?;
    Ok(RuntimeScope { root: root.to_path_buf(), id, lock })
}

/// Opens `path` and takes `flock(LOCK_EX|LOCK_NB)`. The fd is returned
/// (held = owner alive; dropped/released = dead) — the in-process
/// liveness equivalent of v2's socket ping (`daemon.ts:217-226`).
fn take_lock(path: &Path) -> Result<std::fs::File, ()> {
    use std::os::unix::io::AsRawFd as _;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(path)
        .map_err(|_| ())?;
    // Contained flock(2) on the lock file this machine just opened.
    #[allow(unsafe_code)] // contained flock(2) on the freshly-opened lock file
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(());
    }
    Ok(file)
}

/// Moves `service.lock` + `service.json` from `from` into `to`
/// (`daemon.ts:435-444`): rename preserves the lock's inode, so a
/// held lock survives the move.
fn migrate_runtime(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    std::fs::rename(from.join(SERVICE_LOCK), to.join(SERVICE_LOCK))?;
    std::fs::rename(from.join(SERVICE_JSON), to.join(SERVICE_JSON))?;
    Ok(())
}

/// `0o700` on a directory, verifying it is one (`binary.bun.ts`
/// `privateDirectory` semantics).
fn set_private(directory: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let meta = std::fs::metadata(directory)?;
    if !meta.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a directory"));
    }
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
}

/// Reads `<dir>/service.json`.
fn read_registration(directory: &Path) -> Option<Registration> {
    read_json(&directory.join(SERVICE_JSON))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok())
}

/// Atomic JSON write: temp file (`0o600`) + rename, the
/// `pty-handoff.ts:54-66` publish discipline.
fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let payload = serde_json::to_string(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let temp = path.with_extension(format!("{}.tmp", new_random_token()));
    let mut file =
        std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&temp)?;
    file.write_all(payload.as_bytes())?;
    file.sync_all()?;
    drop(file);
    match std::fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Random 32-hex token (`crypto.randomUUID` stand-in).
fn new_random_token() -> String {
    use rand::RngCore as _;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(32), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn now_plus(clock: &dyn Clock, ttl_ms: u64) -> u64 {
    u64::try_from(clock.unix_ms().max(0)).unwrap_or(u64::MAX).saturating_add(ttl_ms)
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(data: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// A directory-name fallback when the handoff directory has no file
/// name (degenerate input).
fn id_from_instance(instance_id: &str) -> String {
    instance_id.chars().filter(char::is_ascii_alphanumeric).take(32).collect()
}

/// Read-only projection the `read` path needs without holding the lock.
struct LiveShadow {
    info: Info,
    rows: Vec<String>,
    cursor: (usize, usize),
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions

    use super::*;

    #[tokio::test]
    async fn persistent_pty_read_requires_lines_in_range() {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let service = PersistentPty::new(bus);
        let session = SessionId::parse("ses_x").expect("session");
        let error = service.read(&session, Some(0)).await.expect_err("lines=0 rejected");
        assert_eq!(error.message, "lines must be an integer between 1 and 65535");
        let error = service.read(&session, Some(65536)).await.expect_err("lines overflow rejected");
        assert_eq!(error.message, "lines must be an integer between 1 and 65535");
    }

    #[tokio::test]
    async fn persistent_pty_smoke_create_write_read_snapshot() {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let service =
            PersistentPty::new(bus).with_default_program(Arc::new(|| Some("/bin/cat".to_string())));
        let session = SessionId::parse("ses_1").expect("session");
        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "printf 'line1'; sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: "workbench".to_string(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 10)),
                },
            )
            .await
            .expect("create");
        assert!(info.id.0.starts_with("pty_persistent_"));
        assert_eq!(info.session_id, session);
        assert_eq!(info.title, "workbench");

        // No controller attached: read returns None.
        let none = service.read(&session, None).await.expect("read").is_none();
        assert!(none, "read before a controller attach is None");

        // Controller attach selects the terminal.
        let attachment = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "attach-1",
                    role: Role::Controller,
                    takeover: false,
                    on_event: Box::new(|_| {}),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach");
        attachment.activate();
        let read = service.read(&session, None).await.expect("read").expect("selected");
        assert_eq!(read.pty_id, info.id);
        assert_eq!(read.title, "workbench");
        assert_eq!(read.screen.cols, 80);

        // write + snapshot round trip.
        service.write(&info.id, "hello", None).await.expect("write");
        service.resize(&info.id, 100, 20, None).await.expect("resize");
        let snapshot = service.snapshot(&info.id).expect("snapshot");
        assert_eq!(snapshot.info.size, (100, 20));
        assert!(
            !snapshot.checkpoint.is_empty(),
            "checkpoint carries the §7 v1 scrollback encoding"
        );

        // shutdown clears everything.
        service.shutdown().await.expect("shutdown");
        let listed = service.list(None).expect("list");
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn persistent_pty_id_scheme() {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let service = PersistentPty::new(bus);
        let first = service.next_terminal_id();
        let second = service.next_terminal_id();
        assert!(first.0.starts_with("pty_persistent_"));
        assert_eq!(first.0, "pty_persistent_1");
        assert_eq!(second.0, "pty_persistent_2");
    }

    // -- R-09: cross-restart handover (daemon.ts:403-428 ladder) ---------------

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oc-pty-handover-{tag}-"))
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        dir.keep()
    }

    async fn bus_for_test() -> crate::bus::Bus {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic())
    }

    /// `handoff()` without a runtime directory stays `None` (the
    /// embedded/test surface — v2's transport with no daemon spawned).
    #[tokio::test]
    async fn handoff_without_runtime_dir_is_none() {
        let service = PersistentPty::new(bus_for_test().await);
        assert!(service.handoff().expect("handoff").is_none());
    }

    /// The flagship restart chain: server A creates a terminal,
    /// produces output, hands off, "dies" (dropped); server B re-attaches
    /// to the inherited directory and replays the pre-restart output
    /// window at attach time.
    #[tokio::test]
    async fn restarted_server_reattaches_to_terminal_state() {
        let root = temp_root("restore");
        let session = SessionId::parse("ses_restart").expect("session");

        // Server A: live terminal with real PTY output.
        let a = PersistentPty::new(bus_for_test().await)
            .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        a.set_runtime_dir(&root).expect("runtime dir");
        let info = a
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "printf 'hello '; sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: "workbench".to_string(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 10)),
                },
            )
            .await
            .expect("create");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while a.get(&info.id).expect("get").output.1 < 6 {
            assert!(std::time::Instant::now() < deadline, "terminal produced no output");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        // Clean stop: prepare the handoff ticket + snapshot, then die
        // (the lock release IS the death signal — same kernel
        // semantics as process exit closing the fd).
        let handoff = a.handoff().expect("handoff").expect("ticket");
        let a_directory = a.runtime_dir().expect("a runtime dir");
        a.release_runtime();
        drop(a);

        // Server B boots on the same root, adopts the inherited ticket.
        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("b runtime dir");
        match b.adopt(&handoff) {
            Adoption::Adopted { restored, directory } => {
                assert_eq!(restored, 1);
                assert_eq!(directory, a_directory, "re-attached to the inherited directory");
            }
            Adoption::Discarded { reason } => panic!("adoption discarded: {reason}"),
        }

        // The terminal survived the restart as state: list/get see it,
        // marked exited (the child died with server A — v2's
        // daemon-died outcome), and attach replays the pre-restart
        // bytes.
        let restored_info = b.get(&info.id).expect("restored terminal");
        assert_eq!(restored_info.title, "workbench");
        assert_eq!(restored_info.status, crate::pty::Status::Exited);
        let attachment = b
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "adopted-1",
                    role: Role::Controller,
                    takeover: false,
                    on_event: Box::new(|_| {}),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach after restart");
        assert_eq!(attachment.replay.requested_offset, 0);
        assert!(!attachment.replay.truncated);
        assert_eq!(attachment.data, b"hello ", "the pre-restart replay window survived");

        // Writes to a dead terminal are rejected by status (no io
        // handles restored) — read-only history until removed.
        b.write(&info.id, "ignored", None).await.expect("write on exited is a no-op, not an error");
        // New terminals continue the id sequence past the restored one.
        let next = b.next_terminal_id();
        assert_eq!(next.0, "pty_persistent_2", "id counter continues past the restore");
    }

    /// `daemon.ts:406-407` — an expired ticket discards to a fresh start.
    #[tokio::test]
    async fn adopt_expired_ticket_discards() {
        let root = temp_root("expired");
        let service = PersistentPty::new(bus_for_test().await);
        service.set_runtime_dir(&root).expect("runtime");
        let stale = Handoff {
            directory: root.join("nowhere").to_string_lossy().into_owned(),
            instance_id: "any".to_owned(),
            ticket: "any".to_owned(),
            expires_at: 0,
        };
        match service.adopt(&stale) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::expired()),
            Adoption::Adopted { .. } => panic!("expired ticket adopted"),
        }
        assert!(service.list(None).expect("list").is_empty(), "fresh start");
    }

    /// `daemon.ts:411-412` — an instance mismatch discards.
    #[tokio::test]
    async fn adopt_instance_mismatch_discards() {
        let root = temp_root("changed");
        let a = PersistentPty::new(bus_for_test().await);
        a.set_runtime_dir(&root).expect("runtime");
        let mut handoff = a.handoff().expect("handoff").expect("ticket");
        handoff.instance_id = format!("{}-tampered", handoff.instance_id);
        drop(a);
        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("runtime");
        match b.adopt(&handoff) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::changed()),
            Adoption::Adopted { .. } => panic!("mismatched instance adopted"),
        }
    }

    /// `daemon.ts:415` — a still-running previous owner keeps its lock;
    /// the claim fails (the in-process machine has no daemon socket to
    /// transfer ownership through — the documented Phase-9 boundary).
    #[tokio::test]
    async fn adopt_while_owner_alive_discards() {
        let root = temp_root("owned");
        let a = PersistentPty::new(bus_for_test().await);
        a.set_runtime_dir(&root).expect("runtime");
        let handoff = a.handoff().expect("handoff").expect("ticket");
        // A is still alive here (holds the lock on service.lock).
        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("runtime");
        match b.adopt(&handoff) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::owned()),
            Adoption::Adopted { .. } => panic!("adopted out from under a live owner"),
        }
    }

    /// The daemon-died path (kill/refresh): a registration exists but
    /// no handoff snapshot was written (crash, or the explicit
    /// shutdown) — the replacement starts fresh
    /// (`pty-handoff.ts:35-37`).
    #[tokio::test]
    async fn adopt_after_unhanded_death_discards_fresh() {
        let root = temp_root("died");
        let a = PersistentPty::new(bus_for_test().await);
        a.set_runtime_dir(&root).expect("runtime");
        let directory = a.runtime_dir().expect("dir");
        let instance_id = {
            let text = std::fs::read_to_string(Path::new(&directory).join("service.json"))
                .expect("registration");
            let registration: Registration = serde_json::from_str(&text).expect("parse");
            registration.instance_id
        };
        drop(a); // died without handoff()

        let bogus = Handoff {
            directory: directory.clone(),
            instance_id,
            ticket: "fabricated".to_owned(),
            expires_at: u64::try_from(
                oc_util::determinism::SystemClock.unix_ms().saturating_add(60_000),
            )
            .unwrap_or(u64::MAX),
        };
        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("runtime");
        match b.adopt(&bogus) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::no_state()),
            Adoption::Adopted { .. } => panic!("adopted a dead owner without a handoff"),
        }
        assert!(b.list(None).expect("list").is_empty());
    }

    /// `shutdown()` is the kill path: any pending handoff state is
    /// invalidated (`pty-handoff.ts:35-37`).
    #[tokio::test]
    async fn shutdown_invalidates_the_handoff_state() {
        let root = temp_root("killed");
        let a = PersistentPty::new(bus_for_test().await);
        a.set_runtime_dir(&root).expect("runtime");
        let handoff = a.handoff().expect("handoff").expect("ticket");
        a.shutdown().await.expect("shutdown");
        drop(a);
        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("runtime");
        match b.adopt(&handoff) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::no_state()),
            Adoption::Adopted { .. } => panic!("adopted a shutdown (killed) owner"),
        }
    }

    /// The ticket is single-use: after one adoption consumes the
    /// snapshot, a second contender finds no state and starts fresh
    /// (`pty-handoff.ts:32-34`).
    #[tokio::test]
    async fn second_contender_after_adoption_starts_fresh() {
        let root = temp_root("contender");
        let session = SessionId::parse("ses_contender").expect("session");
        let a = PersistentPty::new(bus_for_test().await)
            .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        a.set_runtime_dir(&root).expect("runtime");
        a.create(
            &session,
            CreateInput {
                command: Some("/bin/sh".to_string()),
                args: vec!["-c".to_string(), "sleep 30".to_string()],
                cwd: Some("/tmp".to_string()),
                title: String::new(),
                env: std::collections::BTreeMap::new(),
                size: Some((80, 10)),
            },
        )
        .await
        .expect("create");
        let handoff = a.handoff().expect("handoff").expect("ticket");
        a.release_runtime();
        drop(a);

        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&root).expect("runtime");
        assert!(matches!(b.adopt(&handoff), Adoption::Adopted { .. }));

        let c = PersistentPty::new(bus_for_test().await);
        c.set_runtime_dir(&root).expect("runtime");
        match c.adopt(&handoff) {
            Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::no_state()),
            Adoption::Adopted { .. } => panic!("second contender adopted the consumed ticket"),
        }
        assert!(c.list(None).expect("list").is_empty());
    }

    /// `daemon.ts:413-414`, `435-444` — when the inherited directory
    /// sits outside this machine's root, the lock + registration move
    /// (inode-preserving renames) into the root.
    #[tokio::test]
    async fn adopt_migrates_the_runtime_directory_into_the_root() {
        let old_root = temp_root("migrate-old");
        let new_root = temp_root("migrate-new");
        let a = PersistentPty::new(bus_for_test().await);
        a.set_runtime_dir(&old_root).expect("runtime");
        let handoff = a.handoff().expect("handoff").expect("ticket");
        let inherited = handoff.directory.clone();
        drop(a);

        let b = PersistentPty::new(bus_for_test().await);
        b.set_runtime_dir(&new_root).expect("runtime");
        let adopted_directory = match b.adopt(&handoff) {
            Adoption::Adopted { directory, .. } => directory,
            Adoption::Discarded { reason } => panic!("migration discarded: {reason}"),
        };
        assert!(adopted_directory.starts_with(new_root.to_str().expect("utf8")));
        assert!(Path::new(&adopted_directory).join("service.json").exists());
        assert!(!Path::new(&inherited).join("service.json").exists(), "old registration moved");
        // The moved lock is the one B now holds: a competing claim on
        // the MIGRATED path fails — the lock rode its inode through
        // the rename (the daemon.ts:433-444 comment, verbatim
        // mechanism).
        assert!(
            take_lock(&Path::new(&adopted_directory).join(SERVICE_LOCK)).is_err(),
            "the migrated lock file still carries B's lock"
        );
    }

    // -- D1: host-ability surface (design §3.1, §6, §7) -----------------------

    /// The protocol version mirrors v2's daemon protocol (design §4.2).
    #[test]
    fn protocol_version_is_seven() {
        assert_eq!(PTY_PROTOCOL_VERSION, 7);
    }

    /// Host-injected publications (§3.1): the sink receives the same
    /// event names and payloads the bus route would carry, including
    /// the exit-pump `removed` with ZERO subscribers.
    #[tokio::test]
    async fn hosted_event_sink_receives_bus_publications() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let service = PersistentPty::hosted(Arc::new(move |type_, data| {
            sink_events.lock().push((type_.to_owned(), data));
        }))
        .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        let session = SessionId::parse("ses_hosted").expect("session");

        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "printf ok; sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: "hosted".to_string(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 10)),
                },
            )
            .await
            .expect("create");

        // `added` fires on create; `removed` on explicit remove.
        service.remove(&info.id).await.expect("remove");
        let added = events.lock().iter().any(|(type_, _)| type_ == "persistent-pty.added");
        assert!(added, "sink saw persistent-pty.added");
        let removed = events
            .lock()
            .iter()
            .any(|(type_, data)| type_ == "persistent-pty.removed" && data["ptyID"] == info.id.0);
        assert!(removed, "sink saw persistent-pty.removed for the removed terminal");
    }

    /// The exit-pump `removed` fires with ZERO subscribers (§3.1 —
    /// every child is reaped by the machine's wait threads).
    #[tokio::test]
    async fn hosted_event_sink_fires_exit_removed_without_subscribers() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let service = PersistentPty::hosted(Arc::new(move |type_, data| {
            sink_events.lock().push((type_.to_owned(), data));
        }))
        .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        let session = SessionId::parse("ses_exitpump").expect("session");
        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "true".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: String::new(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 5)),
                },
            )
            .await
            .expect("create");
        // No attach: zero subscribers. The wait thread reaps the child;
        // the exit pump publishes `removed`.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let fired = loop {
            let hit = events.lock().iter().any(|(type_, data)| {
                type_ == "persistent-pty.removed" && data["ptyID"] == info.id.0
            });
            if hit {
                break true;
            }
            assert!(std::time::Instant::now() < deadline, "exit-pump removed never fired");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        assert!(fired);
    }

    /// `reset_selection_epoch` (§6 arm A): the selection map empties
    /// and controllers drop so resize generation bumps work again for
    /// a fresh owner attachment (generation counters stay monotonic).
    #[tokio::test]
    async fn reset_selection_epoch_clears_current_and_controllers() {
        let service = PersistentPty::new(bus_for_test().await)
            .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        let session = SessionId::parse("ses_epoch").expect("session");
        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: String::new(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 5)),
                },
            )
            .await
            .expect("create");
        let first = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "old-server-attach",
                    role: Role::Controller,
                    takeover: false,
                    on_event: Box::new(|_| {}),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach");
        assert_eq!(first.generation, 1, "first controller bumped the generation");

        service.reset_selection_epoch();
        // Selection cleared: read finds nothing.
        assert!(service.read(&session, None).await.expect("read").is_none());
        // Controller cleared: a DIFFERENT attachment becomes controller
        // without takeover and the generation keeps counting upward.
        let replacement = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "new-server-attach",
                    role: Role::Controller,
                    takeover: false,
                    on_event: Box::new(|_| {}),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach after epoch reset");
        assert_eq!(replacement.generation, 2, "generations stay monotonic across the reset");
    }

    /// `reanchor_runtime_dir` (§6 arm A, gate finding 6): after the
    /// client migrates lock+registration, the machine's writes follow
    /// the new directory.
    #[tokio::test]
    async fn reanchor_runtime_dir_moves_write_paths() {
        let old_root = temp_root("reanchor-old");
        let new_root = temp_root("reanchor-new");
        let service = PersistentPty::new(bus_for_test().await);
        service.set_runtime_dir(&old_root).expect("runtime");
        let directory = service.runtime_dir().expect("dir");
        let id = Path::new(&directory).file_name().expect("id").to_string_lossy().into_owned();
        let target = new_root.join(&id);

        // The client-side migration (lock + registration, inode-preserving).
        migrate_runtime(Path::new(&directory), &target).expect("migrate");
        assert!(service.reanchor_runtime_dir(&target), "reanchor reports the change");
        assert_eq!(service.runtime_dir().expect("dir"), target.to_string_lossy());
        let handoff = service.handoff().expect("handoff").expect("ticket");
        assert_eq!(handoff.directory, target.to_string_lossy());
        assert!(target.join("state.json").exists(), "handoff wrote into the re-anchored dir");
        assert!(!Path::new(&directory).join("state.json").exists());
        // Re-anchoring to the same directory is a no-op.
        assert!(!service.reanchor_runtime_dir(&target));
    }

    /// `bind_runtime_dir` claims the NAMED directory (the daemon's
    /// `--name`), writes a protocol-7 registration, and refuses a
    /// directory whose lock a live predecessor holds (§5.1 exit≠0).
    #[tokio::test]
    async fn bind_runtime_dir_claims_named_directory() {
        let root = temp_root("bind-named");
        let daemon = PersistentPty::hosted(Arc::new(|_, _| {}));
        daemon.bind_runtime_dir(&root, "daemon-name-1").expect("bind");
        assert_eq!(
            daemon.runtime_dir().expect("dir"),
            root.join("daemon-name-1").to_string_lossy().into_owned()
        );
        let registration = daemon.registration().expect("registration");
        assert_eq!(registration.protocol, 7);
        assert_eq!(registration.socket, "");
        daemon.set_registration_socket("/some/daemon.sock").expect("socket");
        assert_eq!(daemon.registration().expect("registration").socket, "/some/daemon.sock");

        // A second claim on the SAME directory fails while the lock is held.
        let contender = PersistentPty::hosted(Arc::new(|_, _| {}));
        let error = contender.bind_runtime_dir(&root, "daemon-name-1").expect_err("lock busy");
        assert!(error.contains("locking"), "unexpected error: {error}");
    }

    /// `bind_runtime_dir` rejects names that do not denote a CHILD of
    /// the root (D1 review): empty, `/`-containing, and `.`/`..` — a
    /// `--name ..` daemon would otherwise lock the root's PARENT.
    #[tokio::test]
    async fn bind_runtime_dir_rejects_non_child_names() {
        let root = temp_root("bind-bad");
        let daemon = PersistentPty::hosted(Arc::new(|_, _| {}));
        for bad in ["", "..", ".", "a/b", "/etc"] {
            let error = daemon.bind_runtime_dir(&root, bad).expect_err("rejected");
            assert_eq!(error, format!("invalid PTY runtime directory name: {bad}"));
        }
        // The root itself is untouched by the rejected claims.
        assert!(!root.join("service.lock").exists());
        assert!(!root.parent().expect("parent").join("service.lock").exists());
    }

    /// `ControllerChanged` (§12 Q3): an existing subscriber hears about
    /// a takeover; the attaching subscriber gets it via `attached`.
    #[tokio::test]
    async fn controller_takeover_emits_controller_changed() {
        let service = PersistentPty::new(bus_for_test().await)
            .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        let session = SessionId::parse("ses_takeover").expect("session");
        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: String::new(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 5)),
                },
            )
            .await
            .expect("create");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::clone(&seen);
        let attachment = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "observer-1",
                    role: Role::Observer,
                    takeover: false,
                    on_event: Box::new(move |event| events.lock().push(event)),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach");
        attachment.activate();

        let taker = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "controller-2",
                    role: Role::Controller,
                    takeover: true,
                    on_event: Box::new(|_| {}),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("takeover attach");
        assert_eq!(taker.generation, 1);
        let events = seen.lock();
        assert!(
            events.iter().any(|event| matches!(
                event,
                StreamEvent::ControllerChanged { attachment_id, generation }
                    if attachment_id.as_deref() == Some("controller-2") && *generation == 1
            )),
            "observer saw controller_changed; got {events:?}"
        );
    }

    /// The snapshot/resized checkpoints are the §7 v1 format (decodable
    /// by the shared wire codec, in-process and daemon backends
    /// byte-identical).
    #[tokio::test]
    async fn snapshot_and_resize_carry_v1_checkpoints() {
        let service = PersistentPty::new(bus_for_test().await)
            .with_default_program(Arc::new(|| Some("/bin/sh".to_string())));
        let session = SessionId::parse("ses_checkpoint").expect("session");
        let info = service
            .create(
                &session,
                CreateInput {
                    command: Some("/bin/sh".to_string()),
                    args: vec!["-c".to_string(), "printf 'ab'; sleep 30".to_string()],
                    cwd: Some("/tmp".to_string()),
                    title: String::new(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((20, 4)),
                },
            )
            .await
            .expect("create");
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let attachment = service
            .attach(
                &info.id,
                AttachInput {
                    cursor: 0,
                    attachment_id: "checkpoint-observer",
                    role: Role::Observer,
                    takeover: false,
                    on_event: Box::new(move |event| sink.lock().push(event)),
                    on_end: Box::new(|| {}),
                },
            )
            .expect("attach");
        attachment.activate();
        service.resize(&info.id, 30, 6, None).await.expect("resize");

        let snapshot = service.snapshot(&info.id).expect("snapshot");
        let decoded = wire::decode_checkpoint(&snapshot.checkpoint).expect("v1 checkpoint");
        assert_eq!((decoded.cols, decoded.rows), (30, 6));
        assert_eq!(decoded.scrollback.split('\n').count(), 6);

        let stream_checkpoint = events
            .lock()
            .iter()
            .find_map(|event| match event {
                StreamEvent::Resized { checkpoint, .. } => Some(checkpoint.clone()),
                _ => None,
            })
            .expect("resized event");
        assert_eq!(wire::decode_checkpoint(&stream_checkpoint).expect("decode"), decoded);
    }

    // -- RW-02: vt100 screen tracking (deterministic ANSI streams) ----------

    /// Feeds a raw byte stream through the machine's VT grid at `size`
    /// and returns the derived `(rows, cursor)` — the exact derivation
    /// `read`/`snapshot`/resize checkpoints use.
    fn track(size: (u16, u16), stream: &[u8]) -> (Vec<String>, (usize, usize)) {
        let parser = Live::new_screen(size, stream);
        (vt_rows(&parser), vt_cursor(&parser))
    }

    /// Plain CR/LF text keeps the naive model's observable shape.
    #[test]
    fn vt_screen_plain_text() {
        let (rows, cursor) = track((20, 5), b"hello\r\nworld");
        assert_eq!(rows[0], "hello");
        assert_eq!(rows[1], "world");
        assert_eq!(cursor, (5, 1));
    }

    /// CSI SGR color attributes never leak into the text; only the
    /// payload characters render (colored prompts stay readable).
    #[test]
    fn vt_screen_csi_colors_are_not_text() {
        let (rows, _cursor) = track((40, 5), b"\x1b[32mgreen\x1b[0m \x1b[1;34mbold-blue\x1b[m!");
        assert_eq!(rows[0], "green bold-blue!");
    }

    /// CSI cursor movement (CUP/CUF) places subsequent text at the
    /// moved position instead of appending.
    #[test]
    fn vt_screen_cursor_movement() {
        // Home to (row 2, col 6) [1-based] and overwrite.
        let (rows, cursor) = track((20, 5), b"abcdef\r\nghijkl\x1b[2;6HX");
        assert_eq!(rows[1], "ghijkX");
        assert_eq!(cursor, (6, 1));
        // Cursor-forward then text leaves the skipped cells blank.
        let (rows, _cursor) = track((20, 5), b"a\x1b[3Cb");
        assert_eq!(rows[0], "a   b");
    }

    /// OSC sequences (window title etc.) are consumed, not printed.
    #[test]
    fn vt_screen_osc_title_is_consumed() {
        let (rows, cursor) = track((40, 5), b"\x1b]0;my title\x07visible");
        assert_eq!(rows[0], "visible");
        assert_eq!(cursor, (7, 0));
    }

    /// Erase-in-display (ED) clears the requested region.
    #[test]
    fn vt_screen_erase_in_display() {
        let (rows, _cursor) = track((20, 5), b"keep\r\naaa\r\nbbb\r\nccc\x1b[3;1H\x1b[2J");
        assert_eq!(rows[0], "");
        assert_eq!(rows[1], "");
        assert_eq!(rows[2], "");
        // Erase from cursor to end of line (EL variant of the family).
        let (rows, _cursor) = track((20, 5), b"abcdef\x1b[1;4H\x1b[K");
        assert_eq!(rows[0], "abc");
    }

    /// The alternate screen: entering it snapshots the primary screen,
    /// leaving it restores the primary contents (less/vim lifecycle).
    #[test]
    fn vt_screen_alt_screen_enter_exit() {
        let stream = b"prompt$ \x1b[?1049hless-full-screen\x1b[?1049l";
        let (rows, _cursor) = track((40, 5), stream);
        assert_eq!(rows[0], "prompt$");
        // While INSIDE the alt screen the primary content is hidden.
        let parser = Live::new_screen((40, 5), b"prompt$ \x1b[?1049hless");
        assert_eq!(vt_rows(&parser)[0], "less");
    }

    /// Resize applies the new grid size (RW-02): the same path
    /// `resize` -> `screen.set_size` takes. vt100 keeps physical rows
    /// (no reflow) — narrower cols truncate each row.
    #[test]
    fn vt_screen_resize_keeps_content() {
        let mut parser = Live::new_screen((10, 5), b"hello world");
        parser.set_size(10, 4);
        let rows = vt_rows(&parser);
        assert_eq!(rows.len(), 10);
        assert_eq!(rows[0], "hell");
        assert_eq!(rows[1], "d");
    }

    /// Line wrap: text past `cols` wraps onto the next row instead of
    /// being dropped (the naive model clipped at the edge).
    #[test]
    fn vt_screen_wraps_long_lines() {
        let (rows, _cursor) = track((5, 5), b"abcdefghij");
        assert_eq!(rows[0], "abcde");
        assert_eq!(rows[1], "fghij");
    }

    /// Handoff restore path: replaying the retained buffer through a
    /// fresh parser reconstructs the pre-restart screen (RW-02 +
    /// `Inner::restore`).
    #[test]
    fn vt_screen_restore_replays_buffer() {
        let buffer = b"\x1b[33myellow\x1b[m\r\nnext";
        let parser = Live::new_screen((20, 5), buffer);
        let rows = vt_rows(&parser);
        assert_eq!(rows[0], "yellow");
        assert_eq!(rows[1], "next");
    }

    /// G-12: the typed [`oc_schema::persistent_pty::Info`] serialization
    /// must match the pre-typed `terminal_wire_json` output byte-for-byte
    /// (same keys, casing, nullability) — no wire drift.
    #[test]
    fn persistent_pty_terminal_wire_no_drift() {
        let session = SessionId::parse("ses_1").expect("session");
        for exit_code in [None, Some(0), Some(127)] {
            let info = Info {
                id: PtyId("pty_persistent_9".to_string()),
                title: "workbench".to_string(),
                command: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 1".to_string()],
                cwd: "/tmp".to_string(),
                status: if exit_code.is_some() {
                    crate::pty::Status::Exited
                } else {
                    crate::pty::Status::Running
                },
                pid: 4242,
                exit_code,
                session_id: session.clone(),
                foreground_process: Some("git status".to_string()),
                size: (80, 24),
                output: (12, 3456),
            };
            assert_eq!(
                serde_json::to_value(terminal_schema_info(&info)).unwrap(),
                terminal_wire_json(&info),
                "typed schema wire must equal terminal_wire_json (exitCode={exit_code:?})"
            );
        }
        // Nullable foregroundProcess round.
        let info = Info {
            id: PtyId("pty_persistent_9".to_string()),
            title: "t".to_string(),
            command: "/bin/cat".to_string(),
            args: vec![],
            cwd: "/tmp".to_string(),
            status: crate::pty::Status::Running,
            pid: 1,
            exit_code: None,
            session_id: session,
            foreground_process: None,
            size: (2, 3),
            output: (0, 0),
        };
        let typed = serde_json::to_value(terminal_schema_info(&info)).unwrap();
        assert_eq!(typed["foregroundProcess"], serde_json::Value::Null);
        assert_eq!(typed, terminal_wire_json(&info));
    }

    /// RW2 fix: the OUTPUT_LIMIT trim must back up to a UTF-8 char
    /// boundary — draining by an arbitrary byte count can split a
    /// multi-byte codepoint and replay invalid UTF-8 (ref trims its
    /// string buffer — pty.ts:219-221).
    #[test]
    fn output_trim_respects_char_boundaries() {
        let service = PersistentPty::hosted(Arc::new(|_, _| {}));
        let id = PtyId("pty_t".into());
        // Pre-fill so the next chunk overflows the limit and the trim
        // point lands in the middle of a trailing two-byte char.
        let mut live = Live {
            info: Info {
                id: id.clone(),
                title: "t".into(),
                command: "sh".into(),
                args: vec![],
                cwd: "/".into(),
                status: crate::pty::Status::Running,
                pid: 0,
                exit_code: None,
                session_id: SessionId::parse("ses_x").expect("session"),
                foreground_process: None,
                size: (80, 24),
                output: (0, 0),
            },
            buffer: vec![b'a'; OUTPUT_LIMIT - 2],
            buffer_cursor: 0,
            writer: None,
            master: None,
            killer: None,
            screen: Live::new_screen((80, 24), &[]),
            generation: 0,
            controller: None,
            subscribers: HashMap::new(),
            ended: HashMap::new(),
        };
        live.buffer.extend_from_slice("\u{e9}x".as_bytes()); // 2-byte char + 'x' -> LIMIT+1 bytes
        service.inner.terminals.lock().insert(id.clone(), live);

        // A LIMIT-2 byte chunk pushes the trim point (excess = LIMIT-1)
        // exactly one byte into the trailing é — the naive drain would
        // leave the buffer starting mid-codepoint.
        service.inner.on_output(&id, &vec![b'b'; OUTPUT_LIMIT - 2]);
        let terminals = service.inner.terminals.lock();
        let live = terminals.get(&id).expect("live");
        let text = String::from_utf8(live.buffer.clone()).expect("buffer stays valid UTF-8");
        assert!(text.starts_with('x'), "buffer head: {:?}", &text[..4]);
        assert!(text.ends_with('b'));
        assert!(live.buffer.len() < OUTPUT_LIMIT);
        assert_eq!(live.buffer_cursor, OUTPUT_LIMIT); // excess + the split codepoint byte
    }
}
