//! The daemon CLIENT transport (design §2.2, §5.2, §8, §9) — a port of
//! `makeDaemonTransport` (`ref: daemon.ts:158-431`): discover/ping,
//! spawn-if-absent with the fresh-dir retry, the owner connection
//! (demux of exchange responses vs `bus_event` vs `owner_superseded`),
//! per-request one-shots, the §5.2 failure ladder and the §6 adoption
//! ladder. The frontend (`super::frontend`) maps these onto the
//! existing client API's op surface.
//!
//! Hermeticity seam (§10.1): [`Launcher`] abstracts "spawn the `oc-pty`
//! binary" — the production [`BinaryLauncher`] resolves the binary per
//! §8.1 (`OPENCODE_PTY_BIN` → exe-sibling → PATH) and spawns it
//! detached; tests inject a fake launcher that starts an in-process
//! listener speaking the same [`super::wire`] codec over a real
//! filesystem socket, so no binary is needed (real-binary process
//! tests are D3).
//!
//! Signals: the ONLY signal path here is [`Spawned::terminate`] on the
//! child handle the client itself just spawned (§4.4(b)) — never a pid
//! read from disk, never a process group.
//!
//! Lint pins (mirroring the D1 daemon crate boundary): the §5.2 ladder
//! fns mirror `daemon.ts` structure one-arm-per-step (too_many_lines),
//! and every connection future embeds a 16 KiB read chunk — the
//! protocol's one-in-flight-per-connection shape (large_futures).
#![allow(clippy::too_many_lines, clippy::large_futures, clippy::too_many_arguments)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oc_schema::id::SessionId;
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

use super::wire::{
    self, FrameDecoder, OwnerFrame, WireRequest, WireResponse, WireRole, WireTerminal,
};
use super::{
    Adoption, AdoptionError, Handoff, HandoffState, Info, Registration, Replay, Role, StreamEvent,
    PTY_PROTOCOL_VERSION,
};

/// Per one-shot request / claim / handoff-prepare timeout (`daemon.ts:195-199`,
/// `:365-369`, `:538-548`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Discover poll cadence during daemon startup (`daemon.ts:246-262`).
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Startup deadline before the child is terminated (`daemon.ts:246-262`).
const DEFAULT_SPAWN_DEADLINE: Duration = Duration::from_secs(5);
/// Post-`shutdown` wait for discover to fail (`daemon.ts:340-354`).
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// Chunked-replay reassembly guard (§3.2 — no live deadline in the ref;
/// this bounds a stuck transfer).
const REPLAY_ASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Errors (§9)
// ---------------------------------------------------------------------------

/// Client transport error kinds — the port of `DaemonError`
/// (`daemon.ts:134-138`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Socket unreachable / claim timeout (`daemon.ts:195-199`).
    Connect,
    /// Connected but no/bad reply (`daemon.ts:542-546`).
    Response,
    /// Auth/identity invalid → drop cached registration + rediscover
    /// (`daemon.ts:527-533`; `error{"daemon is owned"}`, §6).
    Registration,
    /// Version/shape mismatch (`daemon.ts:209-216`).
    Protocol,
    /// Binary missing / child failed (§8.1).
    Spawn,
}

/// A transport error: the §9 kind plus a message that rides the
/// client API's `UnavailableError` verbatim (the machine's own error
/// messages — e.g. `PersistentPty.NotFoundError: …` — surface unchanged
/// through `kind: Response`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TransportError {
    pub kind: ErrorKind,
    pub message: String,
}

impl TransportError {
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    /// `daemon.ts:527-533` — the only wire error message with transport
    /// semantics.
    fn wire_error(message: &str) -> Self {
        let kind = if message == "authentication failed" {
            ErrorKind::Registration
        } else {
            ErrorKind::Response
        };
        Self { kind, message: message.to_owned() }
    }
}

// ---------------------------------------------------------------------------
// Binary discovery (§8.1) + the launcher seam
// ---------------------------------------------------------------------------

/// The spawn seam: "start an `oc-pty` for `<root>/<id>`". Production
/// uses [`BinaryLauncher`]; hermetic tests inject fakes that serve the
/// wire codec in-process.
pub trait Launcher: Send + Sync {
    fn launch(&self, root: &Path, id: &str) -> Result<Box<dyn Spawned>, TransportError>;
}

/// A spawned daemon child: exit polling + the §4.4(b) SIGTERM path
/// (own child handle only).
pub trait Spawned: Send {
    fn has_exited(&self) -> bool;
    /// SIGTERM the child through the owned handle (§4.4(b)): a single
    /// strictly-positive pid the client itself spawned — never a pid
    /// read from disk, never a process group.
    fn terminate(&self);
}

/// §8.1 resolution order as a pure function (fixture-testable):
/// `OPENCODE_PTY_BIN` → `<exe-dir>/oc-pty` → PATH lookup of `oc-pty`.
#[must_use]
pub fn resolve_binary_path(
    env_value: Option<&str>,
    exe_dir: Option<&Path>,
    path_env: &str,
) -> Option<PathBuf> {
    if let Some(value) = env_value.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    if let Some(dir) = exe_dir {
        let candidate = dir.join("oc-pty");
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join("oc-pty");
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    path.is_file()
        && std::fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

/// The production launcher: resolves the binary once (reset on
/// failure, `ref: index.ts:131-135`) and spawns it detached with
/// `stdio: ignore` (`daemon.ts:229-244`).
pub struct BinaryLauncher {
    cache: Mutex<Option<PathBuf>>,
}

impl BinaryLauncher {
    #[must_use]
    pub fn new() -> Self {
        Self { cache: Mutex::new(None) }
    }
}

impl Default for BinaryLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl Launcher for BinaryLauncher {
    fn launch(&self, root: &Path, id: &str) -> Result<Box<dyn Spawned>, TransportError> {
        use std::os::unix::process::CommandExt as _;
        let binary = self.cache.lock().clone().or_else(|| {
            let resolved = resolve_binary_path(
                std::env::var("OPENCODE_PTY_BIN").ok().as_deref(),
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf))
                    .as_deref(),
                &std::env::var("PATH").unwrap_or_default(),
            );
            resolved
        });
        let Some(binary) = binary else {
            *self.cache.lock() = None;
            return Err(TransportError::new(
                ErrorKind::Spawn,
                "could not find the opencode-pty binary (OPENCODE_PTY_BIN, exe-sibling, PATH)",
            ));
        };
        let mut command = std::process::Command::new(&binary);
        command
            .args(["daemon", "--runtime-dir"])
            .arg(root)
            .arg("--name")
            .arg(id)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Detached analogue of `detached: true` (`daemon.ts:236`):
            // own process group, so terminal signals never reach it.
            .process_group(0);
        match command.spawn() {
            Ok(child) => {
                *self.cache.lock() = Some(binary);
                Ok(Box::new(ChildSpawned {
                    child: Mutex::new(Some(child)),
                    terminated: std::sync::atomic::AtomicBool::new(false),
                }))
            }
            Err(error) => {
                // Reset-on-failure (`index.ts:131-135`).
                *self.cache.lock() = None;
                Err(TransportError::new(
                    ErrorKind::Spawn,
                    format!("spawning {}: {error}", binary.display()),
                ))
            }
        }
    }
}

/// A real `oc-pty` child held by its own handle.
struct ChildSpawned {
    child: Mutex<Option<std::process::Child>>,
    terminated: std::sync::atomic::AtomicBool,
}

