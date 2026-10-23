//! D1 integration tests — the daemon server driven over its real
//! socket (tempdir runtime roots, in-process `run()`). Covers the §10.1
//! daemon-side matrix: auth ladder (§4.3), registration/lock lifecycle
//! (§5.1, §5.3), own/supersession/tickets (§6), `bus_event` owner frames
//! incl. zero-subscriber exits (§3.1), orphan-grace self-exit (§5.1),
//! shutdown drain, replay transport both arms (§3.2), checkpoint
//! round-trip over the wire (§7).
#![allow(
    // Each test is one end-to-end scenario narrative (§10.3 style).
    clippy::too_many_lines,
    // Futures sized by the widest exchange+event frames they hold.
    clippy::large_futures,
    // Wire fixtures carry the §3 parameter sets verbatim.
    clippy::too_many_arguments
)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oc_core::persistent_pty::{
    wire::{self, FrameDecoder, WireRequest, WireResponse},
    Registration,
};
use oc_core::pty::PtyId;
use oc_pty::{DaemonConfig, DEFAULT_ORPHAN_GRACE_SECS};
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;

const LARGE_OUTPUT: usize = 1_500_000; // > the 1 MiB inline threshold

fn temp_root(tag: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("oc-pty-d1-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("temp dir");
    dir.keep()
}

fn config(root: &Path, name: &str, grace_secs: u64) -> DaemonConfig {
    DaemonConfig {
        runtime_root: root.to_path_buf(),
        name: name.to_owned(),
        orphan_grace: Duration::from_secs(grace_secs),
    }
}

/// A spawned daemon: the `run()` task plus its runtime directory.
struct Daemon {
    directory: PathBuf,
    registration: Registration,
    handle: tokio::task::JoinHandle<i32>,
}

impl Daemon {
    async fn start(root: &Path, name: &str) -> Self {
        Self::start_with_grace(root, name, DEFAULT_ORPHAN_GRACE_SECS).await
    }

    async fn start_with_grace(root: &Path, name: &str, grace_secs: u64) -> Self {
        let handle = tokio::spawn(oc_pty::daemon::run(config(root, name, grace_secs)));
        let directory = root.join(name);
        // Readiness = the §8.2 client loop: poll the registration, then
        // connect-probe with ping.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "daemon did not become ready");
            if let Some(registration) = read_registration(&directory) {
                if registration.socket.len() > 1 {
                    if let Ok(mut probe) = Client::connect(&registration).await {
                        if matches!(
                            probe.request(WireRequest::Ping).await,
                            WireResponse::Pong { .. }
                        ) {
                            return Self { directory, registration, handle };
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn exit_code(self) -> i32 {
        tokio::time::timeout(Duration::from_secs(15), self.handle)
            .await
            .expect("daemon exit within 15s")
            .expect("daemon task join")
    }
}

fn read_registration(directory: &Path) -> Option<Registration> {
    let text = std::fs::read_to_string(directory.join("service.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Minimal §5.2-style client: one connection, sequential exchanges,
/// owner frames demuxed from exchange responses by `type`.
struct Client {
    stream: UnixStream,
    decoder: FrameDecoder,
    token: String,
    pending: Vec<Vec<u8>>,
}

impl Client {
    async fn connect(registration: &Registration) -> std::io::Result<Self> {
        let stream = UnixStream::connect(&registration.socket).await?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
            token: registration.token.clone(),
            pending: Vec::new(),
        })
    }

    async fn send(&mut self, request: &WireRequest) {
        let envelope = wire::Envelope { token: self.token.clone(), request: request.clone() };
        let payload = serde_json::to_vec(&envelope).expect("envelope");
        let frame = wire::encode_frame(&payload).expect("frame");
        self.stream.write_all(&frame).await.expect("write");
    }

    /// Reads one payload frame (any type).
    async fn read_frame(&mut self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(frame) = self.take_pending() {
                return frame;
            }
            assert!(Instant::now() < deadline, "timed out waiting for a frame");
            let mut chunk = [0u8; 16 * 1024];
            let read = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut chunk))
                .await
                .expect("read within 5s")
                .expect("read ok");
            assert!(read > 0, "daemon closed the connection");
            self.pending.extend(self.decoder.feed(&chunk[..read]).expect("framed"));
        }
    }

    fn take_pending(&mut self) -> Option<Vec<u8>> {
        if self.pending.is_empty() {
            None
        } else {
            Some(self.pending.remove(0))
        }
    }

    /// One exchange: skip owner frames, return the response + skipped.
    async fn request(&mut self, request: WireRequest) -> WireResponse {
        let (_, response) = self.exchange_with_events(request).await;
        response
    }

    async fn exchange_with_events(
        &mut self,
        request: WireRequest,
    ) -> (Vec<wire::OwnerFrame>, WireResponse) {
        self.send(&request).await;
        let mut events = Vec::new();
        loop {
            let frame = self.read_frame().await;
            if frame.first() == Some(&0x00) {
                // Binary output frame — irrelevant to exchanges; keep it.
                continue;
            }
            if let Some(owner) = parse_json::<wire::OwnerFrame>(&frame) {
                events.push(owner);
                continue;
            }
            return match parse_json::<WireResponse>(&frame) {
                Some(response) => (events, response),
                None => panic!("unparseable frame: {}", String::from_utf8_lossy(&frame)),
            };
        }
    }

    /// Reads binary output frames (after `attached`) until the reader is
    /// explicitly driven by the caller.
    async fn next_stream_frame(&mut self) -> StreamFrame {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "timed out waiting for a stream frame");
            let frame = self.read_frame().await;
            if frame.first() == Some(&0x00) {
                let output = wire::parse_output_frame(&frame).unwrap_or_else(|error| {
                    panic!(
                        "output frame parse failed: {error}; len={} head={:?}",
                        frame.len(),
                        &frame[..frame.len().min(24)]
                    )
                });
                return StreamFrame::Output(output);
            }
            if let Some(event) = parse_json::<wire::OwnerFrame>(&frame) {
                return StreamFrame::Owner(event);
            }
            if let Some(response) = parse_json::<WireResponse>(&frame) {
                return StreamFrame::Response(response);
            }
        }
    }
}

#[derive(Debug)]
enum StreamFrame {
    Output(wire::OutputFrame),
    Owner(wire::OwnerFrame),
    Response(WireResponse),
}

fn parse_json<T: DeserializeOwned>(frame: &[u8]) -> Option<T> {
    serde_json::from_slice(frame).ok()
}

/// A create request for `/bin/sh -c <command>`.
fn create_request(command: &str, group: &str) -> WireRequest {
    WireRequest::Create {
        program: Some("/bin/sh".to_owned()),
        args: vec!["-c".to_owned(), command.to_owned()],
        cwd: Some("/tmp".to_owned()),
        title: Some("d1".to_owned()),
        group_id: group.to_owned(),
        env: std::collections::BTreeMap::new(),
        cols: Some(200),
        rows: Some(10),
    }
}

fn terminal_number(response: &WireResponse) -> u64 {
    match response {
        WireResponse::Created { terminal } => terminal.id,
        other => panic!("expected created, got {other:?}"),
    }
}

async fn wait_for_output(registration: &Registration, id: u64, at_least: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "terminal produced no output");
        let mut probe = Client::connect(registration).await.expect("probe conn");
        match probe.request(WireRequest::Snapshot { id }).await {
            WireResponse::Snapshot { terminal, .. } => {
                let tail = usize::try_from(terminal.output_tail).unwrap_or(0);
                if tail >= at_least {
                    return tail;
                }
            }
            other => panic!("snapshot failed: {other:?}"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn file_mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn process_gone(pid: u32) -> bool {
    !Path::new(&format!("/proc/{pid}")).exists()
}

// ---------------------------------------------------------------------------
// Registration / lock / discovery surface (§5.1, §5.3, §8)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registration_files_and_ping_guard() {
    let root = temp_root("regfiles");
    let daemon = Daemon::start(&root, "d-reg").await;
    let dir = &daemon.directory;

    // §5.3: 0700 dir, 0600 service.json + socket, protocol 7, 32-hex
    // token, service.boot sidecar present.
    assert_eq!(file_mode(dir), 0o700);
    assert_eq!(file_mode(&dir.join("service.json")), 0o600);
    assert_eq!(file_mode(&dir.join("daemon.sock")), 0o600);
    let registration = read_registration(dir).expect("registration");
    assert_eq!(registration.protocol, 7);
    assert_eq!(registration.token.len(), 32);
    assert!(registration.token.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(registration.socket.ends_with("daemon.sock"));
    let boot = wire::read_boot_record(dir).expect("service.boot");
    assert_eq!(boot.boot_id, wire::boot_id().expect("boot id"));

    // §4.2 ping guard: pong echoes the registration identity.
    let mut client = Client::connect(&daemon.registration).await.expect("connect");
    match client.request(WireRequest::Ping).await {
        WireResponse::Pong { instance_id, pid, protocol } => {
            assert_eq!(instance_id, registration.instance_id);
            assert_eq!(pid, std::process::id(), "in-process daemon shares our pid");
            assert_eq!(protocol, 7);
        }
        other => panic!("expected pong, got {other:?}"),
    }

    shutdown(&mut client, daemon).await;
}

#[tokio::test]
async fn claim_of_a_locked_directory_exits_nonzero() {
    let root = temp_root("claimbusy");
    let daemon = Daemon::start(&root, "d-busy").await;
    // A second daemon on the SAME directory: lock busy ⇒ exit(code≠0)
    // (§5.1) — the client retries in a FRESH directory (§8.2).
    let second = tokio::spawn(oc_pty::daemon::run(config(&root, "d-busy", 60)));
    let code =
        tokio::time::timeout(Duration::from_secs(10), second).await.expect("exits").expect("join");
    assert_eq!(code, 1);

    let mut client = Client::connect(&daemon.registration).await.expect("connect");
    shutdown(&mut client, daemon).await;
}

// ---------------------------------------------------------------------------
// Auth ladder (§4.3, §5.2 mapping)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn token_auth_rejection_ladder() {
    let root = temp_root("auth");
    let daemon = Daemon::start(&root, "d-auth").await;
    let mut client = Client::connect(&daemon.registration).await.expect("connect");

    // Wrong token → the exact §4.3 message (client maps it to the
    // `registration` kind and re-discovers — §5.2).
    client.token = format!("{}0", daemon.registration.token);
    match client.request(WireRequest::Ping).await {
        WireResponse::Error { message } => assert_eq!(message, "authentication failed"),
        other => panic!("expected auth error, got {other:?}"),
    }

    // Missing token (empty string ≠ registration token) → same error.
    client.token = String::new();
    match client.request(WireRequest::Ping).await {
        WireResponse::Error { message } => assert_eq!(message, "authentication failed"),
        other => panic!("expected auth error, got {other:?}"),
    }

    // Per-frame auth: the SAME connection recovers with the true token.
    client.token = daemon.registration.token.clone();
    assert!(matches!(client.request(WireRequest::Ping).await, WireResponse::Pong { .. }));

    // Malformed (non-JSON) payload → the §11.4 error shape.
    let payload = wire::encode_frame(b"not json").expect("frame");
    client.stream.write_all(&payload).await.expect("write");
    match client.next_stream_frame().await {
        StreamFrame::Response(WireResponse::Error { message }) => {
            assert_eq!(message, "malformed request");
        }
        other => panic!("expected malformed-request error, got {other:?}"),
    }

    // Unknown op → the §11.4 error shape.
    match client.request(WireRequest::Unknown).await {
        WireResponse::Error { message } => assert_eq!(message, "unknown op"),
        other => panic!("expected unknown-op error, got {other:?}"),
    }

    shutdown(&mut client, daemon).await;
}

// ---------------------------------------------------------------------------
// Ownership lifecycle: own / supersession / tickets (§6, §4.3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn own_ladder_supersession_and_single_use_tickets() {
    let root = temp_root("own");
    let daemon = Daemon::start(&root, "d-own").await;
    let registration = daemon.registration.clone();

    // OWNERLESS → first plain own succeeds (§5.1).
    let mut first = Client::connect(&registration).await.expect("first conn");
    assert!(matches!(
        first
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));

    // own-while-OWNED without ticket → `error{"daemon is owned"}` (the
    // client maps this to kind `registration` → ladder Discard(owned)).
    let mut second = Client::connect(&registration).await.expect("second conn");
    match second
        .request(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: None,
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => assert_eq!(message, "daemon is owned"),
        other => panic!("expected owned error, got {other:?}"),
    }

    // Instance mismatch rejects before anything else.
    let mut stranger = Client::connect(&registration).await.expect("stranger conn");
    match stranger
        .request(WireRequest::Own {
            instance_id: "not-the-instance".to_owned(),
            ticket: None,
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => assert_eq!(message, "PTY restart daemon changed"),
        other => panic!("expected changed error, got {other:?}"),
    }

    // prepare_handoff is owner-only (§4.3): a bare token holder cannot
    // mint tickets.
    match second.request(WireRequest::PrepareHandoff).await {
        WireResponse::Error { message } => assert_eq!(message, "owner connection required"),
        other => panic!("expected owner-only error, got {other:?}"),
    }

    // The owner mints a ticket.
    let ticket = match first.request(WireRequest::PrepareHandoff).await {
        WireResponse::Handoff { ticket, expires_at } => {
            assert!(expires_at > unix_ms_now_test());
            ticket
        }
        other => panic!("expected handoff, got {other:?}"),
    };
    assert!(daemon.directory.join("state.json").exists(), "crash insurance written");

    // Bogus ticket rejected.
    match second
        .request(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: Some("bogus".to_owned()),
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => {
            assert_eq!(message, "PTY restart handoff ticket rejected");
        }
        other => panic!("expected ticket rejection, got {other:?}"),
    }

    // VALID ticket while OWNED → TRANSFER: the old owner receives
    // `owner_superseded` before the close (§6, gate finding 10).
    let (events, response) = second
        .exchange_with_events(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: Some(ticket.clone()),
            directory: None,
        })
        .await;
    assert!(matches!(response, WireResponse::Owned), "transfer claim: {response:?}");
    drop(events);
    loop {
        match first.next_stream_frame().await {
            StreamFrame::Owner(wire::OwnerFrame::OwnerSuperseded) => break,
            StreamFrame::Owner(wire::OwnerFrame::BusEvent { .. }) => {}
            other => panic!("expected owner_superseded, got {other:?}"),
        }
    }

    // The ticket is single-use: a third claim with the consumed ticket
    // is rejected.
    let mut third = Client::connect(&registration).await.expect("third conn");
    match third
        .request(WireRequest::Own {
            instance_id: registration.instance_id.clone(),
            ticket: Some(ticket),
            directory: None,
        })
        .await
    {
        WireResponse::Error { message } => {
            assert_eq!(message, "PTY restart handoff ticket rejected");
        }
        other => panic!("expected consumed-ticket rejection, got {other:?}"),
    }

    // `own` consumed state.json too (§4.3).
    assert!(!daemon.directory.join("state.json").exists());

    shutdown(&mut second, daemon).await;
    let _ = third.stream.shutdown().await;
    let _ = first.stream.shutdown().await;
}

#[tokio::test]
async fn own_reanchors_after_root_move() {
    let root_a = temp_root("move-a");
    let root_b = temp_root("move-b");
    let daemon = Daemon::start(&root_a, "d-move").await;
    let registration = daemon.registration.clone();

    // First owner + a terminal so handoff state has content.
    let mut owner = Client::connect(&registration).await.expect("owner");
    assert!(matches!(
        owner
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));
    let id = terminal_number(&owner.request(create_request("sleep 30", "ses_move")).await);

    // Client-side migration (§6): lock + registration move
    // inode-preservingly into the new root.
    let target = root_b.join("d-move");
    std::fs::create_dir_all(&target).expect("mkdir");
    std::fs::rename(daemon.directory.join("service.lock"), target.join("service.lock"))
        .expect("move lock");
    std::fs::rename(daemon.directory.join("service.json"), target.join("service.json"))
        .expect("move registration");

    // The claim carries the new path: the daemon RE-ANCHORS (gate
    // finding 6) — later handoff writes land in the new directory.
    assert!(matches!(
        owner
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: Some(target.to_string_lossy().into_owned()),
            })
            .await,
        WireResponse::Owned
    ));
    let ticket = match owner.request(WireRequest::PrepareHandoff).await {
        WireResponse::Handoff { ticket, .. } => ticket,
        other => panic!("expected handoff, got {other:?}"),
    };
    assert_eq!(ticket.len(), 32);
    assert!(target.join("state.json").exists(), "handoff wrote into the re-anchored dir");

    // The moved registration is still protocol 7 and the terminal lives.
    let moved = read_registration(&target).expect("moved registration");
    let mut probe = Client::connect(&moved).await.expect("probe");
    match probe.request(WireRequest::List).await {
        WireResponse::Terminals { terminals } => {
            assert_eq!(terminals.len(), 1);
            assert_eq!(terminals[0].id, id);
        }
        other => panic!("expected terminals, got {other:?}"),
    }
    let _ = probe.request(WireRequest::Terminate { id }).await;

    shutdown(&mut owner, daemon).await;
}

