//! PTY lane — port of `pty.ts` (core-runner-map §6.1), the
//! **non-persistent** terminal sessions over [`portable_pty`].
//!
//! Semantics (pty.ts):
//!
//! - sessions live in a map keyed `pty_…` (`Pty.ID`), exited sessions stay
//!   observable (status, exit code, retained output) until removed, with a
//!   25-exited retention cap;
//! - output buffers up to 2 MiB with an absolute cursor for replay
//!   (`attach({cursor})` — `-1` tails from the current end, omitted
//!   replays the full retained buffer, a past cursor clamps to the
//!   retained window);
//! - attachments activate explicitly: chunks arriving before activation
//!   are queued and flushed on `activate()`; an end arriving before
//!   activation is replayed there too; `detach` stops delivery;
//! - events `pty.created` / `pty.updated` / `pty.exited` /
//!   `pty.deleted` publish on the bus (ephemeral).
//!
//! The shell tool does **not** use this (`ChildProcess` + output file);
//! persistent terminals live in [`crate::persistent_pty`].

use std::collections::HashMap;
use std::sync::Arc;

use oc_util::determinism::{AscendingIdGen, IdGen, SystemClock, SystemRng};
use parking_lot::Mutex;

use tokio::sync::mpsc;

use crate::bus::{Bus, PublishOptions};
use crate::environment::pty::{PtyChildKiller, PtyMaster, PtySpawnRequest, PtySpawner};

/// Retained-output ceiling (pty.ts:12).
pub const BUFFER_LIMIT: usize = 1024 * 1024 * 2;
/// Exited-session retention cap (pty.ts:15).
pub const EXITED_LIMIT: usize = 25;

/// `Pty.ID` — ascending `pty_…`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct PtyId(pub String);

impl PtyId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PtyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `Pty.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub id: PtyId,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: Status,
    pub pid: u32,
    pub exit_code: Option<i32>,
}

/// `Pty.Info.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Exited,
}

/// `Pty.CreateInput`.
#[derive(Debug, Clone, Default)]
pub struct CreateInput {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub env: Option<std::collections::BTreeMap<String, String>>,
    pub size: Option<(u16, u16)>,
}

/// `Pty.UpdateInput`.
#[derive(Debug, Clone, Default)]
pub struct UpdateInput {
    pub title: Option<String>,
    pub size: Option<(u16, u16)>,
}

/// `Pty.NotFoundError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Pty.NotFoundError: {pty_id}")]
pub struct NotFoundError {
    pub pty_id: PtyId,
}

/// `Pty.ExitedError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Pty.ExitedError: {pty_id}")]
pub struct ExitedError {
    pub pty_id: PtyId,
}

type OnData = Box<dyn Fn(&str) + Send + Sync>;
type OnEnd = Box<dyn FnOnce(Option<i32>) + Send + Sync>;

fn noop_end() -> OnEnd {
    Box::new(|_| ())
}

/// One live data-chunk subscriber (pty.ts:18-25). `activate` works even
/// after the map was cleared on exit — the attachment keeps its token and
/// the ended subscribers park in [`Session::ended`].
struct Subscriber {
    on_data: OnData,
    on_end: OnEnd,
    active: bool,
    detached: bool,
    pending: Vec<String>,
    /// Outer `Some` = the end arrived; inner = the exit code.
    #[allow(clippy::option_option)]
    end: Option<Option<i32>>,
}

impl Subscriber {
    /// (G-116②) Invokes one data callback; `true` = the callback
    /// panicked (caller removes the subscriber, ref pty.ts:206-212).
    fn call_on_data(&mut self, chunk: &str) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.on_data)(chunk))).is_err()
    }

    /// (G-116②) Invokes the end callback; panics are swallowed + logged
    /// (the subscriber is on its way out either way).
    fn call_on_end(&mut self, exit_code: Option<i32>) {
        let on_end = std::mem::replace(&mut self.on_end, noop_end());
        if let Err(panic) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_end(exit_code)))
        {
            eprintln!("[pty] subscriber end callback panicked ({})", panic_description(&*panic));
        }
    }

    /// (G-116②) Returns whether any callback panicked — the caller
    /// removes the offender (ref activate try/catch, pty.ts:299-301).
    fn deliver_pending(&mut self) -> bool {
        self.active = true;
        let pending = std::mem::take(&mut self.pending);
        let mut panicked = false;
        for chunk in pending {
            panicked |= self.call_on_data(&chunk);
            if panicked {
                break;
            }
        }
        if !panicked {
            if let Some(exit_code) = self.end.take() {
                panicked = self.call_on_end_raw(exit_code);
            }
        }
        panicked
    }

    fn call_on_end_raw(&mut self, exit_code: Option<i32>) -> bool {
        let on_end = std::mem::replace(&mut self.on_end, noop_end());
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_end(exit_code))).is_err()
    }
}

struct Session {
    info: Info,
    /// Retained output window + the absolute cursor it starts at.
    buffer: String,
    buffer_cursor: usize,
    cursor: usize,
    subscribers: HashMap<u64, Subscriber>,
    /// Subscribers parked by an exit while inactive.
    ended: HashMap<u64, Subscriber>,
    writer: Option<Box<dyn std::io::Write + Send>>,
    killer: Option<Box<dyn PtyChildKiller>>,
    resizer: Option<Box<dyn PtyMaster>>,
}

enum ChildEvent {
    Data(String),
    Exit(Option<i32>),
}

