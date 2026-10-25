//! Port of `packages/core/src/shell.ts` — the location-owned non-interactive
//! shell command process service (§4.4, §6.2 of core-runner-map).
//!
//! Each `create` spawns one shell command, captures combined stdout/stderr
//! to `<data>/shell/<projectID>/<id>.out`, and exposes poll/wait/result/
//! timeout/output. Every command is expected to be wrapped in a Job by the
//! shell tool; permission state lives with the caller.

pub mod parse;
pub mod result;
pub mod scan;
pub mod select;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use oc_schema::id::ShellId;
use oc_schema::shell::{CreateInput, Info, Output, OutputInput, Status};
use oc_util::determinism::{Clock, IdGen};
use parking_lot::Mutex;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::watch;

pub use result::{Capture, Notification, NotificationInput, ShellResult};

/// `Shell.NotFoundError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("shell not found: {0}")]
pub struct NotFoundError(pub ShellId);

/// Process-local cap complementing the 7-day retention sweep.
pub const EXITED_LIMIT: usize = 25;
/// `Shell.RETENTION`.
pub const RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// `Shell.DIRECTORY`.
pub const DIRECTORY: &str = "shell";

/// `Shell.cleanup` (shell.ts:83-117): the hourly retention sweep over the
/// two-level capture layout `<data>/shell/<projectID>/sh_<id>.out` — drops
/// capture files whose modification time predates the 7-day cutoff.
/// Missing directories/files are expected between sweeps and ignored.
pub fn cleanup_data_root(data_root: &std::path::Path, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(RETENTION_MS);
    let Ok(projects) = std::fs::read_dir(data_root.join(DIRECTORY)) else { return };
    for project in projects.flatten() {
        let Ok(captures) = std::fs::read_dir(project.path()) else { continue };
        for entry in captures.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !(name.starts_with("sh_") && name.ends_with(".out")) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else { continue };
            let modified_ms = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0);
            if (modified_ms as u64) < cutoff {
                // Retention removals are best-effort (v2 ignores errors).
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Spawns the hourly shell-capture retention sweep (shell.ts:104-109 — a
/// scoped fiber in v2; a detached tokio task here, cancelled by dropping
/// the join handle). The first tick runs immediately, so boot performs one
/// sweep before settling into the hourly cadence.
pub fn spawn_cleanup_task(
    data_root: PathBuf,
    clock: Arc<dyn Clock>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3_600));
        loop {
            interval.tick().await;
            cleanup_data_root(&data_root, u64::try_from(clock.unix_ms()).unwrap_or(0));
        }
    })
}

/// `ShellCreateBefore` — the mutable invocation handed to hooks.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub command: String,
    pub cwd: String,
    pub timeout: u64,
    pub shell: String,
    /// Full child environment (session environment or process env, plus
    /// terminal markers).
    pub env: HashMap<String, String>,
}

/// Environment markers set on every shell invocation.
pub const TERMINAL_ENV: &[(&str, &str)] = &[("TERM", "xterm-256color"), ("OPENCODE_TERMINAL", "1")];

type BeforeHook = Box<dyn FnOnce(&mut Invocation) + Send>;

struct Active {
    info: Mutex<Info>,
    file: PathBuf,
    size: AtomicU64,
    newlines: AtomicU64,
    /// Terminal `Info` broadcast; `status != running` once settled.
    done: watch::Sender<Option<Info>>,
    /// Set once the capture file is fully written (`beforeWait` in v2).
    /// The original receiver is kept alive so the channel never closes
    /// before a finisher subscribes.
    output_done: watch::Sender<bool>,
    _output_keepalive: watch::Receiver<bool>,
    /// Process id captured at spawn (the group id for group kills).
    pid: Option<u32>,
    timeout: Mutex<Option<tokio::task::JoinHandle<()>>>,
    child: Mutex<Option<Child>>,
}

impl Active {
    #[allow(dead_code)] // parity surface kept for later phases
    fn terminal_info(&self) -> Option<Info> {
        self.done.borrow().clone()
    }
}

