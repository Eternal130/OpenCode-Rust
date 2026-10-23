//! D3 process-level integration tests — the REAL `oc-pty` binary behind
//! the REAL oc-core client transport (design §10.3, items I1–I15's
//! process slice). No fake daemon, no in-process `run()`: every
//! scenario spawns the actual `oc-pty` daemon binary (its own process,
//! detached like production — `stdio` null, own process group) and
//! drives it through `oc_core::persistent_pty::frontend::Frontend`
//! (the D2 transport) and raw wire connections.
//!
//! Server-death simulation: the client library runs in the test
//! process, so "the server died" is simulated by CLOSING the server's
//! owner connection exactly as the kernel would on process death —
//! `Transport::set_root` drops the owner connection (EOF at the
//! daemon) and rolls a fresh identity. From the daemon's perspective
//! this is byte-for-byte what a `kill -9` of the server process looks
//! like: owner socket EOF, terminals + children alive (R5). Daemon
//! death is REAL: the launcher keeps every spawned child handle and
//! the tests SIGKILL/SIGTERM only those own handles (§4.4).
//!
//! Hermeticity: tempdir runtime roots under /tmp/opencode, every test
//! ≤60 s, every daemon explicitly drained (or SIGTERM'd by the
//! launcher's Drop guard) — no leaked processes. Run serialized under
//! the repo's pty profile (`--profile pty`, `test-threads = 1`) like
//! the other real-process PTY suites.
#![allow(
    // Each test is one end-to-end scenario narrative (§10.3 style).
    clippy::too_many_lines,
    // Wire fixtures carry the §3 parameter sets verbatim.
    clippy::too_many_arguments,
    // Scenario futures hold the 8 MiB replay buffers they reassemble.
    clippy::large_futures,
    // Adoption arms match the full Adoption enum (D1/D2 suite style).
    clippy::match_wildcard_for_single_variants
)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use futures::StreamExt as _;
use oc_core::bus::{Bus, BusOptions};
use oc_core::database::Database;
use oc_core::persistent_pty::frontend::{Backend, Frontend};
use oc_core::persistent_pty::transport::{ErrorKind, Launcher, Spawned, Transport, TransportError};
use oc_core::persistent_pty::wire::{self, FrameDecoder, WireRequest, WireResponse, WireRole};
use oc_core::persistent_pty::{
    Adoption, AdoptionError, AttachInput, CreateInput, Handoff, HandoffState, Registration, Role,
    StreamEvent,
};
use oc_core::pty::{PtyId, Status};
use oc_schema::id::SessionId;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;

/// The real daemon binary under test (cargo sets this for the crate's
/// own test targets).
const PTY_BIN: &str = env!("CARGO_BIN_EXE_oc-pty");

/// §3.2: a full replay buffer is exactly the machine's `OUTPUT_LIMIT`.
const OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;
/// Fill volume: ≥9 MiB guarantees the 8 MiB ring wrapped (I15(b)); the
/// child writes exactly this many NUL bytes, so the final tail is
/// deterministic.
const FILL_BYTES: usize = 9_437_184;

// ---------------------------------------------------------------------------
// Harness: real-binary launcher + raw wire client
// ---------------------------------------------------------------------------

