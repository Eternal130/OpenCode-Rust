//! The daemon server: registration/lock lifecycle (§5.1, §5.3), the
//! framed Unix-socket server with per-frame token auth (§4), every §3
//! op handler mapped onto the hosted machine, the owner-connection
//! `bus_event` channel (§3.1) and the orphan-grace self-exit (§5.1).
//!
//! The daemon is a thin host by construction (R1): all terminal /
//! buffer / role / generation / replay semantics live in the
//! `oc-core` machine; nothing here re-implements them. Child reaping
//! stays in the machine's per-child wait threads (§5.1, gate finding
//! 12); the only signal paths are the machine's own `ChildKiller`
//! handles (§4.4) — no bare-PID kills anywhere.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use oc_core::persistent_pty::{
    wire, AttachInput, Attachment, CreateInput, EventSink, PersistentPty, Registration, Role,
    StreamEvent, PTY_PROTOCOL_VERSION,
};
use oc_schema::id::SessionId;
use parking_lot::Mutex;
use tokio::io::AsyncReadExt as _;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::args::DaemonConfig;

/// Owner-connection writer handle: framed bytes queued for the socket.
type ConnWriter = mpsc::UnboundedSender<Vec<u8>>;

/// A `prepare_handoff` ticket held for one `own` claim (§4.3: the
/// daemon consumes the ticket in memory AND removes `state.json`).
#[derive(Debug, Clone)]
struct HandoffTicket {
    ticket: String,
    expires_at: u64,
    consumed: bool,
}

/// The current owner connection (§5.1 OWNED state).
#[derive(Debug, Clone)]
struct OwnerHandle {
    conn_id: u64,
    writer: ConnWriter,
}

/// Daemon-lifecycle state guarded by a lock; never held across awaits.
struct DaemonState {
    machine: PersistentPty,
    token: String,
    instance_id: String,
    socket_path: PathBuf,
    owner: Option<OwnerHandle>,
    handoff_ticket: Option<HandoffTicket>,
    orphan_grace: Duration,
    /// §5.1 orphan predicate: when the daemon CONTINUOUSLY had zero
    /// terminals AND no owner since this instant (`None` whenever
    /// either condition is false — either pin restarts the window).
    zero_since: Option<Instant>,
    /// A shutdown op is draining; the grace loop stands down.
    exiting: bool,
}

type Shared = Arc<Mutex<DaemonState>>;

/// Runs the daemon to completion, returning the process exit code
/// (§5.1): `1` on claim/bind failure (the client retries in a fresh
/// directory, §8.2), `0` on `shutdown` op (DRAIN) and orphan-grace
/// self-exit.
pub async fn run(config: DaemonConfig) -> i32 {
    let (owner_events_tx, owner_events_rx) =
        mpsc::unbounded_channel::<(String, serde_json::Value)>();
    let sink: EventSink = Arc::new(move |type_, data| {
        // §3.1: machine publications become owner frames. The pump in
        // the serve loop forwards them to the owner connection (or
        // drops them while ownerless — same-user trust boundary).
        let _ = owner_events_tx.send((type_.to_owned(), data));
    });
    let machine = PersistentPty::hosted(sink).with_default_program(Arc::new(default_program));

    // Claim: mkdir <root>/<name> (0700), flock(service.lock, EX|NB),
    // write service.json (§5.1). A lock held by a live predecessor is
    // the unreachable-daemon case: exit(code≠0); the client retries in
    // a FRESH directory (§8.2) — stale-socket detection per §4.4 is
    // the client's connect-probe, never a signal from here.
    if let Err(error) = machine.bind_runtime_dir(&config.runtime_root, &config.name) {
        eprintln!("oc-pty: {error}");
        return 1;
    }
    let Some(directory) = machine.runtime_dir().map(PathBuf::from) else {
        eprintln!("oc-pty: runtime directory vanished after claim");
        return 1;
    };
    let socket_path = directory.join("daemon.sock");
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("oc-pty: binding {}: {error}", socket_path.display());
            return 1;
        }
    };
    if let Err(error) = set_private(&socket_path) {
        eprintln!("oc-pty: {}: {error}", socket_path.display());
        return 1;
    }
    // service.json carries the bound socket for discovery (`daemon.ts:38`).
    if let Err(error) = machine.set_registration_socket(&socket_path.to_string_lossy()) {
        eprintln!("oc-pty: {error}");
        return 1;
    }
    // The §4.4 generation sidecar: `(boot_id, start_time)` pins THIS
    // process for stale-record probes (safekill semantics).
    if let Err(error) = wire::write_boot_record(&directory, std::process::id()) {
        eprintln!("oc-pty: warning: boot record unavailable: {error}");
    }
    let Some(Registration { token, instance_id, .. }) = machine.registration() else {
        eprintln!("oc-pty: registration unreadable after claim");
        return 1;
    };
    let state = Arc::new(Mutex::new(DaemonState {
        machine,
        token,
        instance_id,
        socket_path: socket_path.clone(),
        owner: None,
        handoff_ticket: None,
        orphan_grace: config.orphan_grace,
        zero_since: None,
        exiting: false,
    }));
    serve(listener, state, owner_events_rx).await
}