#[tokio::test]
async fn restore_is_owner_only_and_restores_snapshot() {
    let root = temp_root("restore");
    let daemon = Daemon::start(&root, "d-restore").await;
    let registration = daemon.registration.clone();

    let mut owner = Client::connect(&registration).await.expect("owner");
    assert!(matches!(
        owner
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));

    // A fabricated snapshot (§6 arm B shape): one exited terminal with
    // a replay window.
    let state = fabricate_handoff_state();
    let mut bystander = Client::connect(&registration).await.expect("bystander");
    match bystander.request(WireRequest::Restore { state: state.clone() }).await {
        WireResponse::Error { message } => assert_eq!(message, "owner connection required"),
        other => panic!("expected owner-only error, got {other:?}"),
    }
    assert!(matches!(owner.request(WireRequest::Restore { state }).await, WireResponse::Ok));
    match owner.request(WireRequest::List).await {
        WireResponse::Terminals { terminals } => {
            assert_eq!(terminals.len(), 1);
            assert_eq!(terminals[0].id, 9);
            assert_eq!(terminals[0].lifecycle, wire::WireLifecycle::Exited { exit_code: None });
        }
        other => panic!("expected terminals, got {other:?}"),
    }

    shutdown(&mut owner, daemon).await;
}

// ---------------------------------------------------------------------------
// Terminal ops + §7 checkpoint over the wire
// ---------------------------------------------------------------------------