fn temp_root(tag: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("oc-pty-d3-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("temp dir");
    dir.keep()
}

async fn bus_for_test() -> Bus {
    let database = Database::open_in_memory().await.expect("database");
    Bus::new(database, BusOptions::deterministic())
}

fn session() -> SessionId {
    SessionId::parse("ses_d3").expect("session")
}

fn create_input(title: &str, command: &str, cwd: &Path) -> CreateInput {
    CreateInput {
        command: Some("/bin/sh".to_owned()),
        args: vec!["-c".to_owned(), command.to_owned()],
        cwd: Some(cwd.to_string_lossy().into_owned()),
        title: title.to_owned(),
        env: BTreeMap::new(),
        size: Some((80, 24)),
    }
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// A real `oc-pty` child held by its own handle (kill9/SIGTERM only
/// ever touch THESE pids — §4.4's no-bare-PID-kill rule).
struct RealChild {
    child: parking_lot::Mutex<Option<std::process::Child>>,
    terminated: AtomicBool,
}

impl RealChild {
    /// §4.4(b)-class signal: SIGKILL through the owned child handle.
    fn kill9(&self) {
        if self.terminated.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(mut child) = self.child.lock().take() else { return };
        let pid = child.id();
        if matches!(child.try_wait(), Ok(None)) && pid > 0 {
            // SAFETY: plain kill(2) on one strictly-positive pid — our
            // own spawned child handle, never a pid read from disk.
            #[allow(unsafe_code)] // §4.4: own spawned child handle
            unsafe {
                libc::kill(libc::pid_t::try_from(pid).unwrap_or(0), libc::SIGKILL);
            }
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }

    /// Polls `try_wait` until the child is reaped or the timeout hits.
    fn wait_exit(&self, timeout: Duration) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut guard = self.child.lock();
            let child = guard.as_mut()?;
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            }
            drop(guard);
            assert!(Instant::now() < deadline, "daemon child did not exit in time");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Spawned for RealChild {
    fn has_exited(&self) -> bool {
        if self.terminated.load(Ordering::SeqCst) {
            return true;
        }
        let mut child = self.child.lock();
        match child.as_mut().map(std::process::Child::try_wait) {
            Some(Ok(Some(_)) | Err(_)) | None => true,
            Some(Ok(None)) => false,
        }
    }

    fn terminate(&self) {
        if self.terminated.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(mut child) = self.child.lock().take() else { return };
        if matches!(child.try_wait(), Ok(None)) {
            let pid = child.id();
            if pid > 0 {
                // SAFETY: plain kill(2), one strictly-positive pid —
                // our own spawned child (the §4.4(b) SIGTERM path).
                #[allow(unsafe_code)] // §4.4(b): own spawned child handle
                unsafe {
                    libc::kill(libc::pid_t::try_from(pid).unwrap_or(0), libc::SIGTERM);
                }
            }
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// The test launcher: spawns the REAL binary detached (production
/// shape — `stdio` null, own process group) with a test-chosen orphan
/// grace, keeping every child handle for death simulation + teardown.
struct RealLauncher {
    grace_secs: u64,
    children: parking_lot::Mutex<Vec<Arc<RealChild>>>,
}

impl RealLauncher {
    fn new(grace_secs: u64) -> Self {
        Self { grace_secs, children: parking_lot::Mutex::new(Vec::new()) }
    }

    /// The most recently spawned daemon's child handle.
    fn latest(&self) -> Arc<RealChild> {
        self.children.lock().last().expect("at least one daemon was spawned").clone()
    }

    fn kill9_latest(&self) {
        self.latest().kill9();
    }

    /// Waits for every spawned daemon to be reaped (teardown check).
    fn wait_all(&self, timeout: Duration) {
        for child in self.children.lock().iter() {
            let _ = child.wait_exit(timeout);
        }
    }
}

impl Launcher for RealLauncher {
    fn launch(&self, root: &Path, id: &str) -> Result<Box<dyn Spawned>, TransportError> {
        let mut command = std::process::Command::new(PTY_BIN);
        command
            .args(["daemon", "--runtime-dir"])
            .arg(root)
            .arg("--name")
            .arg(id)
            .arg("--orphan-grace")
            .arg(self.grace_secs.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Detached like production (§8.2): own process group, so
            // the daemon survives the test process's group signals.
            .process_group(0);
        let child = command.spawn().map_err(|error| {
            TransportError::new(ErrorKind::Spawn, format!("spawning {PTY_BIN}: {error}"))
        })?;
        let handle = Arc::new(RealChild {
            child: parking_lot::Mutex::new(Some(child)),
            terminated: AtomicBool::new(false),
        });
        self.children.lock().push(Arc::clone(&handle));
        Ok(Box::new(ChildGuard(handle)))
    }
}

/// Forwards the `Spawned` surface while keeping the handle registered
/// on the launcher (kill9/wait need it after the transport drops its
/// box).
struct ChildGuard(Arc<RealChild>);

impl Spawned for ChildGuard {
    fn has_exited(&self) -> bool {
        self.0.has_exited()
    }

    fn terminate(&self) {
        self.0.terminate();
    }
}

impl Drop for RealLauncher {
    /// Teardown safety net: any daemon still running at test end is
    /// SIGTERM'd through its own child handle (children get HUP as the
    /// masters close — daemon-death semantics).
    fn drop(&mut self) {
        for child in self.children.lock().iter() {
            child.terminate();
        }
    }
}

/// A frontend pair: the `Frontend` for ops + the raw `Transport` for
/// the server-death seam (`set_root` closes the owner connection).
fn daemon_frontend(bus: &Bus, launcher: Arc<RealLauncher>) -> (Frontend, Transport) {
    let transport = Transport::new(bus.clone(), launcher);
    let frontend = Frontend::from_backend(Backend::Daemon(Arc::new(transport.clone())));
    (frontend, transport)
}

/// The daemon-observable effect of server death: the owner connection
/// EOFs (in production the kernel closes the dying process's sockets;
/// here `set_root` performs the same close) and the client identity is
/// gone. Terminals + children stay alive (R5) — that is the assertion
/// every caller makes next.
fn kill_server(transport: &Transport, fresh_root: &Path) {
    transport.set_root(fresh_root);
}

/// A raw §4.1 wire client — used where a test must speak the protocol
/// below the transport (own races, stale records, chunk offsets).
struct RawClient {
    stream: UnixStream,
    decoder: FrameDecoder,
    token: String,
    pending: std::collections::VecDeque<Vec<u8>>,
}

impl RawClient {
    async fn connect(registration: &Registration) -> std::io::Result<Self> {
        let stream = UnixStream::connect(&registration.socket).await?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
            token: registration.token.clone(),
            pending: std::collections::VecDeque::new(),
        })
    }

    async fn send(&mut self, request: &WireRequest) {
        let envelope = wire::Envelope { token: self.token.clone(), request: request.clone() };
        let payload = serde_json::to_vec(&envelope).expect("envelope");
        let frame = wire::encode_frame(&payload).expect("frame");
        self.stream.write_all(&frame).await.expect("write");
    }

    /// Reads one payload frame (any type) with a generous deadline.
    async fn frame(&mut self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(frame) = self.pending.pop_front() {
                return frame;
            }
            assert!(Instant::now() < deadline, "timed out waiting for a frame");
            let mut chunk = vec![0u8; 64 * 1024];
            let read = tokio::time::timeout(Duration::from_secs(8), self.stream.read(&mut chunk))
                .await
                .expect("read within 8s")
                .expect("read ok");
            assert!(read > 0, "daemon closed the connection");
            self.pending.extend(self.decoder.feed(&chunk[..read]).expect("framed"));
        }
    }

    /// One exchange: skip binary/owner frames, return the response.
    async fn request(&mut self, request: WireRequest) -> WireResponse {
        self.send(&request).await;
        loop {
            let frame = self.frame().await;
            if frame.first() == Some(&0x00) {
                continue;
            }
            if serde_json::from_slice::<wire::OwnerFrame>(&frame).is_ok() {
                continue;
            }
            return serde_json::from_slice::<WireResponse>(&frame).unwrap_or_else(|error| {
                panic!("unparseable frame ({error}): {}", String::from_utf8_lossy(&frame))
            });
        }
    }
}

fn parse_json<T: DeserializeOwned>(frame: &[u8]) -> Option<T> {
    serde_json::from_slice(frame).ok()
}

fn read_registration(directory: &Path) -> Option<Registration> {
    let text = std::fs::read_to_string(directory.join("service.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn registration_of(frontend: &Frontend) -> Registration {
    let dir = PathBuf::from(&frontend.runtime_dir().expect("runtime dir"));
    read_registration(&dir).expect("registration")
}

fn proc_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn file_inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path).expect("metadata").ino()
}

fn unix_ms_now() -> u64 {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis()))
        .unwrap_or(0)
}

fn base64_decode_std(data: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(data).expect("base64")
}

/// Polls an async predicate until it holds or the deadline hits.
async fn wait_until<F, Fut>(secs: u64, what: &str, mut predicate: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if predicate().await {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The terminal's current absolute output tail.
async fn output_tail(frontend: &Frontend, id: &PtyId) -> u64 {
    u64::try_from(frontend.get(id).await.expect("get").output.1).unwrap_or(0)
}

/// Waits until the terminal's absolute output tail reaches `at_least`.
async fn wait_for_output(frontend: &Frontend, id: &PtyId, at_least: usize) {
    let target = u64::try_from(at_least).unwrap_or(u64::MAX);
    wait_until(20, "terminal output", || async { output_tail(frontend, id).await >= target }).await;
}

/// Collects live stream events from an attachment (output bytes +
/// structured events) for the assertions that follow `activate`.
#[derive(Default, Clone)]
struct EventLog {
    output: Arc<parking_lot::Mutex<Vec<u8>>>,
    events: Arc<parking_lot::Mutex<Vec<StreamEvent>>>,
}

impl EventLog {
    fn new() -> Self {
        Self::default()
    }

    fn attach_input<'a>(&self, attachment_id: &'a str, cursor: usize) -> AttachInput<'a> {
        let log = self.clone();
        AttachInput {
            cursor,
            attachment_id,
            role: Role::Observer,
            takeover: false,
            on_event: Box::new(move |event| {
                if let StreamEvent::Output(bytes) = &event {
                    log.output.lock().extend_from_slice(bytes);
                }
                log.events.lock().push(event);
            }),
            on_end: Box::new(|| {}),
        }
    }

    fn output_contains(&self, needle: &[u8]) -> bool {
        bytes_contain(&self.output.lock(), needle)
    }

    fn take_resized(&self) -> Option<(u16, u16, Vec<u8>)> {
        self.events.lock().iter().find_map(|event| match event {
            StreamEvent::Resized { cols, rows, checkpoint, .. } => {
                Some((*cols, *rows, checkpoint.clone()))
            }
            _ => None,
        })
    }
}

// ---------------------------------------------------------------------------
// I1 FLAGSHIP — kill server → restart → adopt (arm A) → child ALIVE
// (+ I2 single-use ticket pin)
// ---------------------------------------------------------------------------

/// §10.3 I1: server A creates a terminal running a marker child,
/// prepares a handoff, DIES; server B boots from the handoff ticket
/// and adopts the SAME live daemon — the child is the SAME pid, still
/// running, still echoing input, and the replay window carries the
/// pre-death bytes. The consumed ticket is single-use (I2).
#[tokio::test]
async fn pty_d3_i1_kill_server_arm_a_adopt_child_alive() {
    let root = temp_root("i1");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    // A marker child: pidfile + marker output + `cat` (echoes input).
    let pidfile = root.join("i1-child.pid");
    let command = format!("echo $$ > {}; printf I1-MARKER; cat", pidfile.display());
    let info = a.create(&session(), create_input("i1", &command, &root)).await.expect("create");
    wait_for_output(&a, &info.id, "I1-MARKER".len()).await;

    let child_pid =
        std::fs::read_to_string(&pidfile).expect("pidfile").trim().parse::<u32>().expect("pid");
    let listed = a.list(None).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].pid, child_pid, "the terminal's pid is the marker child");
    assert_eq!(listed[0].status, Status::Running);
    assert!(proc_alive(child_pid));

    // prepare_handoff: ticket + crash-insurance state.json (§4.3).
    let handoff = a.handoff().await.expect("handoff op").expect("handoff ticket");
    let dir_a = PathBuf::from(&handoff.directory);
    assert!(dir_a.join("state.json").exists(), "crash insurance written");

    // --- server A dies (owner EOF; children must survive — R5) -----
    kill_server(&transport_a, &root.join("dead-a"));
    drop(a);
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The daemon is still serving; the child is still alive.
    let registration = read_registration(&dir_a).expect("registration");
    let mut raw = RawClient::connect(&registration).await.expect("probe conn");
    assert!(matches!(raw.request(WireRequest::Ping).await, WireResponse::Pong { .. }));
    assert!(proc_alive(child_pid), "R5: the child outlives the server");

    // --- server B boots with the handoff → ARM A live claim ----------
    let (b, _transport_b) = daemon_frontend(&bus, Arc::new(RealLauncher::new(60)));
    match b.adopt(&handoff).await {
        Adoption::Adopted { restored, directory } => {
            assert_eq!(restored, 1);
            assert_eq!(PathBuf::from(&directory), dir_a, "same-dir adoption (no migration)");
        }
        other => panic!("expected arm-A adoption, got {other:?}"),
    }

    // list still shows the terminal; the CHILD IS STILL ALIVE.
    let listed = b.list(None).await.expect("list after adopt");
    assert_eq!(listed.len(), 1, "the terminal survived the server restart");
    assert_eq!(listed[0].status, Status::Running);
    assert_eq!(listed[0].pid, child_pid, "SAME child pid across the restart");
    assert!(proc_alive(child_pid));

    // Replay window restores the pre-restart bytes (attach at 0).
    let log = EventLog::new();
    let attachment = b.attach(&info.id, log.attach_input("i1-server-b", 0)).await.expect("attach");
    assert!(bytes_contain(&attachment.data, b"I1-MARKER"), "replay carries pre-death bytes");
    attachment.activate();

    // New input reaches the SAME child: `cat` echoes it back.
    b.write(&info.id, "i1-ping\n", None).await.expect("write");
    wait_until(5, "echo through the adopted child", || async { log.output_contains(b"i1-ping") })
        .await;

    // I2: the ticket is single-use — a second claim is rejected over
    // the real wire, and the owning `own` consumed state.json.
    let mut contender = RawClient::connect(&registration).await.expect("contender conn");
    match contender
        .request(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: Some(handoff.ticket.clone()),
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => {
            assert_eq!(message, "PTY restart handoff ticket rejected");
        }
        other => panic!("expected consumed-ticket rejection, got {other:?}"),
    }
    assert!(!dir_a.join("state.json").exists(), "state.json consumed by the owning own");

    // Teardown: drain the daemon (children die with it — by design).
    b.remove(&info.id).await.expect("remove");
    b.shutdown().await.expect("shutdown");
    attachment.detach();
}