/// The shell process service (clone shares state).
#[derive(Clone)]
pub struct Shell {
    inner: Arc<Inner>,
}

struct Inner {
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
    commands: Mutex<HashMap<ShellId, Arc<Active>>>,
    exit_order: Mutex<Vec<ShellId>>,
    output_dir: PathBuf,
    /// Optional bus for `shell.created`/`shell.exited`/`shell.deleted`.
    bus: Option<crate::bus::Bus>,
    /// Optional per-location hook registry for `shell.create.before`.
    hooks: Option<Arc<crate::hooks::HookRegistry>>,
    /// Config `shell` key (`ShellSelect` priority "config" input).
    config_shell: Option<String>,
}

impl Shell {
    /// Assembles the service writing capture files under
    /// `<data>/shell/<projectID>`.
    pub fn new(
        data_root: &std::path::Path,
        project_id: &str,
        ids: Arc<dyn IdGen>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let output_dir = data_root.join(DIRECTORY).join(project_id);
        let _ = std::fs::create_dir_all(&output_dir);
        Self {
            inner: Arc::new(Inner {
                ids,
                clock,
                commands: Mutex::new(HashMap::new()),
                exit_order: Mutex::new(Vec::new()),
                output_dir,
                bus: None,
                hooks: None,
                config_shell: None,
            }),
        }
    }