/// G-116① — UTF-16 code units of a string (JS `string.length`).
fn utf16_units(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// G-243② — the byte offset of the `units`-th UTF-16 code unit in
/// `text` (the inverse of [`utf16_units`]); `None` when `units` is at
/// or past the end (ref compares against `buffer.length` and replays
/// nothing, pty.ts:284). A `units` that lands inside a surrogate pair
/// clamps to the pair's start so the char replays whole (JS could
/// slice mid-pair; Rust cannot split a `char`).
fn utf16_byte_offset(text: &str, units: usize) -> Option<usize> {
    let mut seen = 0usize;
    for (offset, ch) in text.char_indices() {
        if seen + ch.len_utf16() > units {
            return Some(offset);
        }
        seen += ch.len_utf16();
    }
    (seen >= units).then_some(text.len())
}

/// G-335 — incremental UTF-8 stream decoder with carry. node-pty
/// decodes the PTY byte stream with a stream-buffered TextDecoder; a
/// naive `from_utf8_lossy` per ≤8192B read chunk corrupts any multi-
/// byte sequence split across a read boundary (CJK/emoji mojibake).
/// `feed` emits the longest valid UTF-8 prefix and keeps the invalid
/// (possibly merely incomplete) tail as carry for the next read;
/// `finish` flushes a still-pending tail lossily at EOF — a truncated
/// stream end is genuinely broken input (mirrors the ref's final
/// `fatal: false` decode emitting U+FFFD).
#[derive(Debug, Default)]
pub(crate) struct Utf8StreamDecoder {
    carry: Vec<u8>,
}

impl Utf8StreamDecoder {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feeds one raw chunk; returns everything decodable right now.
    /// A DEFINITIVELY invalid sequence (`error_len` known — e.g. a
    /// stray continuation byte) is replaced with U+FFFD immediately,
    /// like the ref's streaming TextDecoder; a possibly-merely-
    /// truncated tail (no error length) stays in carry until more
    /// bytes or EOF prove it.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> String {
        self.carry.extend_from_slice(chunk);
        let mut out = String::new();
        loop {
            let mut pending = std::mem::take(&mut self.carry);
            let (valid_up_to, error_len) = match std::str::from_utf8(&pending) {
                Ok(_) => (pending.len(), None),
                Err(error) => (error.valid_up_to(), error.error_len()),
            };
            let rest = pending.split_off(valid_up_to);
            // The prefix is valid UTF-8 by construction; the
            // `unwrap_or_default` never engages.
            out.push_str(&String::from_utf8(pending).unwrap_or_default());
            match error_len {
                // Definitively invalid: replace + continue with the rest.
                Some(len) => {
                    out.push('\u{fffd}');
                    let mut rest = rest;
                    self.carry = rest.split_off(len.min(rest.len()));
                }
                // Clean, or truncated-at-the-end: park the tail as carry.
                None => {
                    self.carry = rest;
                    return out;
                }
            }
        }
    }

    /// EOF: any pending tail (truncated sequence) flushes lossily.
    pub(crate) fn finish(&mut self) -> String {
        String::from_utf8_lossy(&self.carry).into_owned()
    }
}

/// G-116③ — the win32 UTF-8 locale overrides (pty.ts:178-180). Pure so
/// the table is testable on every target; the injection itself is
/// `#[cfg(windows)]` at the spawn site.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn win32_utf8_locale_overrides() -> [(&'static str, &'static str); 3] {
    [("LC_ALL", "C.UTF-8"), ("LC_CTYPE", "C.UTF-8"), ("LANG", "C.UTF-8")]
}

/// G-116② — a compact panic payload description for the seam log.
fn panic_description(panic: &(dyn std::any::Any + Send)) -> &str {
    if let Some(text) = panic.downcast_ref::<&str>() {
        text
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text
    } else {
        "unknown panic"
    }
}

struct Inner {
    sessions: Arc<Mutex<HashMap<PtyId, Session>>>,
    exit_order: Arc<Mutex<Vec<PtyId>>>,
    next_token: Mutex<u64>,
    ids: Arc<dyn IdGen>,
    bus: Bus,
}

impl Inner {
    /// Bus publish (ephemeral definition fallback) — on `Inner` so the
    /// per-session pump tasks never hold a `PtyService` clone
    /// (G-383a: a service clone parked in a live session's pump task
    /// would keep the shutdown guard alive forever).
    async fn publish(&self, type_: &str, data: serde_json::Value) {
        let definition = crate::bus::registry::by_type(type_)
            .cloned()
            .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(type_));
        let _ = self.bus.publish(&definition, data, PublishOptions::default()).await;
    }

    /// Exited-session retention cap (drains beyond [`EXITED_LIMIT`]).
    async fn enforce_exited_limit(&self) {
        let evict: Vec<PtyId> = {
            let mut order = self.exit_order.lock();
            let mut evict = Vec::new();
            while order.len() > EXITED_LIMIT {
                let Some(oldest) = order.first().cloned() else { break };
                order.remove(0);
                evict.push(oldest);
            }
            evict
        };
        for id in evict {
            self.teardown(&id);
            self.publish(
                "pty.deleted",
                serde_json::to_value(oc_schema::pty::Deleted { id: id.0.clone() })
                    .unwrap_or_default(),
            )
            .await;
        }
    }

    fn on_data(&self, id: &PtyId, chunk: &str) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(id) else { return };
        // G-116① — the ref counts JS string length = UTF-16 code units
        // (pty.ts:206 `session.cursor += chunk.length`); byte counts
        // drift on CJK/emoji output.
        session.cursor += utf16_units(chunk);
        // G-116② — one panicking subscriber must not unwind the reader
        // thread (the stream would silently die); catch, drop the
        // offender, keep serving the rest (pty.ts:206-212 try/catch +
        // `subscribers.delete(token)`).
        let mut panicked = Vec::new();
        for (token, subscriber) in &mut session.subscribers {
            if !subscriber.active {
                subscriber.pending.push(chunk.to_string());
                continue;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (subscriber.on_data)(chunk);
            }));
            if let Err(panic) = result {
                eprintln!(
                    "[pty] subscriber {token} panicked on data ({}); removing",
                    panic_description(&*panic)
                );
                panicked.push(*token);
            }
        }
        for token in panicked {
            session.subscribers.remove(&token);
        }
        session.buffer.push_str(chunk);
        // G-383c — the retention cap counts UTF-16 code units (JS
        // `buffer.length` on a 2-unit-per-char string, pty.ts:218-221),
        // not bytes: byte accounting evicted CJK/emoji windows ~3×
        // early. The buffer always holds exactly `cursor -
        // buffer_cursor` units (both advance in units), so the check
        // and the excess are O(1) — no rescan of the window.
        let units = session.cursor - session.buffer_cursor;
        if units > BUFFER_LIMIT {
            let excess_units = units - BUFFER_LIMIT;
            // G-243① — map the excess back to a byte offset on a char
            // boundary (`utf16_byte_offset` clamps inside a surrogate
            // pair to the pair's start, so the trim never splits a
            // codepoint and at most one char over the cap survives).
            let excess_bytes = utf16_byte_offset(&session.buffer, excess_units)
                .unwrap_or(session.buffer.len());
            let drained_units: usize =
                session.buffer.as_str()[..excess_bytes].chars().map(char::len_utf16).sum();
            session.buffer.drain(..excess_bytes);
            session.buffer_cursor += drained_units;
        }
    }

    /// Marks the session exited (idempotent), notifies subscribers, and
    /// records the retention order.
    fn mark_exited(&self, id: &PtyId, exit_code: Option<i32>) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(id) else { return };
        if session.info.status == Status::Exited {
            return;
        }
        session.info.status = Status::Exited;
        session.info.exit_code = exit_code;
        // Notify actives now; park inactives so a late `activate` still
        // replays pending + end (the attachment outlives the map clear).
        let mut ended = std::mem::take(&mut session.ended);
        for (token, mut subscriber) in session.subscribers.drain() {
            if subscriber.active {
                // G-116② — a panicking end callback must not unwind the
                // exit pump.
                subscriber.call_on_end(exit_code);
            } else {
                subscriber.end = Some(exit_code);
                ended.insert(token, subscriber);
            }
        }
        session.ended = ended;
        self.exit_order.lock().push(id.clone());
    }

    /// Ends one session's resources + subscribers (shared by
    /// [`Inner::teardown`] and the [`ShutdownGuard`] finalizer).
    fn teardown_session(session: &mut Session) {
        if let Some(mut killer) = session.killer.take() {
            let _ = killer.kill();
        }
        session.resizer = None;
        session.writer = None;
        let exit_code = session.info.exit_code;
        for (_, mut subscriber) in session.subscribers.drain() {
            if subscriber.active {
                subscriber.call_on_end(exit_code); // G-116②
            }
        }
        for (_, mut subscriber) in session.ended.drain() {
            if subscriber.active {
                subscriber.call_on_end(exit_code); // G-116②
            } else {
                subscriber.end = Some(exit_code);
                subscriber.deliver_pending();
            }
        }
    }

    fn teardown(&self, id: &PtyId) {
        let mut sessions = self.sessions.lock();
        let Some(mut session) = sessions.remove(id) else { return };
        self.exit_order.lock().retain(|other| other != id);
        Self::teardown_session(&mut session);
    }

    fn write(&self, id: &PtyId, data: &str) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(id) else { return };
        if session.info.status != Status::Running {
            return;
        }
        if let Some(writer) = session.writer.as_mut() {
            use std::io::Write;
            let _ = writer.write_all(data.as_bytes());
            let _ = writer.flush();
        }
    }

    fn activate(&self, id: &PtyId, token: u64) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(id) else { return };
        if let Some(subscriber) = session.subscribers.get_mut(&token) {
            if !subscriber.active && !subscriber.detached && subscriber.deliver_pending() {
                // G-116② — panicking replay removes the offender (ref
                // activate try/catch, pty.ts:299-301).
                eprintln!("[pty] subscriber {token} panicked during replay; removing");
                session.subscribers.remove(&token);
            }
            return;
        }
        if let Some(subscriber) = session.ended.get_mut(&token) {
            if !subscriber.active && !subscriber.detached && subscriber.deliver_pending() {
                eprintln!("[pty] subscriber {token} panicked during replay; removing");
                session.ended.remove(&token);
            }
        }
    }

    fn detach(&self, id: &PtyId, token: u64) {
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get_mut(id) {
            if let Some(mut subscriber) = session.subscribers.remove(&token) {
                subscriber.detached = true;
                subscriber.pending.clear();
                subscriber.end = None;
            }
            session.ended.remove(&token);
        }
    }
}