impl Spawned for ChildSpawned {
    fn has_exited(&self) -> bool {
        if self.terminated.load(std::sync::atomic::Ordering::SeqCst) {
            return true;
        }
        let mut child = self.child.lock();
        match child.as_mut().map(std::process::Child::try_wait) {
            Some(Ok(Some(_)) | Err(_)) | None => true,
            Some(Ok(None)) => false,
        }
    }

    fn terminate(&self) {
        if self.terminated.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let Some(mut child) = self.child.lock().take() else { return };
        let running = matches!(child.try_wait(), Ok(None));
        if running {
            // §4.4(b): SIGTERM through the owned child handle — a
            // single strictly-positive pid we spawned ourselves, never
            // a pid read from disk, never a process group.
            if let Ok(pid) = libc::pid_t::try_from(child.id()) {
                if pid > 0 {
                    // SAFETY: plain kill(2), integer args, one strictly
                    // positive pid — our own child.
                    #[allow(unsafe_code)] // §4.4(b): own spawned child handle
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
            }
        }
        // Reap without blocking the caller.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

// ---------------------------------------------------------------------------
// The transport
// ---------------------------------------------------------------------------

enum OwnerCommand {
    Bytes(Vec<u8>),
    Close,
}

/// The pending owner-connection exchange slot (owner ops are
/// serialized one at a time, like the ref's sequential `exchange`).
struct ExchangeSlot(Mutex<Option<oneshot::Sender<Result<WireResponse, TransportError>>>>);

#[derive(Clone)]
struct OwnerConn {
    epoch: u64,
    commands: mpsc::UnboundedSender<OwnerCommand>,
    exchange: Arc<ExchangeSlot>,
}

/// Everything the owner-connection reader task needs.
struct OwnerCtx {
    epoch: u64,
    exchange: Arc<ExchangeSlot>,
    shared: Arc<Shared>,
}

struct Inner {
    root: Option<PathBuf>,
    id: String,
    registration: Option<Registration>,
    owner: Option<OwnerConn>,
    next_epoch: u64,
}

struct Shared {
    bus: crate::bus::Bus,
    launcher: Arc<dyn Launcher>,
    inner: Mutex<Inner>,
}

/// The daemon client transport (§5.2).
#[derive(Clone)]
pub struct Transport {
    shared: Arc<Shared>,
    startup: Arc<tokio::sync::Mutex<()>>,
    spawn_deadline: Duration,
}

impl Transport {
    #[must_use]
    pub fn new(bus: crate::bus::Bus, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            shared: Arc::new(Shared {
                bus,
                launcher,
                inner: Mutex::new(Inner {
                    root: None,
                    id: super::new_random_token(),
                    registration: None,
                    owner: None,
                    next_epoch: 1,
                }),
            }),
            startup: Arc::new(tokio::sync::Mutex::new(())),
            spawn_deadline: DEFAULT_SPAWN_DEADLINE,
        }
    }

    /// Overrides the §8.2 startup deadline (tests).
    #[must_use]
    pub fn with_spawn_deadline(mut self, deadline: Duration) -> Self {
        self.spawn_deadline = deadline;
        self
    }

    /// Records the runtime root + a FRESH random identity (`§11.4`:
    /// every boot claims `<root>/<fresh-id>` — a client only ever looks
    /// in its own directory). No daemon contact happens here; spawn is
    /// `create`-only (§8.2).
    pub fn set_root(&self, root: &Path) {
        let mut inner = self.shared.inner.lock();
        if let Some(owner) = inner.owner.take() {
            let _ = owner.commands.send(OwnerCommand::Close);
        }
        inner.registration = None;
        inner.root = Some(root.to_path_buf());
        inner.id = super::new_random_token();
    }

    /// The current identity directory `<root>/<id>`.
    fn directory_of(inner: &Inner) -> Option<PathBuf> {
        inner.root.as_ref().map(|root| root.join(&inner.id))
    }

    /// The current identity directory (frontend `runtime_dir`).
    #[must_use]
    pub fn current_directory(&self) -> Option<PathBuf> {
        Self::directory_of(&self.shared.inner.lock())
    }

    // -- discovery (§4.4: read + connect-probe) ---------------------------

    /// `daemon.ts:201-226`: read `service.json`, check the protocol,
    /// ping + echo-check. Read failures are `connect` (the caller may
    /// spawn), everything else is a hard error.
    async fn discover(&self, directory: &Path) -> Result<Registration, TransportError> {
        let text = std::fs::read_to_string(directory.join("service.json"))
            .map_err(|_| TransportError::new(ErrorKind::Connect, "no opencode-pty registration"))?;
        let registration: Registration = serde_json::from_str(&text).map_err(|_| {
            TransportError::new(ErrorKind::Protocol, "opencode-pty registration unreadable")
        })?;
        if registration.protocol != PTY_PROTOCOL_VERSION {
            return Err(TransportError::new(
                ErrorKind::Protocol,
                wire::protocol_mismatch_message(registration.protocol),
            ));
        }
        match self.one_shot(&registration, &WireRequest::Ping).await {
            Ok(WireResponse::Pong { instance_id, pid, protocol }) => {
                if instance_id == registration.instance_id
                    && pid == registration.pid
                    && protocol == PTY_PROTOCOL_VERSION
                {
                    Ok(registration)
                } else {
                    Err(TransportError::new(
                        ErrorKind::Protocol,
                        "opencode-pty registration mismatch",
                    ))
                }
            }
            Ok(_) => {
                Err(TransportError::new(ErrorKind::Protocol, "opencode-pty registration mismatch"))
            }
            Err(error) => Err(error),
        }
    }

    // -- one-shot connections (§4.1) ---------------------------------------

    /// `daemon.ts:495-565`: connect, send, read exactly one response.
    /// Connect failures classify `connect` before dispatch, `response`
    /// after (`daemon.ts:542-546`).
    async fn one_shot(
        &self,
        registration: &Registration,
        request: &WireRequest,
    ) -> Result<WireResponse, TransportError> {
        let mut stream =
            tokio::time::timeout(REQUEST_TIMEOUT, UnixStream::connect(&registration.socket))
                .await
                .map_err(|_| {
                    TransportError::new(ErrorKind::Connect, "connecting to opencode-pty timed out")
                })?
                .map_err(|error| {
                    TransportError::new(
                        ErrorKind::Connect,
                        format!("could not connect to opencode-pty: {error}"),
                    )
                })?;
        let payload = serde_json::to_vec(&wire::Envelope {
            token: registration.token.clone(),
            request: request.clone(),
        })
        .map_err(|_| TransportError::new(ErrorKind::Protocol, "encoding opencode-pty request"))?;
        let frame = wire::encode_frame(&payload).map_err(|_| {
            TransportError::new(ErrorKind::Protocol, "encoding opencode-pty request")
        })?;
        let dispatched = stream.write_all(&frame).await.is_ok();
        let _ = stream.flush().await;
        if !dispatched {
            return Err(TransportError::new(
                ErrorKind::Response,
                "opencode-pty closed before the request was written",
            ));
        }
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut decoder = FrameDecoder::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::new(
                    ErrorKind::Response,
                    "opencode-pty request timed out",
                ));
            }
            let read = tokio::time::timeout(remaining, stream.read(&mut chunk)).await;
            let read = match read {
                Err(_) => {
                    return Err(TransportError::new(
                        ErrorKind::Response,
                        "opencode-pty request timed out",
                    ));
                }
                Ok(Err(error)) => {
                    return Err(TransportError::new(
                        ErrorKind::Response,
                        format!("opencode-pty connection error: {error}"),
                    ));
                }
                Ok(Ok(0)) => {
                    return Err(TransportError::new(
                        ErrorKind::Response,
                        "opencode-pty closed without response",
                    ));
                }
                Ok(Ok(read)) => read,
            };
            let frames = decoder.feed(&chunk[..read]).map_err(|_| {
                TransportError::new(ErrorKind::Protocol, "opencode-pty framing error")
            })?;
            let Some(frame) = frames.into_iter().next() else { continue };
            if frame.first() == Some(&0x00) {
                return Err(TransportError::new(
                    ErrorKind::Protocol,
                    "unexpected binary frame from opencode-pty",
                ));
            }
            let response: WireResponse = serde_json::from_slice(&frame).map_err(|_| {
                TransportError::new(ErrorKind::Protocol, "opencode-pty response undecodable")
            })?;
            return match response {
                WireResponse::Error { message } => Err(TransportError::wire_error(&message)),
                other => Ok(other),
            };
        }
    }

    // -- the owner connection (§3.1, §6) -----------------------------------

    /// `daemon.ts:446-481` (`openOwner`): connect, send `own`, await
    /// `owned`; then the reader task demuxes exchange responses /
    /// `bus_event` / `owner_superseded` until the connection ends.
    async fn open_owner(
        &self,
        registration: &Registration,
        ticket: Option<&str>,
        directory: Option<&Path>,
    ) -> Result<OwnerConn, TransportError> {
        let stream =
            tokio::time::timeout(REQUEST_TIMEOUT, UnixStream::connect(&registration.socket))
                .await
                .map_err(|_| {
                    TransportError::new(ErrorKind::Connect, "PTY ownership claim timed out")
                })?
                .map_err(|error| {
                    TransportError::new(
                        ErrorKind::Connect,
                        format!("could not claim opencode-pty: {error}"),
                    )
                })?;
        let (mut read_half, write_half) = stream.into_split();
        let (commands, command_rx) = mpsc::unbounded_channel::<OwnerCommand>();
        tokio::spawn(conn_writer(write_half, command_rx));
        let request = WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: ticket.map(str::to_owned),
            directory: directory.map(|path| path.to_string_lossy().into_owned()),
        };
        let payload =
            serde_json::to_vec(&wire::Envelope { token: registration.token.clone(), request })
                .map_err(|_| {
                    TransportError::new(ErrorKind::Protocol, "encoding opencode-pty claim")
                })?;
        let frame = wire::encode_frame(&payload)
            .map_err(|_| TransportError::new(ErrorKind::Protocol, "encoding opencode-pty claim"))?;
        let _ = commands.send(OwnerCommand::Bytes(frame));

        // Await `owned` (or the rejection) with the claim timeout.
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let mut decoder = FrameDecoder::new();
        let mut chunk = [0u8; 16 * 1024];
        let mut claimed = false;
        while !claimed {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let _ = commands.send(OwnerCommand::Close);
                return Err(TransportError::new(
                    ErrorKind::Connect,
                    "PTY ownership claim timed out",
                ));
            }
            let read = tokio::time::timeout(remaining, read_half.read(&mut chunk)).await;
            let read = match read {
                Err(_) => {
                    let _ = commands.send(OwnerCommand::Close);
                    return Err(TransportError::new(
                        ErrorKind::Connect,
                        "PTY ownership claim timed out",
                    ));
                }
                Ok(Err(_) | Ok(0)) => {
                    let _ = commands.send(OwnerCommand::Close);
                    return Err(TransportError::new(
                        ErrorKind::Connect,
                        "PTY daemon closed its ownership connection",
                    ));
                }
                Ok(Ok(read)) => read,
            };
            let frames = decoder.feed(&chunk[..read]).map_err(|_| {
                TransportError::new(ErrorKind::Protocol, "opencode-pty framing error")
            })?;
            if let Some(frame) = frames.into_iter().next() {
                if frame.first() == Some(&0x00) {
                    let _ = commands.send(OwnerCommand::Close);
                    return Err(TransportError::new(
                        ErrorKind::Protocol,
                        "unexpected binary frame from opencode-pty",
                    ));
                }
                match serde_json::from_slice::<WireResponse>(&frame) {
                    Ok(WireResponse::Owned) => {
                        claimed = true;
                    }
                    Ok(WireResponse::Error { message }) => {
                        let _ = commands.send(OwnerCommand::Close);
                        let kind = match message.as_str() {
                            "authentication failed" | "daemon is owned" => ErrorKind::Registration,
                            other if other.starts_with("PTY restart") => ErrorKind::Registration,
                            _ => ErrorKind::Response,
                        };
                        return Err(TransportError { kind, message });
                    }
                    Ok(_) => {
                        let _ = commands.send(OwnerCommand::Close);
                        return Err(TransportError::new(
                            ErrorKind::Protocol,
                            "PTY daemon does not support server ownership",
                        ));
                    }
                    Err(_) => {
                        let _ = commands.send(OwnerCommand::Close);
                        return Err(TransportError::new(
                            ErrorKind::Protocol,
                            "opencode-pty response undecodable",
                        ));
                    }
                }
            }
        }
        let epoch = {
            let mut inner = self.shared.inner.lock();
            let epoch = inner.next_epoch;
            inner.next_epoch += 1;
            epoch
        };
        let ctx = Arc::new(OwnerCtx {
            epoch,
            exchange: Arc::new(ExchangeSlot(Mutex::new(None))),
            shared: Arc::clone(&self.shared),
        });
        tokio::spawn(owner_reader(read_half, Arc::clone(&ctx)));
        Ok(OwnerConn { epoch, commands, exchange: Arc::clone(&ctx.exchange) })
    }

    /// `claim`: open the owner connection + cache the registration.
    async fn claim(
        &self,
        registration: &Registration,
        ticket: Option<&str>,
        directory: Option<&Path>,
    ) -> Result<(), TransportError> {
        let owner = self.open_owner(registration, ticket, directory).await?;
        let mut inner = self.shared.inner.lock();
        if let Some(previous) = inner.owner.take() {
            let _ = previous.commands.send(OwnerCommand::Close);
        }
        inner.owner = Some(owner);
        inner.registration = Some(registration.clone());
        Ok(())
    }

    /// An owner-connection exchange (§4.3 owner-only ops:
    /// `prepare_handoff`, `restore`).
    async fn owner_exchange(&self, request: WireRequest) -> Result<WireResponse, TransportError> {
        let owner = {
            let inner = self.shared.inner.lock();
            inner.owner.clone()
        };
        let Some(owner) = owner else {
            return Err(TransportError::new(
                ErrorKind::Connect,
                "no opencode-pty ownership connection",
            ));
        };
        let (tx, rx) = oneshot::channel();
        {
            let mut slot = owner.exchange.0.lock();
            if slot.is_some() {
                return Err(TransportError::new(
                    ErrorKind::Response,
                    "another opencode-pty ownership request is in flight",
                ));
            }
            *slot = Some(tx);
        }
        let payload = serde_json::to_vec(&wire::Envelope {
            token: self
                .shared
                .inner
                .lock()
                .registration
                .as_ref()
                .map(|registration| registration.token.clone())
                .unwrap_or_default(),
            request,
        })
        .map_err(|_| TransportError::new(ErrorKind::Protocol, "encoding opencode-pty request"))?;
        let frame = wire::encode_frame(&payload).map_err(|_| {
            TransportError::new(ErrorKind::Protocol, "encoding opencode-pty request")
        })?;
        if owner.commands.send(OwnerCommand::Bytes(frame)).is_err() {
            if let Some(tx) = owner.exchange.0.lock().take() {
                let _ = tx.send(Err(TransportError::new(
                    ErrorKind::Response,
                    "PTY daemon closed its ownership connection",
                )));
            }
        }
        let response = tokio::time::timeout(REQUEST_TIMEOUT, rx).await;
        match response {
            Ok(Ok(Ok(WireResponse::Error { message }))) => {
                Err(TransportError::wire_error(&message))
            }
            Ok(Ok(Ok(other))) => Ok(other),
            Ok(Ok(Err(error))) => Err(error),
            Ok(Err(_)) => Err(TransportError::new(
                ErrorKind::Response,
                "PTY daemon closed its ownership connection",
            )),
            Err(_) => {
                owner.exchange.0.lock().take();
                Err(TransportError::new(ErrorKind::Response, "opencode-pty request timed out"))
            }
        }
    }

    // -- connect + spawn ladder (§8) ---------------------------------------

    /// `daemon.ts:296-312` (`connect`): cached registration, else
    /// discover (+ spawn when `should_start`) + claim.
    async fn connect_locked(&self, should_start: bool) -> Result<Registration, TransportError> {
        if let Some(registration) = self.shared.inner.lock().registration.clone() {
            return Ok(registration);
        }
        let Some(directory) = Self::directory_of(&self.shared.inner.lock()) else {
            return Err(TransportError::new(
                ErrorKind::Connect,
                "no PTY runtime directory configured",
            ));
        };
        match self.discover(&directory).await {
            Ok(registration) => {
                let needs_claim = self.shared.inner.lock().owner.is_none();
                if needs_claim {
                    self.claim(&registration, None, None).await?;
                } else {
                    self.shared.inner.lock().registration = Some(registration.clone());
                }
                Ok(registration)
            }
            // §8.2: only create starts daemons; connect-kind discover
            // failures on other ops surface (soft-empty is the
            // requestIfRunning layer's job).
            Err(error) if should_start && error.kind == ErrorKind::Connect => {
                self.start_locked().await
            }
            Err(error) => Err(error),
        }
    }

    async fn connect(&self, should_start: bool) -> Result<Registration, TransportError> {
        if self.shared.inner.lock().registration.is_some() {
            return self.connect_locked(should_start).await;
        }
        let _guard = self.startup.lock().await;
        self.connect_locked(should_start).await
    }

    /// One spawn attempt: launch, poll discover every 50 ms until the
    /// deadline, claim on success; SIGTERM the child on any failure
    /// (`daemon.ts:228-273`).
    async fn spawn_once(&self) -> Result<Registration, TransportError> {
        let (root, id, directory) = {
            let inner = self.shared.inner.lock();
            let Some(root) = inner.root.clone() else {
                return Err(TransportError::new(
                    ErrorKind::Connect,
                    "no PTY runtime directory configured",
                ));
            };
            (root.clone(), inner.id.clone(), root.join(&inner.id))
        };
        let child = self.shared.launcher.launch(&root, &id)?;
        let deadline = Instant::now() + self.spawn_deadline;
        let mut last: Option<TransportError> = None;
        let outcome = loop {
            if Instant::now() >= deadline {
                break Err(last.unwrap_or_else(|| {
                    TransportError::new(ErrorKind::Connect, "opencode-pty did not become ready")
                }));
            }
            if child.has_exited() {
                break Err(TransportError::new(
                    ErrorKind::Connect,
                    "opencode-pty exited during startup",
                ));
            }
            match self.discover(&directory).await {
                Ok(registration) => match self.claim(&registration, None, None).await {
                    Ok(()) => break Ok(registration),
                    Err(error) => break Err(error),
                },
                Err(error) => last = Some(error),
            }
            tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
        };
        if outcome.is_err() {
            // §8.2 deadline/failure ⇒ SIGTERM the child we spawned.
            child.terminate();
        }
        outcome
    }

    /// `abandon` (`daemon.ts:275-292`): drop the owner connection and
    /// cached registration, roll a fresh random identity.
    fn abandon(&self) {
        let mut inner = self.shared.inner.lock();
        if let Some(owner) = inner.owner.take() {
            let _ = owner.commands.send(OwnerCommand::Close);
        }
        inner.registration = None;
        inner.id = super::new_random_token();
    }

    /// `start` (`daemon.ts:275-292`): one spawn attempt, then — on
    /// failure — abandon to a FRESH directory and retry once.
    async fn start_locked(&self) -> Result<Registration, TransportError> {
        match self.spawn_once().await {
            Ok(registration) => Ok(registration),
            Err(error) => {
                eprintln!("Starting opencode-pty in a fresh runtime directory ({})", error.message);
                self.abandon();
                self.spawn_once().await
            }
        }
    }

    // -- the request ladder (§5.2) -----------------------------------------

    async fn attempt(
        &self,
        request: &WireRequest,
        should_start: bool,
    ) -> Result<WireResponse, TransportError> {
        let registration = self.connect(should_start).await?;
        match self.one_shot(&registration, request).await {
            Err(error) if error.kind == ErrorKind::Registration => {
                // Drop the cached registration so the retry re-discovers.
                let mut inner = self.shared.inner.lock();
                if inner.registration.as_ref() == Some(&registration) {
                    inner.registration = None;
                }
                drop(inner);
                Err(error)
            }
            other => other,
        }
    }

    /// `daemon.ts:313-333` (`request`): `registration` errors retry
    /// once after re-discovery; `connect` errors on start-capable ops
    /// respawn once.
    pub async fn request(
        &self,
        request: WireRequest,
        should_start: bool,
    ) -> Result<WireResponse, TransportError> {
        match self.attempt(&request, should_start).await {
            Err(error) if error.kind == ErrorKind::Registration => {
                self.attempt(&request, should_start).await
            }
            Err(error) if error.kind == ErrorKind::Connect => {
                self.shared.inner.lock().registration = None;
                if !should_start {
                    return Err(error);
                }
                self.attempt(&request, true).await
            }
            other => other,
        }
    }

    /// `requestIfRunning`: absent daemon ⇒ `Ok(None)` (soft-empty,
    /// `index.ts:143-145`).
    pub async fn request_if_running(
        &self,
        request: WireRequest,
    ) -> Result<Option<WireResponse>, TransportError> {
        match self.request(request, false).await {
            Ok(response) => Ok(Some(response)),
            Err(error) if error.kind == ErrorKind::Connect => Ok(None),
            Err(error) => Err(error),
        }
    }

    // -- shutdown / handoff -------------------------------------------------

    /// `daemon.ts:340-354`: TOKEN-level one-shot `shutdown` (absent ⇒
    /// Ok), then wait for discover to fail ≤5s.
    pub async fn shutdown(&self) -> Result<(), TransportError> {
        self.request_if_running(WireRequest::Shutdown).await?;
        {
            let mut inner = self.shared.inner.lock();
            if let Some(owner) = inner.owner.take() {
                let _ = owner.commands.send(OwnerCommand::Close);
            }
            inner.registration = None;
        }
        let Some(directory) = self.current_directory() else { return Ok(()) };
        let deadline = Instant::now() + SHUTDOWN_DRAIN_TIMEOUT;
        while Instant::now() < deadline {
            if self.discover(&directory).await.is_err() {
                return Ok(());
            }
            tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
        }
        Err(TransportError::new(ErrorKind::Connect, "opencode-pty did not stop"))
    }

    /// `prepare_handoff` over the owner connection; `None` when no
    /// daemon is running (v2's transport before it first connects —
    /// the machine's `handoff() → None` parity).
    pub async fn handoff(&self) -> Result<Option<Handoff>, TransportError> {
        let _guard = self.startup.lock().await;
        let (directory, registration) = {
            let inner = self.shared.inner.lock();
            match (Self::directory_of(&inner), inner.registration.clone()) {
                (Some(directory), Some(registration)) => (directory, registration),
                _ => return Ok(None),
            }
        };
        match self.owner_exchange(WireRequest::PrepareHandoff).await {
            Ok(WireResponse::Handoff { ticket, expires_at }) => Ok(Some(Handoff {
                directory: directory.to_string_lossy().into_owned(),
                instance_id: registration.instance_id,
                ticket,
                expires_at,
            })),
            Ok(_) => Err(TransportError::new(
                ErrorKind::Protocol,
                "expected opencode-pty handoff ticket",
            )),
            Err(error) => Err(error),
        }
    }

    // -- subscribe (§3, §3.2) ----------------------------------------------

    /// Opens a subscribe connection: `attached` (inline replay iff
    /// ≤1 MiB), then — for the chunked arm — binary frames reassembled
    /// by absolute offset until `replay_complete` marks the transfer
    /// done (client-deferred completion, §3.2). The returned stream
    /// forwards live frames once activated.
    pub async fn subscribe(
        &self,
        id: u64,
        offset: u64,
        attachment_id: &str,
        role: WireRole,
        takeover: bool,
    ) -> Result<WireStream, TransportError> {
        let request = WireRequest::Subscribe {
            id,
            offset,
            attachment_id: attachment_id.to_owned(),
            role,
            takeover,
        };
        match self.subscribe_attempt(&request).await {
            Err(error) if error.kind == ErrorKind::Registration => {
                self.subscribe_attempt(&request).await
            }
            other => other,
        }
    }

    async fn subscribe_attempt(&self, request: &WireRequest) -> Result<WireStream, TransportError> {
        let registration = self.connect(false).await?;
        let mut stream =
            tokio::time::timeout(REQUEST_TIMEOUT, UnixStream::connect(&registration.socket))
                .await
                .map_err(|_| {
                    TransportError::new(ErrorKind::Connect, "connecting to opencode-pty timed out")
                })?
                .map_err(|error| {
                    TransportError::new(ErrorKind::Connect, format!("could not attach: {error}"))
                })?;
        let payload = serde_json::to_vec(&wire::Envelope {
            token: registration.token.clone(),
            request: request.clone(),
        })
        .map_err(|_| TransportError::new(ErrorKind::Protocol, "encoding opencode-pty attach"))?;
        let frame = wire::encode_frame(&payload).map_err(|_| {
            TransportError::new(ErrorKind::Protocol, "encoding opencode-pty attach")
        })?;
        stream.write_all(&frame).await.map_err(|_| {
            TransportError::new(ErrorKind::Response, "opencode-pty closed before attach")
        })?;
        let _ = stream.flush().await;
        // The subscribe connection never sends again, but the write
        // half is HELD (not dropped): the daemon must not observe EOF
        // while frames are still queued for us (AF_UNIX close discards
        // the peer's pending receive queue).
        let (read_half, write_half) = stream.into_split();
        let mut reader = FrameReader {
            stream: read_half,
            decoder: FrameDecoder::new(),
            pending: std::collections::VecDeque::new(),
            _keepalive: write_half,
        };

        // First frame: `attached` (or the wire error).
        let attached = match tokio::time::timeout(REQUEST_TIMEOUT, reader.next_frame()).await {
            Err(_) => {
                return Err(TransportError::new(
                    ErrorKind::Response,
                    "opencode-pty attach timed out",
                ));
            }
            Ok(Err(error)) => return Err(error),
            Ok(Ok(frame)) => frame,
        };
        let attached = if attached.first() == Some(&0x00) {
            return Err(TransportError::new(
                ErrorKind::Protocol,
                "unexpected binary frame from opencode-pty",
            ));
        } else {
            match serde_json::from_slice::<WireResponse>(&attached) {
                Ok(WireResponse::Error { message }) => {
                    return Err(TransportError::wire_error(&message))
                }
                Ok(WireResponse::Attached { .. }) => {
                    serde_json::from_slice(&attached).map_err(|_| {
                        TransportError::new(
                            ErrorKind::Protocol,
                            "opencode-pty response undecodable",
                        )
                    })?
                }
                _ => {
                    return Err(TransportError::new(
                        ErrorKind::Protocol,
                        "unexpected opencode-pty response",
                    ));
                }
            }
        };
        let WireResponse::Attached {
            terminal,
            role,
            generation,
            requested_offset,
            available_offset,
            end_offset,
            truncated,
            replay_base64,
            replay_chunked,
            replay_length,
        } = attached
        else {
            unreachable!("matched Attached above");
        };

        let info = info_from_wire(&terminal)
            .map_err(|message| TransportError::new(ErrorKind::Protocol, message))?;
        let replay = Replay {
            requested_offset: usize::try_from(requested_offset).unwrap_or(usize::MAX),
            available_offset: usize::try_from(available_offset).unwrap_or(usize::MAX),
            end_offset: usize::try_from(end_offset).unwrap_or(usize::MAX),
            truncated,
        };

        let mut pending = Vec::new();
        let data = if let Some(base64) = replay_base64 {
            // Inline arm (§3.2): ≤1 MiB rides the attached frame.
            super::base64_decode(&base64).ok_or_else(|| {
                TransportError::new(ErrorKind::Protocol, "opencode-pty replay is not valid base64")
            })?
        } else if replay_chunked == Some(true) && replay_length.is_some() {
            // Chunked arm: binary frames reassemble by absolute offset;
            // completion is deferred until contiguous up to end_offset.
            let length = replay_length.unwrap_or(0);
            let base = end_offset.saturating_sub(length);
            let mut assembled_end = base;
            let mut data = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
            let deadline = Instant::now() + REPLAY_ASSEMBLY_TIMEOUT;
            loop {
                if Instant::now() >= deadline {
                    return Err(TransportError::new(
                        ErrorKind::Response,
                        "opencode-pty replay transfer stalled",
                    ));
                }
                let frame = match tokio::time::timeout(REPLAY_ASSEMBLY_TIMEOUT, reader.next_frame())
                    .await
                {
                    Err(_) => {
                        return Err(TransportError::new(
                            ErrorKind::Response,
                            "opencode-pty replay transfer stalled",
                        ));
                    }
                    Ok(Err(error)) => return Err(error),
                    Ok(Ok(frame)) => frame,
                };
                if frame.first() == Some(&0x00) {
                    let parsed = wire::parse_output_frame(&frame).map_err(|_| {
                        TransportError::new(
                            ErrorKind::Protocol,
                            "invalid opencode-pty output frame",
                        )
                    })?;
                    match parsed.start.cmp(&assembled_end) {
                        std::cmp::Ordering::Equal => {
                            data.extend_from_slice(&parsed.data);
                            assembled_end = parsed.end;
                        }
                        std::cmp::Ordering::Greater => {
                            // Output that accumulated during the transfer
                            // (higher offsets) is LIVE output: defer it.
                            pending.push(StreamEvent::Output(parsed.data));
                        }
                        std::cmp::Ordering::Less => {
                            return Err(TransportError::new(
                                ErrorKind::Protocol,
                                "opencode-pty replay offsets are not contiguous",
                            ));
                        }
                    }
                    continue;
                }
                match serde_json::from_slice::<WireResponse>(&frame) {
                    Ok(WireResponse::ReplayComplete { end_offset: done }) => {
                        if done == assembled_end {
                            break;
                        }
                        // The marker must not precede the last chunk.
                        return Err(TransportError::new(
                            ErrorKind::Protocol,
                            "opencode-pty replay completed before the transfer was contiguous",
                        ));
                    }
                    Ok(_) => {
                        return Err(TransportError::new(
                            ErrorKind::Protocol,
                            "unexpected opencode-pty frame during replay",
                        ));
                    }
                    Err(_) => {
                        return Err(TransportError::new(
                            ErrorKind::Protocol,
                            "opencode-pty response undecodable",
                        ));
                    }
                }
            }
            data
        } else {
            Vec::new()
        };

        Ok(WireStream {
            info,
            role: Role::from(role),
            generation,
            replay,
            data,
            parts: Some(PumpParts {
                reader,
                pending,
                on_event: Box::new(|_| {}),
                on_end: Box::new(|| {}),
            }),
        })
    }

    // -- the adoption ladder (§6) ------------------------------------------

    /// The §6 ladder with both arms: A (live daemon — `own` with the
    /// single-use ticket; terminals + children stay alive, R5) and B
    /// (dead daemon — `state.json` restore through a freshly spawned
    /// daemon). Every discard logs the v2 warning, best-effort shuts a
    /// FOUND daemon down and abandons to a fresh identity — boot never
    /// fails (`daemon.ts:403-428`).
    pub async fn adopt(&self, inherited: &Handoff) -> Adoption {
        // 1. Expiry (`daemon.ts:406-407`).
        if inherited.expires_at <= unix_ms_now() {
            return self
                .discard(AdoptionError::expired(), &PathBuf::from(&inherited.directory))
                .await;
        }
        let directory = PathBuf::from(&inherited.directory);
        // 2. Registration readable (`daemon.ts:203-205`).
        let Some(registration_text) = std::fs::read_to_string(directory.join("service.json")).ok()
        else {
            return self.discard(AdoptionError::unreachable(), &directory).await;
        };
        let Ok(registration) = serde_json::from_str::<Registration>(&registration_text) else {
            return self.discard(AdoptionError::unreachable(), &directory).await;
        };
        // 3. Protocol (`daemon.ts:209-216`) — the protocol-mismatch
        //    emitter lives HERE (client side); the message is the I7
        //    pinned shape.
        if registration.protocol != PTY_PROTOCOL_VERSION {
            eprintln!(
                "Discarding PTY restart handoff; persistent terminals will start fresh ({})",
                wire::protocol_mismatch_message(registration.protocol)
            );
            self.abandon();
            return Adoption::Discarded { reason: AdoptionError::protocol() };
        }
        // 4. Instance (`daemon.ts:411-412`).
        if registration.instance_id != inherited.instance_id {
            return self.discard(AdoptionError::changed(), &directory).await;
        }

        // The client root this transport owns (falls back to the
        // inherited directory's parent when unset).
        let root = match self.current_directory() {
            Some(current) => current.parent().map_or_else(
                || directory.parent().map_or_else(|| directory.clone(), Path::to_path_buf),
                Path::to_path_buf,
            ),
            None => directory.parent().map_or_else(|| directory.clone(), Path::to_path_buf),
        };
        let id = directory.file_name().map_or_else(
            || super::id_from_instance(&registration.instance_id),
            |name| name.to_string_lossy().into_owned(),
        );
        let target = root.join(&id);

        match self.discover(&directory).await {
            // 5. ARM A — the live daemon (R4).
            Ok(found) => {
                if target != directory {
                    // The client migrates FIRST, inode-preserving
                    // (`daemon.ts:413-415`, `:435-444`); the claim then
                    // carries the new path so the daemon re-anchors.
                    if migrate_runtime_files(&directory, &target).is_err() {
                        return self.discard(AdoptionError::unreachable(), &directory).await;
                    }
                }
                match self.claim(&found, Some(&inherited.ticket), Some(&target)).await {
                    Ok(()) => {
                        // The adopted identity is the live one now.
                        {
                            let mut inner = self.shared.inner.lock();
                            inner.root = Some(root);
                            inner.id = id;
                        }
                        let restored = match self.request_if_running(WireRequest::List).await {
                            Ok(Some(WireResponse::Terminals { terminals })) => terminals.len(),
                            _ => 0,
                        };
                        Adoption::Adopted {
                            restored,
                            directory: target.to_string_lossy().into_owned(),
                        }
                    }
                    Err(error) => {
                        let reason = match error.message.as_str() {
                            "PTY restart handoff ticket rejected" => {
                                AdoptionError::invalid_ticket()
                            }
                            "daemon is owned" => AdoptionError::owned(),
                            "PTY restart daemon changed" => AdoptionError::changed(),
                            _ => AdoptionError::unreachable(),
                        };
                        self.discard(reason, &directory).await
                    }
                }
            }
            // 6. ARM B — the daemon died (ping fails; the lock is free).
            Err(error) if error.kind == ErrorKind::Connect => {
                let state_path = directory.join("state.json");
                let Some(state_text) = std::fs::read_to_string(&state_path).ok() else {
                    return self.discard(AdoptionError::no_state(), &directory).await;
                };
                let Ok(state) = serde_json::from_str::<HandoffState>(&state_text) else {
                    return self.discard(AdoptionError::no_state(), &directory).await;
                };
                if state.instance_id != inherited.instance_id || state.ticket != inherited.ticket {
                    return self.discard(AdoptionError::invalid_ticket(), &directory).await;
                }
                if lock_held(&directory.join("service.lock")) {
                    // Daemon alive but socket-dead = unreachable owner.
                    return self.discard(AdoptionError::owned(), &directory).await;
                }
                // Migrate the runtime files FIRST (inode-preserving —
                // the moved lock keeps its exclusion history), then
                // spawn into `<root>/<id>` and restore over the owner
                // connection (§6 arm B).
                if target != directory && migrate_runtime_files(&directory, &target).is_err() {
                    return self.discard(AdoptionError::unreachable(), &directory).await;
                }
                {
                    let mut inner = self.shared.inner.lock();
                    inner.root = Some(root);
                    inner.id = id;
                }
                if self.spawn_once().await.is_err() {
                    return self.discard(AdoptionError::unreachable(), &target).await;
                }
                match self.owner_exchange(WireRequest::Restore { state: state.clone() }).await {
                    Ok(_) => {
                        // Consume the snapshot (single-use).
                        let _ = std::fs::remove_file(target.join("state.json"));
                        Adoption::Adopted {
                            restored: state.terminals.len(),
                            directory: target.to_string_lossy().into_owned(),
                        }
                    }
                    Err(_) => self.discard(AdoptionError::unreachable(), &target).await,
                }
            }
            // Ping-echo / protocol-level failures from the probe.
            Err(error) => {
                eprintln!(
                    "Discarding PTY restart handoff; persistent terminals will start fresh ({})",
                    error.message
                );
                self.abandon();
                Adoption::Discarded { reason: AdoptionError::protocol() }
            }
        }
    }

    /// ANY discard (§6): the v2 warning, best-effort `shutdown` of a
    /// FOUND daemon (TOKEN-level, §4.3 — `daemon.ts:423`), abandon to a
    /// fresh identity. Boot stays green.
    async fn discard(&self, reason: AdoptionError, directory: &Path) -> Adoption {
        eprintln!(
            "Discarding PTY restart handoff; persistent terminals will start fresh ({reason})"
        );
        if let Ok(found) = self.discover(directory).await {
            let _ = self.one_shot(&found, &WireRequest::Shutdown).await;
        }
        self.abandon();
        Adoption::Discarded { reason }
    }
}