    /// Attaches a bus for the ephemeral `shell.*` events.
    #[must_use]
    pub fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let inner = Arc::get_mut(&mut self.inner).expect("bus must attach before cloning");
        inner.bus = Some(bus);
        self
    }

    /// Attaches the location's hook registry — the `shell.create.before`
    /// dispatch site (v2 `PluginHooks.node`, shell.ts:274).
    #[must_use]
    pub fn with_hooks(mut self, hooks: Arc<crate::hooks::HookRegistry>) -> Self {
        let inner = Arc::get_mut(&mut self.inner).expect("hooks must attach before cloning");
        inner.hooks = Some(hooks);
        self
    }

    /// Attaches the config `shell` key — the `ShellSelect` editor's
    /// configured value (config/plugin/shell.ts; `resolve` priority
    /// "config").
    #[must_use]
    pub fn with_config_shell(mut self, shell: Option<String>) -> Self {
        let inner = Arc::get_mut(&mut self.inner).expect("config shell must attach before cloning");
        inner.config_shell = shell;
        self
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.inner.clock.unix_ms()).unwrap_or(0)
    }

    async fn publish(&self, type_: &str, data: serde_json::Value) {
        if let Some(bus) = &self.inner.bus {
            let definition = crate::bus::registry::EventDefinition::ephemeral(type_);
            let _ = bus.publish(&definition, data, crate::bus::PublishOptions::default()).await;
        }
    }

    fn require(&self, id: &ShellId) -> Result<Arc<Active>, NotFoundError> {
        self.inner.commands.lock().get(id).cloned().ok_or_else(|| NotFoundError(id.clone()))
    }

    /// `Shell.remove`.
    pub async fn remove(&self, id: &ShellId) -> Result<(), NotFoundError> {
        self.require(id)?;
        self.remove_command(id).await;
        Ok(())
    }

    async fn remove_command(&self, id: &ShellId) {
        let command = {
            let mut commands = self.inner.commands.lock();
            let mut exit_order = self.inner.exit_order.lock();
            if let Some(position) = exit_order.iter().position(|item| item == id) {
                exit_order.remove(position);
            }
            commands.remove(id)
        };
        let Some(command) = command else { return };
        if let Some(timeout) = command.timeout.lock().take() {
            timeout.abort();
        }
        // Unblock any wait still pending when the command is removed before
        // it terminated.
        let _ = command.done.send(None);
        let _ = tokio::fs::remove_file(&command.file).await;
        self.publish("shell.deleted", serde_json::json!({ "id": id.as_str() })).await;
    }

    /// `Shell.list` — currently running commands only.
    #[must_use]
    pub fn list(&self) -> Vec<Info> {
        self.inner
            .commands
            .lock()
            .values()
            .filter(|command| command.info.lock().status == Status::Running)
            .map(|command| command.info.lock().clone())
            .collect()
    }

    /// `Shell.get`.
    pub fn get(&self, id: &ShellId) -> Result<Info, NotFoundError> {
        Ok(self.require(id)?.info.lock().clone())
    }

    /// `Shell.wait` — resolves with the terminal `Info`.
    pub async fn wait(&self, id: &ShellId) -> Result<Info, NotFoundError> {
        let command = self.require(id)?;
        let mut receiver = command.done.subscribe();
        loop {
            {
                let current = receiver.borrow();
                if let Some(info) = current.as_ref() {
                    return Ok(info.clone());
                }
                if !self.inner.commands.lock().contains_key(id) {
                    return Err(NotFoundError(id.clone()));
                }
            }
            if receiver.changed().await.is_err() {
                return Err(NotFoundError(id.clone()));
            }
        }
    }

    /// `Shell.timeout` — replaces the running command's timeout from now;
    /// zero clears it.
    // parity surface: async signature mirrors the TS Shell API
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn timeout(&self, id: &ShellId, duration: u64) -> Result<Info, NotFoundError> {
        let command = self.require(id)?;
        if command.info.lock().status != Status::Running {
            return Ok(command.info.lock().clone());
        }
        let shared = Arc::clone(&command);
        self.arm_timeout(&shared, duration);
        let info = command.info.lock().clone();
        Ok(info)
    }

    fn arm_timeout(&self, command: &Arc<Active>, duration: u64) {
        if let Some(existing) = command.timeout.lock().take() {
            existing.abort();
        }
        if duration == 0 || command.info.lock().status != Status::Running {
            return;
        }
        let timeout_command = Arc::clone(command);
        let shell = self.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(duration)).await;
            // finish("timeout", …, beforeWait = kill): v2 marks the status,
            // then kills (TERM escalating to force after 3s — SIGKILL is the
            // closest primitive without signal APIs), then waits for the
            // capture to flush before resolving waiters.
            shell.finish(&timeout_command, Status::Timeout, None, None, true).await;
        });
        *command.timeout.lock() = Some(handle);
    }

    /// `Shell.output` — one page of captured output by cursor.
    #[allow(clippy::cast_possible_truncation)] // paging: cursor/limit/size are bounded by the capture file
    pub async fn output(
        &self,
        id: &ShellId,
        input: Option<OutputInput>,
    ) -> Result<Output, NotFoundError> {
        use tokio::io::AsyncSeekExt;

        let command = self.require(id)?;
        let cursor = input.as_ref().and_then(|input| input.cursor).unwrap_or(0);
        let limit = input.as_ref().and_then(|input| input.limit).unwrap_or(65_536) as usize;
        let size = command.size.load(Ordering::SeqCst);
        if cursor >= size {
            return Ok(Output { output: String::new(), cursor: size, size, truncated: false });
        }
        let start = cursor.min(size) as usize;
        let length = limit.min((size as usize) - start);
        let mut buffer = vec![0u8; length];
        let bytes = match tokio::fs::File::open(&command.file).await {
            Ok(mut file) => {
                let _ = file.seek(std::io::SeekFrom::Start(start as u64)).await;
                file.read(&mut buffer).await.unwrap_or(0)
            }
            Err(_) => 0,
        };
        buffer.truncate(bytes);
        Ok(Output {
            output: String::from_utf8_lossy(&buffer).to_string(),
            cursor: (start + bytes) as u64,
            size,
            truncated: false,
        })
    }

    /// `Shell.result` — terminal state plus the bounded tail capture
    /// (`capture: None` when the command is gone).
    pub async fn result(&self, started: Info) -> ShellResult {
        let info = if let Ok(info) = self.wait(&started.id).await {
            info
        } else {
            let mut fallback = started.clone();
            fallback.status = Status::Killed;
            fallback.time.completed = Some(self.now_ms());
            fallback
        };
        let capture = self.capture(&info).await;
        ShellResult { info, capture }
    }

    /// The bounded tail capture of a finished command (`tool_output` limits).
    async fn capture(&self, info: &Info) -> Option<result::Capture> {
        const MAX_LINES: usize = crate::tool_output::MAX_LINES;
        const MAX_BYTES: usize = crate::tool_output::MAX_BYTES;
        let latest = self
            .output(&info.id, Some(OutputInput { cursor: Some(u64::MAX), limit: None }))
            .await
            .ok()?;
        let page = self
            .output(
                &info.id,
                Some(OutputInput {
                    cursor: Some(latest.size.saturating_sub(MAX_BYTES as u64)),
                    limit: Some(MAX_BYTES as u64),
                }),
            )
            .await
            .ok()?;
        let mut lines: Vec<&str> = page.output.split('\n').collect();
        if page.output.ends_with('\n') {
            lines.pop();
        }
        let truncated = latest.size > MAX_BYTES as u64 || lines.len() > MAX_LINES;
        let text = if lines.len() > MAX_LINES {
            lines[lines.len() - MAX_LINES..].join("\n")
        } else {
            page.output.clone()
        };
        let newlines = self.newlines_of(&info.id);
        let total = newlines + u64::from(!page.output.is_empty() && !page.output.ends_with('\n'));
        let shown = lines.len().min(MAX_LINES);
        let notice = truncated.then(|| {
            format!(
                "\n\n[showing lines {}-{total} of {total}; full output saved to {}]",
                total - shown as u64 + 1,
                info.file
            )
        });
        Some(result::Capture {
            output: format!(
                "{}{}",
                if text.is_empty() { "(no output)" } else { text.as_str() },
                notice.unwrap_or_default()
            ),
            truncated,
        })
    }

    fn newlines_of(&self, id: &ShellId) -> u64 {
        self.inner
            .commands
            .lock()
            .get(id)
            .map_or(0, |command| command.newlines.load(Ordering::SeqCst))
    }

    async fn finish(
        &self,
        command: &Arc<Active>,
        status: Status,
        exit: Option<f64>,
        signal: Option<String>,
        kill: bool,
    ) {
        // Mark the terminal status FIRST (v2 `produce` before `beforeWait`),
        // so the exit watcher cannot race a timeout with "exited". The
        // marking finisher always resolves the waiters.
        let winner = {
            let mut info = command.info.lock();
            if info.status == Status::Running {
                info.status = status;
                info.exit = exit.map(oc_schema::common::Num);
                info.signal = signal;
                info.time.completed = Some(self.now_ms());
                true
            } else {
                false
            }
        };
        if !winner {
            return;
        }
        if kill {
            // v2 `handle.kill({ forceKillAfter: 3s })`: TERM the detached
            // group first; escalate to SIGKILL only if it survives 3s.
            // The command spawned its own process group (`process_group(0)`);
            // signalling the group releases orphaned grandchildren holding
            // the output pipes. Signals go through a dedicated blocking
            // thread — tokio's own child driver is unreliable for one-shot
            // helpers here.
            #[cfg(unix)]
            if let Some(pid) = command.pid {
                let _ = tokio::task::spawn_blocking(move || {
                    signal_process_group(pid, libc::SIGTERM);
                })
                .await;
                // Escalation (3s → KILL). Detached: the TERM path usually
                // settles the group, and the escalation must fire even if
                // this finish future is cancelled.
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let _ = tokio::task::spawn_blocking(move || {
                        signal_process_group(pid, libc::SIGKILL);
                    })
                    .await;
                });
            }
            #[cfg(not(unix))]
            {
                let _ = command.child.lock().as_mut().is_some_and(|live| live.start_kill().is_ok());
            }
        }
        // The capture file must be fully written before waiters resolve
        // (`outputDone.await`).
        {
            let mut receiver = command.output_done.subscribe();
            if !*receiver.borrow() {
                let _ = receiver.changed().await;
            }
        }
        let info = command.info.lock().clone();
        let _ = command.done.send(Some(info.clone()));
        self.inner.exit_order.lock().push(info.id.clone());
        let mut data =
            serde_json::json!({ "id": info.id.as_str(), "status": status_str(info.status) });
        if let Some(exit) = info.exit {
            data["exit"] = serde_json::json!(exit.0);
        }
        self.publish("shell.exited", data).await;
        // Retention: keep at most EXITED_LIMIT exited commands.
        loop {
            let evict = {
                let exit_order = self.inner.exit_order.lock();
                if exit_order.len() <= EXITED_LIMIT {
                    None
                } else {
                    exit_order.first().cloned()
                }
            };
            match evict {
                Some(oldest) => self.remove_command(&oldest).await,
                None => break,
            }
        }
        if let Some(timeout) = command.timeout.lock().take() {
            timeout.abort();
        }
    }

    /// `Shell.create` — spawn one detached shell command.
    pub async fn create(
        &self,
        input: CreateInput,
        shell_path: Option<&str>,
        session_env: Option<&HashMap<String, String>>,
        before: Option<BeforeHook>,
    ) -> Result<Info, std::io::Error> {
        let mut env: HashMap<String, String> =
            session_env.cloned().unwrap_or_else(|| std::env::vars().collect());
        for (key, value) in TERMINAL_ENV {
            env.insert((*key).to_string(), (*value).to_string());
        }
        let mut invocation = Invocation {
            command: input.command.clone(),
            cwd: input.cwd.clone().unwrap_or_default(),
            timeout: input.timeout.unwrap_or(0),
            shell: shell_path.map_or_else(
                || select::resolve(self.inner.config_shell.as_deref(), false),
                String::from,
            ),
            env,
        };
        // `hooks.trigger("shell", "create.before", invocation)` — BEFORE
        // the caller's `before` closure (shell.ts:274-275): the hook shapes
        // the invocation the permission pipeline then validates.
        self.trigger_create_before(&mut invocation).await?;
        if let Some(before) = before {
            before(&mut invocation);
        }
        self.create_invocation(invocation, input.metadata.unwrap_or_default()).await
    }

    /// The production `shell.create.before` dispatch site (shell.ts:274):
    /// the location's hook registry mutates the invocation copy-in, and
    /// the (possibly rewritten) invocation is copied back out. A hook
    /// failure fails the create — nothing spawns. The tool path calls
    /// this before its permission pipeline, mirroring the v2 ordering.
    pub async fn trigger_create_before(
        &self,
        invocation: &mut Invocation,
    ) -> Result<(), std::io::Error> {
        let Some(hooks) = self.inner.hooks.clone() else { return Ok(()) };
        let event = crate::hooks::ShellCreateBeforeEvent {
            command: invocation.command.clone(),
            cwd: invocation.cwd.clone(),
            timeout_ms: invocation.timeout,
            shell: invocation.shell.clone(),
            env: invocation
                .env
                .iter()
                .map(|(key, value)| (key.clone(), Some(value.clone())))
                .collect(),
        };
        let shaped = hooks.dispatch_shell_create_before(event).await.map_err(|error| {
            std::io::Error::other(format!("shell.create.before hook failed: {error}"))
        })?;
        invocation.command = shaped.command;
        invocation.cwd = shaped.cwd;
        invocation.timeout = shaped.timeout_ms;
        invocation.shell = shaped.shell;
        // Hook env entries: `Some(value)` sets, `None` removes (the v2
        // JSON shape's null erasure).
        invocation.env = shaped
            .env
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect();
        Ok(())
    }

    /// `Shell.create` over a fully prepared invocation (the shell tool runs
    /// its permission pipeline first, then spawns here).
    #[allow(clippy::too_many_lines)] // spawn pipeline mirrors Shell.create
    pub async fn create_invocation(
        &self,
        invocation: Invocation,
        metadata: oc_schema::common::Record,
    ) -> Result<Info, std::io::Error> {
        let input_timeout = invocation.timeout;
        let id = ShellId(self.inner.ids.new_id("sh"));
        let args = select::args(&invocation.shell, &invocation.command);
        let file = self.inner.output_dir.join(format!("{}.out", id.as_str()));

        let mut info = Info {
            id: id.clone(),
            status: Status::Running,
            command: invocation.command.clone(),
            cwd: invocation.cwd.clone(),
            shell: invocation.shell.clone(),
            file: file.to_string_lossy().to_string(),
            pid: None,
            exit: None,
            signal: None,
            metadata,
            time: oc_schema::shell::Time { started: self.now_ms(), completed: None },
        };
        let _ = &mut info;

        let mut command = Command::new(&invocation.shell);
        command.args(&args);
        if !invocation.cwd.is_empty() {
            command.current_dir(&invocation.cwd);
        }
        command.env_clear();
        command.envs(&invocation.env);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        command.kill_on_drop(false);
        let mut child = command.spawn()?;
        let child_pid = child.id();
        info.pid = child_pid.map(u64::from);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (done_tx, _) = watch::channel(None);
        let (output_tx, output_rx) = watch::channel(false);
        let active = Arc::new(Active {
            info: Mutex::new(info.clone()),
            file: file.clone(),
            size: AtomicU64::new(0),
            newlines: AtomicU64::new(0),
            done: done_tx,
            output_done: output_tx,
            _output_keepalive: output_rx,
            pid: child_pid,
            timeout: Mutex::new(None),
            child: Mutex::new(Some(child)),
        });
        self.inner.commands.lock().insert(id.clone(), Arc::clone(&active));

        // Combined streams → file, with newline counting while streaming so
        // truncation notices never rescan the output file. stdout and
        // stderr are pumped CONCURRENTLY (v2 streams `handle.all` — one
        // merged arrival-ordered stream): draining them sequentially
        // deadlocks once the unpumped pipe fills (64 KiB), killing
        // healthy commands with a false timeout.
        {
            let active = Arc::clone(&active);
            let file = file.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                /// One pump source pipe.
                type PumpStream = Box<dyn tokio::io::AsyncRead + Unpin + Send>;
                let Ok(file) = tokio::fs::File::create(&file).await else {
                    let _ = active.output_done.send(true);
                    return;
                };
                let writer = Arc::new(tokio::sync::Mutex::new(file));
                let mut pumps = Vec::new();
                let streams: Vec<PumpStream> = [
                    stdout.map(|stream| Box::new(stream) as PumpStream),
                    stderr.map(|stream| Box::new(stream) as PumpStream),
                ]
                .into_iter()
                .flatten()
                .collect();
                for stream in streams {
                    let writer = Arc::clone(&writer);
                    let active = Arc::clone(&active);
                    // One pump per pipe: chunks land in the capture in
                    // arrival order as each read completes.
                    pumps.push(tokio::spawn(async move {
                        let mut stream = stream;
                        let mut buffer = vec![0u8; 16 * 1024];
                        loop {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => break,
                                Ok(read) => {
                                    let chunk = &buffer[..read];
                                    active.size.fetch_add(read as u64, Ordering::SeqCst);
                                    // Streaming count mirrors v2's
                                    // incremental newline tally.
                                    #[allow(clippy::naive_bytecount)]
                                    active.newlines.fetch_add(
                                        chunk.iter().filter(|&&byte| byte == b'\n').count() as u64,
                                        Ordering::SeqCst,
                                    );
                                    let mut writer = writer.lock().await;
                                    if writer.write_all(chunk).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }));
                }
                for pump in pumps {
                    let _ = pump.await;
                }
                let mut writer = writer.lock().await;
                let _ = writer.flush().await;
                let _ = active.output_done.send(true);
            });
        }

        self.arm_timeout(&active, input_timeout);

        // Observe exit and settle.
        {
            let active = Arc::clone(&active);
            let shell = self.clone();
            tokio::spawn(async move {
                let mut child = active.child.lock().take();
                let Some(live) = child.as_mut() else { return };
                // Poll `try_wait`: tokio's SIGCHLD-driven wakeups are flaky
                // under load, and the settle path must be deterministic.
                let status = loop {
                    match live.try_wait() {
                        Ok(Some(status)) => {
                            break Ok(status);
                        }
                        Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
                        Err(error) => break Err(error),
                    }
                };
                if let Ok(status) = status {
                    let exit = status.code().map(f64::from);
                    #[cfg(unix)]
                    let signal = {
                        use std::os::unix::process::ExitStatusExt;
                        status.signal().map(signal_name)
                    };
                    #[cfg(not(unix))]
                    let signal = None;
                    shell.finish(&active, Status::Exited, exit, signal, false).await;
                } else {
                    let id = active.info.lock().id.clone();
                    shell.remove_command(&id).await;
                }
            });
        }
        self.publish("shell.created", serde_json::json!({ "info": info })).await;
        Ok(info)
    }
}