/// `Pty.attach` result (pty.ts:62-71).
pub struct Attachment {
    /// Retained output from the requested cursor to the current end.
    pub replay: String,
    /// Absolute output cursor after replay.
    pub cursor: usize,
    inner: Arc<Inner>,
    id: PtyId,
    token: u64,
}

impl Attachment {
    /// Writes to the child (running sessions only).
    pub fn write(&self, data: &str) {
        self.inner.write(&self.id, data);
    }

    /// Starts live delivery after the caller applied replay + cursor.
    pub fn activate(&self) {
        self.inner.activate(&self.id, self.token);
    }

    /// Stops delivery.
    pub fn detach(&self) {
        self.inner.detach(&self.id, self.token);
    }
}

/// G-383a — the service-close finalizer handle. `PtyService` is
/// `Clone`; the guard's `Drop` runs when the LAST handle goes away
/// (mirroring the ref's `Effect.addFinalizer` teardown, pty.ts:128-144)
/// and kills every live child so a dropped service cannot leak
/// processes. It holds the session store directly (not `Arc<Inner>`)
/// so builder-time `Arc::get_mut` calls keep working; the pump tasks
/// hold `Arc<Inner>` (not the service), so they never pin this guard —
/// killing the children ends them (master EOF → reader exit → pump
/// drain).
struct ShutdownGuard {
    sessions: Arc<Mutex<HashMap<PtyId, Session>>>,
    exit_order: Arc<Mutex<Vec<PtyId>>>,
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        let mut sessions = self.sessions.lock();
        let ids: Vec<PtyId> = sessions.keys().cloned().collect();
        for id in ids {
            if let Some(mut session) = sessions.remove(&id) {
                Inner::teardown_session(&mut session);
            }
        }
        self.exit_order.lock().clear();
    }
}

/// The non-persistent PTY service (`Pty.Service`).
#[derive(Clone)]
pub struct PtyService {
    inner: Arc<Inner>,
    /// G-383a — kills children + ends subscribers on last-handle drop.
    #[allow(dead_code)] // Drop-only: the guard acts in `ShutdownGuard::drop`
    shutdown: Arc<ShutdownGuard>,
    /// Default command resolver (`shell.resolve({priority: "config"})`).
    default_command: Arc<dyn Fn() -> Option<(String, Vec<String>)> + Send + Sync>,
    /// Default working directory (location directory).
    default_cwd: Arc<dyn Fn() -> String + Send + Sync>,
    /// G-1 — the default PTY provisioner (the local portable-pty
    /// driver). Per-location spawns can override through
    /// [`PtyService::create_with`].
    spawner: Arc<dyn PtySpawner>,
}