/// The subscribe stream after `attached` + replay reassembly: live
/// frames flow once [`WireStream::activate`] starts the pump.
pub struct WireStream {
    pub info: Info,
    pub role: Role,
    pub generation: u64,
    pub replay: Replay,
    pub data: Vec<u8>,
    parts: Option<PumpParts>,
}

struct PumpParts {
    reader: FrameReader,
    pending: Vec<StreamEvent>,
    on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
    on_end: Box<dyn FnOnce() + Send + Sync>,
}

/// The §3 stream pump: activated once (`ref: subscribePromise.activate`
/// — live delivery starts only after the replay contract was applied),
/// detached by closing the connection (the daemon-side detach).
pub struct WirePump {
    parts: parking_lot::Mutex<Option<PumpParts>>,
    cancel: Arc<tokio::sync::Notify>,
    activated: std::sync::atomic::AtomicBool,
    detached: std::sync::atomic::AtomicBool,
}

impl WireStream {
    /// Wraps the stream's reader + the attachment callbacks into the
    /// activate/detach handle (the frontend's `Attachment`). The
    /// stream's mapped fields (`info`/`role`/`generation`/`replay`/
    /// `data`) stay with the caller — destructure them first.
    #[must_use]
    pub fn into_pump(
        self,
        on_event: Box<dyn Fn(StreamEvent) + Send + Sync>,
        on_end: Box<dyn FnOnce() + Send + Sync>,
    ) -> (Info, Role, u64, Replay, Vec<u8>, WirePump) {
        let (info, role, generation, replay, data, parts) = match self {
            Self { info, role, generation, replay, data, parts } => {
                (info, role, generation, replay, data, parts)
            }
        };
        let mut parts = parts;
        if let Some(state) = parts.as_mut() {
            state.on_event = on_event;
            state.on_end = on_end;
        }
        let pump = WirePump {
            parts: parking_lot::Mutex::new(parts),
            cancel: Arc::new(tokio::sync::Notify::new()),
            activated: std::sync::atomic::AtomicBool::new(false),
            detached: std::sync::atomic::AtomicBool::new(false),
        };
        (info, role, generation, replay, data, pump)
    }
}