/// The accept/event/grace loop (§5.1).
async fn serve(
    listener: UnixListener,
    state: Shared,
    mut owner_events: mpsc::UnboundedReceiver<(String, serde_json::Value)>,
) -> i32 {
    let (exit_tx, mut exit_rx) = mpsc::channel::<i32>(1);
    let mut next_conn_id = 0u64;
    // 250 ms cadence keeps the orphan window honest without busy-wait.
    let mut grace_ticker = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    next_conn_id += 1;
                    tokio::spawn(handle_conn(stream, Arc::clone(&state), next_conn_id, exit_tx.clone()));
                }
                Err(error) => eprintln!("oc-pty: accept: {error}"),
            },
            event = owner_events.recv() => {
                if let Some((event, data)) = event {
                    let owner = state.lock().owner.clone();
                    if let Some(owner) = owner {
                        // §3.1: the publication rides the owner
                        // connection interleaved with exchange
                        // responses, self-describing by `type`.
                        let frame = wire::OwnerFrame::BusEvent { event, data };
                        let bytes = serde_json::to_vec(&frame)
                            .ok()
                            .and_then(|payload| wire::encode_frame(&payload).ok());
                        if let Some(bytes) = bytes {
                            let _ = owner.writer.send(bytes);
                        }
                    }
                }
            }
            code = exit_rx.recv() => {
                let _ = code;
                break;
            }
            _ = grace_ticker.tick() => {
                if orphan_grace_expired(&state) {
                    // §5.1 SELF-EXIT: lock fd closes with the process;
                    // children get HUP as the masters close (daemon-death
                    // semantics — we never signal them ourselves).
                    break;
                }
            }
        }
    }
    // Exit cleanup: unlink the bound socket (the lock releases with
    // the process; `state.json` was already invalidated by shutdown or
    // is simply left for the next claim's ladder).
    let socket_path = state.lock().socket_path.clone();
    let _ = std::fs::remove_file(&socket_path);
    0
}

/// The §5.1 orphan-grace predicate: the self-exit timer runs only
/// while the daemon has CONTINUOUSLY had zero terminals (live or
/// exited) AND no owner connection for the grace period.
fn orphan_grace_expired(state: &Shared) -> bool {
    let mut shared = state.lock();
    if shared.exiting {
        return false;
    }
    let has_owner = shared.owner.is_some();
    // Token-level ops (list, ping, …) do NOT pin the daemon — only
    // owner connections and terminals do (§5.1).
    let has_terminals = !shared.machine.list(None).unwrap_or_default().is_empty();
    if has_owner || has_terminals {
        shared.zero_since = None;
        return false;
    }
    let since = *shared.zero_since.get_or_insert_with(Instant::now);
    shared.orphan_grace.is_zero() || Instant::now().duration_since(since) >= shared.orphan_grace
}

