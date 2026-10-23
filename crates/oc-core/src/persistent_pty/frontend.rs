//! The persistent-PTY FRONTEND (design §2.2, §3, §11) — the same op
//! surface the oc-server handlers use today, backed by either the
//! in-process machine ([`Backend::InProcess`] — the `OC_PTY_DAEMON=0`
//! one-release escape hatch, R3) or the daemon transport
//! ([`Backend::Daemon`] — DEFAULT ON with spawn-if-absent, §8).
//!
//! Backend selection reads `OC_PTY_DAEMON` ONCE at startup (the lane
//! `OnceLock` pattern — see the M-engine precedent
//! `oc-tui-core/src/lib.rs` in the 6a8cd9d^ tree: read once at first
//! use so a running session never switches mid-flight; the retirement
//! lane removes the flag entirely).
//!
//! Bus publications stay byte-identical under both backends (§3.1):
//! in-process the machine publishes directly; over the daemon the
//! machine's publications ride the owner connection as `bus_event`
//! frames and the transport republishes them through the SAME registry
//! definitions — the TUI reducer (`oc-tui/src/store.rs`) and EV-1 are
//! unchanged.

#![allow(
    // Every op future embeds the transport's exchange buffers — the
    // §5.2 one-in-flight-per-connection shape (same pin as transport).
    clippy::large_futures
)]

use std::sync::Arc;

use oc_schema::id::SessionId;

use super::transport::{
    self, info_from_wire, BinaryLauncher, Transport, TransportError, WireStream,
};
use super::wire::{WireRequest, WireResponse, WireRole};
use super::{
    AttachInput, CreateInput, Handoff, Info, PubResult, ReadResult, Replay, Role, Snapshot,
    UnavailableError,
};
use crate::bus::Bus;
use crate::pty::PtyId;

/// The `OC_PTY_DAEMON=0` escape-hatch predicate (pure — testable).
#[must_use]
pub fn escape_hatch_enabled(value: Option<&str>) -> bool {
    value == Some("0")
}

/// R3/§11: `OC_PTY_DAEMON=0` forces the in-process backend for exactly
/// one release window; every other value (including unset) selects the
/// daemon backend. The lane `OnceLock` pattern: read once at startup.
#[must_use]
pub fn daemon_disabled() -> bool {
    cfg_if_test_override().unwrap_or_else(|| {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| escape_hatch_enabled(std::env::var("OC_PTY_DAEMON").ok().as_deref()))
    })
}

