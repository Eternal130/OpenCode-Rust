//! D2 client tests — the oc-core daemon transport + frontend driven
//! against a FAKE daemon speaking the real wire codec over a real
//! filesystem Unix socket (design §10.1 "owner-connection frame demux"
//! and the §5.2 client ladder, hermetically — no `oc-pty` binary
//! needed; the process-level real-binary matrix is D3).
//!
//! The seam is the `Launcher` trait
//! (`oc_core::persistent_pty::transport`): the frontend's
//! spawn-if-absent path calls `launch(root, id)`; the fake launcher
//! starts an in-process listener that writes the §5.3 runtime records
//! exactly like the real daemon — `service.json`, `service.lock`
//! flock, `daemon.sock` — and serves §3 frames through
//! `oc_core::persistent_pty::wire`.
#![allow(
    // Each test is one end-to-end client scenario narrative.
    clippy::too_many_lines,
    // Wire fixtures carry the §3 parameter sets verbatim.
    clippy::too_many_arguments,
    // Adoption arms match the full Adoption enum (D1 suite style).
    clippy::match_wildcard_for_single_variants,
    // Scenario futures hold the wire fixtures they drive.
    clippy::large_futures,
    // Deliberate empty-state assertions (machine-suite precedent).
    clippy::assert_is_empty
)]

use std::io::{Read as _, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::io::AsRawFd as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::StreamExt as _;
use oc_core::bus::{Bus, BusOptions};
use oc_core::database::Database;
use oc_core::persistent_pty::frontend::{escape_hatch_enabled, Backend, Frontend};
use oc_core::persistent_pty::transport::{ErrorKind, Launcher, Spawned, Transport, TransportError};
use oc_core::persistent_pty::wire::{self, FrameDecoder, WireRequest, WireResponse};
use oc_core::persistent_pty::{
    Adoption, AdoptionError, AttachInput, CreateInput, Handoff, HandoffState, Info, Registration,
    Role, StreamEvent, TerminalSnapshot, PTY_PROTOCOL_VERSION,
};
use oc_core::pty::{PtyId, Status};
use oc_schema::id::SessionId;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn temp_root(tag: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("oc-pty-d2-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("temp dir");
    dir.keep()
}

async fn bus_for_test() -> Bus {
    let database = Database::open_in_memory().await.expect("database");
    Bus::new(database, BusOptions::deterministic())
}

fn session() -> SessionId {
    SessionId::parse("ses_d2").expect("session")
}

fn create_input(title: &str) -> CreateInput {
    CreateInput {
        command: Some("/bin/sh".to_owned()),
        args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        cwd: Some("/tmp".to_owned()),
        title: title.to_owned(),
        env: std::collections::BTreeMap::new(),
        size: Some((80, 24)),
    }
}

fn base64_std() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// What the fake launcher does per `launch(root, id)` call.
enum LaunchAction {
    /// The launch itself fails (binary-missing analogue → `spawn` kind).
    Fail(String),
    /// A child that exits immediately (claim-failure analogue).
    ExitImmediately,
    /// A child that stays alive but never becomes ready.
    NeverReady,
    /// Start the fake daemon in `<root>/<id>`.
    Start(FakeConfig),
}

// Wire-behavior knobs accumulate per review fixture (bools by nature).
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
struct FakeConfig {
    /// `own` without a ticket is answered `error{"daemon is owned"}`.
    reject_unticketed_own: bool,
    /// Accept this ticket in `own` (arm A).
    accept_ticket: Option<String>,
    /// `read` answers `Rows{terminal: null}` (D1 report concern 2).
    read_returns_null_terminal: bool,
    /// Rewrite `service.json` with the true token on the first
    /// `authentication failed` (models correcting a stale registration).
    fix_registration_after_auth_failure: bool,
    /// Delay the chunked `replay_complete` marker (§3.2 deferred
    /// completion window).
    replay_complete_delay: Option<Duration>,
    /// F2 fixture: the `own` reply writes `owned` AND a
    /// `persistent-pty.removed` `bus_event` in ONE write — frames
    /// coalesced with `owned` must survive the claim loop.
    coalesce_event_with_owned: bool,
    /// §7 fixture: `snapshot` answers with a checkpoint blob that is
    /// not valid base64 (a protocol error, not silent empty bytes).
    invalid_snapshot_checkpoint: bool,
}

/// The running fake daemon's shared state (observable by the test).
struct FakeState {
    config: FakeConfig,
    directory: PathBuf,
    token: String,
    instance_id: String,
    owns: AtomicUsize,
    auth_failures: AtomicUsize,
    last_own: parking_lot::Mutex<Option<(Option<String>, Option<String>)>>,
    restores: parking_lot::Mutex<Vec<HandoffState>>,
    shutdowns: AtomicUsize,
    /// The current owner connection's stream (`bus_event` /
    /// `owner_superseded` frames ride it, §3.1 / §6).
    owner_stream: parking_lot::Mutex<Option<Arc<std::sync::Mutex<std::os::unix::net::UnixStream>>>>,
    stop: AtomicBool,
    /// Replay served to the next subscriber (inline or chunked per
    /// `chunked_replay`).
    replay_bytes: parking_lot::Mutex<Vec<u8>>,
    chunked_replay: AtomicBool,
    /// Frames pushed on the subscribe connection after the replay
    /// contract.
    live_frames: parking_lot::Mutex<Vec<Vec<u8>>>,
    /// The daemon's terminal table (create pushes, list reads).
    terminals: parking_lot::Mutex<Vec<wire::WireTerminal>>,
}

impl FakeState {
    #[allow(clippy::unused_self)] // symmetry with owner_frame
    fn write_frame(
        &self,
        stream: &std::sync::Mutex<std::os::unix::net::UnixStream>,
        payload: &[u8],
    ) {
        if let Ok(bytes) = wire::encode_frame(payload) {
            let _ = stream.lock().unwrap().write_all(&bytes);
        }
    }

    /// Pushes one frame onto the OWNER connection (§3.1 `bus_event`, §6
    /// `owner_superseded`).
    fn owner_frame(&self, frame: &wire::OwnerFrame) {
        if let Some(stream) = self.owner_stream.lock().as_ref() {
            let payload = serde_json::to_vec(frame).expect("owner frame");
            self.write_frame(stream, &payload);
        }
    }

    /// §6 supersession trigger: the frame flushes, then the daemon
    /// closes the owner connection.
    fn supersede(&self) {
        self.owner_frame(&wire::OwnerFrame::OwnerSuperseded);
        if let Some(stream) = self.owner_stream.lock().take() {
            let _ = stream.lock().unwrap().shutdown(std::net::Shutdown::Write);
        }
    }
}

/// Starts the fake daemon: §5.3 records (flock held for the daemon
/// "process lifetime"), socket listener, accept thread.
fn start_fake_daemon(root: &Path, id: &str, config: FakeConfig) -> Arc<FakeState> {
    let directory = root.join(id);
    std::fs::create_dir_all(&directory).expect("mkdir");
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .expect("0700 runtime dir");
    let _lock = flock_new(&directory.join("service.lock"));
    let socket_path = directory.join("daemon.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind socket");
    let registration = Registration {
        instance_id: format!("inst_{id}"),
        pid: std::process::id(),
        protocol: PTY_PROTOCOL_VERSION,
        socket: socket_path.to_string_lossy().into_owned(),
        token: format!("tok_{id}"),
    };
    write_registration(&directory, &registration);
    let state = Arc::new(FakeState {
        config,
        directory: directory.clone(),
        token: registration.token.clone(),
        instance_id: registration.instance_id.clone(),
        owns: AtomicUsize::new(0),
        auth_failures: AtomicUsize::new(0),
        last_own: parking_lot::Mutex::new(None),
        restores: parking_lot::Mutex::new(Vec::new()),
        shutdowns: AtomicUsize::new(0),
        owner_stream: parking_lot::Mutex::new(None),
        stop: AtomicBool::new(false),
        replay_bytes: parking_lot::Mutex::new(Vec::new()),
        chunked_replay: AtomicBool::new(false),
        live_frames: parking_lot::Mutex::new(Vec::new()),
        terminals: parking_lot::Mutex::new(Vec::new()),
    });
    let accept_state = Arc::clone(&state);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let state = Arc::clone(&accept_state);
            std::thread::spawn(move || serve_conn(stream, &state));
        }
    });
    state
}