/// Per-connection handler: frame loop + per-frame auth + dispatch.
#[allow(clippy::too_many_lines)] // one straight-line loop: read, dispatch, teardown
async fn handle_conn(stream: UnixStream, state: Shared, conn_id: u64, exit_tx: mpsc::Sender<i32>) {
    let (mut read_half, write_half) = stream.into_split();
    let (writer_tx, writer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(conn_writer(write_half, writer_rx));
    let mut decoder = wire::FrameDecoder::new();
    let mut attachment: Option<Attachment> = None;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match read_half.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let frames = match decoder.feed(&buffer[..read]) {
                    Ok(frames) => frames,
                    Err(error) => {
                        eprintln!("oc-pty: connection {conn_id}: {error}");
                        break;
                    }
                };
                for frame in frames {
                    let dispatch =
                        dispatch_frame(&frame, &state, conn_id, &writer_tx, &mut attachment).await;
                    if let Some(response) = dispatch.response {
                        send_response(&writer_tx, &response);
                    }
                    if dispatch.shutdown {
                        // DRAIN path: the Ok frame must flush BEFORE the
                        // process exits — close every sender clone (drop
                        // our subscription + ownership hold), await the
                        // writer, then signal the serve loop.
                        attachment.take();
                        {
                            let mut shared = state.lock();
                            if shared.owner.as_ref().is_some_and(|owner| owner.conn_id == conn_id) {
                                shared.owner = None;
                            }
                        }
                        drop(writer_tx);
                        let _ = writer.await;
                        let _ = exit_tx.send(0).await;
                        return;
                    }
                }
            }
        }
    }
    // Teardown: an owner socket close returns the daemon to OWNERLESS
    // (§5.1); a subscribe connection detaches.
    {
        let mut shared = state.lock();
        if shared.owner.as_ref().is_some_and(|owner| owner.conn_id == conn_id) {
            shared.owner = None;
        }
    }
    drop(attachment);
    drop(writer_tx);
    let _ = writer.await;
}

/// Serializes + queues one response frame.
fn send_response(writer: &ConnWriter, response: &wire::WireResponse) {
    if let Ok(bytes) = response.to_frame() {
        let _ = writer.send(bytes);
    }
}

/// The outcome of one request: the exchange response (if any —
/// subscribe responds inline) and whether the daemon drains.
struct Dispatch {
    response: Option<wire::WireResponse>,
    shutdown: bool,
}

async fn dispatch_frame(
    frame: &[u8],
    state: &Shared,
    conn_id: u64,
    writer: &ConnWriter,
    attachment_slot: &mut Option<Attachment>,
) -> Dispatch {
    let envelope: wire::Envelope = match serde_json::from_slice(frame) {
        Ok(envelope) => envelope,
        Err(_) => {
            return Dispatch {
                response: Some(wire::WireResponse::Error {
                    message: "malformed request".to_owned(),
                }),
                shutdown: false,
            }
        }
    };
    // Per-frame token check (R7): every request frame's token must
    // equal the registration token.
    {
        let shared = state.lock();
        if envelope.token != shared.token {
            drop(shared);
            return Dispatch {
                response: Some(wire::WireResponse::Error {
                    message: "authentication failed".to_owned(),
                }),
                shutdown: false,
            };
        }
    }
    handle_request(envelope.request, state, conn_id, writer, attachment_slot).await
}

/// Writes framed bytes to the socket until the channel closes.
async fn conn_writer(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    use tokio::io::AsyncWriteExt as _;
    while let Some(bytes) = rx.recv().await {
        if write_half.write_all(&bytes).await.is_err() {
            break;
        }
        if write_half.flush().await.is_err() {
            break;
        }
    }
    let _ = write_half.shutdown().await;
}

fn default_program() -> Option<String> {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .or_else(|| Some("/bin/sh".to_owned()))
}

fn unix_ms_now() -> u64 {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis()))
        .unwrap_or(0)
}