impl PtyService {
    /// Assembles the service over a bus.
    #[must_use]
    pub fn new(bus: Bus) -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        let inner = Arc::new(Inner {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            exit_order: Arc::new(Mutex::new(Vec::new())),
            next_token: Mutex::new(0),
            ids,
            bus,
        });
        let shutdown = Arc::new(ShutdownGuard {
            sessions: Arc::clone(&inner.sessions),
            exit_order: Arc::clone(&inner.exit_order),
        });
        Self {
            shutdown,
            inner,
            default_command: Arc::new(|| None),
            default_cwd: Arc::new(|| std::env::temp_dir().to_string_lossy().into_owned()),
            spawner: Arc::new(crate::environment::pty::LocalPtySpawner),
        }
    }

    /// Overrides the PTY provisioner seam (G-1: the Environment `pty`
    /// capability; the local driver wraps the previous portable-pty
    /// spawn verbatim).
    #[must_use]
    pub fn with_spawner(mut self, spawner: Arc<dyn PtySpawner>) -> Self {
        self.spawner = spawner;
        self
    }

    /// Overrides the shell-selection seam (the C2 shell lane wires the
    /// real `ShellSelect`; login shells get `-l` appended).
    #[must_use]
    pub fn with_default_command(
        mut self,
        resolver: Arc<dyn Fn() -> Option<(String, Vec<String>)> + Send + Sync>,
    ) -> Self {
        self.default_command = resolver;
        self
    }

    /// Overrides the location-directory seam.
    #[must_use]
    pub fn with_default_cwd(mut self, cwd: Arc<dyn Fn() -> String + Send + Sync>) -> Self {
        self.default_cwd = cwd;
        self
    }

    fn next_id(&self) -> PtyId {
        PtyId(self.inner.ids.new_id("pty"))
    }

    async fn publish(&self, type_: &str, data: serde_json::Value) {
        self.inner.publish(type_, data).await;
    }

    /// Wire conversion into the typed `Pty.Info` schema (camelCase
    /// `exitCode`, absent while running).
    fn schema_info(info: &Info) -> oc_schema::pty::Info {
        oc_schema::pty::Info {
            id: info.id.0.clone(),
            title: info.title.clone(),
            command: info.command.clone(),
            args: info.args.clone(),
            cwd: info.cwd.clone(),
            status: match info.status {
                Status::Running => oc_schema::pty::Status::Running,
                Status::Exited => oc_schema::pty::Status::Exited,
            },
            pid: u64::from(info.pid),
            exit_code: info.exit_code.map(|code| u64::try_from(code).unwrap_or(0)),
        }
    }

    /// `Pty.list`.
    #[must_use]
    pub fn list(&self) -> Vec<Info> {
        self.inner.sessions.lock().values().map(|session| session.info.clone()).collect()
    }

    /// `Pty.get`.
    pub fn get(&self, id: &PtyId) -> Result<Info, NotFoundError> {
        self.inner
            .sessions
            .lock()
            .get(id)
            .map(|session| session.info.clone())
            .ok_or_else(|| NotFoundError { pty_id: id.clone() })
    }

    /// `Pty.create` (pty.ts:166-245): spawn through the pty, wire the data
    /// path into the buffer + subscribers, publish `pty.created`.
    ///
    /// Spawns through the service's default provisioner (the local
    /// portable-pty driver); per-location callers use
    /// [`PtyService::create_with`] with the Environment-resolved spawner.
    pub fn create(&self, input: &CreateInput) -> std::io::Result<Info> {
        self.create_with(&self.spawner, input)
    }

    /// `Pty.create` over an explicit PTY provisioner — the G-1
    /// Environment `pty` seam (the server handler resolves it per
    /// location, like the Files seam).
    #[allow(clippy::too_many_lines)] // mirrors pty.ts create: spawn, wire, publish
    pub fn create_with(
        &self,
        spawner: &Arc<dyn PtySpawner>,
        input: &CreateInput,
    ) -> std::io::Result<Info> {
        let id = self.next_id();
        let (command, base_args) = match &input.command {
            Some(command) => (command.clone(), Vec::new()),
            None => self
                .default_command()
                .ok_or_else(|| std::io::Error::other("no shell configured"))?,
        };
        let mut args = base_args;
        args.extend(input.args.clone().unwrap_or_default());
        // Login shells get `-l` appended (`ShellSelect.login`).
        if is_login_shell(&command) {
            args.push("-l".to_string());
        }
        let cwd = input.cwd.clone().unwrap_or_else(|| (self.default_cwd)());
        let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
        if let Some(extra) = &input.env {
            env.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("OPENCODE_TERMINAL".to_string(), "1".to_string());
        // G-116③ (ref pty.ts:177-181): win32 children get a UTF-8
        // locale — the pty converter decodes UTF-8 regardless of the
        // console codepage. ref: win32-only, unverifiable on this
        // target.
        #[cfg(windows)]
        for (key, value) in win32_utf8_locale_overrides() {
            env.insert(key.to_string(), value.to_string());
        }

        let (cols, rows) = input.size.unwrap_or((80, 24));
        // G-1 — spawn through the Environment `pty` seam (the local
        // driver wraps the previous portable-pty spawn verbatim).
        let spawn_out = spawner.spawn(PtySpawnRequest {
            command: command.clone(),
            args: args.clone(),
            cwd: cwd.clone(),
            env,
            cols,
            rows,
        })?;
        let crate::environment::pty::PtySpawned { reader, writer, master, mut child } = spawn_out;
        let killer = child.clone_killer();
        let pid = child.process_id().unwrap_or(0);

        let info = Info {
            id: id.clone(),
            title: input
                .title
                .clone()
                .unwrap_or_else(|| format!("Terminal {}", &id.0[id.0.len().saturating_sub(4)..])),
            command,
            args,
            cwd,
            status: Status::Running,
            pid,
            exit_code: None,
        };
        let session = Session {
            info: info.clone(),
            buffer: String::new(),
            buffer_cursor: 0,
            cursor: 0,
            subscribers: HashMap::new(),
            ended: HashMap::new(),
            writer: Some(writer),
            killer: Some(killer),
            resizer: Some(master),
        };
        self.inner.sessions.lock().insert(id.clone(), session);

        let (events_tx, mut events_rx) = mpsc::unbounded_channel::<ChildEvent>();
        // Data path: blocking reads feed the channel; EOF reports through
        // the wait thread below (stdout ending means the server is gone).
        {
            let reader = reader;
            let data_tx = events_tx.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut reader = reader;
                // G-335 — stream-buffered decode: never lossy-decode a
                // partial multi-byte sequence split across reads.
                let mut decoder = Utf8StreamDecoder::new();
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let text = decoder.feed(&chunk[..read]);
                            if !text.is_empty()
                                && data_tx.send(ChildEvent::Data(text)).is_err()
                            {
                                break;
                            }
                        }
                    }
                }
                // EOF: a truncated tail is genuinely broken input — flush
                // it lossily (U+FFFD) instead of dropping it silently.
                let tail = decoder.finish();
                if !tail.is_empty() {
                    let _ = data_tx.send(ChildEvent::Data(tail));
                }
            });
        }
        // Exit path: reap the child for the real exit code.
        let exit_tx = events_tx.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().flatten();
            let _ = exit_tx.send(ChildEvent::Exit(code));
        });

        let inner = Arc::clone(&self.inner);
        let id_for_exit = id.clone();
        // G-383a — the pump holds only `Arc<Inner>` (no service clone):
        // a service clone here would pin the shutdown guard for the
        // life of the session and a dropped service could never kill
        // its children.
        tokio::spawn(async move {
            while let Some(event) = events_rx.recv().await {
                match event {
                    ChildEvent::Data(chunk) => inner.on_data(&id_for_exit, &chunk),
                    ChildEvent::Exit(code) => {
                        inner.mark_exited(&id_for_exit, code);
                        // G-383d — the ref publishes the exit code it
                        // has (`onExit({exitCode})`, JSON-omits an
                        // undefined one — node-pty leaves it undefined
                        // on signals); `unwrap_or(0)` would report
                        // success for a missing code. Absent stays
                        // absent.
                        inner
                            .publish(
                                "pty.exited",
                                serde_json::to_value(oc_schema::pty::Exited {
                                    id: id_for_exit.0.clone(),
                                    exit_code: code
                                        .and_then(|c| u64::try_from(c).ok()),
                                })
                                .unwrap_or_default(),
                            )
                            .await;
                        inner.enforce_exited_limit().await;
                        break;
                    }
                }
            }
        });

        let created = info.clone();
        let created_inner = Arc::clone(&self.inner);
        // G-383e (bounded) — the ref publishes `pty.created` awaited
        // inside create and `pty.exited` via `runFork` (async) from
        // onExit (pty.ts:225-233); both rides here are spawned. The
        // only deviation is created-vs-exited ordering for a child
        // that exits within the first scheduler tick — both events are
        // ephemeral bus publishes and every consumer observes the
        // session state through get/list anyway; pinned as bounded.
        tokio::spawn(async move {
            created_inner
                .publish(
                    "pty.created",
                    serde_json::to_value(oc_schema::pty::Created {
                        info: PtyService::schema_info(&created),
                    })
                    .unwrap_or_default(),
                )
                .await;
        });
        Ok(info)
    }

    fn default_command(&self) -> Option<(String, Vec<String>)> {
        (self.default_command)().or_else(|| {
            // Fallback mirrors v2's shell resolution: `$SHELL`, then the
            // standard login shells, then `sh`.
            std::env::var("SHELL")
                .ok()
                .filter(|shell| !shell.is_empty())
                .map(|shell| (shell, Vec::new()))
                .or_else(|| {
                    ["/bin/bash", "/bin/zsh", "/bin/sh"]
                        .iter()
                        .find(|path| std::path::Path::new(path).exists())
                        .map(|path| (path.to_string(), Vec::new()))
                })
        })
    }

    /// `Pty.update` (pty.ts:247-253).
    pub async fn update(&self, id: &PtyId, input: UpdateInput) -> Result<Info, NotFoundError> {
        let info = {
            let mut sessions = self.inner.sessions.lock();
            let session =
                sessions.get_mut(id).ok_or_else(|| NotFoundError { pty_id: id.clone() })?;
            if let Some(title) = input.title {
                session.info.title = title;
            }
            if let Some((cols, rows)) = input.size {
                if session.info.status == Status::Running {
                    if let Some(resizer) = session.resizer.as_mut() {
                        let _ = resizer.resize(cols, rows);
                    }
                }
            }
            session.info.clone()
        };
        self.publish(
            "pty.updated",
            serde_json::to_value(oc_schema::pty::Updated { info: Self::schema_info(&info) })
                .unwrap_or_default(),
        )
        .await;
        Ok(info)
    }

    /// `Pty.remove`.
    pub async fn remove(&self, id: &PtyId) -> Result<(), NotFoundError> {
        if !self.inner.sessions.lock().contains_key(id) {
            return Err(NotFoundError { pty_id: id.clone() });
        }
        self.inner.teardown(id);
        self.publish(
            "pty.deleted",
            serde_json::to_value(oc_schema::pty::Deleted { id: id.0.clone() }).unwrap_or_default(),
        )
        .await;
        Ok(())
    }

    /// `Pty.write`.
    pub fn write(&self, id: &PtyId, data: &str) -> Result<(), NotFoundError> {
        if !self.inner.sessions.lock().contains_key(id) {
            return Err(NotFoundError { pty_id: id.clone() });
        }
        self.inner.write(id, data);
        Ok(())
    }

    /// `Pty.attach` (pty.ts:260-311): `cursor` `-1` tails from the current
    /// end; omitted replays the full retained buffer; a past cursor clamps
    /// to the retained window.
    pub fn attach(
        &self,
        id: &PtyId,
        cursor: Option<i64>,
        on_data: OnData,
        on_end: OnEnd,
    ) -> Result<Attachment, AttachError> {
        let token = {
            let mut next = self.inner.next_token.lock();
            *next += 1;
            *next
        };
        let (replay, end) = {
            let mut sessions = self.inner.sessions.lock();
            let session = sessions
                .get_mut(id)
                .ok_or(AttachError::NotFound(NotFoundError { pty_id: id.clone() }))?;
            if session.info.status != Status::Running {
                return Err(AttachError::Exited(ExitedError { pty_id: id.clone() }));
            }
            session.subscribers.insert(
                token,
                Subscriber {
                    on_data,
                    on_end,
                    active: false,
                    detached: false,
                    pending: Vec::new(),
                    end: None,
                },
            );
            let start = session.buffer_cursor;
            let end = session.cursor;
            // Cursor math over the absolute byte cursor; PTY buffers stay
            // far below i64::MAX and cursors are non-negative by contract.
            #[allow(clippy::cast_possible_wrap)]
            let from = match cursor {
                Some(-1) => end as i64,
                Some(value) if value >= 0 => value,
                _ => 0,
            };
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            #[allow(clippy::cast_possible_wrap)]
            let replay = if session.buffer.is_empty() || from >= end as i64 {
                String::new()
            } else {
                // G-243② — `from`/`start` are UTF-16 code-unit cursors
                // (G-116①); convert to a byte offset before slicing the
                // buffer. Treating the units as a byte index mis-sliced
                // (or panicked) on multi-byte content; the ref slices in
                // the same unit it counts (pty.ts:281-286).
                let units = (from as usize).saturating_sub(start);
                match utf16_byte_offset(&session.buffer, units) {
                    Some(offset) => session.buffer[offset..].to_string(),
                    None => String::new(),
                }
            };
            (replay, end)
        };
        Ok(Attachment {
            replay,
            cursor: end,
            inner: Arc::clone(&self.inner),
            id: id.clone(),
            token,
        })
    }
}