/// Signal every process in `leader`'s process group (TERM first, KILL on
/// escalation — v2 `forceKillAfter`). Some sandboxes silently drop
/// `kill(-pgid, …)`, so members are enumerated from `/proc` and
/// signalled individually.
///
/// Safety (incident 2026-10): the pgrp scan only runs while the group
/// LEADER (`/proc/<leader>`) still exists — a live leader pins the process
/// group id, so it cannot have been recycled onto innocent processes.
/// Members are signalled via kill(2) on individually-verified pids.
#[cfg(unix)]
fn signal_process_group(leader: u32, signal: i32) {
    // A live leader is the precondition for interpreting `pgrp == leader`
    // at all; without it the id may name a recycled group.
    if std::fs::read_to_string(format!("/proc/{leader}/stat")).is_err() {
        return;
    }
    let entries = std::fs::read_dir("/proc").ok();
    let Some(entries) = entries else { return };
    let mut members: Vec<u32> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Ok(member) = name.parse::<u32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{member}/stat")) else { continue };
        // Fields after `(comm)`: state ppid pgrp …
        let Some(tail) = stat.rsplit_once(')') else { continue };
        let mut fields = tail.1.split_whitespace();
        let _state = fields.next();
        let _ppid = fields.next();
        let Some(pgrp) = fields.next().and_then(|field| field.parse::<u32>().ok()) else {
            continue;
        };
        if pgrp == leader {
            members.push(member);
        }
    }
    // Signal only pids that are STILL group members at delivery time (they
    // were re-verified by the scan above; /bin/kill is never invoked).
    #[allow(unsafe_code)] // contained kill(2) on scan-verified group members
    for member in members {
        // SAFETY: kill(2) on a pid whose /proc entry was just read as a
        // member of the pinned group; the live-leader precondition rules
        // out group-id recycling.
        unsafe {
            let rc = libc::kill(i32::try_from(member).unwrap_or(-1), signal);
            let _ = rc;
        }
    }
}

fn status_str(status: Status) -> &'static str {
    match status {
        Status::Running => "running",
        Status::Exited => "exited",
        Status::Timeout => "timeout",
        Status::Killed => "killed",
    }
}

#[cfg(unix)]
fn signal_name(signal: i32) -> String {
    match signal {
        1 => "SIGHUP".into(),
        2 => "SIGINT".into(),
        3 => "SIGQUIT".into(),
        6 => "SIGABRT".into(),
        9 => "SIGKILL".into(),
        13 => "SIGPIPE".into(),
        15 => "SIGTERM".into(),
        other => format!("SIG{other}"),
    }
}

/// `Shell.Event` definition helper for tests.
#[must_use]
pub fn event_definition(type_: &str) -> crate::bus::registry::EventDefinition {
    crate::bus::registry::EventDefinition::ephemeral(type_)
}

#[cfg(test)]
mod tests;