/// `0600` on the socket file — the 0700 dir is the outer wall (§4.3).
fn set_private(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

fn base64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn base64_decode(data: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// Maps a machine error onto the wire `error` shape.
fn error_response(error: &oc_core::persistent_pty::UnavailableError) -> wire::WireResponse {
    wire::WireResponse::Error { message: error.message.clone() }
}

fn wire_terminal(info: &oc_core::persistent_pty::Info) -> wire::WireTerminal {
    // `wire_terminal` errors only on non-`pty_persistent_<n>` ids,
    // which this daemon never stores.
    wire::wire_terminal(info)
        .unwrap_or_else(|error| unreachable!("machine produced an unmappable terminal id: {error}"))
}

/// Every §3 op, mapped onto the machine API. Subscribe responds inline
/// (its `attached` + stream frames ride `writer`); `shutdown` marks
/// the drain.
async fn handle_request(
    request: wire::WireRequest,
    state: &Shared,
    conn_id: u64,
    writer: &ConnWriter,
    attachment_slot: &mut Option<Attachment>,
) -> Dispatch {
    let machine = state.lock().machine.clone();
    match request {
        wire::WireRequest::Ping => {
            // §4.2 ping guard: echo the registration identity.
            let shared = state.lock();
            Dispatch {
                response: Some(wire::WireResponse::Pong {
                    instance_id: shared.instance_id.clone(),
                    pid: std::process::id(),
                    protocol: PTY_PROTOCOL_VERSION,
                }),
                shutdown: false,
            }
        }

        // §6 arm A / §4.3: the ownership claim (owner-connection op by
        // construction — `own` is what MAKES a connection the owner).
        wire::WireRequest::Own { instance_id, ticket, directory } => {
            let mut shared = state.lock();
            if instance_id != shared.instance_id {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "PTY restart daemon changed".to_owned(),
                    }),
                    shutdown: false,
                };
            }
            let already_owner = shared.owner.as_ref().is_some_and(|owner| owner.conn_id == conn_id);
            let owner_present = shared.owner.is_some();
            // Ticket validation (single-use, OUR R4 design — §4.3):
            // unexpired, unconsumed, matching the minted ticket.
            let ticket_ok = ticket.as_deref().is_some_and(|ticket| {
                shared.handoff_ticket.as_ref().is_some_and(|record| {
                    !record.consumed && record.ticket == ticket && record.expires_at > unix_ms_now()
                })
            });
            if ticket.is_some() && !ticket_ok {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "PTY restart handoff ticket rejected".to_owned(),
                    }),
                    shutdown: false,
                };
            }
            if owner_present && !already_owner && ticket.is_none() {
                // §6 supersession rules: no ticket + live owner conn.
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "daemon is owned".to_owned(),
                    }),
                    shutdown: false,
                };
            }
            // Transfer: the OLD owner receives `owner_superseded` and
            // is then closed (§6, gate finding 10) — the frame rides
            // the old connection's writer; dropping the handle closes
            // its write half after the flush.
            if owner_present && !already_owner {
                if let Some(previous) = shared.owner.take() {
                    if let Ok(payload) = serde_json::to_vec(&wire::OwnerFrame::OwnerSuperseded) {
                        if let Ok(bytes) = wire::encode_frame(&payload) {
                            let _ = previous.writer.send(bytes);
                        }
                    }
                }
            }
            // Consume the ticket (§4.3) AND `state.json` (crash
            // insurance is spent on a live claim).
            if ticket.is_some() {
                if let Some(record) = shared.handoff_ticket.as_mut() {
                    record.consumed = true;
                }
                drop(shared);
                machine.invalidate_handoff_state();
                shared = state.lock();
            }
            shared.owner = Some(OwnerHandle { conn_id, writer: writer.clone() });
            // §6 arm A: fresh selection epoch + controller clear
            // (generations stay monotonic — gate ruling 2)…
            machine.reset_selection_epoch();
            // …and re-anchor the runtime dir to the CURRENT directory
            // after a client-side migration (gate finding 6). The
            // bound socket path is unchanged (daemon.sock is not
            // migrated); `shared.socket_path` keeps it for exit
            // cleanup.
            if let Some(directory) = directory.as_deref() {
                // Whether the path actually moved is the client's
                // bookkeeping; the daemon only rebinds.
                let _ = machine.reanchor_runtime_dir(Path::new(directory));
            }
            Dispatch { response: Some(wire::WireResponse::Owned), shutdown: false }
        }

        // §4.3 owner-only: mint a single-use ticket; `Machine::handoff`
        // also writes the `state.json` crash insurance (§6).
        wire::WireRequest::PrepareHandoff => {
            let mut shared = state.lock();
            if shared.owner.as_ref().is_none_or(|owner| owner.conn_id != conn_id) {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "owner connection required".to_owned(),
                    }),
                    shutdown: false,
                };
            }
            match shared.machine.handoff() {
                Ok(Some(handoff)) => {
                    shared.handoff_ticket = Some(HandoffTicket {
                        ticket: handoff.ticket.clone(),
                        expires_at: handoff.expires_at,
                        consumed: false,
                    });
                    Dispatch {
                        response: Some(wire::WireResponse::Handoff {
                            ticket: handoff.ticket,
                            expires_at: handoff.expires_at,
                        }),
                        shutdown: false,
                    }
                }
                Ok(None) => Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "no PTY runtime directory".to_owned(),
                    }),
                    shutdown: false,
                },
                Err(error) => Dispatch { response: Some(error_response(&error)), shutdown: false },
            }
        }

        // §4.3: TOKEN-level on purpose — the adoption discard path
        // calls it from a NON-owner one-shot connection.
        wire::WireRequest::Shutdown => match machine.shutdown().await {
            Ok(()) => Dispatch { response: Some(wire::WireResponse::Ok), shutdown: true },
            Err(error) => Dispatch { response: Some(error_response(&error)), shutdown: false },
        },

        wire::WireRequest::List => {
            let terminals = machine
                .list(None)
                .unwrap_or_default()
                .iter()
                .map(wire_terminal)
                .collect::<Vec<_>>();
            Dispatch {
                response: Some(wire::WireResponse::Terminals { terminals }),
                shutdown: false,
            }
        }

        // The only start=true op (§3): absent daemon ⇒ the client
        // spawns one; present daemon ⇒ create directly.
        wire::WireRequest::Create { program, args, cwd, title, group_id, env, cols, rows } => {
            let Ok(session) = SessionId::parse(&group_id) else {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: format!("invalid group_id: {group_id}"),
                    }),
                    shutdown: false,
                };
            };
            let size = match (cols, rows) {
                (Some(cols), Some(rows)) => Some((cols, rows)),
                _ => None,
            };
            let input = CreateInput {
                command: program,
                args,
                cwd,
                title: title.unwrap_or_default(),
                env,
                size,
            };
            match machine.create(&session, input).await {
                Ok(info) => Dispatch {
                    response: Some(wire::WireResponse::Created { terminal: wire_terminal(&info) }),
                    shutdown: false,
                },
                Err(error) => Dispatch { response: Some(error_response(&error)), shutdown: false },
            }
        }

        wire::WireRequest::Write { id, attachment_id, data_base64 } => {
            let Some(data) = base64_decode(&data_base64) else {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "write payload is not valid base64".to_owned(),
                    }),
                    shutdown: false,
                };
            };
            // The machine's `write` takes text (`index.ts:182-192`).
            let Ok(text) = String::from_utf8(data) else {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "write payload is not utf-8".to_owned(),
                    }),
                    shutdown: false,
                };
            };
            let response = match machine
                .write(&wire::terminal_id(id), &text, attachment_id.as_deref())
                .await
            {
                Ok(()) => wire::WireResponse::Ok,
                Err(error) => error_response(&error),
            };
            Dispatch { response: Some(response), shutdown: false }
        }

        wire::WireRequest::Resize { id, attachment_id, cols, rows } => {
            let response = match machine
                .resize(&wire::terminal_id(id), cols, rows, attachment_id.as_deref())
                .await
            {
                Ok(()) => wire::WireResponse::Ok,
                Err(error) => error_response(&error),
            };
            Dispatch { response: Some(response), shutdown: false }
        }

        wire::WireRequest::Control { id, attachment_id, cols, rows } => {
            let response =
                match machine.control(&wire::terminal_id(id), &attachment_id, cols, rows).await {
                    Ok(()) => wire::WireResponse::Ok,
                    Err(error) => error_response(&error),
                };
            Dispatch { response: Some(response), shutdown: false }
        }

        wire::WireRequest::Input { id, attachment_id, cols, rows, data_base64 } => {
            let Some(data) = base64_decode(&data_base64) else {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "input payload is not valid base64".to_owned(),
                    }),
                    shutdown: false,
                };
            };
            let response = match machine
                .input(&wire::terminal_id(id), &attachment_id, cols, rows, &data)
                .await
            {
                Ok(()) => wire::WireResponse::Ok,
                Err(error) => error_response(&error),
            };
            Dispatch { response: Some(response), shutdown: false }
        }

        wire::WireRequest::Snapshot { id } => match machine.snapshot(&wire::terminal_id(id)) {
            Ok(snapshot) => Dispatch {
                response: Some(wire::WireResponse::Snapshot {
                    terminal: wire_terminal(&snapshot.info),
                    text: snapshot.text,
                    checkpoint_base64: base64_encode(&snapshot.checkpoint),
                    cursor_x: snapshot.cursor.0 as u64,
                    cursor_y: snapshot.cursor.1 as u64,
                }),
                shutdown: false,
            },
            Err(error) => Dispatch { response: Some(error_response(&error)), shutdown: false },
        },

        // `read_rows` (id-keyed wire parity): resolve the session from
        // the terminal, then the machine owns `current[]` selection.
        wire::WireRequest::ReadRows { id, rows } => {
            let terminal = wire::terminal_id(id);
            let session = machine.get(&terminal).ok().map(|info| info.session_id);
            match session {
                Some(session) => rows_response(&machine, &session, rows).await,
                None => Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: format!("terminal not found: {}", terminal.0),
                    }),
                    shutdown: false,
                },
            }
        }

        // Extension (§3): session-keyed read straight at the machine.
        wire::WireRequest::Read { session_id, rows } => {
            let Ok(session) = SessionId::parse(&session_id) else {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: format!("invalid session_id: {session_id}"),
                    }),
                    shutdown: false,
                };
            };
            rows_response(&machine, &session, rows).await
        }

        wire::WireRequest::Terminate { id } => {
            let response = match machine.remove(&wire::terminal_id(id)).await {
                Ok(()) => wire::WireResponse::Ok,
                Err(error) => error_response(&error),
            };
            Dispatch { response: Some(response), shutdown: false }
        }

        // §6 arm B: snapshot restore — OWNER-only (§4.3), so a bare
        // token holder cannot inject fake terminal history.
        wire::WireRequest::Restore { state: handoff_state } => {
            let shared = state.lock();
            if shared.owner.as_ref().is_none_or(|owner| owner.conn_id != conn_id) {
                return Dispatch {
                    response: Some(wire::WireResponse::Error {
                        message: "owner connection required".to_owned(),
                    }),
                    shutdown: false,
                };
            }
            let machine = shared.machine.clone();
            drop(shared);
            let _ = machine.restore(&handoff_state);
            Dispatch { response: Some(wire::WireResponse::Ok), shutdown: false }
        }

        wire::WireRequest::Subscribe { id, offset, attachment_id, role, takeover } => {
            subscribe(&machine, id, offset, attachment_id, role, takeover, writer, attachment_slot)
        }

        // §11.4 compat guard: mixed versions degrade to clear errors.
        wire::WireRequest::Unknown => Dispatch {
            response: Some(wire::WireResponse::Error { message: "unknown op".to_owned() }),
            shutdown: false,
        },
    }
}