/// Serves one connection: framed request/response + owner frames.
fn serve_conn(stream: std::os::unix::net::UnixStream, state: &Arc<FakeState>) {
    let stream = Arc::new(std::sync::Mutex::new(stream));
    let Ok(mut reader) = stream.lock().unwrap().try_clone() else { return };
    let mut decoder = FrameDecoder::new();
    let mut chunk = [0u8; 16 * 1024];
    'conn: loop {
        if state.stop.load(Ordering::SeqCst) {
            break 'conn;
        }
        let Ok(read) = reader.read(&mut chunk) else { break 'conn };
        if read == 0 {
            break 'conn;
        }
        let Ok(frames) = decoder.feed(&chunk[..read]) else { break 'conn };
        for frame in frames {
            match dispatch_frame(&frame, state, &stream) {
                Reply::Frame(payload) => state.write_frame(&stream, &payload),
                Reply::OkThenClose => {
                    state.write_frame(
                        &stream,
                        &serde_json::to_vec(&WireResponse::Ok).expect("ok json"),
                    );
                    let _ = stream.lock().unwrap().shutdown(std::net::Shutdown::Write);
                    break 'conn;
                }
                Reply::None => {}
            }
        }
    }
    if state.owner_stream.lock().is_some() {
        // The owner connection ended.
    }
}

enum Reply {
    /// One unframed response payload to write back.
    Frame(Vec<u8>),
    /// The §3 shutdown contract: `ok` flushes, then the socket closes.
    OkThenClose,
    /// Stream connection: nothing further for this exchange.
    None,
}

fn dispatch_frame(
    frame: &[u8],
    state: &Arc<FakeState>,
    stream: &Arc<std::sync::Mutex<std::os::unix::net::UnixStream>>,
) -> Reply {
    let envelope: wire::Envelope = match serde_json::from_slice(frame) {
        Ok(envelope) => envelope,
        Err(_) => return reply(&WireResponse::Error { message: "malformed".to_owned() }),
    };
    if envelope.token != state.token {
        if state.config.fix_registration_after_auth_failure
            && state.auth_failures.fetch_add(1, Ordering::SeqCst) == 0
        {
            // The stale on-disk registration is corrected before the
            // error reply flushes — the client's drop-and-rediscover
            // ladder recovers on the retry (§5.2).
            write_registration(
                &state.directory,
                &Registration {
                    instance_id: state.instance_id.clone(),
                    pid: std::process::id(),
                    protocol: PTY_PROTOCOL_VERSION,
                    socket: state.directory.join("daemon.sock").to_string_lossy().into_owned(),
                    token: state.token.clone(),
                },
            );
        }
        return reply(&WireResponse::Error { message: "authentication failed".to_owned() });
    }
    match envelope.request {
        WireRequest::Ping => reply(&WireResponse::Pong {
            instance_id: state.instance_id.clone(),
            pid: std::process::id(),
            protocol: PTY_PROTOCOL_VERSION,
        }),
        WireRequest::Own { instance_id, ticket, directory } => {
            if instance_id != state.instance_id {
                return reply(&WireResponse::Error {
                    message: "PTY restart daemon changed".to_owned(),
                });
            }
            let ticket_ok = ticket
                .as_deref()
                .is_some_and(|ticket| state.config.accept_ticket.as_deref() == Some(ticket));
            if ticket.is_some() && !ticket_ok {
                return reply(&WireResponse::Error {
                    message: "PTY restart handoff ticket rejected".to_owned(),
                });
            }
            if ticket.is_none() && state.config.reject_unticketed_own {
                return reply(&WireResponse::Error { message: "daemon is owned".to_owned() });
            }
            state.owns.fetch_add(1, Ordering::SeqCst);
            *state.last_own.lock() = Some((ticket, directory));
            *state.owner_stream.lock() = Some(Arc::clone(stream));
            if state.config.coalesce_event_with_owned {
                // F2 fixture: `owned` + a `bus_event` in ONE write — the
                // client's claim read receives both frames in one chunk.
                let mut bytes =
                    wire::encode_frame(&serde_json::to_vec(&WireResponse::Owned).expect("owned"))
                        .expect("owned frame");
                bytes.extend(
                    wire::encode_frame(
                        &serde_json::to_vec(&wire::OwnerFrame::BusEvent {
                            event: "persistent-pty.removed".to_owned(),
                            data: serde_json::json!({
                                "sessionID": session().as_str(),
                                "ptyID": "pty_persistent_1",
                            }),
                        })
                        .expect("bus event json"),
                    )
                    .expect("bus event frame"),
                );
                let _ = stream.lock().unwrap().write_all(&bytes);
                return Reply::None;
            }
            reply(&WireResponse::Owned)
        }
        WireRequest::PrepareHandoff => reply(&WireResponse::Handoff {
            ticket: state.config.accept_ticket.clone().unwrap_or_else(|| "tick-1".to_owned()),
            expires_at: 4_102_444_800_000, // 2100-01-01 — far future
        }),
        WireRequest::Shutdown => {
            state.shutdowns.fetch_add(1, Ordering::SeqCst);
            state.stop.store(true, Ordering::SeqCst);
            Reply::OkThenClose
        }
        WireRequest::List => {
            reply(&WireResponse::Terminals { terminals: state.terminals.lock().clone() })
        }
        WireRequest::Create { title, group_id, .. } => {
            let terminal = wire::WireTerminal {
                id: state.terminals.lock().len() as u64 + 1,
                pid: Some(1234),
                title: title.unwrap_or_else(|| "t".to_owned()),
                foreground_process: None,
                group_id,
                command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "sleep 30".to_owned()],
                cwd: "/tmp".to_owned(),
                cols: 80,
                rows: 24,
                lifecycle: wire::WireLifecycle::Running,
                output_head: 0,
                output_tail: 0,
            };
            state.terminals.lock().push(terminal.clone());
            // §3.1: the daemon serializes the machine's publication to
            // the OWNER connection as a bus_event frame.
            state.owner_frame(&wire::OwnerFrame::BusEvent {
                event: "persistent-pty.added".to_owned(),
                data: serde_json::json!({
                    "sessionID": terminal.group_id,
                    "terminal": { "id": format!("pty_persistent_{}", terminal.id) },
                }),
            });
            reply(&WireResponse::Created { terminal })
        }
        WireRequest::Write { .. }
        | WireRequest::Resize { .. }
        | WireRequest::Control { .. }
        | WireRequest::Input { .. } => reply(&WireResponse::Ok),
        WireRequest::Terminate { id } => {
            let removed = state
                .terminals
                .lock()
                .iter()
                .find(|terminal| terminal.id == id)
                .map(|terminal| (terminal.group_id.clone(), terminal.id))
                .map(|(group_id, id)| {
                    // §3.1: `removed` rides the owner connection like
                    // `added` (the machine's exit publication).
                    wire::OwnerFrame::BusEvent {
                        event: "persistent-pty.removed".to_owned(),
                        data: serde_json::json!({
                            "sessionID": group_id,
                            "ptyID": format!("pty_persistent_{id}"),
                        }),
                    }
                });
            state.terminals.lock().retain(|terminal| terminal.id != id);
            if let Some(event) = removed {
                state.owner_frame(&event);
            }
            reply(&WireResponse::Ok)
        }
        WireRequest::Read { .. } | WireRequest::ReadRows { .. } => {
            let terminal = (!state.config.read_returns_null_terminal).then(|| wire::WireTerminal {
                id: 1,
                pid: Some(1234),
                title: "t".to_owned(),
                foreground_process: None,
                group_id: "ses_d2".to_owned(),
                command: vec!["/bin/sh".to_owned()],
                cwd: "/tmp".to_owned(),
                cols: 80,
                rows: 24,
                lifecycle: wire::WireLifecycle::Running,
                output_head: 0,
                output_tail: 0,
            });
            reply(&WireResponse::Rows {
                terminal,
                lines: vec!["row-one".to_owned(), "row-two".to_owned()],
                cursor_x: 3,
                cursor_y: 1,
            })
        }
        WireRequest::Snapshot { id } => reply(&WireResponse::Snapshot {
            terminal: wire::WireTerminal {
                id,
                pid: None,
                title: "t".to_owned(),
                foreground_process: None,
                group_id: "ses_d2".to_owned(),
                command: vec!["/bin/sh".to_owned()],
                cwd: "/tmp".to_owned(),
                cols: 80,
                rows: 24,
                lifecycle: wire::WireLifecycle::Exited { exit_code: Some(0) },
                output_head: 0,
                output_tail: 4,
            },
            text: "row-one".to_owned(),
            checkpoint_base64: if state.config.invalid_snapshot_checkpoint {
                "!!not-base64!!".to_owned()
            } else {
                base64_std().encode(b"checkpoint-bytes")
            },
            cursor_x: 1,
            cursor_y: 0,
        }),
        WireRequest::Restore { state: handoff } => {
            state.restores.lock().push(handoff);
            reply(&WireResponse::Ok)
        }
        WireRequest::Subscribe { id, offset, role, .. } => {
            subscribe(state, id, offset, role, stream)
        }
        WireRequest::Unknown => reply(&WireResponse::Error { message: "unknown op".to_owned() }),
    }
}

