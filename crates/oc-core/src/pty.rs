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
use portable_pty::{ChildKiller, CommandBuilder, PtySize};

use tokio::sync::mpsc;

use crate::bus::{Bus, PublishOptions};

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
    fn deliver_pending(&mut self) {
        self.active = true;
        let pending = std::mem::take(&mut self.pending);
        for chunk in pending {
            (self.on_data)(&chunk);
        }
        if let Some(exit_code) = self.end.take() {
            let on_end = std::mem::replace(&mut self.on_end, noop_end());
            on_end(exit_code);
        }
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
    killer: Option<Box<dyn ChildKiller + Send>>,
    resizer: Option<Box<dyn portable_pty::MasterPty + Send>>,
}

enum ChildEvent {
    Data(String),
    Exit(Option<i32>),
}

struct Inner {
    sessions: Mutex<HashMap<PtyId, Session>>,
    exit_order: Mutex<Vec<PtyId>>,
    next_token: Mutex<u64>,
    ids: Arc<dyn IdGen>,
    bus: Bus,
}

impl Inner {
    fn on_data(&self, id: &PtyId, chunk: &str) {
        let mut sessions = self.sessions.lock();
        let Some(session) = sessions.get_mut(id) else { return };
        session.cursor += chunk.len();
        for subscriber in session.subscribers.values_mut() {
            if subscriber.active {
                (subscriber.on_data)(chunk);
            } else {
                subscriber.pending.push(chunk.to_string());
            }
        }
        session.buffer.push_str(chunk);
        if session.buffer.len() > BUFFER_LIMIT {
            let excess = session.buffer.len() - BUFFER_LIMIT;
            session.buffer.drain(..excess);
            session.buffer_cursor += excess;
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
                let on_end = std::mem::replace(&mut subscriber.on_end, noop_end());
                on_end(exit_code);
            } else {
                subscriber.end = Some(exit_code);
                ended.insert(token, subscriber);
            }
        }
        session.ended = ended;
        self.exit_order.lock().push(id.clone());
    }

    fn teardown(&self, id: &PtyId) {
        let mut sessions = self.sessions.lock();
        let Some(mut session) = sessions.remove(id) else { return };
        self.exit_order.lock().retain(|other| other != id);
        if let Some(mut killer) = session.killer.take() {
            let _ = killer.kill();
        }
        session.resizer = None;
        session.writer = None;
        let exit_code = session.info.exit_code;
        for (_, mut subscriber) in session.subscribers.drain() {
            if subscriber.active {
                let on_end = std::mem::replace(&mut subscriber.on_end, noop_end());
                on_end(exit_code);
            }
        }
        for (_, mut subscriber) in session.ended.drain() {
            if subscriber.active {
                let on_end = std::mem::replace(&mut subscriber.on_end, noop_end());
                on_end(exit_code);
            } else {
                subscriber.end = Some(exit_code);
                subscriber.deliver_pending();
            }
        }
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
            if !subscriber.active && !subscriber.detached {
                subscriber.deliver_pending();
            }
            return;
        }
        if let Some(subscriber) = session.ended.get_mut(&token) {
            if !subscriber.active && !subscriber.detached {
                subscriber.deliver_pending();
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

/// The non-persistent PTY service (`Pty.Service`).
#[derive(Clone)]
pub struct PtyService {
    inner: Arc<Inner>,
    /// Default command resolver (`shell.resolve({priority: "config"})`).
    default_command: Arc<dyn Fn() -> Option<(String, Vec<String>)> + Send + Sync>,
    /// Default working directory (location directory).
    default_cwd: Arc<dyn Fn() -> String + Send + Sync>,
}

impl PtyService {
    /// Assembles the service over a bus.
    #[must_use]
    pub fn new(bus: Bus) -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        Self {
            inner: Arc::new(Inner {
                sessions: Mutex::new(HashMap::new()),
                exit_order: Mutex::new(Vec::new()),
                next_token: Mutex::new(0),
                ids,
                bus,
            }),
            default_command: Arc::new(|| None),
            default_cwd: Arc::new(|| std::env::temp_dir().to_string_lossy().into_owned()),
        }
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
        let definition = crate::bus::registry::by_type(type_)
            .cloned()
            .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(type_));
        let _ = self.inner.bus.publish(&definition, data, PublishOptions::default()).await;
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
    #[allow(clippy::too_many_lines)] // mirrors pty.ts create: spawn, wire, publish
    pub fn create(&self, input: &CreateInput) -> std::io::Result<Info> {
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

        let (cols, rows) = input.size.unwrap_or((80, 24));
        let mut builder = CommandBuilder::new(&command);
        builder.args(&args);
        builder.cwd(&cwd);
        for (key, value) in &env {
            builder.env(key, value);
        }
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let writer =
            pair.master.take_writer().map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        drop(pair.slave);
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
            resizer: Some(pair.master),
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
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let text = String::from_utf8_lossy(&chunk[..read]).into_owned();
                            if data_tx.send(ChildEvent::Data(text)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        // Exit path: reap the child for the real exit code.
        let exit_tx = events_tx.clone();
        std::thread::spawn(move || {
            let code =
                child.wait().ok().map(|status| i32::try_from(status.exit_code()).unwrap_or(0));
            let _ = exit_tx.send(ChildEvent::Exit(code));
        });

        let inner = Arc::clone(&self.inner);
        let service = self.clone();
        let id_for_exit = id.clone();
        tokio::spawn(async move {
            while let Some(event) = events_rx.recv().await {
                match event {
                    ChildEvent::Data(chunk) => inner.on_data(&id_for_exit, &chunk),
                    ChildEvent::Exit(code) => {
                        inner.mark_exited(&id_for_exit, code);
                        service
                            .publish(
                                "pty.exited",
                                serde_json::to_value(oc_schema::pty::Exited {
                                    id: id_for_exit.0.clone(),
                                    exit_code: u64::try_from(code.unwrap_or(0)).unwrap_or(0),
                                })
                                .unwrap_or_default(),
                            )
                            .await;
                        service.enforce_exited_limit().await;
                        break;
                    }
                }
            }
        });

        let created = info.clone();
        let bus_service = self.clone();
        tokio::spawn(async move {
            bus_service
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

    async fn enforce_exited_limit(&self) {
        let evict: Vec<PtyId> = {
            let mut order = self.inner.exit_order.lock();
            let mut evict = Vec::new();
            while order.len() > EXITED_LIMIT {
                let Some(oldest) = order.first().cloned() else { break };
                order.remove(0);
                evict.push(oldest);
            }
            evict
        };
        for id in evict {
            self.inner.teardown(&id);
            self.publish(
                "pty.deleted",
                serde_json::to_value(oc_schema::pty::Deleted { id: id.0.clone() })
                    .unwrap_or_default(),
            )
            .await;
        }
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
                        let _ =
                            resizer.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
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
                let offset = ((from as usize).saturating_sub(start)).min(session.buffer.len());
                session.buffer[offset..].to_string()
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

fn is_login_shell(command: &str) -> bool {
    let base = command.rsplit('/').next().unwrap_or(command);
    matches!(base, "bash" | "zsh" | "sh" | "ksh" | "fish" | "dash")
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
        assert!(!is_login_shell("/usr/bin/env"));
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
}