/// The in-process override for backend-parity tests: the flag latches
/// once per process, so libtest's shared process cannot A/B through
/// the env — the override wins. `None` in non-test builds (the M1
/// `OC_ENGINE_OPENTUI` pattern).
#[cfg(test)]
static TEST_BACKEND_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(test)]
fn cfg_if_test_override() -> Option<bool> {
    match TEST_BACKEND_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

#[cfg(not(test))]
fn cfg_if_test_override() -> Option<bool> {
    None
}

#[cfg(test)]
#[doc(hidden)]
pub fn __set_test_backend_override(disabled: Option<bool>) {
    if let Some(value) = disabled {
        TEST_BACKEND_OVERRIDE.store(u8::from(!value) + 1, std::sync::atomic::Ordering::SeqCst);
    } else {
        TEST_BACKEND_OVERRIDE.store(0, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Which implementation backs the frontend.
#[derive(Clone)]
pub enum Backend {
    /// The machine, in-process (`OC_PTY_DAEMON=0`).
    InProcess(super::PersistentPty),
    /// The daemon transport (default) — spawn-if-absent on `create`.
    Daemon(Arc<Transport>),
}

/// The persistent-PTY frontend: every op the handlers use today
/// (`list/get/create/write/resize/control/input/snapshot/read/remove/
/// shutdown/handoff/attach` + the boot surface `set_runtime_dir` /
/// `runtime_dir` / `adopt`), routed per backend.
#[derive(Clone)]
pub struct Frontend {
    backend: Backend,
}

impl Frontend {
    /// Production constructor: selects the backend from
    /// [`daemon_disabled`] (default daemon, spawn-if-absent).
    #[must_use]
    pub fn new(bus: Bus) -> Self {
        if daemon_disabled() {
            Self::in_process(bus)
        } else {
            Self::daemon(bus, Arc::new(BinaryLauncher::new()))
        }
    }

    /// The `OC_PTY_DAEMON=0` path (and the hermetic-test default).
    #[must_use]
    pub fn in_process(bus: Bus) -> Self {
        Self { backend: Backend::InProcess(super::PersistentPty::new(bus)) }
    }

    /// The daemon backend with an explicit launcher (the hermetic seam).
    #[must_use]
    pub fn daemon(bus: Bus, launcher: Arc<dyn transport::Launcher>) -> Self {
        Self::from_backend(Backend::Daemon(Arc::new(Transport::new(bus, launcher))))
    }

    /// Tests/embedders: an explicit backend.
    #[must_use]
    pub fn from_backend(backend: Backend) -> Self {
        Self { backend }
    }

    /// Which backend is live (diagnostics/tests).
    #[must_use]
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    // -- boot surface ------------------------------------------------------

    /// Records the runtime root. In-process this claims `<root>/<id>`
    /// immediately; the daemon backend only records it — spawn is
    /// `create`-only (§8.2).
    pub fn set_runtime_dir(&self, root: &std::path::Path) -> Result<(), String> {
        match &self.backend {
            Backend::InProcess(machine) => machine.set_runtime_dir(root),
            Backend::Daemon(transport) => {
                transport.set_root(root);
                Ok(())
            }
        }
    }

    /// The runtime directory identity (diagnostics/boot logging).
    #[must_use]
    pub fn runtime_dir(&self) -> Option<String> {
        match &self.backend {
            Backend::InProcess(machine) => machine.runtime_dir(),
            Backend::Daemon(transport) => transport
                .current_directory()
                .map(|directory| directory.to_string_lossy().into_owned()),
        }
    }

    /// The §6 adoption ladder: in-process keeps the machine's ladder
    /// (the fallback arm); the daemon backend runs the socket ladder
    /// (arm A live-claim / arm B snapshot-restore).
    pub async fn adopt(&self, inherited: &Handoff) -> super::Adoption {
        match &self.backend {
            Backend::InProcess(machine) => machine.adopt(inherited),
            Backend::Daemon(transport) => transport.adopt(inherited).await,
        }
    }

    // -- §3 ops ------------------------------------------------------------

    /// `list` — absent daemon ⇒ `[]` (`index.ts:143-145`).
    pub async fn list(&self, session_id: Option<&SessionId>) -> PubResult<Vec<Info>> {
        match &self.backend {
            Backend::InProcess(machine) => machine.list(session_id),
            Backend::Daemon(transport) => {
                let response =
                    transport.request_if_running(WireRequest::List).await.map_err(unavailable)?;
                match response {
                    None => Ok(Vec::new()),
                    Some(WireResponse::Terminals { terminals }) => {
                        let mut infos = Vec::with_capacity(terminals.len());
                        for terminal in &terminals {
                            infos.push(
                                info_from_wire(terminal)
                                    .map_err(|message| UnavailableError { message })?,
                            );
                        }
                        Ok(infos
                            .into_iter()
                            .filter(|info| {
                                session_id.is_none_or(|session| &info.session_id == session)
                            })
                            .collect())
                    }
                    Some(_) => Err(protocol_unexpected("terminals")),
                }
            }
        }
    }

    /// `get` (via `list`, `index.ts:151-156`).
    pub async fn get(&self, id: &PtyId) -> PubResult<Info> {
        match &self.backend {
            Backend::InProcess(machine) => machine.get(id),
            Backend::Daemon(_) => {
                let listed = self.list(None).await?;
                listed.into_iter().find(|info| &info.id == id).ok_or_else(|| UnavailableError {
                    message: format!("PersistentPty.NotFoundError: {}", id.0),
                })
            }
        }
    }

    /// `create` — the ONLY spawn-if-absent op (§8.2). The
    /// `persistent-pty.added` publication arrives over the owner
    /// channel under the daemon backend (§3.1 — the client layer does
    /// NOT publish again).
    pub async fn create(&self, session_id: &SessionId, input: CreateInput) -> PubResult<Info> {
        match &self.backend {
            Backend::InProcess(machine) => machine.create(session_id, input).await,
            Backend::Daemon(transport) => {
                let (cols, rows) = input.size.unwrap_or((80, 24));
                let request = WireRequest::Create {
                    program: input.command,
                    args: input.args,
                    cwd: input.cwd,
                    title: Some(input.title),
                    group_id: session_id.as_str().to_owned(),
                    env: input.env,
                    cols: Some(cols),
                    rows: Some(rows),
                };
                match transport.request(request, true).await.map_err(unavailable)? {
                    WireResponse::Created { terminal } => {
                        info_from_wire(&terminal).map_err(|message| UnavailableError { message })
                    }
                    _ => Err(protocol_unexpected("created")),
                }
            }
        }
    }

    /// `write` (index.ts:182-192).
    pub async fn write(
        &self,
        id: &PtyId,
        data: &str,
        attachment_id: Option<&str>,
    ) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.write(id, data, attachment_id).await,
            Backend::Daemon(transport) => {
                let request = WireRequest::Write {
                    id: terminal_number(id)?,
                    attachment_id: attachment_id.map(str::to_owned),
                    data_base64: super::base64_encode(data.as_bytes()),
                };
                expect_ok(transport.request(request, false).await)
            }
        }
    }

    /// `resize` (index.ts:194-211).
    pub async fn resize(
        &self,
        id: &PtyId,
        cols: u16,
        rows: u16,
        attachment_id: Option<&str>,
    ) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.resize(id, cols, rows, attachment_id).await,
            Backend::Daemon(transport) => {
                let request = WireRequest::Resize {
                    id: terminal_number(id)?,
                    attachment_id: attachment_id.map(str::to_owned),
                    cols,
                    rows,
                };
                expect_ok(transport.request(request, false).await)
            }
        }
    }

    /// `control` (index.ts:213-230).
    pub async fn control(
        &self,
        id: &PtyId,
        attachment_id: &str,
        cols: u16,
        rows: u16,
    ) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.control(id, attachment_id, cols, rows).await,
            Backend::Daemon(transport) => {
                let request = WireRequest::Control {
                    id: terminal_number(id)?,
                    attachment_id: attachment_id.to_owned(),
                    cols,
                    rows,
                };
                expect_ok(transport.request(request, false).await)
            }
        }
    }

    /// `input` (index.ts:232-251).
    pub async fn input(
        &self,
        id: &PtyId,
        attachment_id: &str,
        cols: u16,
        rows: u16,
        data: &[u8],
    ) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.input(id, attachment_id, cols, rows, data).await,
            Backend::Daemon(transport) => {
                let request = WireRequest::Input {
                    id: terminal_number(id)?,
                    attachment_id: attachment_id.to_owned(),
                    cols,
                    rows,
                    data_base64: super::base64_encode(data),
                };
                expect_ok(transport.request(request, false).await)
            }
        }
    }

    /// `snapshot` — the checkpoint bytes are the §7 v1 format over the
    /// wire, identical to the in-process bytes.
    pub async fn snapshot(&self, id: &PtyId) -> PubResult<Snapshot> {
        match &self.backend {
            Backend::InProcess(machine) => machine.snapshot(id),
            Backend::Daemon(transport) => {
                let request = WireRequest::Snapshot { id: terminal_number(id)? };
                match transport.request(request, false).await.map_err(unavailable)? {
                    WireResponse::Snapshot {
                        terminal,
                        text,
                        checkpoint_base64,
                        cursor_x,
                        cursor_y,
                    } => Ok(Snapshot {
                        info: info_from_wire(&terminal)
                            .map_err(|message| UnavailableError { message })?,
                        text,
                        checkpoint: super::base64_decode(&checkpoint_base64).unwrap_or_default(),
                        cursor: (
                            usize::try_from(cursor_x).unwrap_or(0),
                            usize::try_from(cursor_y).unwrap_or(0),
                        ),
                    }),
                    _ => Err(protocol_unexpected("snapshot")),
                }
            }
        }
    }

    /// `read(sessionID, lines?)` — `Rows{terminal: null}` maps to
    /// `None` (design §3, D1 report concern 2), as does an absent
    /// daemon.
    pub async fn read(
        &self,
        session_id: &SessionId,
        lines: Option<u32>,
    ) -> PubResult<Option<ReadResult>> {
        match &self.backend {
            Backend::InProcess(machine) => machine.read(session_id, lines).await,
            Backend::Daemon(transport) => {
                let request =
                    WireRequest::Read { session_id: session_id.as_str().to_owned(), rows: lines };
                match transport.request_if_running(request).await.map_err(unavailable)? {
                    // Absent daemon and Rows{terminal: null} both map to
                    // `None` (§3, D1 report concern 2).
                    None | Some(WireResponse::Rows { terminal: None, .. }) => Ok(None),
                    Some(WireResponse::Rows {
                        terminal: Some(terminal),
                        lines,
                        cursor_x,
                        cursor_y,
                    }) => {
                        let info = info_from_wire(&terminal)
                            .map_err(|message| UnavailableError { message })?;
                        Ok(Some(ReadResult {
                            pty_id: info.id,
                            title: info.title,
                            cwd: info.cwd,
                            foreground_process: info.foreground_process,
                            screen: super::Screen {
                                text: lines.join("\n"),
                                cols: info.size.0,
                                rows: info.size.1,
                                cursor: (
                                    usize::try_from(cursor_x).unwrap_or(0),
                                    usize::try_from(cursor_y).unwrap_or(0),
                                ),
                            },
                        }))
                    }
                    Some(_) => Err(protocol_unexpected("rows")),
                }
            }
        }
    }

    /// `remove` (`terminate`).
    pub async fn remove(&self, id: &PtyId) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.remove(id).await,
            Backend::Daemon(transport) => {
                let request = WireRequest::Terminate { id: terminal_number(id)? };
                expect_ok(transport.request(request, false).await)
            }
        }
    }

    /// `shutdown` — absent daemon is a no-op; a live daemon drains and
    /// the client waits for discover to fail (§3).
    pub async fn shutdown(&self) -> PubResult<()> {
        match &self.backend {
            Backend::InProcess(machine) => machine.shutdown().await,
            Backend::Daemon(transport) => {
                transport.shutdown().await.map_err(unavailable)?;
                Ok(())
            }
        }
    }

    /// `handoff` (`prepare_handoff`) — `None` when no daemon runs (the
    /// in-process machine's no-runtime-directory parity).
    pub async fn handoff(&self) -> PubResult<Option<Handoff>> {
        match &self.backend {
            Backend::InProcess(machine) => machine.handoff(),
            Backend::Daemon(transport) => transport.handoff().await.map_err(unavailable),
        }
    }

    /// `attach` — over the daemon this opens a §3 subscribe connection;
    /// a chunked replay (§3.2) reassembles BEFORE the attachment is
    /// returned, so `data` is the contiguous replay and
    /// `replay_complete` semantics are preserved for the WS consumer.
    pub async fn attach(&self, id: &PtyId, input: AttachInput<'_>) -> PubResult<Attachment> {
        match &self.backend {
            Backend::InProcess(machine) => {
                let attachment = machine.attach(id, input)?;
                Ok(Attachment {
                    info: attachment.info.clone(),
                    role: attachment.role,
                    generation: attachment.generation,
                    replay: attachment.replay.clone(),
                    data: attachment.data.clone(),
                    handle: AttachmentHandle::InProcess(attachment),
                })
            }
            Backend::Daemon(transport) => {
                let Some(number) = super::wire::parse_terminal_number(id) else {
                    return Err(UnavailableError {
                        message: format!("PersistentPty.NotFoundError: {}", id.0),
                    });
                };
                let stream: WireStream = transport
                    .subscribe(
                        number,
                        input.cursor as u64,
                        input.attachment_id,
                        wire_role(input.role),
                        input.takeover,
                    )
                    .await
                    .map_err(unavailable)?;
                let (info, role, generation, replay, data, pump) =
                    stream.into_pump(input.on_event, input.on_end);
                Ok(Attachment {
                    info,
                    role,
                    generation,
                    replay,
                    data,
                    handle: AttachmentHandle::Wire(pump),
                })
            }
        }
    }
}