// ---------------------------------------------------------------------------
// I2 — handoff ticket expiry discards + fresh identity
// ---------------------------------------------------------------------------

/// §10.3 I2 (expiry arm): an expired ticket discards BEFORE any daemon
/// contact ladder, best-effort shuts the found daemon down, and the
/// discarding frontend recovers on a fresh identity (create spawns a
/// brand-new daemon — boot stays green).
#[tokio::test]
async fn pty_d3_i2_expired_handoff_discards_and_recovers_fresh() {
    let root = temp_root("i2");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, _transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    let _info = a
        .create(&session(), create_input("i2", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    let handoff = a.handoff().await.expect("handoff op").expect("ticket");
    let old_instance = registration_of(&a).instance_id;

    // Expire the ticket (the ladder's first check, daemon.ts:406-407).
    let expired = Handoff { expires_at: unix_ms_now().saturating_sub(1), ..handoff.clone() };
    match a.adopt(&expired).await {
        Adoption::Discarded { reason } => assert_eq!(reason, AdoptionError::expired()),
        other => panic!("expected expired discard, got {other:?}"),
    }

    // The discard's best-effort TOKEN-level shutdown drained the found
    // daemon (§6 ANY-Discard) — its child (ours) is reaped.
    launcher.wait_all(Duration::from_secs(8));

    // Boot stays green on a FRESH identity: create spawns a new daemon
    // (the new daemon's own terminal counter restarts — ids are
    // per-daemon; the identity assertions carry the separation).
    let _info2 = a
        .create(&session(), create_input("i2-fresh", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create after discard");
    let new_dir = PathBuf::from(&a.runtime_dir().expect("runtime dir"));
    assert_ne!(new_dir, PathBuf::from(&handoff.directory), "the identity rolled fresh");
    let new_instance = read_registration(&new_dir).expect("new registration").instance_id;
    assert_ne!(new_instance, old_instance, "a brand-new daemon instance");

    a.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I3 — SIGKILL the daemon → arm B snapshot restore (state.json)
// ---------------------------------------------------------------------------

/// §10.3 I3: SIGKILL the daemon through its own child handle AFTER a
/// handoff snapshot exists; a replacement server (fresh root — the
/// handoff dir is outside it) adopts via arm B: files migrate
/// inode-preservingly, a fresh daemon spawns, `restore` rebuilds the
/// terminal as EXITED (children died with the daemon — v2 semantics),
/// the replay window survives, ids continue.
#[tokio::test]
async fn pty_d3_i3_sigkill_daemon_arm_b_snapshot_restore() {
    let root_a = temp_root("i3a");
    let root_b = temp_root("i3b");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, _transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root_a).expect("set root");

    let info = a
        .create(&session(), create_input("i3", "printf I3-MARKER; sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    wait_for_output(&a, &info.id, "I3-MARKER".len()).await;
    let child_pid = a.list(None).await.expect("list")[0].pid;
    let handoff = a.handoff().await.expect("handoff op").expect("ticket");
    let dir_a = PathBuf::from(&handoff.directory);
    let lock_inode_a = file_inode(&dir_a.join("service.lock"));

    // --- daemon death: SIGKILL our own child handle ------------------
    launcher.kill9_latest();
    launcher.wait_all(Duration::from_secs(8));
    wait_until(10, "child to die with the daemon (HUP on master close)", || async {
        !proc_alive(child_pid)
    })
    .await;

    // --- replacement server, root OUTSIDE the handoff dir → arm B ---
    let (b, _transport_b) = daemon_frontend(&bus, Arc::new(RealLauncher::new(60)));
    b.set_runtime_dir(&root_b).expect("set root");
    let id_name = dir_a.file_name().expect("dir name").to_string_lossy().into_owned();
    let target = root_b.join(&id_name);
    match b.adopt(&handoff).await {
        Adoption::Adopted { restored, directory } => {
            assert_eq!(restored, 1);
            assert_eq!(PathBuf::from(&directory), target, "migrated into the new root");
        }
        other => panic!("expected arm-B adoption, got {other:?}"),
    }

    // Inode-preserving migration (§6 arm B, gate finding 7): the moved
    // lock keeps its identity; the old dir holds neither record.
    assert_eq!(file_inode(&target.join("service.lock")), lock_inode_a);
    assert!(!dir_a.join("service.json").exists());
    assert!(!target.join("state.json").exists(), "snapshot consumed (single-use)");

    // Terminals restored EXITED (v2 daemon-death outcome) with the
    // pre-death replay window intact.
    let listed = b.list(None).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, Status::Exited, "v2 semantics: children died with the daemon");
    let log = EventLog::new();
    let attachment = b.attach(&info.id, log.attach_input("i3-observer", 0)).await.expect("attach");
    assert!(
        bytes_contain(&attachment.data, b"I3-MARKER"),
        "replay window restored from the snapshot"
    );
    attachment.detach();

    // Ids continue (pty_persistent_N+1 — no id reset).
    let next = b
        .create(&session(), create_input("i3-next", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create continues the id sequence");
    assert_eq!(next.id.0, "pty_persistent_2");

    b.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I4 — checkpoint v1 round-trip over the real wire (§7)
// ---------------------------------------------------------------------------

/// §10.3 I4: create → output → resize → `snapshot` decodes as a §7 v1
/// checkpoint (magic/version/crc/cols/rows/scrollback/cursor), a
/// corrupted byte fails the CRC, and the `resized` STREAM event
/// carries its own decodable checkpoint (the resize fast-forward
/// path).
#[tokio::test]
async fn pty_d3_i4_checkpoint_v1_round_trip_over_the_wire() {
    let root = temp_root("i4");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, _transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    let info = a
        .create(&session(), create_input("i4", "printf CKPT-LINE; sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    wait_for_output(&a, &info.id, "CKPT-LINE".len()).await;

    // Resize (the snapshot path captures the post-resize state).
    a.resize(&info.id, 120, 20, None).await.expect("resize");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let snapshot = a.snapshot(&info.id).await.expect("snapshot");
    assert!(snapshot.text.contains("CKPT-LINE"));
    assert_eq!(snapshot.info.size, (120, 20));
    assert_eq!(&snapshot.checkpoint[0..8], b"OCPTYCHK", "§7 magic");
    let decoded = wire::decode_checkpoint(&snapshot.checkpoint).expect("v1 checkpoint decodes");
    assert_eq!((decoded.cols, decoded.rows), (120, 20));
    assert_eq!(decoded.scrollback, snapshot.text, "scrollback == snapshot text");
    assert_eq!(
        (
            usize::try_from(decoded.cursor_x).unwrap_or(0),
            usize::try_from(decoded.cursor_y).unwrap_or(0)
        ),
        snapshot.cursor,
        "cursor fields round-trip"
    );

    // A corrupted byte is a CRC failure — never silent misreads.
    let mut corrupted = snapshot.checkpoint.clone();
    let flip = corrupted.len() - 6; // inside the scrollback blob
    corrupted[flip] = corrupted[flip].wrapping_add(1);
    assert_eq!(
        wire::decode_checkpoint(&corrupted),
        Err(wire::CheckpointError::Crc),
        "corrupted checkpoint rejected by crc"
    );

    // The `resized` stream event carries its own decodable checkpoint.
    let log = EventLog::new();
    let attachment = a.attach(&info.id, log.attach_input("i4-observer", 0)).await.expect("attach");
    attachment.activate();
    a.resize(&info.id, 130, 24, None).await.expect("resize 2");
    wait_until(5, "resized stream event", || async { log.take_resized().is_some() }).await;
    let (cols, rows, checkpoint) = log.take_resized().expect("resized event observed");
    assert_eq!((cols, rows), (130, 24));
    let decoded = wire::decode_checkpoint(&checkpoint).expect("event checkpoint decodes");
    assert_eq!((decoded.cols, decoded.rows), (130, 24));

    attachment.detach();
    a.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I5 — stale registration + boot-generation guard (§4.4)
// ---------------------------------------------------------------------------

/// §10.3 I5: a fabricated registration pointing at a dead socket with
/// a `service.boot` naming a RECYCLED pid (a live pid — ours — with a
/// mismatched `start_time`): the generation guard refuses to treat it as
/// live, the client's connect-probe fails, spawn-if-absent reclaims
/// the directory, and NOTHING is signalled (the recorded pid is this
/// test process — a rogue signal would kill the test).
#[tokio::test]
async fn pty_d3_i5_stale_registration_generation_guard_reclaims() {
    let root = temp_root("i5");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    // Fabricate the PREVIOUS run's stale records inside the identity
    // directory the transport just claimed for itself (§5.2: a boot
    // only ever looks in its OWN directory).
    let dir = transport_a.current_directory().expect("identity dir");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    let stale = Registration {
        instance_id: "stale-instance".to_owned(),
        // A RECYCLED-pid shape: this test's OWN pid (alive!) with the
        // WRONG start_time — liveness must be refused by generation.
        pid: std::process::id(),
        protocol: 7,
        socket: dir.join("daemon.sock").to_string_lossy().into_owned(),
        token: "0123456789abcdef0123456789abcdef".to_owned(),
    };
    std::fs::write(dir.join("service.json"), serde_json::to_string(&stale).expect("json"))
        .expect("write service.json");
    let wrong_start = wire::proc_start_time(std::process::id()).map_or(1, |start| start + 999);
    std::fs::write(
        dir.join("service.boot"),
        serde_json::to_string(&wire::BootRecord {
            boot_id: wire::boot_id().expect("boot id"),
            start_time: wrong_start,
        })
        .expect("json"),
    )
    .expect("write service.boot");
    // No daemon.sock (dead socket path) and no lock holder.

    // §4.4 generation guard: a recycled pid is DEAD even though
    // /proc/<pid> exists.
    assert_eq!(wire::registration_alive(&dir), Some(false));

    // The client's connect-probe fails (no listener), so `create`
    // spawn-if-absent reclaims the directory with a fresh daemon.
    let info = a
        .create(&session(), create_input("i5", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create reclaims the stale directory");
    let reclaimed = read_registration(&dir).expect("reclaimed registration");
    assert_ne!(reclaimed.pid, stale.pid, "a NEW daemon process owns the dir");
    assert_ne!(reclaimed.instance_id, stale.instance_id);
    assert!(dir.join("daemon.sock").exists(), "the new daemon bound the socket");
    let listed = a.list(None).await.expect("list");
    assert_eq!(listed.len(), 1);

    // Nothing was signalled: the stale record named THIS process.
    assert!(proc_alive(std::process::id()), "no rogue signal hit the recorded pid");

    a.remove(&info.id).await.expect("remove");
    a.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I9 — competing claims: exactly one winner
// ---------------------------------------------------------------------------

/// §10.3 I9/I10 slice: two replacement servers boot from the SAME
/// inherited handoff — exactly one adopts (the migration+claim race
/// resolves to one); the loser discards, gets a fresh identity and its
/// OWN daemon, and — because its claim never reached the found daemon
/// after migration — the winner keeps serving. While owned, a bare
/// `own` (no ticket) is refused with `error{"daemon is owned"}`.
#[tokio::test]
async fn pty_d3_i9_competing_claims_exactly_one_wins() {
    let root_a = temp_root("i9a");
    let root_b = temp_root("i9b");
    let root_c = temp_root("i9c");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root_a).expect("set root");

    let _info = a
        .create(&session(), create_input("i9", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    let child_pid = a.list(None).await.expect("list")[0].pid;
    let handoff = a.handoff().await.expect("handoff op").expect("ticket");
    kill_server(&transport_a, &root_a.join("dead"));
    drop(a);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Winner: B (fresh root → the client migrates the runtime files
    // inode-preservingly, then claims with the ticket).
    let (b, _transport_b) = daemon_frontend(&bus, Arc::new(RealLauncher::new(60)));
    b.set_runtime_dir(&root_b).expect("set root");
    match b.adopt(&handoff).await {
        Adoption::Adopted { restored, .. } => assert_eq!(restored, 1),
        other => panic!("expected B to win the claim, got {other:?}"),
    }

    // Loser: C — the SAME handoff, but the files already moved.
    let (c, _transport_c) = daemon_frontend(&bus, Arc::new(RealLauncher::new(60)));
    c.set_runtime_dir(&root_c).expect("set root");
    match c.adopt(&handoff).await {
        Adoption::Discarded { reason } => {
            // The loser saw the migrated-away registration — never a
            // second adoption of the same daemon.
            assert_eq!(reason, AdoptionError::unreachable());
        }
        other => panic!("expected C to lose the claim, got {other:?}"),
    }

    // The winner is functional: same child alive across the restart.
    let winner_list = b.list(None).await.expect("winner list");
    assert_eq!(winner_list.len(), 1);
    assert_eq!(winner_list[0].pid, child_pid);
    assert!(proc_alive(child_pid));

    // The winner's daemon is owned: a bare `own` without a ticket is
    // refused (the wire-level competing-claim pin).
    let registration = registration_of(&b);
    let mut contender = RawClient::connect(&registration).await.expect("contender conn");
    match contender
        .request(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: None,
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => assert_eq!(message, "daemon is owned"),
        other => panic!("expected owned refusal, got {other:?}"),
    }

    // The loser recovered on its OWN fresh identity + OWN daemon.
    let _info_c = c
        .create(&session(), create_input("i9-loser", "sleep 30", Path::new("/tmp")))
        .await
        .expect("loser create");
    let loser_instance = registration_of(&c).instance_id;
    assert_ne!(loser_instance, registration.instance_id, "a separate daemon instance");

    // C's discard did NOT take the winner's daemon down (the files had
    // already migrated — no shutdown could be addressed to it).
    assert_eq!(b.list(None).await.expect("winner still serving").len(), 1);

    b.shutdown().await.expect("shutdown b");
    c.shutdown().await.expect("shutdown c");
}

// ---------------------------------------------------------------------------
// I14 — kill -9 the SERVER without prepare_handoff (orphan case)
// ---------------------------------------------------------------------------

/// §10.3 I14: the server dies WITHOUT a handoff — the daemon is an
/// orphan that keeps serving its live children, but a fresh server
/// never discovers it (fresh random identity, `list` ⇒ [] — the
/// v2-faithful loss), spawns its OWN daemon instead, and no
/// unauthorized ownership op succeeds (§4.3 owner-only guards). When
/// the orphan's last terminal leaves and the (shortened) grace
/// elapses, it self-exits.
#[tokio::test]
async fn pty_d3_i14_kill9_server_orphan_loss_and_grace_self_exit() {
    let root = temp_root("i14");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(2)); // shortened grace
    let (a, transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    let info = a
        .create(&session(), create_input("i14", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    let orphan_dir = PathBuf::from(&a.runtime_dir().expect("orphan dir"));
    let child_pid = a.list(None).await.expect("list")[0].pid;
    let orphan_instance = read_registration(&orphan_dir).expect("orphan registration").instance_id;

    // --- server dies WITHOUT prepare_handoff --------------------------
    kill_server(&transport_a, &root.join("dead"));
    drop(a);
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The orphan daemon + its child are alive (R5 — that IS the
    // persistence), just no longer owned.
    let registration = read_registration(&orphan_dir).expect("orphan registration");
    let mut raw = RawClient::connect(&registration).await.expect("orphan probe");
    assert!(matches!(raw.request(WireRequest::Ping).await, WireResponse::Pong { .. }));
    assert!(proc_alive(child_pid), "the orphan keeps its children alive");

    // A fresh server never discovers the orphan: fresh identity, and
    // `list` is soft-empty (v2-faithful loss — no root scanning).
    let (b, _transport_b) = daemon_frontend(&bus, Arc::new(RealLauncher::new(60)));
    b.set_runtime_dir(&root).expect("set root");
    let fresh_dir = PathBuf::from(&b.runtime_dir().expect("fresh dir"));
    assert_ne!(fresh_dir, orphan_dir, "fresh random identity — never the orphan's dir");
    assert!(b.list(None).await.expect("list").is_empty(), "the orphan is invisible");
    assert!(
        !fresh_dir.join("service.json").exists(),
        "the fresh identity directory is untouched by the orphan"
    );

    // The fresh server's create spawns its OWN daemon (a second one;
    // each daemon numbers its own terminals from 1).
    let info_b = b
        .create(&session(), create_input("i14-fresh", "sleep 30", Path::new("/tmp")))
        .await
        .expect("fresh create");
    let fresh_instance = read_registration(&fresh_dir).expect("fresh registration").instance_id;
    assert_ne!(fresh_instance, orphan_instance, "two distinct daemons coexist");

    // No unauthorized ownership op: bare token holders cannot inject
    // terminal history or mint tickets (§4.3 owner-only pins).
    let state = HandoffState {
        instance_id: orphan_instance.clone(),
        ticket: "not-a-ticket".to_owned(),
        expires_at: unix_ms_now() + 60_000,
        terminals: Vec::new(),
    };
    match raw.request(WireRequest::Restore { state }).await {
        WireResponse::Error { message } => assert_eq!(message, "owner connection required"),
        other => panic!("expected owner-only guard on restore, got {other:?}"),
    }
    match raw.request(WireRequest::PrepareHandoff).await {
        WireResponse::Error { message } => assert_eq!(message, "owner connection required"),
        other => panic!("expected owner-only guard on handoff, got {other:?}"),
    }

    // Orphan lifecycle: the last terminal leaves (TOKEN-level
    // terminate), so zero terminals + no owner → grace (2 s) self-exit.
    let number = wire::parse_terminal_number(&info.id).expect("terminal number");
    assert!(
        matches!(raw.request(WireRequest::Terminate { id: number }).await, WireResponse::Ok),
        "token-level terminate of the orphan terminal"
    );
    let status = launcher
        .latest()
        .wait_exit(Duration::from_secs(10))
        .expect("orphan self-exited after the grace");
    assert_eq!(status.code(), Some(0), "the orphan-grace self-exit is a clean 0");

    b.remove(&info_b.id).await.expect("remove");
    b.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I15(b) — full-8 MiB replay arm: chunked transfer + deferred completion
// ---------------------------------------------------------------------------

/// §10.3 I15(b): drive the buffer past the 1 MiB inline threshold all
/// the way to the FULL 8 MiB window, then attach — the raw wire shows
/// `replay_chunked` + binary frames with ABSOLUTE offsets reassembling
/// contiguously into exactly `replay_length` bytes and a deferred
/// `replay_complete`; the CLIENT side returns from `attach` only after
/// the full reassembly (deferred completion, §3.2).
#[tokio::test]
async fn pty_d3_i15_full_buffer_chunked_replay_with_absolute_offsets() {
    let root = temp_root("i15");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, _transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    let info = a
        .create(
            &session(),
            create_input(
                "i15",
                &format!("head -c {FILL_BYTES} /dev/zero; sleep 30"),
                Path::new("/tmp"),
            ),
        )
        .await
        .expect("create");
    // The child writes exactly FILL_BYTES NULs — the final tail is
    // deterministic (no echo, no prompts).
    wait_for_output(&a, &info.id, FILL_BYTES).await;
    let tail = output_tail(&a, &info.id).await;
    assert_eq!(tail, FILL_BYTES as u64, "the 8 MiB ring wrapped exactly once over");
    let number = wire::parse_terminal_number(&info.id).expect("terminal number");
    let registration = registration_of(&a);

    // RAW arm: the wire contract (offsets, chunking, deferred marker).
    let mut raw = RawClient::connect(&registration).await.expect("raw conn");
    raw.send(&WireRequest::Subscribe {
        id: number,
        offset: 0,
        attachment_id: "i15-raw".to_owned(),
        role: WireRole::Observer,
        takeover: false,
    })
    .await;
    let attached = raw.frame().await;
    let WireResponse::Attached {
        requested_offset,
        available_offset,
        end_offset,
        truncated,
        replay_base64,
        replay_chunked,
        replay_length,
        ..
    } = parse_json::<WireResponse>(&attached).expect("attached frame")
    else {
        panic!("expected attached, got {}", String::from_utf8_lossy(&attached));
    };
    assert!(replay_base64.is_none(), "no inline replay at full-buffer size");
    assert_eq!(replay_chunked, Some(true));
    assert_eq!(replay_length, Some(OUTPUT_LIMIT), "exactly the capped window");
    assert!(truncated, "offset 0 predates the wrapped window");
    assert_eq!(requested_offset, 0);
    assert_eq!(end_offset, tail, "end_offset is the absolute output tail");
    assert_eq!(
        available_offset,
        tail - OUTPUT_LIMIT,
        "the window starts at the wrapped ring's head"
    );

    // Binary frames reassemble EXACTLY by absolute offset, then the
    // deferred replay_complete marker (client completion defers to it).
    let base = end_offset - OUTPUT_LIMIT;
    let mut assembled_end = base;
    let mut total = 0u64;
    let complete = loop {
        let frame = raw.frame().await;
        if frame.first() == Some(&0x00) {
            let chunk = wire::parse_output_frame(&frame).expect("output frame");
            assert_eq!(chunk.start, assembled_end, "contiguous absolute offset");
            assert!(chunk.end > chunk.start);
            assert!(
                chunk.data.iter().all(|byte| *byte == 0),
                "the written bytes arrive uncorrupted"
            );
            assembled_end = chunk.end;
            total += chunk.data.len() as u64;
        } else {
            match parse_json::<WireResponse>(&frame) {
                Some(WireResponse::ReplayComplete { end_offset }) => {
                    assert_eq!(end_offset, assembled_end, "marker lands exactly at the tail");
                    break end_offset;
                }
                other => panic!("no stream frame may precede replay_complete: {other:?}"),
            }
        }
    };
    assert_eq!(complete, tail);
    assert_eq!(total, OUTPUT_LIMIT, "the full window reassembled");
    drop(raw);

    // CLIENT arm: `attach` returns only AFTER contiguous reassembly
    // (the deferred completion surfaced to the WS consumer, §3.2).
    let log = EventLog::new();
    let attachment = a.attach(&info.id, log.attach_input("i15-client", 0)).await.expect("attach");
    assert!(attachment.replay.truncated);
    assert_eq!(attachment.data.len(), usize::try_from(OUTPUT_LIMIT).expect("window size"));
    assert!(attachment.data.iter().all(|byte| *byte == 0));
    assert_eq!(
        u64::try_from(attachment.replay.end_offset).unwrap_or(0),
        tail,
        "the reassembled stream ends at the absolute tail"
    );
    attachment.detach();

    a.remove(&info.id).await.expect("remove");
    a.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// D1-interplay pins — deferred stream events + slow subscriber
// ---------------------------------------------------------------------------

/// The D1↔D2 protocol contract on a chunked replay connection: NO
/// stream frame (`resized` here; `exited` shares the same ordered
/// writer path) may reach the subscriber before `replay_complete`,
/// and a subscriber that stops reading — blocking the daemon's socket
/// buffer mid-transfer — must not wedge the daemon (other connections
/// keep serving); teardown after the stall is clean.
#[tokio::test]
async fn pty_d3_slow_subscriber_replay_complete_defers_stream_events() {
    let root = temp_root("slow");
    let bus = bus_for_test().await;
    let launcher = Arc::new(RealLauncher::new(60));
    let (a, _transport_a) = daemon_frontend(&bus, Arc::clone(&launcher));
    a.set_runtime_dir(&root).expect("set root");

    let info = a
        .create(
            &session(),
            create_input(
                "slow",
                &format!("head -c {FILL_BYTES} /dev/zero; sleep 30"),
                Path::new("/tmp"),
            ),
        )
        .await
        .expect("create");
    wait_for_output(&a, &info.id, FILL_BYTES).await;
    let tail = output_tail(&a, &info.id).await;
    assert_eq!(tail, FILL_BYTES as u64, "the child finished before the attach");
    let number = wire::parse_terminal_number(&info.id).expect("terminal number");
    let registration = registration_of(&a);

    // Subscribe and read ONLY the `attached` frame, then stall: the
    // socket buffer fills and the daemon's per-connection writer task
    // blocks mid-transfer while a resize queues behind it.
    let mut slow = RawClient::connect(&registration).await.expect("slow conn");
    slow.send(&WireRequest::Subscribe {
        id: number,
        offset: 0,
        attachment_id: "slow-reader".to_owned(),
        role: WireRole::Observer,
        takeover: false,
    })
    .await;
    let attached = slow.frame().await;
    match parse_json::<WireResponse>(&attached) {
        Some(WireResponse::Attached { replay_chunked: Some(true), end_offset, .. }) => {
            assert_eq!(end_offset, tail);
        }
        other => panic!("expected a chunked attach, got {other:?}"),
    }

    // The resize fires while the transfer is stalled behind the full
    // socket buffer (queued on the connection's ordered writer).
    a.resize(&info.id, 150, 40, None).await.expect("resize during the stall");
    tokio::time::sleep(Duration::from_millis(700)).await;

    // The daemon must NOT be wedged: other connections keep serving.
    assert_eq!(a.list(None).await.expect("list during the stall").len(), 1);

    // Resume reading: chunks stay contiguous, `replay_complete` comes
    // before ANY stream frame, and the deferred `resized` follows it.
    let mut assembled_end = None;
    let mut saw_resized = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !saw_resized {
        assert!(Instant::now() < deadline, "resized never followed replay_complete");
        let frame = slow.frame().await;
        if frame.first() == Some(&0x00) {
            let chunk = wire::parse_output_frame(&frame).expect("output frame");
            if let Some(end) = assembled_end {
                assert_eq!(chunk.start, end, "contiguous chunk after the stall");
            }
            assembled_end = Some(chunk.end);
            continue;
        }
        match parse_json::<WireResponse>(&frame) {
            Some(WireResponse::ReplayComplete { end_offset }) => {
                assert_eq!(Some(end_offset), assembled_end);
            }
            Some(WireResponse::Resized { cols, rows, checkpoint_base64, .. }) => {
                assert_eq!((cols, rows), (150, 40), "the deferred resize lands after the marker");
                assert_eq!(
                    wire::decode_checkpoint(&base64_decode_std(&checkpoint_base64))
                        .expect("decodable checkpoint")
                        .cols,
                    150
                );
                saw_resized = true;
            }
            other => panic!("unexpected frame: {other:?}"),
        }
    }

    // Clean teardown after the stall: the daemon drains normally.
    drop(slow);
    a.remove(&info.id).await.expect("remove");
    a.shutdown().await.expect("shutdown");
}

// ---------------------------------------------------------------------------
// I12 + I13 — bus events across the process boundary (EV-1 surface)
// ---------------------------------------------------------------------------

/// §10.3 I12/I13 through the PRODUCTION launcher (§8.1 env override →
/// `BinaryLauncher`): `persistent-pty.added`/`removed` publications
/// cross the process boundary (daemon → owner `bus_event` frames →
/// client republish → server bus) with the EV-1 payload shapes, and an
/// UNATTACHED terminal's spontaneous exit still fires `removed` (the
/// zero-subscriber exit pump — the v2 gap our daemon closes).
#[tokio::test]
async fn pty_d3_i12_bus_events_and_i13_unattached_exit_across_the_boundary() {
    // §8.1: the production binary discovery — env override first.
    std::env::set_var("OPENCODE_PTY_BIN", PTY_BIN);
    let root = temp_root("i12");
    let bus = bus_for_test().await;
    let frontend = Frontend::from_backend(Backend::Daemon(Arc::new(Transport::new(
        bus.clone(),
        Arc::new(oc_core::persistent_pty::transport::BinaryLauncher::new()),
    ))));
    frontend.set_runtime_dir(&root).expect("set root");

    let mut subscription = bus.subscribe_types(&["persistent-pty.added", "persistent-pty.removed"]);

    // I12: create publishes `added` with the EV-1 payload (sessionID +
    // terminal.id — the TUI reducer / parity_event_payloads shape).
    let kept = frontend
        .create(&session(), create_input("i12", "sleep 30", Path::new("/tmp")))
        .await
        .expect("create");
    let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .expect("added within 5s")
        .expect("bus event");
    assert_eq!(event.type_, "persistent-pty.added");
    assert_eq!(event.data["sessionID"], session().as_str());
    assert_eq!(event.data["terminal"]["id"], kept.id.0);

    // I13: an UNATTACHED short-lived child (ZERO subscribers) — the
    // daemon's exit pump still publishes `removed` across the boundary.
    let short = frontend
        .create(&session(), create_input("i13", "sleep 1", Path::new("/tmp")))
        .await
        .expect("create short");
    let deadline = Instant::now() + Duration::from_secs(10);
    let removed = loop {
        assert!(Instant::now() < deadline, "unattached-exit removed never arrived");
        let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
            .await
            .expect("event within 5s")
            .expect("bus event");
        if event.type_ == "persistent-pty.removed"
            && event.data["ptyID"] == short.id.0
            && event.data["sessionID"] == session().as_str()
        {
            break true;
        }
    };
    assert!(removed, "the zero-subscriber exit pump fired persistent-pty.removed");

    // Explicit remove publishes removed as well (the v2 surface).
    frontend.remove(&kept.id).await.expect("remove");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "explicit removed never arrived");
        let event = tokio::time::timeout(Duration::from_secs(5), subscription.next())
            .await
            .expect("event within 5s")
            .expect("bus event");
        if event.type_ == "persistent-pty.removed" && event.data["ptyID"] == kept.id.0 {
            break;
        }
    }

    frontend.shutdown().await.expect("shutdown");
}