impl WirePump {
    /// Starts live delivery: pending events (output that accumulated
    /// during a chunked replay transfer) flush first.
    pub fn activate(&self) {
        if self.detached.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if self.activated.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let Some(parts) = self.parts.lock().take() else { return };
        let on_event: Arc<dyn Fn(StreamEvent) + Send + Sync> = parts.on_event.into();
        let cancel = Arc::clone(&self.cancel);
        tokio::spawn(async move {
            pump(parts.reader, parts.pending, on_event, parts.on_end, cancel).await;
        });
    }

    /// Stops delivery and closes the connection. Before activation the
    /// socket simply drops (no `on_end` — the in-process machine's
    /// detach never fires it); after activation the pump wakes, ends
    /// and fires `on_end`.
    pub fn detach(&self) {
        if self.detached.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        self.parts.lock().take();
        self.cancel.notify_one();
    }
}

struct FrameReader {
    stream: tokio::net::unix::OwnedReadHalf,
    decoder: FrameDecoder,
    /// Frames already decoded but not yet consumed (one chunk may
    /// carry several frames — §3.2 replay chunks + live output).
    pending: std::collections::VecDeque<Vec<u8>>,
    /// Held open for the stream's lifetime (never written again):
    /// dropping it would EOF the daemon, whose close would discard
    /// undelivered frames from this connection's receive queue
    /// (`AF_UNIX` close purges the peer's queue — unlike TCP).
    _keepalive: tokio::net::unix::OwnedWriteHalf,
}

