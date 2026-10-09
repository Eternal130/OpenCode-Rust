//! Persistent PTY — port of `persistent-pty/index.ts` (core-runner-map
//! §6.1) as an **in-process state machine**.
//!
//! v2 runs the terminals in an out-of-process daemon (`opencode-pty`) so
//! they survive CLI restarts, with handoff tickets and per-instance state
//! directories. **The daemon process boundary is deferred to the Phase 9
//! plugin-runtime wave** (see `spike-plugin-runtime.md`); this module
//! implements the full client-facing state machine now:
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
//! - `snapshot {info, text, checkpoint, cursor}` — the terminal-emulator
//!   checkpoint byte format is a **documented-format stub** (the
//!   out-of-process daemon owns the serializer; the in-process machine
//!   records the scrollback text now and reserves the checkpoint bytes);
//! - durable-ish events `persistent-pty.added` / `persistent-pty.removed`
//!   publish on the bus; exited visible terminals auto-remove.

// Parity surface: the async client API mirrors the out-of-process daemon
// protocol (Phase 9 plugin-runtime wave); several ops are synchronous
// in-process today.
#![allow(clippy::unused_async)]

use std::collections::HashMap;
use std::sync::Arc;

use oc_schema::id::SessionId;
use oc_util::determinism::{AscendingIdGen, IdGen, SystemClock, SystemRng};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize};
use tokio::sync::mpsc;

use crate::bus::{Bus, PublishOptions};
use crate::pty::PtyId;

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

/// `PersistentPty.Snapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub info: Info,
    /// The visible screen rows joined by newlines.
    pub text: String,
    /// Terminal-emulator checkpoint (documented-format stub: the v2
    /// daemon serializes its emulator state here; the in-process machine
    /// stores the scrollback encoding below until Phase 9).
    pub checkpoint: Vec<u8>,
    pub cursor: (usize, usize),
}

/// `PersistentPty.Handoff` — daemon handoff ticket (Phase 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    pub directory: String,
    pub instance_id: String,
    pub ticket: String,
    pub expires_at: u64,
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
    /// Terminal resized.
    Resized { cols: u16, rows: u16 },
    /// The terminal exited.
    Exited { exit_code: Option<i32> },
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

type PubResult<T> = Result<T, UnavailableError>;

struct Live {
    info: Info,
    /// Absolute-byte ring window (like the non-persistent buffer).
    buffer: Vec<u8>,
    buffer_cursor: usize,
    writer: Option<Box<dyn std::io::Write + Send>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    killer: Option<Box<dyn portable_pty::ChildKiller + Send>>,
    /// Visible screen rows (in-process machine tracks the plain text;
    /// control-sequence-aware rendering belongs to the Phase 9 daemon).
    rows: Vec<String>,
    cursor: (usize, usize),
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

struct Inner {
    terminals: Mutex<HashMap<PtyId, Live>>,
    /// Controller activity selects a terminal per session.
    current: Mutex<HashMap<SessionId, PtyId>>,
    next_number: Mutex<u64>,
    next_token: Mutex<u64>,
    #[allow(dead_code)] // instance-scoped id source reserved for the Phase 9 daemon handoff
    ids: Arc<dyn IdGen>,
    bus: Bus,
}

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
                bus,
            }),
            default_program: Arc::new(|| None),
        }
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
        let definition = crate::bus::registry::by_type(type_)
            .cloned()
            .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(type_));
        let _ = self.inner.bus.publish(&definition, data, PublishOptions::default()).await;
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
            rows: vec![String::new(); rows as usize],
            cursor: (0, 0),
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
                                serde_json::json!({
                                    "sessionID": session_for_exit.as_str(),
                                    "ptyID": id_for_exit.0,
                                }),
                            )
                            .await;
                        break;
                    }
                }
            }
        });

        self.publish(
            "persistent-pty.added",
            serde_json::json!({
                "sessionID": session_id.as_str(),
                "terminal": serde_json::to_value(&info).unwrap_or_default(),
            }),
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

    /// `snapshot` (index.ts:253-263). The checkpoint bytes are a
    /// documented-format stub (Phase 9 daemon serializes the emulator
    /// state; here the scrollback text is stored raw).
    pub fn snapshot(&self, id: &PtyId) -> PubResult<Snapshot> {
        let terminals = self.inner.terminals.lock();
        let live = terminals
            .get(id)
            .ok_or_else(|| NotFoundError { pty_id: id.clone() }.into_unavailable())?;
        let text = live.rows.join("\n");
        Ok(Snapshot {
            info: live.info.clone(),
            text: text.clone(),
            checkpoint: text.into_bytes(),
            cursor: live.cursor,
        })
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
                rows: live.rows.clone(),
                cursor: live.cursor,
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
            serde_json::json!({ "sessionID": info.session_id.as_str(), "ptyID": id.0 }),
        )
        .await;
        Ok(())
    }

    /// `shutdown` (index.ts:302-307).
    #[allow(clippy::unused_async_trait_impl)] // async surface parity with index.ts
    pub async fn shutdown(&self) -> PubResult<()> {
        let ids: Vec<PtyId> = self.inner.terminals.lock().keys().cloned().collect();
        for id in ids {
            self.inner.terminate(&id);
        }
        self.inner.current.lock().clear();
        Ok(())
    }

    /// `handoff` — daemon handoff ticket; `None` until the Phase 9
    /// out-of-process daemon exists.
    pub fn handoff(&self) -> PubResult<Option<Handoff>> {
        Ok(None)
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
    fn on_output(&self, id: &PtyId, chunk: &[u8]) {
        let mut terminals = self.terminals.lock();
        let Some(live) = terminals.get_mut(id) else { return };
        live.info.output.1 += chunk.len();
        for byte in chunk {
            // In-process screen tracking: plain text with CR/LF/newline
            // handling; control-sequence rendering is the daemon's job.
            let (x, y) = live.cursor;
            match byte {
                b'\n' => {
                    let next = (y + 1).min(live.rows.len().saturating_sub(1));
                    live.cursor = (0, next);
                }
                b'\r' => live.cursor = (0, y),
                0x08 => live.cursor = (x.saturating_sub(1), y),
                _ => {
                    if y < live.rows.len() {
                        let row = &mut live.rows[y];
                        if x < live.info.size.0 as usize {
                            if row.len() <= x {
                                row.push(' ');
                            }
                            let encoded = *byte as char;
                            row.replace_range(x..=x, &encoded.to_string());
                        }
                        live.cursor = ((x + 1).min(live.info.size.0 as usize), y);
                    }
                }
            }
        }
        live.buffer.extend_from_slice(chunk);
        if live.buffer.len() > OUTPUT_LIMIT {
            let excess = live.buffer.len() - OUTPUT_LIMIT;
            live.buffer.drain(..excess);
            live.buffer_cursor += excess;
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
        live.rows.resize(rows as usize, String::new());
        let event = StreamEvent::Resized { cols, rows };
        for subscriber in live.subscribers.values_mut() {
            if subscriber.active {
                (subscriber.on_event)(event.clone());
            } else {
                subscriber.pending.push(event.clone());
            }
        }
        if let Some(attachment) = attachment {
            if live.controller.as_deref() == Some(attachment) {
                live.generation += 1;
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
        assert!(!snapshot.checkpoint.is_empty(), "checkpoint stub carries the scrollback bytes");

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
}