#[tokio::test]
async fn terminal_ops_round_trip_with_v1_checkpoint() {
    let root = temp_root("ops");
    let daemon = Daemon::start(&root, "d-ops").await;
    let registration = daemon.registration.clone();
    let mut client = Client::connect(&registration).await.expect("connect");

    // create → created (running, id 1 — spawn-if-absent is the CLIENT's
    // job; here the daemon already runs).
    let id = match client.request(create_request("printf marker; sleep 30", "ses_ops")).await {
        WireResponse::Created { terminal } => {
            assert_eq!(terminal.id, 1);
            assert_eq!(terminal.lifecycle, wire::WireLifecycle::Running);
            assert_eq!(terminal.group_id, "ses_ops");
            assert_eq!(
                terminal.command,
                vec!["/bin/sh".to_owned(), "-c".to_owned(), "printf marker; sleep 30".to_owned()]
            );
            assert!(terminal.pid.is_some());
            terminal.id
        }
        other => panic!("expected created, got {other:?}"),
    };

    // Output flows; snapshot carries a decodable §7 v1 checkpoint.
    wait_for_output(&registration, id, 6).await;
    match client.request(WireRequest::Snapshot { id }).await {
        WireResponse::Snapshot { terminal, text, checkpoint_base64, cursor_x, cursor_y } => {
            assert!(text.contains("marker"), "snapshot text: {text:?}");
            assert_eq!(terminal.cols, 200);
            let checkpoint = base64_decode_std(&checkpoint_base64);
            let decoded = wire::decode_checkpoint(&checkpoint).expect("v1 checkpoint");
            assert_eq!((decoded.cols, decoded.rows), (200, 10));
            assert_eq!(decoded.scrollback, text);
            assert!(cursor_x > 0 || cursor_y > 0);
        }
        other => panic!("expected snapshot, got {other:?}"),
    }

    // write → ok; read_rows before any controller attach → terminal
    // null (the daemon owns current[] — §3).
    let payload = base64_std(b"echo hello\n");
    assert!(matches!(
        client.request(WireRequest::Write { id, attachment_id: None, data_base64: payload }).await,
        WireResponse::Ok
    ));
    match client.request(WireRequest::ReadRows { id, rows: None }).await {
        WireResponse::Rows { terminal, .. } => assert!(terminal.is_none()),
        other => panic!("expected rows, got {other:?}"),
    }

    // A controller subscribe selects the terminal; read_rows returns
    // rows afterwards.
    let mut subscriber = Client::connect(&registration).await.expect("subscribe conn");
    subscriber
        .send(&WireRequest::Subscribe {
            id,
            offset: 0,
            attachment_id: "ops-controller".to_owned(),
            role: wire::WireRole::Controller,
            takeover: false,
        })
        .await;
    match subscriber.next_stream_frame().await {
        StreamFrame::Response(WireResponse::Attached { role, replay_base64, .. }) => {
            assert_eq!(role, wire::WireRole::Controller);
            assert!(replay_base64.is_some(), "small replay is inline");
        }
        other => panic!("expected attached, got {other:?}"),
    }
    match client.request(WireRequest::ReadRows { id, rows: None }).await {
        WireResponse::Rows { terminal, lines, .. } => {
            assert!(terminal.is_some(), "controller attach selected the terminal");
            assert_ne!(lines, Vec::<String>::new(), "rows are non-empty");
        }
        other => panic!("expected rows, got {other:?}"),
    }

    // The `read` extension (session-keyed) hits the same selection.
    match client
        .request(WireRequest::Read { session_id: "ses_ops".to_owned(), rows: Some(3) })
        .await
    {
        WireResponse::Rows { terminal, lines, .. } => {
            assert!(terminal.is_some());
            assert!(lines.len() <= 3, "rows bound honored: {lines:?}");
        }
        other => panic!("expected rows, got {other:?}"),
    }

    // terminate → ok + list empty.
    assert!(matches!(client.request(WireRequest::Terminate { id }).await, WireResponse::Ok));
    match client.request(WireRequest::List).await {
        WireResponse::Terminals { terminals } => assert_eq!(terminals.len(), 0, "list drained"),
        other => panic!("expected terminals, got {other:?}"),
    }

    let _ = subscriber.stream.shutdown().await;
    shutdown(&mut client, daemon).await;
}