impl FrameReader {
    async fn next_frame(&mut self) -> Result<Vec<u8>, TransportError> {
        loop {
            if let Some(frame) = self.pending.pop_front() {
                return Ok(frame);
            }
            let mut chunk = [0u8; 16 * 1024];
            let read = self.stream.read(&mut chunk).await.map_err(|error| {
                TransportError::new(
                    ErrorKind::Response,
                    format!("opencode-pty connection error: {error}"),
                )
            })?;
            if read == 0 {
                return Err(TransportError::new(
                    ErrorKind::Response,
                    "opencode-pty closed the connection",
                ));
            }
            let frames = self.decoder.feed(&chunk[..read]).map_err(|_| {
                TransportError::new(ErrorKind::Protocol, "opencode-pty framing error")
            })?;
            self.pending.extend(frames);
        }
    }
}

/// Delivers stream frames until `exited`, detach or EOF
/// (`daemon.ts:588-624`).
async fn pump(
    mut reader: FrameReader,
    pending: Vec<StreamEvent>,
    on_event: Arc<dyn Fn(StreamEvent) + Send + Sync>,
    on_end: Box<dyn FnOnce() + Send + Sync>,
    cancel: Arc<tokio::sync::Notify>,
) {
    for event in pending {
        on_event(event);
    }
    loop {
        let frame = tokio::select! {
            frame = reader.next_frame() => match frame {
                Ok(frame) => frame,
                Err(e) => { eprintln!("[pump] read error: {e:?}"); break }
            },
            () = cancel.notified() => { eprintln!("[pump] cancelled"); break },
        };
        eprintln!(
            "[pump] frame: {} bytes, first={:02x}",
            frame.len(),
            frame.first().copied().unwrap_or(0)
        );
        if frame.first() == Some(&0x00) {
            if let Ok(parsed) = wire::parse_output_frame(&frame) {
                on_event(StreamEvent::Output(parsed.data));
            }
            continue;
        }
        let Ok(event) = serde_json::from_slice::<WireResponse>(&frame) else { continue };
        match event {
            WireResponse::Resized { cols, rows, generation, checkpoint_base64 } => {
                on_event(StreamEvent::Resized {
                    cols,
                    rows,
                    generation,
                    checkpoint: super::base64_decode(&checkpoint_base64).unwrap_or_default(),
                });
            }
            WireResponse::Exited { exit_code, .. } => {
                on_event(StreamEvent::Exited { exit_code });
                break;
            }
            WireResponse::ControllerChanged { attachment_id, generation } => {
                on_event(StreamEvent::ControllerChanged { attachment_id, generation });
            }
            _ => {}
        }
    }
    on_end();
}