/// `attach` errors.
#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error(transparent)]
    NotFound(#[from] NotFoundError),
    #[error(transparent)]
    Exited(#[from] ExitedError),
}

/// The ref lowercases the basename before matching (`name()`,
/// select.ts:141-144 `path.basename(file).toLowerCase()`).
fn is_login_shell(command: &str) -> bool {
    let base = command.rsplit('/').next().unwrap_or(command).to_lowercase();
    matches!(base.as_str(), "bash" | "zsh" | "sh" | "ksh" | "fish" | "dash")
}

#[cfg(test)]
mod tests {
    use crate::bus::BusOptions;
    use std::time::Duration;

    use super::*;

    async fn bus() -> Bus {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        Bus::new(database, BusOptions::deterministic())
    }

    #[test]
    fn pty_buffer_limit_constants() {
        assert_eq!(BUFFER_LIMIT, 1024 * 1024 * 2);
        assert_eq!(EXITED_LIMIT, 25);
    }

    #[test]
    fn pty_login_shell_detection() {
        assert!(is_login_shell("/bin/bash"));
        assert!(is_login_shell("zsh"));
        // G-277① — the ref lowercases the basename (select.ts:141-144).
        assert!(is_login_shell("/bin/BASH"));
        assert!(is_login_shell("/usr/local/bin/Fish"));
        assert!(!is_login_shell("/usr/bin/env"));
    }

    /// A session with no backing process — the buffer/attach machinery
    /// is driveable directly through `on_data` (the G-243 seams).
    fn synthetic_session(service: &PtyService, id: &str) -> PtyId {
        let id = PtyId(id.to_string());
        service.inner.sessions.lock().insert(
            id.clone(),
            Session {
                info: Info {
                    id: id.clone(),
                    title: "g243".to_string(),
                    command: "/bin/sh".to_string(),
                    args: Vec::new(),
                    cwd: "/tmp".to_string(),
                    status: Status::Running,
                    pid: 0,
                    exit_code: None,
                },
                buffer: String::new(),
                buffer_cursor: 0,
                cursor: 0,
                subscribers: HashMap::new(),
                ended: HashMap::new(),
                writer: None,
                killer: None,
                resizer: None,
            },
        );
        id
    }

    /// G-243① + G-383c — the retention cap counts UTF-16 code units
    /// (ref `buffer.length`, pty.ts:218-221), and the trim maps the
    /// excess units back to a byte offset on a char boundary so a
    /// codepoint is never split. The ref's JS `slice` can never split;
    /// the old byte-count cap also evicted CJK windows ~3× early.
    #[tokio::test]
    async fn g243_buffer_trim_counts_utf16_units_on_char_boundaries() {
        let service = PtyService::new(bus().await);
        let id = synthetic_session(&service, "pty_g243_trim");
        // "ééé" (1 unit, 2 bytes each) + 'a' run: units = LIMIT+2 —
        // two units over the cap; bytes would be LIMIT+3.
        let mut chunk = "ééé".to_string();
        chunk.push_str(&"a".repeat(BUFFER_LIMIT - 1));
        service.inner.on_data(&id, &chunk);

        let (buffer, buffer_cursor, cursor) = {
            let sessions = service.inner.sessions.lock();
            let session = sessions.get(&id).expect("session");
            (session.buffer.clone(), session.buffer_cursor, session.cursor)
        };
        // Excess = 2 units = the first two é (byte offset 4, a char
        // boundary) — the third é and the whole 'a' run survive whole.
        let mut expected = "é".to_string();
        expected.push_str(&"a".repeat(BUFFER_LIMIT - 1));
        assert_eq!(buffer, expected, "window keeps whole chars only");
        // Cursor invariant in UTF-16 units: what the window holds is
        // exactly what the absolute cursors say was not drained.
        assert_eq!(cursor, BUFFER_LIMIT + 2, "cursor advanced by the chunk's units");
        assert_eq!(buffer_cursor, 2, "drained exactly two é's worth of units");
        assert_eq!(cursor - buffer_cursor, utf16_units(&buffer));
    }

    /// G-243② — `attach({cursor})` replays by UTF-16 code-unit offsets:
    /// `from`/`start` advance in units (G-116①) but the old code used
    /// them as BYTE indexes into the buffer — mis-slicing (or panicking
    /// on) multi-byte content. Ref slices in the same unit it counts
    /// (pty.ts:281-286).
    #[tokio::test]
    async fn g243_attach_replays_from_utf16_cursor_exactly() {
        let service = PtyService::new(bus().await);
        let id = synthetic_session(&service, "pty_g243_attach");
        {
            let mut sessions = service.inner.sessions.lock();
            let session = sessions.get_mut(&id).expect("session");
            // α/β/γ are 1 unit / 2 bytes each; 😀 is 2 units / 4 bytes —
            // 10 bytes, 5 UTF-16 units.
            session.buffer.push_str("αβγ😀");
            session.buffer_cursor = 100;
            session.cursor = 105;
        }
        let attach = |cursor: Option<i64>| {
            service.attach(&id, cursor, Box::new(|_| {}), Box::new(|_| {})).expect("attach").replay
        };
        // Past cursor clamps to the retained window start (full replay).
        assert_eq!(attach(Some(99)), "αβγ😀");
        assert_eq!(attach(Some(100)), "αβγ😀");
        // Unit offsets map onto byte offsets: 1 unit in = the second
        // char (byte 2), NOT byte 1 (the old byte-index path panicked
        // mid-char here).
        assert_eq!(attach(Some(101)), "βγ😀");
        assert_eq!(attach(Some(103)), "😀");
        // A cursor inside the surrogate pair clamps to the char's start
        // so it replays whole (JS could slice mid-pair; Rust cannot).
        assert_eq!(attach(Some(104)), "😀");
        // At/after the end (and far past it): nothing.
        assert_eq!(attach(Some(105)), "");
        assert_eq!(attach(Some(200)), "");
        // -1 tails from the current end.
        assert_eq!(attach(Some(-1)), "");
        // Omitted replays the full retained buffer.
        assert_eq!(attach(None), "αβγ😀");
    }

    #[tokio::test]
    async fn pty_missing_sessions_return_typed_not_found() {
        let service = PtyService::new(bus().await);
        let id = PtyId("pty_missing".to_string());
        assert!(service.get(&id).is_err());
        assert!(service.update(&id, UpdateInput::default()).await.is_err());
        assert!(service.remove(&id).await.is_err());
        assert!(service.write(&id, "input").is_err());
        let attach = service.attach(&id, None, Box::new(|_| {}), Box::new(|_| {}));
        assert!(matches!(attach, Err(AttachError::NotFound(_))));
    }

    #[tokio::test]
    async fn pty_exited_session_retained_until_removed() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/sh".to_string()),
                args: Some(vec!["-c".to_string(), "echo hi; exit 3".to_string()]),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");
        // Wait for the exit settle.
        for _ in 0..400 {
            if service.get(&info.id).expect("get").status == Status::Exited {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let exited = service.get(&info.id).expect("retained");
        assert_eq!(exited.status, Status::Exited);
        assert_eq!(exited.exit_code, Some(3));
        service.remove(&info.id).await.expect("remove");
        assert!(service.get(&info.id).is_err());
    }

    #[tokio::test]
    async fn pty_replays_buffered_output_and_streams_live() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/cat".to_string()),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");

        let output: Arc<parking_lot::Mutex<String>> = Arc::default();
        let sink = Arc::clone(&output);
        let attachment = service
            .attach(
                &info.id,
                None,
                Box::new(move |chunk| sink.lock().push_str(chunk)),
                Box::new(|_| {}),
            )
            .expect("attach");
        attachment.activate();
        service.write(&info.id, "AAA\n").expect("write");
        for _ in 0..400 {
            if output.lock().contains("AAA") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(output.lock().contains("AAA"));

        // A later attachment replays the retained buffer; a tail attach
        // skips it but observes the same end cursor.
        let replayed = service
            .attach(&info.id, None, Box::new(|_| {}), Box::new(|_| {}))
            .expect("replay attach");
        assert!(replayed.replay.contains("AAA"));
        assert!(replayed.cursor > 0);
        let tail = service
            .attach(&info.id, Some(-1), Box::new(|_| {}), Box::new(|_| {}))
            .expect("tail attach");
        assert_eq!(tail.replay, "");
        assert_eq!(tail.cursor, replayed.cursor);
        attachment.detach();
        service.remove(&info.id).await.expect("cleanup");
    }

    #[tokio::test]
    async fn pty_attach_rejects_after_exit() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/sh".to_string()),
                args: Some(vec!["-c".to_string(), "exit 0".to_string()]),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");
        for _ in 0..400 {
            if service.get(&info.id).expect("get").status == Status::Exited {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let attach = service.attach(&info.id, None, Box::new(|_| {}), Box::new(|_| {}));
        assert!(matches!(attach, Err(AttachError::Exited(_))));
    }

    /// G-12: the bus payloads are the typed oc-schema wire shapes
    /// (camelCase `exitCode`, omitted while running).
    #[tokio::test]
    async fn pty_events_publish_typed_payloads() {
        use futures::StreamExt;

        let bus = bus().await;
        let mut created_rx = bus.subscribe_type("pty.created");
        let mut exited_rx = bus.subscribe_type("pty.exited");
        let mut deleted_rx = bus.subscribe_type("pty.deleted");
        let service = PtyService::new(bus);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/sh".to_string()),
                args: Some(vec!["-c".to_string(), "exit 3".to_string()]),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");

        let created = tokio::time::timeout(Duration::from_secs(5), created_rx.next())
            .await
            .expect("timeout")
            .expect("pty.created");
        assert_eq!(created.type_, "pty.created");
        assert_eq!(created.data["info"]["id"], info.id.0);
        assert_eq!(created.data["info"]["status"], "running");
        assert_eq!(created.data["info"]["cwd"], "/tmp");
        assert!(created.data["info"].get("exitCode").is_none(), "running info must omit exitCode");

        for _ in 0..400 {
            if service.get(&info.id).expect("get").status == Status::Exited {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let exited = tokio::time::timeout(Duration::from_secs(5), exited_rx.next())
            .await
            .expect("timeout")
            .expect("pty.exited");
        assert_eq!(exited.type_, "pty.exited");
        assert_eq!(exited.data, serde_json::json!({ "id": info.id.0, "exitCode": 3 }));

        service.remove(&info.id).await.expect("remove");
        let deleted = tokio::time::timeout(Duration::from_secs(5), deleted_rx.next())
            .await
            .expect("timeout")
            .expect("pty.deleted");
        assert_eq!(deleted.type_, "pty.deleted");
        assert_eq!(deleted.data, serde_json::json!({ "id": info.id.0 }));
    }

    // ------------------------------------------------------------------
    // G-116 — UTF-16 cursors + subscriber panic isolation
    // ------------------------------------------------------------------

    #[test]
    fn pty_utf16_unit_math() {
        // ASCII: 1 unit per char; BMP CJK: 1 unit; emoji: 2 units
        // (surrogate pair).
        assert_eq!(utf16_units("abc"), 3);
        assert_eq!(utf16_units("中文"), 2);
        assert_eq!(utf16_units("🎉"), 2);
        assert_eq!(utf16_units("中🎉文"), 4);
        assert_eq!("中🎉文".len(), 10, "sanity: byte count differs");
    }

    #[test]
    fn pty_win32_utf8_locale_overrides_table() {
        assert_eq!(
            win32_utf8_locale_overrides(),
            [("LC_ALL", "C.UTF-8"), ("LC_CTYPE", "C.UTF-8"), ("LANG", "C.UTF-8")]
        );
    }

    /// G-116① — the cursor advances by UTF-16 code units, not bytes
    /// (ref `session.cursor += chunk.length`).
    #[tokio::test]
    async fn pty_cursor_counts_utf16_units() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/sh".to_string()),
                args: Some(vec!["-c".to_string(), "printf '中🎉文 tail'; sleep 2".to_string()]),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");
        // Wait for the output to land.
        let replay = loop {
            let attach =
                service.attach(&info.id, None, Box::new(|_| {}), Box::new(|_| {})).expect("attach");
            if attach.replay.contains("tail") {
                break attach;
            }
            assert!(service.get(&info.id).expect("get").status == Status::Running, "child died");
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(replay.cursor, utf16_units(&replay.replay), "replay={:?}", replay.replay);
        assert!(replay.cursor < replay.replay.chars().count() + 2);
        service.remove(&info.id).await.expect("cleanup");
    }

    /// G-116② — a panicking subscriber is removed; the stream (and the
    /// other subscribers) keep working.
    #[tokio::test]
    async fn pty_panicking_subscriber_is_isolated() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/cat".to_string()),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");
        // Silence the expected panic backtrace in the test output.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let collector: Arc<parking_lot::Mutex<String>> = Arc::default();
        let sink = Arc::clone(&collector);
        let healthy = service
            .attach(
                &info.id,
                None,
                Box::new(move |chunk| sink.lock().push_str(chunk)),
                Box::new(|_| {}),
            )
            .expect("healthy attach");
        healthy.activate();
        let panicking = service
            .attach(&info.id, None, Box::new(|_chunk| panic!("subscriber boom")), Box::new(|_| {}))
            .expect("panicking attach");
        panicking.activate();

        service.write(&info.id, "first\n").expect("write 1");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !collector.lock().contains("first") {
            assert!(std::time::Instant::now() < deadline, "healthy subscriber starved");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // The stream must survive the offender: a second write still
        // reaches the healthy subscriber (a naked unwind in the reader
        // loop would have silently killed the session).
        service.write(&info.id, "second\n").expect("write 2");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !collector.lock().contains("second") {
            assert!(std::time::Instant::now() < deadline, "stream died after subscriber panic");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        std::panic::set_hook(previous_hook);
        healthy.detach();
        service.remove(&info.id).await.expect("cleanup");
    }

    // ------------------------------------------------------------------
    // G-335 — stream-buffered UTF-8 decode (no per-chunk lossiness)
    // ------------------------------------------------------------------

    /// A `Read` that returns ONE byte per call — every possible split
    /// boundary of a multi-byte sequence is exercised.
    struct ByteAtATime(Vec<u8>, usize);

    impl std::io::Read for ByteAtATime {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1 >= self.0.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.0[self.1];
            self.1 += 1;
            Ok(1)
        }
    }

    #[test]
    fn g335_decoder_reassembles_splits_at_every_boundary() {
        // 中 (3 bytes) 🎉 (4 bytes) 文 (3 bytes): feed byte-at-a-time.
        let bytes = "中🎉文".as_bytes().to_vec();
        let mut decoder = Utf8StreamDecoder::new();
        let mut out = String::new();
        for byte in &bytes {
            out.push_str(&decoder.feed(std::slice::from_ref(byte)));
        }
        assert_eq!(out, "中🎉文", "each char emits whole, exactly once");
        assert_eq!(decoder.finish(), "", "nothing pending at EOF");
    }

    #[test]
    fn g335_decoder_flushes_truncated_tail_with_replacement() {
        let mut decoder = Utf8StreamDecoder::new();
        assert_eq!(decoder.feed(b"ok"), "ok");
        // A dangling 3-byte CJK lead + first continuation at EOF: a
        // truncated stream end is genuinely broken input — U+FFFD, no
        // panic, nothing silently dropped.
        assert_eq!(decoder.feed(&[0xe4, 0xb8]), "");
        assert_eq!(decoder.finish(), "\u{fffd}");
    }

    #[test]
    fn g335_decoder_recovers_after_invalid_bytes() {
        let mut decoder = Utf8StreamDecoder::new();
        // Standalone continuation byte is DEFINITIVELY invalid (never a
        // truncated prefix): replaced with U+FFFD immediately, the rest
        // flows on — the streaming TextDecoder semantics.
        assert_eq!(decoder.feed(b"be\x80fore"), "be\u{fffd}fore");
        assert_eq!(decoder.feed(b"!"), "!");
    }

    /// G-335 end-to-end: a reader delivering one byte per read (the
    /// worst-case split of every multi-byte sequence) still yields the
    /// intact string through the session buffer — the old
    /// per-chunk `from_utf8_lossy` produced mojibake here.
    #[tokio::test]
    async fn g335_byte_at_a_time_reader_emits_intact_multibyte() {
        use crate::environment::pty::{PtyChild, PtyChildKiller, PtyMaster, PtySpawned};

        struct FakeKiller(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl PtyChildKiller for FakeKiller {
            fn kill(&mut self) -> std::io::Result<()> {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
        }
        struct FakeMaster;
        impl PtyMaster for FakeMaster {
            fn resize(&mut self, _cols: u16, _rows: u16) -> std::io::Result<()> {
                Ok(())
            }
        }
        struct FakeChild(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl PtyChild for FakeChild {
            fn process_id(&self) -> Option<u32> {
                Some(0)
            }
            fn clone_killer(&self) -> Box<dyn PtyChildKiller> {
                Box::new(FakeKiller(Arc::clone(&self.0)))
            }
            fn wait(&mut self) -> std::io::Result<Option<i32>> {
                while !self.0.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(Some(0))
            }
        }
        struct OneByteSpawner;
        impl crate::environment::pty::PtySpawner for OneByteSpawner {
            fn spawn(
                &self,
                request: crate::environment::pty::PtySpawnRequest,
            ) -> Result<PtySpawned, crate::environment::pty::PtySpawnError> {
                let _ = request;
                let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                Ok(PtySpawned {
                    reader: Box::new(ByteAtATime("中🎉文 ok\n".as_bytes().to_vec(), 0)),
                    writer: Box::new(Vec::new()),
                    master: Box::new(FakeMaster),
                    child: Box::new(FakeChild(Arc::clone(&killed))),
                })
            }
        }

        let service = PtyService::new(bus().await);
        let spawner: Arc<dyn crate::environment::pty::PtySpawner> = Arc::new(OneByteSpawner);
        let info = service
            .create_with(
                &spawner,
                &CreateInput {
                    command: Some("fake".to_string()),
                    ..CreateInput::default()
                },
            )
            .expect("create");
        let replay = loop {
            let attach = service
                .attach(&info.id, None, Box::new(|_| {}), Box::new(|_| {}))
                .expect("attach");
            if attach.replay.contains("ok") {
                break attach.replay;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert!(replay.contains("中🎉文 ok"), "replay={replay:?}");
        drop(service);
    }

    // ------------------------------------------------------------------
    // G-383a — service teardown on last-handle drop
    // ------------------------------------------------------------------

    /// G-383a — dropping the last `PtyService` handle kills every live
    /// child (ref `addFinalizer` teardown, pty.ts:128-144): the child
    /// here is our own (`sleep`), reaped by the exit thread once the
    /// master closes.
    #[tokio::test]
    async fn g383_dropping_service_kills_live_children() {
        let service = PtyService::new(bus().await);
        let info = service
            .create(&CreateInput {
                command: Some("/bin/sh".to_string()),
                args: Some(vec!["-c".to_string(), "sleep 60".to_string()]),
                cwd: Some("/tmp".to_string()),
                ..CreateInput::default()
            })
            .expect("create");
        assert!(info.pid != 0, "child must have a pid");
        assert_eq!(service.get(&info.id).expect("get").status, Status::Running);
        drop(service);
        // The teardown guard TERM'd the child; the exit thread reaps
        // it, after which kill(pid, 0) fails (no such process).
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            #[allow(clippy::cast_possible_wrap)]
            let rc = unsafe { libc::kill(info.pid as i32, 0) };
            if rc != 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "child outlived the dropped service");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