// ---------------------------------------------------------------------------
// §3.1: bus_event owner frames (incl. zero-subscriber exit)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_events_flow_to_the_owner_including_zero_subscriber_exit() {
    let root = temp_root("busevent");
    let daemon = Daemon::start(&root, "d-bus").await;
    let registration = daemon.registration.clone();

    let mut owner = Client::connect(&registration).await.expect("owner conn");
    assert!(matches!(
        owner
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));

    // Zero subscribers: create a short-lived child; the machine's exit
    // pump fires the owner `bus_event{persistent-pty.removed}` (I13).
    let mut one_shot = Client::connect(&registration).await.expect("one-shot conn");
    let id = terminal_number(&one_shot.request(create_request("true", "ses_bus")).await);
    drop(one_shot);

    let mut saw_added = false;
    let mut saw_removed = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while !(saw_added && saw_removed) {
        assert!(Instant::now() < deadline, "bus events did not arrive");
        match owner.next_stream_frame().await {
            StreamFrame::Owner(wire::OwnerFrame::BusEvent { event, data }) => {
                match event.as_str() {
                    "persistent-pty.added" => {
                        saw_added = true;
                        assert_eq!(data["sessionID"], "ses_bus");
                        assert_eq!(data["terminal"]["id"], format!("pty_persistent_{id}"));
                    }
                    "persistent-pty.removed" => {
                        saw_removed = true;
                        assert_eq!(data["sessionID"], "ses_bus");
                        assert_eq!(data["ptyID"], format!("pty_persistent_{id}"));
                    }
                    other => panic!("unexpected bus event: {other}"),
                }
            }
            other => panic!("expected bus_event frame, got {other:?}"),
        }
    }
    assert!(saw_added && saw_removed, "zero-subscriber exit fired the removed event");

    // Explicit terminate also publishes removed to the owner.
    let mut worker = Client::connect(&registration).await.expect("worker conn");
    let id = terminal_number(&worker.request(create_request("sleep 30", "ses_bus2")).await);
    let _ = worker.request(WireRequest::Terminate { id }).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    let removed = loop {
        assert!(Instant::now() < deadline, "terminate removed event missing");
        match owner.next_stream_frame().await {
            StreamFrame::Owner(wire::OwnerFrame::BusEvent { event, data }) => {
                if event == "persistent-pty.removed"
                    && data["ptyID"] == format!("pty_persistent_{id}")
                {
                    break true;
                }
            }
            other => panic!("expected bus_event frame, got {other:?}"),
        }
    };
    assert!(removed);

    shutdown(&mut owner, daemon).await;
    let _ = worker.stream.shutdown().await;
}