/// Writes owner-connection commands until the channel closes or the
/// ordered `Close` arrives (the §6 `owner_superseded`-before-close and
/// §3 `exited`-before-EOF orderings rely on this).
async fn conn_writer(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<OwnerCommand>,
) {
    while let Some(command) = rx.recv().await {
        match command {
            OwnerCommand::Bytes(bytes) => {
                if write_half.write_all(&bytes).await.is_err() {
                    break;
                }
                if write_half.flush().await.is_err() {
                    break;
                }
            }
            OwnerCommand::Close => break,
        }
    }
    let _ = write_half.shutdown().await;
}

/// The owner-connection reader: exchange responses resolve the pending
/// slot; `bus_event` frames are republished onto the server bus (§3.1)
/// through the SAME registry definitions the machine uses — identical
/// event names and payloads under both backends; `owner_superseded`
/// demotes (§6).
async fn owner_reader(mut read_half: tokio::net::unix::OwnedReadHalf, ctx: Arc<OwnerCtx>) {
    let mut decoder = FrameDecoder::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let read = match read_half.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let Ok(frames) = decoder.feed(&chunk[..read]) else { break };
        for frame in frames {
            if frame.first() == Some(&0x00) {
                // Owner connections never receive binary frames.
                ctx.shared.demote(ctx.epoch);
                let _ = ctx.exchange.0.lock().take();
                return;
            }
            if let Ok(owner_frame) = serde_json::from_slice::<OwnerFrame>(&frame) {
                match owner_frame {
                    OwnerFrame::BusEvent { event, data } => {
                        republish(&ctx.shared.bus, &event, data).await;
                    }
                    OwnerFrame::OwnerSuperseded => {
                        ctx.shared.demote(ctx.epoch);
                        let _ = ctx.exchange.0.lock().take();
                        return;
                    }
                }
                continue;
            }
            if let Ok(response) = serde_json::from_slice::<WireResponse>(&frame) {
                if let Some(tx) = ctx.exchange.0.lock().take() {
                    let _ = tx.send(Ok(response));
                }
            } else {
                ctx.shared.demote(ctx.epoch);
                let _ = ctx.exchange.0.lock().take();
                return;
            }
        }
    }
    // EOF: the daemon died or closed — drop to non-owner.
    ctx.shared.demote(ctx.epoch);
    let _ = ctx.exchange.0.lock().take();
}