/// Shared tail of `read_rows`/`read`: `None` selection ⇒ `terminal:
/// null` (the client maps that to `undefined` — §3).
async fn rows_response(
    machine: &PersistentPty,
    session: &SessionId,
    rows: Option<u32>,
) -> Dispatch {
    match machine.read(session, rows).await {
        Ok(Some(result)) => {
            let terminal = machine.get(&result.pty_id).ok().map(|info| wire_terminal(&info));
            Dispatch {
                response: Some(wire::WireResponse::Rows {
                    terminal,
                    lines: result
                        .screen
                        .text
                        .split('\n')
                        .map(std::borrow::ToOwned::to_owned)
                        .collect(),
                    cursor_x: result.screen.cursor.0 as u64,
                    cursor_y: result.screen.cursor.1 as u64,
                }),
                shutdown: false,
            }
        }
        Ok(None) => Dispatch {
            response: Some(wire::WireResponse::Rows {
                terminal: None,
                lines: Vec::new(),
                cursor_x: 0,
                cursor_y: 0,
            }),
            shutdown: false,
        },
        Err(error) => Dispatch { response: Some(error_response(&error)), shutdown: false },
    }
}

/// The subscribe connection (§3 stream table + §3.2 replay transport):
/// first frame `attached` (inline replay iff ≤ `1 MiB`, else chunked
/// binary frames + `replay_complete`), then output/resized/exited/
/// `controller_changed` until the stream ends or the client closes.
#[allow(clippy::needless_pass_by_value)] // owned id rides the §3 wire parameter set verbatim
fn subscribe(
    machine: &PersistentPty,
    id: u64,
    offset: u64,
    attachment_id: String,
    role: wire::WireRole,
    takeover: bool,
    writer: &ConnWriter,
    attachment_slot: &mut Option<Attachment>,
) -> Dispatch {
    let terminal = wire::terminal_id(id);
    // Absolute output offset of the NEXT byte this connection will
    // see; starts at the replay end so live output continues exactly.
    let next_offset = Arc::new(Mutex::new(0u64));
    let event_writer = writer.clone();
    let event_offsets = Arc::clone(&next_offset);
    let on_event: Box<dyn Fn(StreamEvent) + Send + Sync> = {
        Box::new(move |event: StreamEvent| {
            let frame = match event {
                StreamEvent::Output(chunk) => {
                    let mut offsets = event_offsets.lock();
                    let start = *offsets;
                    let end = start.saturating_add(chunk.len() as u64);
                    *offsets = end;
                    wire::encode_output_frame(start, end, &chunk)
                        .and_then(|payload| wire::encode_frame(&payload))
                }
                StreamEvent::Resized { cols, rows, generation, checkpoint } => {
                    wire::WireResponse::Resized {
                        cols,
                        rows,
                        generation,
                        checkpoint_base64: base64_encode(&checkpoint),
                    }
                    .to_frame()
                }
                StreamEvent::Exited { exit_code } => {
                    let final_offset = *event_offsets.lock();
                    wire::WireResponse::Exited { exit_code, final_offset }.to_frame()
                }
                StreamEvent::ControllerChanged { attachment_id, generation } => {
                    wire::WireResponse::ControllerChanged { attachment_id, generation }.to_frame()
                }
            };
            if let Ok(bytes) = frame {
                let _ = event_writer.send(bytes);
            }
        })
    };
    let on_end: Box<dyn FnOnce() + Send + Sync> = Box::new(|| {
        // The stream ended (exited/remove); the connection itself is
        // client-closed — nothing to push here.
    });
    let input = AttachInput {
        cursor: usize::try_from(offset).unwrap_or(usize::MAX),
        attachment_id: &attachment_id,
        role: Role::from(role),
        takeover,
        on_event,
        on_end,
    };
    let Ok(attachment) = machine.attach(&terminal, input) else {
        return Dispatch {
            response: Some(wire::WireResponse::Error {
                message: format!("terminal not found: {}", terminal.0),
            }),
            shutdown: false,
        };
    };
    let replay = attachment.data.clone();
    let end_offset = attachment.replay.end_offset as u64;
    *next_offset.lock() = end_offset;
    let attached = if replay.len() <= wire::INLINE_REPLAY_LIMIT {
        wire::WireResponse::Attached {
            terminal: wire_terminal(&attachment.info),
            role,
            generation: attachment.generation,
            requested_offset: attachment.replay.requested_offset as u64,
            available_offset: attachment.replay.available_offset as u64,
            end_offset,
            truncated: attachment.replay.truncated,
            replay_base64: Some(base64_encode(&replay)),
            replay_chunked: None,
            replay_length: None,
        }
    } else {
        // §3.2 chunked arm: inline base64 of a full buffer (≈11.2 MiB)
        // cannot fit the 8 MiB frame cap.
        wire::WireResponse::Attached {
            terminal: wire_terminal(&attachment.info),
            role,
            generation: attachment.generation,
            requested_offset: attachment.replay.requested_offset as u64,
            available_offset: attachment.replay.available_offset as u64,
            end_offset,
            truncated: attachment.replay.truncated,
            replay_base64: None,
            replay_chunked: Some(true),
            replay_length: Some(replay.len() as u64),
        }
    };
    send_response(writer, &attached);
    if replay.len() > wire::INLINE_REPLAY_LIMIT {
        // Replay bytes as ordinary binary output frames in offset
        // order; live output follows with higher offsets.
        let base = end_offset - replay.len() as u64;
        let chunk_size = 1024 * 1024;
        let mut sent = 0u64;
        for chunk in replay.chunks(chunk_size) {
            let start = base + sent;
            let end = start + chunk.len() as u64;
            sent = end - base;
            if let Ok(bytes) = wire::encode_output_frame(start, end, chunk)
                .and_then(|payload| wire::encode_frame(&payload))
            {
                let _ = writer.send(bytes);
            }
        }
        send_response(writer, &wire::WireResponse::ReplayComplete { end_offset: base + sent });
    }
    // Delivery starts only after the replay contract is on the wire.
    attachment.activate();
    // The attachment handle lives in the connection task: it detaches
    // when the client closes the connection.
    *attachment_slot = Some(attachment);
    Dispatch { response: None, shutdown: false }
}