// ---------------------------------------------------------------------------
// §5.1 lifecycle: orphan grace + shutdown drain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn orphan_grace_self_exit_and_pins() {
    let root = temp_root("grace");

    // OWNERLESS + zero terminals for the (shortened) grace ⇒ self-exit.
    let daemon = Daemon::start_with_grace(&root, "d-grace-1", 1).await;
    let code = daemon.exit_code().await;
    assert_eq!(code, 0);
    assert!(!root.join("d-grace-1").join("daemon.sock").exists(), "socket unlinked on exit");

    // An owner connection pins the daemon past the grace window.
    let daemon = Daemon::start_with_grace(&root, "d-grace-2", 1).await;
    let registration = daemon.registration.clone();
    let mut owner = Client::connect(&registration).await.expect("owner conn");
    assert!(matches!(
        owner
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));
    // Token-level ops (list/ping) do NOT pin (§5.1) — but the owner
    // conn does; after 2× the grace it must still serve.
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    assert!(matches!(owner.request(WireRequest::List).await, WireResponse::Terminals { .. }));

    // Terminals pin too: close the owner, keep a live child.
    let id = terminal_number(&owner.request(create_request("sleep 30", "ses_pin")).await);
    owner.stream.shutdown().await.expect("owner close");
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    let mut probe = Client::connect(&registration).await.expect("probe after owner close");
    match probe.request(WireRequest::List).await {
        WireResponse::Terminals { terminals } => assert_eq!(terminals.len(), 1),
        other => panic!("expected terminals, got {other:?}"),
    }
    // Removing the terminal un-pins; the grace then elapses ⇒ exit.
    assert!(matches!(probe.request(WireRequest::Terminate { id }).await, WireResponse::Ok));
    let code = daemon.exit_code().await;
    assert_eq!(code, 0, "self-exit after the last terminal left");
}