impl Shared {
    fn demote(&self, epoch: u64) {
        let mut inner = self.inner.lock();
        if inner.owner.as_ref().is_some_and(|owner| owner.epoch == epoch) {
            inner.owner = None;
            inner.registration = None;
        }
    }
}

/// §3.1 step 3: republish a daemon `bus_event` onto the server bus
/// through the same registry definitions the machine uses.
async fn republish(bus: &crate::bus::Bus, event: &str, data: serde_json::Value) {
    use crate::bus::PublishOptions;
    let definition = crate::bus::registry::by_type(event)
        .cloned()
        .unwrap_or_else(|| crate::bus::registry::EventDefinition::ephemeral(event));
    let _ = bus.publish(&definition, data, PublishOptions::default()).await;
}

// ---------------------------------------------------------------------------
// Mapping + runtime-file helpers
// ---------------------------------------------------------------------------

/// The inverse of `wire::wire_terminal` (`index.ts:390-408`): maps the
/// wire terminal back onto the client API's [`Info`].
pub fn info_from_wire(terminal: &WireTerminal) -> Result<Info, String> {
    let (command, args) = match terminal.command.split_first() {
        Some((command, args)) => (command.clone(), args.to_vec()),
        None => (String::new(), Vec::new()),
    };
    let (status, exit_code) = match &terminal.lifecycle {
        wire::WireLifecycle::Running => (crate::pty::Status::Running, None),
        wire::WireLifecycle::Exited { exit_code } => (crate::pty::Status::Exited, *exit_code),
    };
    Ok(Info {
        id: wire::terminal_id(terminal.id),
        title: terminal.title.clone(),
        command,
        args,
        cwd: terminal.cwd.clone(),
        status,
        pid: terminal.pid.unwrap_or(0),
        exit_code,
        session_id: SessionId::parse(&terminal.group_id)
            .unwrap_or_else(|error| SessionId(error.value)),
        foreground_process: terminal.foreground_process.clone(),
        size: (terminal.cols, terminal.rows),
        output: (
            usize::try_from(terminal.output_head).unwrap_or(0),
            usize::try_from(terminal.output_tail).unwrap_or(0),
        ),
    })
}