fn reply(response: &WireResponse) -> Reply {
    Reply::Frame(serde_json::to_vec(response).expect("response json"))
}

fn terminal_fixture(id: u64) -> wire::WireTerminal {
    wire::WireTerminal {
        id,
        pid: Some(1234),
        title: "t".to_owned(),
        foreground_process: None,
        group_id: "ses_d2".to_owned(),
        command: vec!["/bin/sh".to_owned()],
        cwd: "/tmp".to_owned(),
        cols: 80,
        rows: 24,
        lifecycle: wire::WireLifecycle::Running,
        output_head: 0,
        output_tail: 0,
    }
}

/// The §3.2 subscribe: inline replay iff ≤1 MiB (unless chunked is
/// forced), else chunked binary frames + a delayed `replay_complete`;
/// queued live frames follow.
fn subscribe(
    state: &Arc<FakeState>,
    id: u64,
    offset: u64,
    role: wire::WireRole,
    stream: &Arc<std::sync::Mutex<std::os::unix::net::UnixStream>>,
) -> Reply {
    let replay = state.replay_bytes.lock().clone();
    let end_offset = offset + replay.len() as u64;
    let chunked = state.chunked_replay.load(Ordering::SeqCst);
    let attached = if replay.len() <= wire::INLINE_REPLAY_LIMIT && !chunked {
        WireResponse::Attached {
            terminal: terminal_fixture(id),
            role,
            generation: 1,
            requested_offset: offset,
            available_offset: offset,
            end_offset,
            truncated: false,
            replay_base64: Some(base64_std().encode(&replay)),
            replay_chunked: None,
            replay_length: None,
        }
    } else {
        WireResponse::Attached {
            terminal: terminal_fixture(id),
            role,
            generation: 1,
            requested_offset: offset,
            available_offset: offset,
            end_offset,
            truncated: false,
            replay_base64: None,
            replay_chunked: Some(true),
            replay_length: Some(replay.len() as u64),
        }
    };
    let chunked_arm = replay.len() > wire::INLINE_REPLAY_LIMIT || chunked;
    state.write_frame(stream, &serde_json::to_vec(&attached).expect("attached json"));
    if chunked_arm {
        let base = end_offset - replay.len() as u64;
        let mut sent = 0u64;
        for chunk in replay.chunks(64 * 1024) {
            let start = base + sent;
            sent += chunk.len() as u64;
            state.write_frame(
                stream,
                &wire::encode_output_frame(start, start + chunk.len() as u64, chunk)
                    .expect("replay chunk payload"),
            );
        }
        if let Some(delay) = state.config.replay_complete_delay {
            std::thread::sleep(delay);
        }
        let marker = serde_json::to_vec(&WireResponse::ReplayComplete { end_offset })
            .expect("replay complete json");
        state.write_frame(stream, &marker);
    }
    for live in state.live_frames.lock().iter() {
        state.write_frame(stream, live);
    }
    Reply::None
}