#[tokio::test]
async fn shutdown_drains_children_and_invalidates_state() {
    let root = temp_root("drain");
    let daemon = Daemon::start(&root, "d-drain").await;
    let registration = daemon.registration.clone();
    let mut client = Client::connect(&registration).await.expect("connect");

    // Own + create + prepare a handoff snapshot (state.json exists).
    assert!(matches!(
        client
            .request(WireRequest::Own {
                instance_id: registration.instance_id.clone(),
                ticket: None,
                directory: None,
            })
            .await,
        WireResponse::Owned
    ));
    match client.request(create_request("sleep 30", "ses_drain")).await {
        WireResponse::Created { .. } => {}
        other => panic!("expected created, got {other:?}"),
    }
    assert!(matches!(
        client.request(WireRequest::PrepareHandoff).await,
        WireResponse::Handoff { .. }
    ));
    let child_pid = match client.request(WireRequest::List).await {
        WireResponse::Terminals { terminals } => terminals[0].pid.expect("child pid"),
        other => panic!("expected terminals, got {other:?}"),
    };
    assert!(daemon.directory.join("state.json").exists());

    // shutdown (TOKEN-level): ok, children dead, state.json
    // invalidated, socket unlinked, exit 0 (§5.1 DRAIN).
    assert!(matches!(client.request(WireRequest::Shutdown).await, WireResponse::Ok));
    let directory = daemon.directory.clone();
    let code = daemon.exit_code().await;
    assert_eq!(code, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !process_gone(child_pid) {
        assert!(Instant::now() < deadline, "child {child_pid} survived the drain");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!directory.join("state.json").exists(), "handoff state invalidated");
    assert!(!directory.join("daemon.sock").exists(), "socket unlinked");

    // I8: discover fails after shutdown — the socket path is gone, so
    // the connect-probe cannot reach a daemon.
    let stale = read_registration(&directory);
    assert!(stale.is_none_or(|r| !Path::new(&r.socket).exists()));
}

// ---------------------------------------------------------------------------
// §3.2 replay transport: inline vs chunked; controller_changed stream
// ---------------------------------------------------------------------------

#[tokio::test]
async fn replay_chunking_both_arms_and_stream_events() {
    let root = temp_root("replay");
    let daemon = Daemon::start(&root, "d-replay").await;
    let registration = daemon.registration.clone();
    let mut client = Client::connect(&registration).await.expect("connect");

    // (a) inline arm: small replay rides `attached` base64.
    let small = terminal_number(&client.request(create_request("printf small", "ses_r1")).await);
    wait_for_output(&registration, small, 5).await;
    let mut observer = Client::connect(&registration).await.expect("observer conn");
    observer
        .send(&WireRequest::Subscribe {
            id: small,
            offset: 0,
            attachment_id: "replay-observer".to_owned(),
            role: wire::WireRole::Observer,
            takeover: false,
        })
        .await;
    let end_offset = match observer.next_stream_frame().await {
        StreamFrame::Response(WireResponse::Attached {
            replay_base64,
            replay_chunked,
            end_offset,
            ..
        }) => {
            assert!(replay_chunked.is_none(), "small replay is inline");
            assert_eq!(base64_decode_std(&replay_base64.expect("inline replay")), b"small");
            end_offset
        }
        other => panic!("expected attached, got {other:?}"),
    };

    // (b) chunked arm: >1 MiB replay — inline would exceed the frame
    // cap after base64 (§3.2, I15).
    let big = terminal_number(
        &client
            .request(create_request(
                &format!("head -c {LARGE_OUTPUT} /dev/zero; sleep 30"),
                "ses_r2",
            ))
            .await,
    );
    wait_for_output(&registration, big, LARGE_OUTPUT).await;
    let mut replayer = Client::connect(&registration).await.expect("replayer conn");
    replayer
        .send(&WireRequest::Subscribe {
            id: big,
            offset: 0,
            attachment_id: "replay-chunked".to_owned(),
            role: wire::WireRole::Observer,
            takeover: false,
        })
        .await;
    let replay_length = match replayer.next_stream_frame().await {
        StreamFrame::Response(WireResponse::Attached {
            replay_base64,
            replay_chunked,
            replay_length,
            end_offset,
            ..
        }) => {
            assert!(replay_base64.is_none(), "oversize replay is not inline");
            assert_eq!(replay_chunked, Some(true));
            assert!(replay_length.unwrap_or(0) > wire::INLINE_REPLAY_LIMIT as u64);
            assert_eq!(end_offset, replay_length.unwrap_or(0), "no truncation at offset 0");
            replay_length.unwrap_or(0)
        }
        other => panic!("expected attached, got {other:?}"),
    };
    // Binary frames reassemble contiguously from 0 to replay_length,
    // then replay_complete{end_offset} (client-deferred completion).
    let mut expected = 0u64;
    let complete = loop {
        match replayer.next_stream_frame().await {
            StreamFrame::Output(frame) => {
                assert_eq!(frame.start, expected, "contiguous chunk");
                expected = frame.end;
            }
            StreamFrame::Response(WireResponse::ReplayComplete { end_offset }) => {
                assert_eq!(end_offset, replay_length);
                break end_offset;
            }
            other => panic!("expected chunk/replay_complete, got {other:?}"),
        }
    };
    assert_eq!(complete, expected, "reassembly reached replay_complete exactly");

    // Takeover by a controller delivers controller_changed to the
    // observing connection (§12 Q3 / I1 arm).
    let mut controller = Client::connect(&registration).await.expect("controller conn");
    controller
        .send(&WireRequest::Subscribe {
            id: small,
            offset: end_offset,
            attachment_id: "replay-controller".to_owned(),
            role: wire::WireRole::Controller,
            takeover: true,
        })
        .await;
    match controller.next_stream_frame().await {
        StreamFrame::Response(WireResponse::Attached { generation, .. }) => {
            assert!(generation >= 1, "takeover bumped the generation");
        }
        other => panic!("expected attached, got {other:?}"),
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let changed = loop {
        assert!(Instant::now() < deadline, "controller_changed never arrived");
        match observer.next_stream_frame().await {
            StreamFrame::Response(WireResponse::ControllerChanged {
                attachment_id,
                generation,
            }) => {
                assert_eq!(attachment_id.as_deref(), Some("replay-controller"));
                assert!(generation >= 1);
                break true;
            }
            StreamFrame::Output(_) | StreamFrame::Owner(_) | StreamFrame::Response(_) => {}
        }
    };
    assert!(changed);

    let _ = client.request(WireRequest::Terminate { id: big }).await;
    shutdown(&mut client, daemon).await;
    let _ = observer.stream.shutdown().await;
    let _ = replayer.stream.shutdown().await;
    let _ = controller.stream.shutdown().await;
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn shutdown(client: &mut Client, daemon: Daemon) {
    assert!(
        matches!(client.request(WireRequest::Shutdown).await, WireResponse::Ok),
        "shutdown op acknowledged"
    );
    let code = daemon.exit_code().await;
    assert_eq!(code, 0);
}

fn base64_std(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode_std(data: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(data).expect("base64")
}

fn unix_ms_now_test() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

fn fabricate_handoff_state() -> oc_core::persistent_pty::HandoffState {
    use oc_core::persistent_pty::{HandoffState, Info, TerminalSnapshot};
    let info = Info {
        id: PtyId("pty_persistent_9".to_owned()),
        title: "fabricated".to_owned(),
        command: "/bin/sh".to_owned(),
        args: vec![],
        cwd: "/tmp".to_owned(),
        status: oc_core::pty::Status::Exited,
        pid: 0,
        exit_code: None,
        session_id: oc_schema::id::SessionId::parse("ses_fab").expect("session"),
        foreground_process: None,
        size: (80, 2),
        output: (0, 5),
    };
    HandoffState {
        instance_id: "fabricated".to_owned(),
        ticket: "fabricated".to_owned(),
        expires_at: u64::MAX,
        terminals: vec![TerminalSnapshot {
            info,
            buffer: base64_std(b"hello"),
            buffer_cursor: 0,
            rows: vec!["hello".to_owned(), String::new()],
            cursor: (0, 0),
            generation: 0,
        }],
    }
}