/// `daemon.ts:435-444`: move `service.lock` + `service.json` (+ the
/// crash-insurance `state.json` when present) inode-preservingly into
/// `to` — a held lock survives the move.
fn migrate_runtime_files(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(to)?;
    std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o700))?;
    std::fs::rename(from.join("service.lock"), to.join("service.lock"))?;
    std::fs::rename(from.join("service.json"), to.join("service.json"))?;
    match std::fs::rename(from.join("state.json"), to.join("state.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Non-blocking flock probe (the §6 arm-B "unreachable owner" check):
/// held ⇒ an unreachable daemon still owns the directory.
fn lock_held(path: &Path) -> bool {
    use std::os::unix::io::AsRawFd as _;
    let Ok(file) =
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).read(true).open(path)
    else {
        return false;
    };
    #[allow(unsafe_code)] // contained flock(2) probe on the freshly-opened lock file
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    drop(file);
    result != 0
}

fn unix_ms_now() -> u64 {
    u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis()),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn escape_hatch_and_discovery_order() {
        // §8.1 with fixture dirs: env override wins, then exe-sibling,
        // then PATH, then nothing (spawn-kind).
        let sandbox = tempfile::Builder::new()
            .prefix("oc-pty-d2-resolve-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        let exe_dir = sandbox.path().join("exe");
        let path_dir = sandbox.path().join("onpath");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::create_dir_all(&path_dir).expect("path dir");
        let sibling = exe_dir.join("oc-pty");
        std::fs::write(&sibling, b"#!/bin/sh\n").expect("sibling");
        std::fs::set_permissions(&sibling, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let on_path = path_dir.join("oc-pty");
        std::fs::write(&on_path, b"#!/bin/sh\n").expect("path hit");
        std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let override_path = sandbox.path().join("custom-pty");
        std::fs::write(&override_path, b"#!/bin/sh\n").expect("override");
        std::fs::set_permissions(&override_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
        let path_env = path_dir.to_string_lossy().into_owned();

        // 1. OPENCODE_PTY_BIN wins even when the sibling exists.
        assert_eq!(
            resolve_binary_path(
                Some(override_path.to_str().expect("utf8")),
                Some(&exe_dir),
                &path_env
            ),
            Some(override_path.clone())
        );
        // 2. Exe-sibling before PATH.
        assert_eq!(resolve_binary_path(None, Some(&exe_dir), &path_env), Some(sibling.clone()));
        // 3. PATH fallback.
        assert_eq!(
            resolve_binary_path(None, Some(&sandbox.path().join("no-exe")), &path_env),
            Some(on_path.clone())
        );
        // 4. Exhausted (no override, no sibling, empty PATH).
        assert_eq!(resolve_binary_path(None, Some(&sandbox.path().join("no-exe")), ""), None);
    }
}