/// Holds an exclusive flock (the daemon "process lifetime" analogue).
fn flock_new(path: &Path) -> std::fs::File {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(path)
        .expect("lock file");
    #[allow(unsafe_code)] // contained flock(2) on the freshly-opened lock file
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    assert_eq!(result, 0, "fake daemon claims its lock");
    file
}

fn write_registration(directory: &Path, registration: &Registration) {
    std::fs::write(
        directory.join("service.json"),
        serde_json::to_string(registration).expect("registration json"),
    )
    .expect("write service.json");
}

fn read_registration(directory: &Path) -> Option<Registration> {
    let text = std::fs::read_to_string(directory.join("service.json")).ok()?;
    serde_json::from_str(&text).ok()
}

// ---------------------------------------------------------------------------
// The fake launcher
// ---------------------------------------------------------------------------

enum FakeSpawned {
    Exited,
    NeverReady(Arc<AtomicUsize>),
    Running(Arc<FakeState>),
}

impl Spawned for FakeSpawned {
    fn has_exited(&self) -> bool {
        match self {
            Self::Exited => true,
            Self::NeverReady(_) => false,
            Self::Running(state) => state.stop.load(Ordering::SeqCst),
        }
    }
    fn terminate(&self) {
        match self {
            Self::NeverReady(counter) => {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            Self::Running(state) => {
                state.stop.store(true, Ordering::SeqCst);
            }
            Self::Exited => {}
        }
    }
}

struct FakeLauncher {
    script: parking_lot::Mutex<Vec<LaunchAction>>,
    launches: parking_lot::Mutex<Vec<(PathBuf, String)>>,
    states: parking_lot::Mutex<Vec<Arc<FakeState>>>,
    terminates: Arc<AtomicUsize>,
}

impl FakeLauncher {
    fn new(script: Vec<LaunchAction>) -> Arc<Self> {
        Arc::new(Self {
            script: parking_lot::Mutex::new(script),
            launches: parking_lot::Mutex::new(Vec::new()),
            states: parking_lot::Mutex::new(Vec::new()),
            terminates: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn last_state(&self) -> Option<Arc<FakeState>> {
        self.states.lock().last().cloned()
    }

    fn terminate_calls(&self) -> usize {
        self.terminates.load(Ordering::SeqCst)
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, root: &Path, id: &str) -> Result<Box<dyn Spawned>, TransportError> {
        self.launches.lock().push((root.to_path_buf(), id.to_owned()));
        let mut script = self.script.lock();
        let action = if script.is_empty() {
            // Script exhausted: the binary-missing analogue.
            LaunchAction::Fail("could not find the opencode-pty binary".to_owned())
        } else {
            script.remove(0)
        };
        Ok(match action {
            LaunchAction::Fail(message) => {
                return Err(TransportError::new(ErrorKind::Spawn, message));
            }
            LaunchAction::ExitImmediately => Box::new(FakeSpawned::Exited),
            LaunchAction::NeverReady => {
                Box::new(FakeSpawned::NeverReady(Arc::clone(&self.terminates)))
            }
            LaunchAction::Start(config) => {
                let state = start_fake_daemon(root, id, config);
                self.states.lock().push(Arc::clone(&state));
                Box::new(FakeSpawned::Running(state))
            }
        })
    }
}

/// A daemon-backend frontend on the fake launcher, with a short spawn
/// poll deadline (the spec's 5s would make the ladder tests crawl).
struct Client {
    frontend: Frontend,
    launcher: Arc<FakeLauncher>,
    #[allow(dead_code)] // diagnostics: the claimed runtime root
    root: PathBuf,
}

impl Client {
    async fn new(tag: &str, script: Vec<LaunchAction>) -> Self {
        let bus = bus_for_test().await;
        let launcher = FakeLauncher::new(script);
        let transport =
            Transport::new(bus, launcher.clone()).with_spawn_deadline(Duration::from_millis(750));
        let frontend = Frontend::from_backend(Backend::Daemon(Arc::new(transport)));
        let root = temp_root(tag);
        frontend.set_runtime_dir(&root).expect("runtime dir");
        Self { frontend, launcher: Arc::clone(&launcher) as Arc<FakeLauncher>, root }
    }
}

// ---------------------------------------------------------------------------
// Backend selection + soft-empty semantics
// ---------------------------------------------------------------------------

/// R3/§11: `OC_PTY_DAEMON=0` forces the in-process backend (one
/// release window); anything else selects the daemon backend.
#[test]
fn escape_hatch_flag_values() {
    assert!(escape_hatch_enabled(Some("0")), "=0 forces in-process");
    assert!(!escape_hatch_enabled(None), "unset → daemon backend");
    assert!(!escape_hatch_enabled(Some("1")));
    assert!(!escape_hatch_enabled(Some("false")));
}

/// §5.2: absent daemon ⇒ `list` ⇒ `[]`, `read` ⇒ `None` — and nothing
/// ever spawns for non-`create` ops (§8.2: only create starts daemons).
#[tokio::test]
async fn absent_daemon_is_soft_empty_and_never_spawns() {
    let client = Client::new("soft-empty", vec![]).await;
    assert!(client.frontend.list(None).await.expect("list").is_empty());
    let read = client.frontend.read(&session(), None).await.expect("read");
    assert!(read.is_none(), "read on an absent daemon is None");
    let error = client
        .frontend
        .write(&PtyId("pty_persistent_1".to_owned()), "x", None)
        .await
        .expect_err("write without a daemon is an error");
    assert!(
        error.message.contains("opencode-pty"),
        "write surfaces the daemon-unavailable error, got: {error}"
    );
    assert!(client.launcher.launches.lock().is_empty(), "no launch may happen for non-create ops");
}

/// §8.2 spawn-if-absent: create discovers nothing, spawns once, polls
/// the registration, claims (own without ticket) and surfaces Created.
#[tokio::test]
async fn create_spawns_claims_and_lists() {
    let client = Client::new("spawn-claim", vec![LaunchAction::Start(FakeConfig::default())]).await;
    let info = client.frontend.create(&session(), create_input("first")).await.expect("create");
    assert_eq!(info.id, PtyId("pty_persistent_1".to_owned()));
    assert_eq!(info.title, "first");
    assert_eq!(info.session_id, session());
    assert_eq!(client.launcher.launches.lock().len(), 1);
    let listed = client.frontend.list(Some(&session())).await.expect("list");
    assert_eq!(listed.len(), 1, "the terminal is visible through list");
    let got = client.frontend.get(&PtyId("pty_persistent_1".to_owned())).await.expect("get");
    assert_eq!(got.id, info.id);
    let state = client.launcher.last_state().expect("state");
    assert_eq!(state.owns.load(Ordering::SeqCst), 1, "the client claimed ownership");
}

/// §8.2 fresh-dir retry: a failed first spawn (child exits — the
/// unreachable-lock-holder analogue) abandons the identity and retries
/// ONCE in a fresh directory.
#[tokio::test]
async fn spawn_failure_abandons_to_a_fresh_directory() {
    let client = Client::new(
        "abandon",
        vec![LaunchAction::ExitImmediately, LaunchAction::Start(FakeConfig::default())],
    )
    .await;
    let info = client.frontend.create(&session(), create_input("retry")).await.expect("create");
    assert_eq!(info.title, "retry");
    let launches = client.launcher.launches.lock().clone();
    assert_eq!(launches.len(), 2, "exactly one abandon+retry");
    assert_ne!(launches[0].1, launches[1].1, "the retry uses a FRESH directory id");
}

/// §8.2: a child that never becomes ready is terminated through the
/// owned child handle and the last error surfaces.
#[tokio::test]
async fn spawn_deadline_terminates_the_child() {
    let client = Client::new("deadline", vec![LaunchAction::NeverReady]).await;
    let error = client
        .frontend
        .create(&session(), create_input("never"))
        .await
        .expect_err("never-ready spawn fails");
    assert!(error.message.contains("opencode-pty"), "the spawn failure surfaces, got: {error}");
    assert!(client.launcher.terminate_calls() >= 1, "the client SIGTERMs the child it spawned");
}

/// §4.2: a registration with a foreign protocol is a hard `protocol`
/// error with the pinned message.
#[tokio::test]
async fn protocol_mismatch_message_is_surfaced() {
    let client = Client::new("proto", vec![]).await;
    let directory = PathBuf::from(client.frontend.runtime_dir().expect("runtime dir"));
    std::fs::create_dir_all(&directory).expect("mkdir");
    write_registration(
        &directory,
        &Registration {
            instance_id: "inst_old".to_owned(),
            pid: 1,
            protocol: 1,
            socket: directory.join("daemon.sock").to_string_lossy().into_owned(),
            token: "t".to_owned(),
        },
    );
    let error = client.frontend.list(None).await.expect_err("protocol mismatch surfaces");
    assert_eq!(
        error.message,
        format!("opencode-pty protocol mismatch: daemon=1, client={PTY_PROTOCOL_VERSION}"),
        "the I7 message rides the client error verbatim"
    );
}

// ---------------------------------------------------------------------------
// Owner connection: demux, bus republish, supersession
// ---------------------------------------------------------------------------

/// §3.1: `bus_event` frames received on the owner connection are
/// republished onto the real server bus — the TUI/EV-1 surface keeps
/// seeing `persistent-pty.added`/`persistent-pty.removed` with the
/// same names+payloads.
#[tokio::test]
async fn owner_bus_events_are_republished_onto_the_bus() {
    let (frontend, bus, launcher) = daemon_frontend("bus-republish").await;
    let mut subscription = bus.subscribe_types(&["persistent-pty.added", "persistent-pty.removed"]);
    let info = frontend.create(&session(), create_input("ev")).await.expect("create");
    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("bus event within 5s")
        .expect("bus event");
    assert_eq!(event.type_, "persistent-pty.added");
    assert_eq!(event.data["sessionID"], session().as_str());
    assert_eq!(event.data["terminal"]["id"], info.id.0);
    // Test gap (review): the `removed` publication republishes too.
    frontend.remove(&info.id).await.expect("remove");
    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("removed event within 5s")
        .expect("bus event");
    assert_eq!(event.type_, "persistent-pty.removed");
    assert_eq!(event.data["sessionID"], session().as_str());
    assert_eq!(event.data["ptyID"], info.id.0);
    assert_eq!(launcher.launches.lock().len(), 1);
}

/// F2: frames coalesced with `owned` in ONE daemon write must survive
/// the claim loop — the `bus_event` riding the same read chunk as
/// `owned` used to be dropped with the rest of the decoded batch (and
/// the claim decoder's buffered state).
#[tokio::test]
async fn frames_coalesced_with_owned_are_not_dropped() {
    let (frontend, bus, launcher) = daemon_frontend_with_script(
        "coalesced-owned",
        vec![LaunchAction::Start(FakeConfig {
            coalesce_event_with_owned: true,
            ..FakeConfig::default()
        })],
    )
    .await;
    let mut subscription = bus.subscribe_types(&["persistent-pty.removed"]);
    let info = frontend.create(&session(), create_input("co")).await.expect("create");
    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("coalesced bus event within 5s")
        .expect("bus event");
    assert_eq!(event.type_, "persistent-pty.removed");
    assert_eq!(event.data["ptyID"], info.id.0, "the frame coalesced with owned was republished");
    assert_eq!(launcher.last_state().expect("state").owns.load(Ordering::SeqCst), 1);
}

/// §7: a snapshot checkpoint that is not valid base64 is a protocol
/// error, never silent empty bytes.
#[tokio::test]
async fn snapshot_with_invalid_checkpoint_is_a_protocol_error() {
    let (frontend, _bus, _launcher) = daemon_frontend_with_script(
        "bad-snapshot",
        vec![LaunchAction::Start(FakeConfig {
            invalid_snapshot_checkpoint: true,
            ..FakeConfig::default()
        })],
    )
    .await;
    frontend.create(&session(), create_input("s")).await.expect("create");
    let error = frontend
        .snapshot(&PtyId("pty_persistent_1".to_owned()))
        .await
        .expect_err("a bad checkpoint blob errors");
    assert_eq!(
        error.message, "opencode-pty snapshot checkpoint is not valid base64",
        "the §7 protocol error surfaces, got: {error}"
    );
}

/// §7: a `resized` frame whose checkpoint blob is not valid base64 is
/// a protocol violation on a one-way stream — the pump ENDS (`on_end`)
/// instead of delivering silent empty checkpoint bytes.
#[tokio::test]
async fn resized_with_invalid_checkpoint_ends_the_stream() {
    let (frontend, _bus, state) =
        prestarted_frontend("bad-resize-checkpoint", FakeConfig::default()).await;
    state.chunked_replay.store(false, Ordering::SeqCst);
    let resized = serde_json::to_vec(&WireResponse::Resized {
        cols: 100,
        rows: 30,
        generation: 2,
        checkpoint_base64: "!!not-base64!!".to_owned(),
    })
    .expect("resized json");
    // live_frames carry unframed PAYLOADS — the fake daemon frames them.
    state.live_frames.lock().push(resized);

    let (events_tx, events_rx) = std::sync::mpsc::channel::<StreamEvent>();
    let (end_tx, end_rx) = std::sync::mpsc::channel::<()>();
    let attachment = frontend
        .attach(
            &PtyId("pty_persistent_1".to_owned()),
            AttachInput {
                cursor: 0,
                attachment_id: "attach-1",
                role: Role::Observer,
                takeover: false,
                on_event: Box::new(move |event| {
                    let _ = events_tx.send(event);
                }),
                on_end: Box::new(move || {
                    let _ = end_tx.send(());
                }),
            },
        )
        .await
        .expect("attach");
    attachment.activate();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if end_rx.try_recv().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the stream ends within 5s");
    assert!(
        events_rx.try_recv().is_err(),
        "no Resized event with silent empty checkpoint bytes may be delivered"
    );
    attachment.detach();
}

async fn daemon_frontend(tag: &str) -> (Frontend, Bus, Arc<FakeLauncher>) {
    daemon_frontend_with_script(tag, vec![LaunchAction::Start(FakeConfig::default())]).await
}

async fn daemon_frontend_with_script(
    tag: &str,
    script: Vec<LaunchAction>,
) -> (Frontend, Bus, Arc<FakeLauncher>) {
    let bus = bus_for_test().await;
    let launcher = FakeLauncher::new(script);
    let transport =
        Transport::new(bus.clone(), launcher.clone()).with_spawn_deadline(Duration::from_secs(2));
    let frontend = Frontend::from_backend(Backend::Daemon(Arc::new(transport)));
    let root = temp_root(tag);
    frontend.set_runtime_dir(&root).expect("runtime dir");
    (frontend, bus, launcher)
}

/// A daemon already running in the client's identity directory (the
/// steady state for non-create ops: discover + claim, no spawn).
async fn prestarted_frontend(tag: &str, config: FakeConfig) -> (Frontend, Bus, Arc<FakeState>) {
    let bus = bus_for_test().await;
    let launcher = FakeLauncher::new(Vec::new());
    let transport =
        Transport::new(bus.clone(), launcher).with_spawn_deadline(Duration::from_secs(2));
    let frontend = Frontend::from_backend(Backend::Daemon(Arc::new(transport)));
    let root = temp_root(tag);
    frontend.set_runtime_dir(&root).expect("runtime dir");
    let directory = PathBuf::from(frontend.runtime_dir().expect("runtime dir"));
    let id = directory.file_name().unwrap().to_string_lossy().into_owned();
    let state = start_fake_daemon(&root, &id, config);
    (frontend, bus, state)
}

/// §6 supersession: the demoted owner drops to non-owner and the next
/// op re-discovers and re-claims (the re-own ladder).
#[tokio::test]
async fn owner_superseded_reowns_on_the_next_op() {
    let (frontend, _bus, launcher) = daemon_frontend("superseded").await;
    frontend.create(&session(), create_input("a")).await.expect("create");
    let state = launcher.last_state().expect("state");
    assert_eq!(state.owns.load(Ordering::SeqCst), 1);

    // A competing valid-ticket owner demoted us: the daemon sent
    // owner_superseded and closed the connection.
    state.supersede();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let listed = frontend.list(None).await.expect("list after supersession");
    assert_eq!(listed.len(), 1, "the re-own ladder re-claimed transparently");
    assert_eq!(state.owns.load(Ordering::SeqCst), 2, "a SECOND own claim happened");
}

// ---------------------------------------------------------------------------
// Rows{terminal: null} + snapshot mapping
// ---------------------------------------------------------------------------

/// D1 report concern 2: `Rows{terminal: null}` maps to `None`.
#[tokio::test]
async fn rows_with_null_terminal_maps_to_none() {
    let (frontend, _bus, _launcher) = daemon_frontend_with_script(
        "rows-null",
        vec![LaunchAction::Start(FakeConfig {
            read_returns_null_terminal: true,
            ..FakeConfig::default()
        })],
    )
    .await;
    let read = frontend.read(&session(), None).await.expect("read");
    assert!(read.is_none(), "Rows{{terminal: null}} ⇒ None");
}

/// The `Rows` arm with a terminal maps onto `ReadResult`.
#[tokio::test]
async fn rows_with_terminal_maps_onto_read_result() {
    let (frontend, _bus, _launcher) = daemon_frontend("rows-some").await;
    frontend.create(&session(), create_input("r")).await.expect("create");
    let read = frontend.read(&session(), None).await.expect("read").expect("selected");
    assert_eq!(read.pty_id, PtyId("pty_persistent_1".to_owned()));
    assert_eq!(read.title, "t");
    assert_eq!(read.screen.text, "row-one\nrow-two");
    assert_eq!(read.screen.cursor, (3, 1));
    assert_eq!(read.screen.cols, 80);
}

/// The snapshot op round-trips through the wire shapes.
#[tokio::test]
async fn snapshot_maps_through_the_wire() {
    let (frontend, _bus, _launcher) = daemon_frontend("snapshot").await;
    frontend.create(&session(), create_input("snap")).await.expect("create");
    let snapshot =
        frontend.snapshot(&PtyId("pty_persistent_1".to_owned())).await.expect("snapshot");
    assert_eq!(snapshot.text, "row-one");
    assert_eq!(snapshot.checkpoint, b"checkpoint-bytes");
    assert_eq!(snapshot.cursor, (1, 0));
}

// ---------------------------------------------------------------------------
// §3.2 replay transport — inline and chunked arms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn inline_replay_rides_the_attached_frame() {
    let attachment = attach_with_replay("inline", b"small replay".to_vec(), false, None).await;
    assert_eq!(attachment.data, b"small replay".to_vec());
    assert_eq!(attachment.replay.end_offset, "small replay".len());
}

/// The chunked arm: >1 MiB chunked replay reassembles by absolute
/// offset and the client defers completion until contiguous — the
/// `replay_complete` marker may lag behind the last chunk.
#[tokio::test]
async fn chunked_replay_reassembles_before_completion() {
    let replay: Vec<u8> = (0u32..1_500_000).map(|i| (i % 251) as u8).collect();
    let started = Instant::now();
    let attachment =
        attach_with_replay("chunked", replay.clone(), true, Some(Duration::from_millis(250))).await;
    assert_eq!(attachment.data, replay, "exact reassembly by absolute offset");
    assert_eq!(attachment.replay.end_offset, 1_500_000);
    assert!(
        started.elapsed() >= Duration::from_millis(250),
        "completion is deferred until the marker arrives"
    );
}

/// After activation, live output frames flow as Output events; nothing
/// flows before `activate()` (the client's replay contract first).
#[tokio::test]
async fn live_output_flows_after_activate() {
    let (frontend, _bus, state) = prestarted_frontend("live-output", FakeConfig::default()).await;
    *state.replay_bytes.lock() = b"prefix".to_vec();
    state.chunked_replay.store(false, Ordering::SeqCst);
    // One live output frame queued after the replay contract.
    state
        .live_frames
        .lock()
        .push(wire::encode_output_frame(6, 11, b"hello").expect("live payload"));

    let (events_tx, events_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let forward = events_tx.clone();
    let attachment = frontend
        .attach(
            &PtyId("pty_persistent_1".to_owned()),
            AttachInput {
                cursor: 0,
                attachment_id: "attach-1",
                role: Role::Controller,
                takeover: false,
                on_event: Box::new(move |event| {
                    if let StreamEvent::Output(bytes) = event {
                        let _ = forward.send(bytes);
                    }
                }),
                on_end: Box::new(|| {}),
            },
        )
        .await
        .expect("attach");
    assert_eq!(attachment.data, b"prefix".to_vec());
    assert!(events_rx.try_recv().is_err(), "no events before activate()");
    attachment.activate();
    let bytes = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = events_rx.try_recv() {
                break bytes;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("live output within 5s");
    assert_eq!(bytes, b"hello");
    attachment.detach();
}

async fn attach_with_replay(
    tag: &str,
    replay: Vec<u8>,
    chunked: bool,
    replay_complete_delay: Option<Duration>,
) -> oc_core::persistent_pty::frontend::Attachment {
    let (frontend, _bus, state) =
        prestarted_frontend(tag, FakeConfig { replay_complete_delay, ..FakeConfig::default() })
            .await;
    *state.replay_bytes.lock() = replay;
    state.chunked_replay.store(chunked, Ordering::SeqCst);
    frontend
        .attach(
            &PtyId("pty_persistent_1".to_owned()),
            AttachInput {
                cursor: 0,
                attachment_id: "attach-1",
                role: Role::Observer,
                takeover: false,
                on_event: Box::new(|_| {}),
                on_end: Box::new(|| {}),
            },
        )
        .await
        .expect("attach")
}

// ---------------------------------------------------------------------------
// §6 adoption ladder (hermetic arms)
// ---------------------------------------------------------------------------

fn future_handoff(directory: &Path, instance_id: &str, ticket: &str) -> Handoff {
    Handoff {
        directory: directory.to_string_lossy().into_owned(),
        instance_id: instance_id.to_owned(),
        ticket: ticket.to_owned(),
        expires_at: 4_102_444_800_000,
    }
}

/// Arm A — the live daemon: claim with the ticket, terminals stay live.
#[tokio::test]
async fn adopt_claims_the_live_daemon_with_the_ticket() {
    let (frontend, _bus, launcher) = daemon_frontend_with_script(
        "arm-a",
        vec![LaunchAction::Start(FakeConfig {
            accept_ticket: Some("tick-9".to_owned()),
            ..FakeConfig::default()
        })],
    )
    .await;
    frontend.create(&session(), create_input("adopted")).await.expect("create");
    let state = launcher.last_state().expect("state");

    let directory = PathBuf::from(frontend.runtime_dir().expect("runtime dir"));
    let handoff = future_handoff(&directory, &state.instance_id, "tick-9");
    match frontend.adopt(&handoff).await {
        Adoption::Adopted { directory: adopted, .. } => {
            assert_eq!(PathBuf::from(adopted), directory, "the live identity is kept");
        }
        other => panic!("arm A adopts the live daemon, got {other:?}"),
    }
    let own = state.last_own.lock().clone();
    assert_eq!(
        own,
        Some((Some("tick-9".to_owned()), Some(directory.to_string_lossy().into_owned()))),
        "the claim carried the single-use ticket and the directory"
    );
}

/// Arm A discard — the ticket is rejected: warn, best-effort shutdown,
/// abandon to a fresh identity.
#[tokio::test]
async fn adopt_rejects_a_bad_ticket_and_discards() {
    let (frontend, _bus, launcher) = daemon_frontend("arm-a-bad").await;
    frontend.create(&session(), create_input("owned")).await.expect("create");
    let state = launcher.last_state().expect("state");
    let directory = PathBuf::from(frontend.runtime_dir().expect("runtime dir"));
    let handoff = future_handoff(&directory, &state.instance_id, "wrong-ticket");
    match frontend.adopt(&handoff).await {
        Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::invalid_ticket()),
        other => panic!("a rejected ticket discards, got {other:?}"),
    }
    assert!(
        state.shutdowns.load(Ordering::SeqCst) >= 1,
        "the discard path best-effort shuts the found daemon down"
    );
}

/// Arm B — the daemon died after `prepare_handoff`: `state.json`
/// restores through the fresh daemon's owner connection.
#[tokio::test]
async fn adopt_restores_from_snapshot_when_the_daemon_died() {
    let (frontend, _bus, launcher) =
        daemon_frontend_with_script("arm-b", vec![LaunchAction::Start(FakeConfig::default())])
            .await;
    // The dead predecessor's directory (outside the client root).
    let predecessor_root = temp_root("arm-b-predecessor");
    let directory = predecessor_root.join("deadhand");
    std::fs::create_dir_all(&directory).expect("mkdir");
    write_registration(
        &directory,
        &Registration {
            instance_id: "inst_dead".to_owned(),
            pid: 1,
            protocol: PTY_PROTOCOL_VERSION,
            socket: directory.join("daemon.sock").to_string_lossy().into_owned(),
            token: "tok_dead".to_owned(),
        },
    );
    std::fs::write(directory.join("service.lock"), b"").expect("lock file (flock free)");
    let state_json = HandoffState {
        instance_id: "inst_dead".to_owned(),
        ticket: "tick-b".to_owned(),
        expires_at: 4_102_444_800_000,
        terminals: vec![TerminalSnapshot {
            info: Info {
                id: PtyId("pty_persistent_1".to_owned()),
                title: "predecessor".to_owned(),
                command: "/bin/sh".to_owned(),
                args: vec![],
                cwd: "/tmp".to_owned(),
                status: Status::Running,
                pid: 1,
                exit_code: None,
                session_id: session(),
                foreground_process: None,
                size: (80, 24),
                output: (0, 0),
            },
            buffer: base64_std().encode(b"replay-window"),
            buffer_cursor: 0,
            rows: vec!["replay-window".to_owned()],
            cursor: (0, 0),
            generation: 1,
        }],
    };
    std::fs::write(
        directory.join("state.json"),
        serde_json::to_string(&state_json).expect("state json"),
    )
    .expect("write state.json");
    let root =
        PathBuf::from(frontend.runtime_dir().expect("runtime dir")).parent().unwrap().to_path_buf();

    let handoff = future_handoff(&directory, "inst_dead", "tick-b");
    match frontend.adopt(&handoff).await {
        Adoption::Adopted { restored, directory: adopted } => {
            assert_eq!(restored, 1, "the snapshot's terminals were restored");
            assert_eq!(PathBuf::from(adopted), root.join("deadhand"), "migrated into the root");
        }
        other => panic!("arm B restores from the snapshot, got {other:?}"),
    }
    assert!(
        !root.join("deadhand").join("state.json").exists(),
        "state.json consumed by the restore"
    );
    let state = launcher.last_state().expect("state");
    let restores = state.restores.lock();
    assert_eq!(restores.len(), 1, "the restore op rode the owner connection");
    assert_eq!(restores[0].terminals.len(), 1);
}

/// Discards: expired, no state.json, lock still held.
#[tokio::test]
async fn adopt_discard_reasons() {
    let (frontend, _bus, launcher) = daemon_frontend("discards").await;
    let root =
        PathBuf::from(frontend.runtime_dir().expect("runtime dir")).parent().unwrap().to_path_buf();
    // Expired.
    let expired = Handoff {
        directory: root.join("x").to_string_lossy().into_owned(),
        instance_id: "i".to_owned(),
        ticket: "t".to_owned(),
        expires_at: 1,
    };
    match frontend.adopt(&expired).await {
        Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::expired()),
        other => panic!("expired discards, got {other:?}"),
    }

    // Dead daemon, no state.json.
    let directory = root.join("nostate");
    std::fs::create_dir_all(&directory).expect("mkdir");
    write_registration(
        &directory,
        &Registration {
            instance_id: "inst_nostate".to_owned(),
            pid: 1,
            protocol: PTY_PROTOCOL_VERSION,
            socket: directory.join("daemon.sock").to_string_lossy().into_owned(),
            token: "t".to_owned(),
        },
    );
    match frontend.adopt(&future_handoff(&directory, "inst_nostate", "t")).await {
        Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::no_state()),
        other => panic!("no state discards, got {other:?}"),
    }

    // Dead socket but the lock is HELD (unreachable owner).
    let held_dir = root.join("held");
    std::fs::create_dir_all(&held_dir).expect("mkdir");
    write_registration(
        &held_dir,
        &Registration {
            instance_id: "inst_held".to_owned(),
            pid: 1,
            protocol: PTY_PROTOCOL_VERSION,
            socket: held_dir.join("daemon.sock").to_string_lossy().into_owned(),
            token: "t".to_owned(),
        },
    );
    std::fs::write(
        held_dir.join("state.json"),
        serde_json::to_string(&HandoffState {
            instance_id: "inst_held".to_owned(),
            ticket: "t".to_owned(),
            expires_at: 4_102_444_800_000,
            terminals: vec![],
        })
        .expect("state"),
    )
    .expect("write");
    let _held = flock_new(&held_dir.join("service.lock"));
    match frontend.adopt(&future_handoff(&held_dir, "inst_held", "t")).await {
        Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::owned()),
        other => panic!("a held lock discards owned, got {other:?}"),
    }
    assert_eq!(launcher.launches.lock().len(), 0, "no spawns on the discard paths");
}

// ---------------------------------------------------------------------------
// In-process fallback path parity
// ---------------------------------------------------------------------------

/// The `OC_PTY_DAEMON=0` fallback: same op surface, machine behavior.
#[tokio::test]
async fn in_process_fallback_behaves_like_the_machine() {
    let bus = bus_for_test().await;
    let frontend = Frontend::in_process(bus);
    let error = frontend.read(&session(), Some(0)).await.expect_err("lines=0 rejected identically");
    assert_eq!(error.message, "lines must be an integer between 1 and 65535");
    let listed = frontend.list(None).await.expect("list");
    assert!(listed.is_empty());
    let error = frontend.get(&PtyId("pty_persistent_9".to_owned())).await.expect_err("not found");
    assert!(
        error.message.starts_with("PersistentPty.NotFoundError"),
        "the NotFound message shape handlers match on, got: {error}"
    );
}

// ---------------------------------------------------------------------------
// Registration ladder: auth failure drop + rediscover
// ---------------------------------------------------------------------------

/// §5.2: `error{"authentication failed"}` ⇒ kind registration ⇒ drop
/// the cached registration ⇒ rediscover ⇒ retry once.
#[tokio::test]
async fn auth_failure_drops_the_registration_and_recovers() {
    let (frontend, _bus, state) = prestarted_frontend(
        "auth-recover",
        FakeConfig { fix_registration_after_auth_failure: true, ..FakeConfig::default() },
    )
    .await;
    let directory = state.directory.clone();
    // Corrupt the on-disk token; the daemon corrects the registration
    // on its first auth failure, so the retry recovers.
    let mut corrupted = read_registration(&directory).expect("registration");
    corrupted.token = "stale-token".to_owned();
    write_registration(&directory, &corrupted);

    let listed = frontend.list(None).await.expect("list recovers after re-discovery");
    assert!(listed.is_empty());
    assert!(
        state.auth_failures.load(Ordering::SeqCst) >= 1,
        "the first exchange really failed auth"
    );
}

/// `handoff()` with no daemon running is `None` (v2's transport before
/// it first connects).
#[tokio::test]
async fn handoff_without_a_daemon_is_none() {
    let (frontend, _bus, _launcher) = daemon_frontend("handoff-none").await;
    assert!(frontend.handoff().await.expect("handoff").is_none());
}

/// `handoff()` over the owner connection carries directory/instance.
#[tokio::test]
async fn handoff_rides_the_owner_connection() {
    let (frontend, _bus, _launcher) = daemon_frontend_with_script(
        "handoff-owner",
        vec![LaunchAction::Start(FakeConfig {
            accept_ticket: Some("tick-1".to_owned()),
            ..FakeConfig::default()
        })],
    )
    .await;
    frontend.create(&session(), create_input("h")).await.expect("create");
    let handoff = frontend.handoff().await.expect("handoff").expect("ticket");
    let directory = PathBuf::from(frontend.runtime_dir().expect("runtime dir"));
    assert_eq!(PathBuf::from(&handoff.directory), directory);
    assert_eq!(handoff.ticket, "tick-1");
    assert!(handoff.expires_at > 0);
}