/// The attach result: same public surface as the machine's
/// `Attachment` (`info/role/generation/replay/data` +
/// `activate`/`detach`).
pub struct Attachment {
    pub info: Info,
    pub role: Role,
    pub generation: u64,
    pub replay: Replay,
    pub data: Vec<u8>,
    handle: AttachmentHandle,
}

enum AttachmentHandle {
    InProcess(super::Attachment),
    Wire(transport::WirePump),
}

impl Attachment {
    /// Starts live delivery after the replay is applied.
    pub fn activate(&self) {
        match &self.handle {
            AttachmentHandle::InProcess(attachment) => attachment.activate(),
            AttachmentHandle::Wire(pump) => pump.activate(),
        }
    }

    /// Stops delivery.
    pub fn detach(&self) {
        match &self.handle {
            AttachmentHandle::InProcess(attachment) => attachment.detach(),
            AttachmentHandle::Wire(pump) => pump.detach(),
        }
    }
}

// -- helpers -------------------------------------------------------------

fn wire_role(role: Role) -> WireRole {
    match role {
        Role::Controller => WireRole::Controller,
        Role::Observer => WireRole::Observer,
    }
}

/// `pty_persistent_<n>` → `n`; foreign ids cannot ride the wire (they
/// can only be missing → the `NotFound` shape).
fn terminal_number(id: &PtyId) -> Result<u64, UnavailableError> {
    super::wire::parse_terminal_number(id).ok_or_else(|| UnavailableError {
        message: format!("PersistentPty.NotFoundError: {}", id.0),
    })
}

fn unavailable(error: TransportError) -> UnavailableError {
    UnavailableError { message: error.message }
}

fn protocol_unexpected(expected: &str) -> UnavailableError {
    UnavailableError {
        message: format!("opencode-pty returned an unexpected response (expected {expected})"),
    }
}

fn expect_ok(response: Result<WireResponse, TransportError>) -> PubResult<()> {
    match response.map_err(unavailable)? {
        WireResponse::Ok => Ok(()),
        _ => Err(protocol_unexpected("ok")),
    }
}
