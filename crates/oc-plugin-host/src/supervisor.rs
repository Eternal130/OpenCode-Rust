//! The plugin-host supervisor: spawn + supervise `bun host.js`, the
//! NDJSON JSON-RPC bridge (both directions), the request-id state
//! machine, crash restart with backoff + plugin-level disable, and the
//! timeout-bounded event dispatch pump.
//!
//! # Deadlock protocol (binding — crate docs §1)
//!
//! The rules the spike discovered the hard way, enforced structurally
//! here:
//!
//! 1. **The IO reader thread never blocks on anything but the pipe.**
//!    Guest→host requests (`command.add`, `theme.register`, `ping`,
//!    `event.subscribe`) are answered *on the reader task*: the result
//!    is computed synchronously (a registration insert / a log line)
//!    and the reply is *enqueued* — the writer task owns stdin, so no
//!    lock on the write path is ever held across a wait.
//! 2. **Callers never hold the write channel while awaiting replies.**
//!    [`Shared::request`] sends the frame (move into the writer's
//!    channel), *then* awaits a oneshot. A hundred concurrent
//!    `request`s never serialize behind each other's replies.
//! 3. **Every bridge call is timeout-bounded** (registration 2 s,
//!    event dispatch 5 s by default). A timed-out entry is removed
//!    from the pending map; a late reply is discarded by id. A hung
//!    plugin costs its own dispatch, never the host.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::context::{
    Bridge, HostState, PermissionPort, SessionPort, ToolPort, CODEMODE_METHODS, GENERATE_METHODS,
    SESSION_METHODS,
};
use crate::protocol::{self, Incoming};
use crate::registrations::{PluginState, PluginStatus, Registrations};
use crate::tap::TapEvent;
use crate::ui::UiRequest;

/// One `storage.scan` query (v2 `StorageScanOptions`).
#[derive(Debug, Clone, Default)]
pub struct StorageScanQuery {
    pub prefix: String,
    pub after: Option<String>,
    pub limit: Option<u64>,
}

/// The plugin-KV routing target (PLUG-D18): guest `ctx.storage.*` calls
/// land here, answered asynchronously off the IO reader (deadlock rule 1
/// — the durable backend's IO must never block the pipe). The embedder
/// implements this over the database KV table with the v2
/// `plugin:<hex(id)>:` namespace (oc-core's `DatabasePluginStorage`);
/// the host crate stays dependency-light, so the trait travels as data.
#[async_trait]
pub trait StoragePort: Send + Sync {
    /// `storage.get` → the value (`None` = absent; the guest surfaces
    /// `undefined`).
    async fn get(&self, plugin_id: &str, key: &str) -> Result<Option<Value>, String>;
    /// `storage.set` (an upsert; failures reject the guest call).
    async fn set(&self, plugin_id: &str, key: &str, value: Value) -> Result<(), String>;
    /// `storage.remove` (idempotent).
    async fn remove(&self, plugin_id: &str, key: &str) -> Result<(), String>;
    /// `storage.scan` → the v2 `{entries: [{key, value}], next?}` page
    /// with the namespace stripped from the keys.
    async fn scan(&self, plugin_id: &str, query: StorageScanQuery) -> Result<Value, String>;
}

/// Everything the supervisor needs at spawn time.
#[derive(Clone)]
pub struct SupervisorConfig {
    /// Resolved bun executable.
    pub bun: PathBuf,
    /// Materialized host script (`host.js` on disk).
    pub host_js: PathBuf,
    /// Working directory for the guest (the sandbox).
    pub cwd: PathBuf,
    /// Plugin entrypoints to load at start (and after each restart).
    pub entries: Vec<PathBuf>,
    /// Hard timeout for `initialize`/`load` (registration-time).
    pub registration_timeout: Duration,
    /// Hard timeout for event dispatch + command execution.
    pub dispatch_timeout: Duration,
    /// Crash-restart backoff base (doubles per consecutive crash).
    pub restart_backoff_base: Duration,
    /// Backoff ceiling.
    pub restart_backoff_max: Duration,
    /// A plugin whose crashes are attributed this often is disabled.
    pub max_plugin_crashes: u32,
    /// Consecutive restart attempts before the host gives up
    /// (attribution should disable the culprit first; this is the
    /// unattributable-crash guard).
    pub max_restarts: u32,
    /// Extra environment for the guest (test fixtures).
    pub env: Vec<(String, String)>,
    /// Host-owned transform-domain base state (seeded into the guest
    /// at load; wave 2).
    pub host_state: HostState,
    /// `ctx.session.*` routing target (default: declining port).
    pub session_port: Option<Arc<dyn SessionPort>>,
    /// `tools.<name>(input)` routing from Code Mode programs (default:
    /// declining port; the integrator binds the oc-core registry).
    pub tool_port: Option<Arc<dyn ToolPort>>,
    /// Config-sourced plugin options (`opencode.jsonc` plugin entries),
    /// keyed by entry path — surfaced as `ctx.options` (v2's
    /// `{ ...host, options: operation.options }`).
    pub entry_options: BTreeMap<String, Value>,
    /// The plugin-KV backend (PLUG-D18): `None` declines every
    /// `ctx.storage.*` call with a clear error; the embedder binds the
    /// database-backed port.
    pub storage_port: Option<Arc<dyn StoragePort>>,
    /// The permission backend (PLUG-D11): `None` declines every
    /// `ctx.permission.*` call; the embedder binds the engine port.
    pub permission_port: Option<Arc<dyn PermissionPort>>,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            bun: PathBuf::new(),
            host_js: PathBuf::new(),
            cwd: std::env::temp_dir(),
            entries: Vec::new(),
            registration_timeout: Duration::from_secs(2),
            dispatch_timeout: Duration::from_secs(5),
            restart_backoff_base: Duration::from_millis(100),
            restart_backoff_max: Duration::from_secs(4),
            max_plugin_crashes: 3,
            max_restarts: 8,
            env: Vec::new(),
            host_state: HostState::default(),
            session_port: None,
            tool_port: None,
            entry_options: BTreeMap::new(),
            storage_port: None,
            permission_port: None,
        }
    }
}

/// A bridge call failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// The guest did not answer within the hard timeout.
    Timeout(String),
    /// The guest process exited (or the channel broke) mid-call.
    Exited(String),
    /// The guest answered with an error.
    Guest(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout(what) => write!(f, "timed out waiting for {what}"),
            Self::Exited(why) => write!(f, "plugin host process unavailable: {why}"),
            Self::Guest(why) => write!(f, "plugin host error: {why}"),
        }
    }
}

impl std::error::Error for HostError {}

/// State shared by every task (reader/writer/pump/supervise) and the
/// synchronous [`crate::PluginHost`] facade.
pub struct Shared {
    pub config: SupervisorConfig,
    /// The CURRENT generation's outbound writer channel. Swapped on
    /// restart; sending a frame never blocks (unbounded).
    outbound: RwLock<mpsc::UnboundedSender<String>>,
    /// Host→guest pending requests (id → completion).
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next_id: AtomicU64,
    /// Process generation — bumped per (re)spawn; late replies from a
    /// dead generation are dropped by the reader's failure sweep.
    generation: AtomicU64,
    /// Set during shutdown: the reader's EOF sweep must not blame
    /// plugins for a deliberate kill.
    shutting_down: AtomicBool,
    /// A codemode-timeout kill of the guest is pending: set before the
    /// supervise loop signals the kill; the reader's EOF sweep consumes
    /// it (skipping crash attribution) or the supervise loop cancels it
    /// if the child died on its own first.
    codemode_kill_pending: AtomicBool,
    /// Bumped to ask the supervise loop to kill the CURRENT child (a
    /// codemode evaluate deadline expired and the guest may be wedged —
    /// a sync loop is not interruptible in-process).
    kill_tx: tokio::sync::watch::Sender<u64>,
    /// The native registration store (written by the reader task on
    /// guest requests, read by the facade).
    pub registrations: RwLock<Registrations>,
    /// entry path → attributed consecutive crashes.
    crash_counts: Mutex<HashMap<String, u32>>,
    /// Plugin ids currently mid-dispatch (guest `plugin.busy`
    /// notifications) — crash attribution.
    busy_plugins: Mutex<HashSet<String>>,
    /// entry path → last known import-graph digests (hot reload).
    digests: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Set once the first initialize+load pass finished (any plugin
    /// outcome) — [`crate::PluginHost::wait_ready`]'s condition.
    pub ready: Arc<(Mutex<bool>, Condvar)>,
    /// Diagnostics counters (the dropped-dispatch observability).
    pub dispatched_events: AtomicU64,
    pub dropped_events: AtomicU64,
    /// The CURRENT guest process id (diagnostics + the adversarial
    /// fuzz's kill target); -1 before the first spawn.
    pub child_pid: AtomicI64,
    /// Plugin → TUI UI requests (the wave-3 slot bridge): pushed on
    /// the IO reader (deadlock rule 1 — inline answer, enqueued
    /// reply), drained between frames by the embedder.
    pub ui_requests: Mutex<VecDeque<UiRequest>>,
    /// Plugins with a registered `sidebar_content` slot (the v2
    /// TUI-plugin surface, beta.13 bug 1) — gates the steady refresh
    /// loop; cleared when the slot stops answering.
    pub sidebar_slots: Mutex<HashSet<String>>,
    /// SA-F16: shielded scheduled guest callbacks' most recent faults
    /// (the EACCES-class filesystem probing), newest first, bounded.
    pub plugin_faults: Mutex<Vec<String>>,
    /// The runtime every supervisor task lives on — the async
    /// guest-request path (`session.*`) spawns onto it (deadlock rule
    /// 1: the reader never blocks on a port that calls back in).
    /// `None` on the dead facade (constructed off-runtime).
    pub(crate) runtime_handle: Option<tokio::runtime::Handle>,
}

impl Shared {
    /// A dead shared (spawn-failure facade): every request fails fast
    /// through the already-closed outbound channel, `ready` stays
    /// false forever.
    pub(crate) fn dead(outbound: mpsc::UnboundedSender<String>) -> Self {
        let config = SupervisorConfig::default();
        Self {
            config,
            outbound: RwLock::new(outbound),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            shutting_down: AtomicBool::new(true),
            registrations: RwLock::new(Registrations::default()),
            crash_counts: Mutex::new(HashMap::new()),
            busy_plugins: Mutex::new(HashSet::new()),
            digests: Mutex::new(HashMap::new()),
            ready: Arc::new((Mutex::new(false), Condvar::new())),
            dispatched_events: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            child_pid: AtomicI64::new(-1),
            ui_requests: Mutex::new(VecDeque::new()),
            sidebar_slots: Mutex::new(HashSet::new()),
            plugin_faults: Mutex::new(Vec::new()),
            runtime_handle: None,
            codemode_kill_pending: AtomicBool::new(false),
            kill_tx: tokio::sync::watch::channel(0).0,
        }
    }

    /// Ask the supervise loop to kill the CURRENT child (a codemode
    /// evaluate ran past its limits-aligned deadline — the guest is
    /// wedged or unbounded, and a sync loop is not interruptible
    /// in-process). Idempotent: while a kill is already in flight the
    /// same dying child covers every wedged evaluation. The deliberate
    /// kill is exempt from crash attribution
    /// ([`Shared::codemode_kill_pending`]).
    pub(crate) fn request_codemode_kill(&self) {
        if self.codemode_kill_pending.swap(true, Ordering::Relaxed) {
            return; // a kill is already in flight
        }
        eprintln!("opencode plugin-host: codemode evaluate deadline expired; killing the wedged guest (restart follows)");
        self.kill_tx.send_modify(|generation| *generation += 1);
    }

    /// One bridge call: send, then await with a hard timeout. See the
    /// deadlock protocol — the send is a channel move, no lock held.
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, HostError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("pending map poisoned").insert(id, tx);
        let frame = protocol::request(id, method, params);
        {
            let outbound = self.outbound.read().expect("outbound lock poisoned");
            if outbound.send(frame).is_err() {
                self.pending.lock().expect("pending map poisoned").remove(&id);
                return Err(HostError::Exited("writer task gone".into()));
            }
        }
        let outcome = tokio::time::timeout(timeout, rx).await;
        match outcome {
            Ok(Ok(result)) => result.map_err(HostError::Guest),
            Ok(Err(_dropped)) => {
                // The reader swept our entry (process died).
                Err(HostError::Exited("process exited mid-call".into()))
            }
            Err(_elapsed) => {
                self.pending.lock().expect("pending map poisoned").remove(&id);
                Err(HostError::Timeout(method.to_owned()))
            }
        }
    }

    /// Swap in a fresh writer channel for a new process generation;
    /// the stale sender is dropped so the old writer task exits, and
    /// everything still outstanding (sent on the dead generation's
    /// channel, registered after the reader's EOF sweep) fails NOW —
    /// callers retry/skip instead of waiting out their timeouts.
    fn rotate_outbound(&self, sender: mpsc::UnboundedSender<String>) {
        *self.outbound.write().expect("outbound lock poisoned") = sender;
        self.fail_pending("plugin host restarting");
    }

    /// Fail every pending host→guest call (reader EOF — the process is
    /// gone). Called by the reader task of the dying generation only.
    fn fail_pending(&self, why: &str) {
        let mut pending = self.pending.lock().expect("pending map poisoned");
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err(why.to_owned()));
        }
    }

    /// Answer a guest→host request ON THE READER TASK — synchronous
    /// registration mutation + an enqueued reply. This is deadlock
    /// rule 1: the guest is blocked on this reply while its own
    /// `load`/`dispatch` reply is still owed to us.
    ///
    /// `None` = the reply is deferred (the `session.*` async path
    /// spawns the port call; the reply is enqueued when it settles).
    fn answer_guest(self: &Arc<Self>, id: u64, method: &str, params: &Value) -> Option<String> {
        match method {
            "command.add" | "theme.register" => Some(answer_wave1(self, id, method, params)),
            // -- wave 2: transform-domain materialization (registration
            //    time only — the only synchronous surface).
            "agent.transform" | "mcp.transform" | "skill.transform" | "tool.transform" => {
                Some(answer_transform(self, id, method, params))
            }
            "hook.register" => {
                let hook = crate::registrations::PluginHook {
                    plugin_id: params
                        .get("pluginId")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_owned(),
                    domain: params
                        .get("domain")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: params.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                };
                if hook.domain.is_empty() || hook.name.is_empty() {
                    return Some(protocol::error_response(
                        id,
                        "hook.register requires domain+name",
                    ));
                }
                self.registrations.write().expect("registrations poisoned").add_hook(hook);
                Some(protocol::response(id, Value::Null))
            }
            // -- PLUG-D14: `ctx.rpc.register(definition, handlers)` —
            //    the definition lands in the native registry (method
            //    names gate dispatch); the handlers stay in the guest.
            "rpc.register" => Some(answer_rpc_register(self, id, params)),
            // -- plugin-ID-scoped durable KV (PLUG-D18): the DB-backed
            //    port answers asynchronously off the reader (deadlock
            //    rule 1 — the backend's IO never blocks the pipe).
            "storage.get" | "storage.set" | "storage.remove" | "storage.scan" => {
                self.answer_storage_async(id, method, params);
                None
            }
            // -- PLUG-D11: `ctx.permission.*` over the engine's
            //    permission machinery (async off the reader — the
            //    reply resolves a pending ask, which may run tools).
            "permission.list" | "permission.get" | "permission.reply" => {
                self.answer_permission_async(id, method, params);
                None
            }
            // -- wave 4: Code Mode nested tool calls (`tools.<path>`
            //    inside an `execute` program) — same async-off-the-reader
            //    routing as session.*, deadlock rule 1.
            other if CODEMODE_METHODS.contains(&other) => {
                self.answer_tool_async(id, other, params);
                None
            }
            // -- wave 4: `ctx.generate.text` rides the session port (the
            //    engine owns one-off generation, GenerateApi).
            other if GENERATE_METHODS.contains(&other) => {
                self.answer_session_async(id, other, params);
                None
            }
            "event.subscribe" | "ping" => Some(protocol::response(id, Value::Null)),
            // -- wave 3: the TUI slot bridge (registration entries +
            //    the runtime UI-request queue; answered inline).
            "ui.toast.show" | "ui.sidebar.section" | "ui.footer.hint" | "ui.dialog.open" => {
                Some(answer_ui_runtime(self, id, method, params))
            }
            // -- TUI-25: `ctx.ui.alert` / `ctx.ui.confirm` — the
            // reply is DEFERRED until the embedder resolves the
            // dialog (the guest's promise settles on it); the request
            // id rides the queue.
            "ui.alert" | "ui.confirm" => answer_ui_dialog(self, id, method, params),
            // -- the v2 TUI-plugin slot surface (beta.13 bug 1): the
            //    flagship's sidebar_content registration + its
            //    rerender asks. Both spawn the bounded materialize
            //    off the reader (deadlock rule 1) and answer inline.
            "ui.slot.register" | "ui.slot.rerender" => {
                let plugin_id =
                    params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
                self.materialize_sidebar_slot(&plugin_id);
                if method == "ui.slot.register" {
                    self.start_sidebar_refresh_loop(&plugin_id);
                }
                Some(protocol::response(id, Value::Null))
            }
            "ui.dialog.register" | "ui.group.rule" | "ui.home.quickaction"
            // -- the v2 TUI-plugin slot/router/keymap registrations
            //    (PLUG-TUI-SLOT): native snapshot entries, answered
            //    inline like every registration surface.
            | "ui.slot.claim" | "ui.router.register" | "keymap.layer" => {
                Some(answer_ui_registration(self, id, method, params))
            }
            "log" => {
                log_guest(params);
                Some(protocol::response(id, Value::Null))
            }
            other => {
                if let Some(method) = other.strip_prefix("session.") {
                    if !SESSION_METHODS.contains(&method) {
                        return Some(protocol::error_response(
                            id,
                            &format!("unknown session method {method:?}"),
                        ));
                    }
                    self.answer_session_async(id, method, params);
                    return None;
                }
                Some(protocol::error_response(id, &format!("unknown host method {other:?}")))
            }
        }
    }

    /// The async `ctx.storage.*` path (PLUG-D18): spawn the port call
    /// onto the supervisor runtime; the reply is enqueued when it
    /// settles. The reader task returns immediately (deadlock rule 1 —
    /// the durable backend's IO never blocks the pipe).
    fn answer_storage_async(self: &Arc<Self>, id: u64, method: &str, params: &Value) {
        let Some(handle) = self.runtime_handle.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(id, "plugin host runtime unavailable"));
            return;
        };
        let Some(port) = self.config.storage_port.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(
                id,
                "storage: no durable storage attached to the plugin host",
            ));
            return;
        };
        let plugin_id =
            params.get("pluginId").and_then(Value::as_str).unwrap_or_default().to_owned();
        let key = params.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
        let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
        let method = method.to_owned();
        let params = params.clone();
        handle.spawn(async move {
            let reply = match method.as_str() {
                "storage.get" => {
                    if key.is_empty() {
                        protocol::error_response(id, "storage.get requires a key")
                    } else {
                        match port.get(&plugin_id, &key).await {
                            Ok(value) => protocol::response(id, json!({ "value": value })),
                            Err(error) => protocol::error_response(id, &error),
                        }
                    }
                }
                "storage.set" => {
                    if key.is_empty() {
                        protocol::error_response(id, "storage.set requires a key")
                    } else if let Some(value) = params.get("value") {
                        match port.set(&plugin_id, &key, value.clone()).await {
                            Ok(()) => protocol::response(id, Value::Null),
                            Err(error) => protocol::error_response(id, &error),
                        }
                    } else {
                        protocol::error_response(id, "storage.set requires a value")
                    }
                }
                "storage.remove" => {
                    if key.is_empty() {
                        protocol::error_response(id, "storage.remove requires a key")
                    } else {
                        match port.remove(&plugin_id, &key).await {
                            Ok(()) => protocol::response(id, Value::Null),
                            Err(error) => protocol::error_response(id, &error),
                        }
                    }
                }
                "storage.scan" => {
                    let query = StorageScanQuery {
                        prefix: params
                            .get("prefix")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        after: params
                            .get("after")
                            .and_then(Value::as_str)
                            .filter(|after| !after.is_empty())
                            .map(str::to_owned),
                        limit: params.get("limit").and_then(Value::as_u64),
                    };
                    match port.scan(&plugin_id, query).await {
                        Ok(page) => protocol::response(id, page),
                        Err(error) => protocol::error_response(id, &error),
                    }
                }
                other => protocol::error_response(id, &format!("unknown storage method {other:?}")),
            };
            let _ = outbound.send(reply);
        });
    }

    /// The async `ctx.permission.*` path (PLUG-D11): spawn the port
    /// call onto the supervisor runtime; the reply is enqueued when it
    /// settles (same deadlock-rule-1 shape as `session.*`).
    fn answer_permission_async(self: &Arc<Self>, id: u64, method: &str, params: &Value) {
        let Some(handle) = self.runtime_handle.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(id, "plugin host runtime unavailable"));
            return;
        };
        let Some(port) = self.config.permission_port.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(
                id,
                &format!("permission.{method}: no permission engine attached to the plugin host"),
            ));
            return;
        };
        let args = params.get("args").cloned().unwrap_or(Value::Null);
        let session_id =
            args.get("sessionID").and_then(Value::as_str).unwrap_or_default().to_owned();
        let request_id =
            args.get("requestID").and_then(Value::as_str).unwrap_or_default().to_owned();
        let decision = args.get("decision").and_then(Value::as_str).unwrap_or_default().to_owned();
        let message = args.get("message").and_then(Value::as_str).map(str::to_owned);
        let method = method.to_owned();
        let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
        handle.spawn(async move {
            let reply = match method.as_str() {
                "permission.list" => match port.list(&session_id).await {
                    Ok(value) => protocol::response(id, json!({ "value": value })),
                    Err(message) => protocol::error_response(id, &message),
                },
                "permission.get" => match port.get(&session_id, &request_id).await {
                    Ok(value) => protocol::response(id, json!({ "value": value })),
                    Err(message) => protocol::error_response(id, &message),
                },
                "permission.reply" => {
                    if request_id.is_empty() {
                        protocol::error_response(id, "permission.reply requires a requestID")
                    } else {
                        match port.reply(&session_id, &request_id, &decision, message).await {
                            Ok(()) => protocol::response(id, Value::Null),
                            Err(message) => protocol::error_response(id, &message),
                        }
                    }
                }
                other => {
                    protocol::error_response(id, &format!("unknown permission method {other:?}"))
                }
            };
            let _ = outbound.send(reply);
        });
    }

    /// The async `session.*` path: spawn the port call onto the
    /// supervisor runtime; the reply is enqueued when it settles. The
    /// reader task returns immediately (deadlock rule 1 — a port that
    /// dispatches hooks back into the guest must never block it).
    fn answer_session_async(self: &Arc<Self>, id: u64, method: &str, params: &Value) {
        let Some(handle) = self.runtime_handle.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(id, "plugin host runtime unavailable"));
            return;
        };
        let Some(port) = self.config.session_port.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(
                id,
                &format!("session.{method}: no session engine attached to the plugin host"),
            ));
            return;
        };
        let bridge = Bridge { shared: Arc::clone(self) };
        let args = params.get("args").cloned().unwrap_or(Value::Null);
        let method = method.to_owned();
        let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
        handle.spawn(async move {
            let reply = match port.call(&bridge, &method, &args).await {
                Ok(value) => protocol::response(id, json!({ "value": value })),
                Err(message) => protocol::error_response(id, &message),
            };
            let _ = outbound.send(reply);
        });
    }

    /// The async `codemode.tool` path — identical shape to
    /// [`Shared::answer_session_async`] over the tool port.
    fn answer_tool_async(self: &Arc<Self>, id: u64, method: &str, params: &Value) {
        let _ = method; // CODEMODE_METHODS routing reached here already
        let Some(handle) = self.runtime_handle.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(id, "plugin host runtime unavailable"));
            return;
        };
        let Some(port) = self.config.tool_port.clone() else {
            let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
            let _ = outbound.send(protocol::error_response(
                id,
                "codemode.tool: no tool engine attached to the plugin host",
            ));
            return;
        };
        let bridge = Bridge { shared: Arc::clone(self) };
        let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        let input = params.get("input").cloned().unwrap_or(Value::Null);
        let token = params.get("token").and_then(Value::as_u64).unwrap_or(0);
        let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
        handle.spawn(async move {
            let reply = match port.call_in(&bridge, &name, &input, token).await {
                Ok(value) => protocol::response(id, json!({ "value": value })),
                Err(message) => protocol::error_response(id, &message),
            };
            let _ = outbound.send(reply);
        });
    }

    /// A guest notification (fire-and-forget).
    fn note(&self, method: &str, params: &Value) {
        match method {
            "plugin.busy" => {
                if let Some(id) = params.get("pluginId").and_then(Value::as_str) {
                    self.busy_plugins.lock().expect("busy set poisoned").insert(id.to_owned());
                }
            }
            "plugin.idle" => {
                if let Some(id) = params.get("pluginId").and_then(Value::as_str) {
                    self.busy_plugins.lock().expect("busy set poisoned").remove(id);
                }
            }
            // The v2 TUI-plugin bridge's navigation ask: a plugin row's
            // activate handler called `route.navigate("session",
            // {sessionID})` (beta.13 bug 2's plugin-world path - the
            // omo-slim sidebar's session targets).
            "tui.route.navigate" => {
                if let Some(session) = params.get("sessionID").and_then(Value::as_str) {
                    self.ui_requests
                        .lock()
                        .expect("ui request queue poisoned")
                        .push_back(UiRequest::NavigateToSession { session_id: session.to_owned() });
                }
            }
            // The plugin-route navigation ask (`ctx.ui.router.navigate(
            // {type: "plugin", name, data?})`, api.tsx:211-224): the
            // plugin's registered page becomes the visible screen.
            "tui.route.plugin" => {
                if let Some(name) = params.get("name").and_then(Value::as_str) {
                    self.ui_requests.lock().expect("ui request queue poisoned").push_back(
                        UiRequest::NavigateToPluginRoute {
                            plugin_id: params
                                .get("pluginId")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            name: name.to_owned(),
                            data: params.get("data").cloned().filter(|data| !data.is_null()),
                        },
                    );
                }
            }
            "log" => log_guest(params),
            // SA-F16: a shielded scheduled guest callback failed (the
            // EACCES-class filesystem probing) — record it as a health
            // fault (bounded, most-recent-first) instead of letting it
            // surface as a process-level uncaught exception.
            "plugin.fault" => {
                let message = params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown fault")
                    .to_owned();
                let mut faults = self.plugin_faults.lock().expect("plugin faults poisoned");
                faults.insert(0, message);
                faults.truncate(16);
            }
            _ => {}
        }
    }

    /// Attribute a crash: plugins that were mid-load or mid-dispatch
    /// get a strike; at [`SupervisorConfig::max_plugin_crashes`] they
    /// are disabled (the wave-1 supervision policy).
    fn attribute_crash(&self) {
        let loading: Vec<String> = self
            .registrations
            .read()
            .expect("registrations poisoned")
            .plugins
            .values()
            .filter(|status| status.state == PluginState::Loading)
            .map(|status| status.entry.to_string_lossy().into_owned())
            .collect();
        let busy: Vec<String> = {
            let busy = self.busy_plugins.lock().expect("busy set poisoned");
            let registrations = self.registrations.read().expect("registrations poisoned");
            busy.iter()
                .filter_map(|plugin_id| {
                    registrations
                        .plugins
                        .values()
                        .find(|status| &status.plugin_id == plugin_id)
                        .map(|status| status.entry.to_string_lossy().into_owned())
                })
                .collect()
        };
        // Busy dispatchers take priority: a Loading plugin crashed at
        // import time, a busy one crashed inside its handler.
        let mut blamed: HashSet<String> = HashSet::new();
        if busy.is_empty() {
            blamed.extend(loading);
        } else {
            blamed.extend(busy);
        }
        self.busy_plugins.lock().expect("busy set poisoned").clear();
        let max = self.config.max_plugin_crashes;
        let mut registrations = self.registrations.write().expect("registrations poisoned");
        for entry in blamed {
            let count = *self
                .crash_counts
                .lock()
                .expect("crash counts poisoned")
                .entry(entry.clone())
                .and_modify(|count| *count += 1)
                .or_insert(1);
            if let Some(status) = registrations.plugins.get_mut(&entry) {
                let plugin_id = status.plugin_id.clone();
                let disabled = count >= max;
                status.state = if disabled {
                    PluginState::Disabled { crashes: count }
                } else {
                    PluginState::Failed(format!("host crash #{count} attributed"))
                };
                if disabled {
                    // Disabled: its registrations die with it (no
                    // reload will ever refresh them).
                    registrations.clear_plugin(&plugin_id);
                }
            }
            eprintln!("opencode plugin-host: crash attributed to {entry} ({count}/{max})");
        }
    }
}

/// The wave-1 registration arms (commands incl. owned keymap entries,
/// theme documents) — extracted to keep [`Shared::answer_guest`]
/// readable.
fn answer_wave1(shared: &Shared, id: u64, method: &str, params: &Value) -> String {
    let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    if name.is_empty() {
        return protocol::error_response(id, &format!("{method} requires a name"));
    }
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
    match method {
        "command.add" => {
            let command = crate::registrations::PluginCommand {
                plugin_id,
                name: name.to_owned(),
                description: params
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                keybinds: params
                    .get("keybinds")
                    .and_then(Value::as_array)
                    .map(|specs| {
                        specs.iter().filter_map(Value::as_str).map(str::to_owned).collect()
                    })
                    .unwrap_or_default(),
                palette: params.get("palette").and_then(Value::as_bool).unwrap_or(true),
            };
            shared
                .registrations
                .write()
                .expect("registrations poisoned")
                .commands
                .insert(name.to_owned(), command);
            protocol::response(id, Value::Null)
        }
        "theme.register" => {
            let Some(document) = params.get("document") else {
                return protocol::error_response(id, "theme.register requires a document");
            };
            let theme = crate::registrations::PluginTheme {
                plugin_id,
                name: name.to_owned(),
                document: document.to_string(),
            };
            shared
                .registrations
                .write()
                .expect("registrations poisoned")
                .themes
                .insert(name.to_owned(), theme);
            protocol::response(id, Value::Null)
        }
        other => protocol::error_response(id, &format!("unknown wave-1 method {other:?}")),
    }
}

/// The transform-domain materialization arms (`agent/mcp/skill/tool.transform`):
/// the guest sends the whole derived table; the merge keeps
/// provenance. Registration-time only.
fn answer_transform(shared: &Shared, id: u64, method: &str, params: &Value) -> String {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?");
    let mut registrations = shared.registrations.write().expect("registrations poisoned");
    match method {
        "agent.transform" => {
            let agents =
                params.get("agents").and_then(Value::as_array).cloned().unwrap_or_default();
            let default = params.get("default").and_then(Value::as_str).filter(|s| !s.is_empty());
            registrations.merge_agents(plugin_id, &agents, default);
        }
        "mcp.transform" => {
            let servers: BTreeMap<String, Value> = params
                .get("servers")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default();
            registrations.merge_mcps(plugin_id, &servers);
        }
        "skill.transform" => {
            let skills =
                params.get("skills").and_then(Value::as_array).cloned().unwrap_or_default();
            registrations.merge_skills(plugin_id, &skills);
        }
        "tool.transform" => {
            let tools = params.get("tools").and_then(Value::as_array).cloned().unwrap_or_default();
            registrations.merge_tools(plugin_id, &tools);
        }
        other => return protocol::error_response(id, &format!("unknown transform {other:?}")),
    }
    protocol::response(id, Value::Null)
}

fn log_guest(params: &Value) {
    let level = params.get("level").and_then(Value::as_str).unwrap_or("info");
    let message = params.get("message").and_then(Value::as_str).unwrap_or("");
    let plugin = params.get("pluginId").and_then(Value::as_str).unwrap_or("host");
    if level == "error" || level == "warn" {
        eprintln!("opencode plugin-host [{plugin}] {level}: {message}");
    }
}

/// The `rpc.register` guest-request arm (PLUG-D14): validate + record
/// the definition; the handlers stay guest-side (`rpc.call` routes back
/// through the supervisor's request path).
fn answer_rpc_register(shared: &Shared, id: u64, params: &Value) -> String {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
    let Some(definition) = params.get("definition") else {
        return protocol::error_response(id, "rpc.register requires a definition");
    };
    let Some(rpc_id) = definition.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return protocol::error_response(id, "rpc.register requires a definition id");
    };
    if rpc_id.is_empty() {
        return protocol::error_response(id, "rpc.register requires a non-empty definition id");
    }
    let methods: Vec<String> = definition
        .get("methods")
        .and_then(Value::as_object)
        .map(|methods| methods.keys().cloned().collect())
        .unwrap_or_default();
    shared.registrations.write().expect("registrations poisoned").register_rpc(
        crate::registrations::PluginRpc {
            plugin_id,
            id: rpc_id,
            methods,
            definition: definition.clone(),
        },
    );
    protocol::response(id, Value::Null)
}

/// The runtime `ui.*` arms (wave 3's slot bridge): parse, push onto
/// the UI-request queue, reply inline. The TUI drains the queue
/// between frames — a slow plugin never stalls the reader (rule 1:
/// this is bounded parsing + a queue push).
fn answer_ui_runtime(shared: &Shared, id: u64, method: &str, params: &Value) -> String {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
    let request = match method {
        "ui.toast.show" => {
            let Some(message) = params.get("message").and_then(Value::as_str) else {
                return protocol::error_response(id, "ui.toast.show requires a message");
            };
            UiRequest::Toast {
                title: params.get("title").and_then(Value::as_str).map(str::to_owned),
                message: message.to_owned(),
                variant: params.get("variant").and_then(Value::as_str).unwrap_or("info").to_owned(),
                duration_ms: params
                    .get("duration")
                    .and_then(Value::as_u64)
                    .unwrap_or(crate::ui::DEFAULT_TOAST_DURATION_MS),
                plugin_id,
            }
        }
        "ui.sidebar.section" => {
            let title = params.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
            let rows: Vec<String> = params
                .get("rows")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            // An empty title + empty rows is `removeSection` (the TUI
            // drops the plugin's section) — plain rows, no handlers.
            let clickable = vec![false; rows.len()];
            UiRequest::SidebarSection { plugin_id, title, rows, clickable }
        }
        "ui.footer.hint" => {
            let Some(text) = params.get("text").and_then(Value::as_str) else {
                return protocol::error_response(id, "ui.footer.hint requires text");
            };
            UiRequest::FooterHint { plugin_id, text: text.to_owned() }
        }
        "ui.dialog.open" => {
            let Some(spec) = params.get("spec") else {
                return protocol::error_response(id, "ui.dialog.open requires a spec");
            };
            UiRequest::DialogOpen { plugin_id, spec: crate::ui::PluginDialogSpec::from_json(spec) }
        }
        other => return protocol::error_response(id, &format!("unknown ui method {other:?}")),
    };
    shared.ui_requests.lock().expect("ui request queue poisoned").push_back(request);
    protocol::response(id, Value::Null)
}

/// TUI-25: the deferred `ui.alert` / `ui.confirm` arms — push the
/// [`UiRequest`] carrying the GUEST's request id and return `None`
/// (the reply goes out later through [`Shared::resolve_ui_request`],
/// when the TUI's dialog settles).
fn answer_ui_dialog(shared: &Shared, id: u64, method: &str, params: &Value) -> Option<String> {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
    let title = params.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
    let message = params.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
    let label_at = |key: &str| {
        params.pointer(&format!("/label/{key}")).and_then(Value::as_str).map(str::to_owned)
    };
    let request = match method {
        "ui.alert" => UiRequest::Alert { request_id: id, plugin_id, title, message },
        "ui.confirm" => UiRequest::Confirm {
            request_id: id,
            plugin_id,
            title,
            message,
            confirm_label: label_at("confirm"),
            cancel_label: label_at("cancel"),
        },
        other => {
            return Some(protocol::error_response(id, &format!("unknown ui method {other:?}")));
        }
    };
    shared.ui_requests.lock().expect("ui request queue poisoned").push_back(request);
    None
}

/// The registration-time `ui.*` arms: dialog-provider coverage, group
/// rules, home quick actions — all native entries in
/// [`Registrations`] (the snapshot is the TUI's only synchronous
/// surface; nothing here crosses IPC again).
#[allow(clippy::too_many_lines)] // one arm per registration surface
fn answer_ui_registration(shared: &Shared, id: u64, method: &str, params: &Value) -> String {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
    let mut registrations = shared.registrations.write().expect("registrations poisoned");
    match method {
        "ui.dialog.register" => {
            let raw = params.get("id").and_then(Value::as_str).unwrap_or_default();
            if raw.is_empty() {
                return protocol::error_response(id, "ui.dialog.register requires an id");
            }
            // Qualified per host so two plugins may reuse a local id.
            let qualified = format!("{plugin_id}:{raw}");
            registrations.dialogs.insert(
                qualified.clone(),
                crate::registrations::PluginDialog { plugin_id, id: qualified.clone() },
            );
            protocol::response(id, json!({ "id": qualified }))
        }
        "ui.group.rule" => {
            let header = params.get("header").and_then(Value::as_str).unwrap_or_default();
            if header.is_empty() {
                return protocol::error_response(id, "ui.group.rule requires a header");
            }
            let rule = crate::registrations::PluginGroupRule {
                plugin_id,
                tool: params
                    .get("tool")
                    .and_then(Value::as_str)
                    .filter(|tool| !tool.is_empty())
                    .map(str::to_owned),
                tool_prefix: params
                    .get("toolPrefix")
                    .and_then(Value::as_str)
                    .filter(|prefix| !prefix.is_empty())
                    .map(str::to_owned),
                header: header.to_owned(),
            };
            if rule.tool.is_none() && rule.tool_prefix.is_none() {
                return protocol::error_response(id, "ui.group.rule requires tool or toolPrefix");
            }
            registrations.group_rules.push(rule);
            protocol::response(id, Value::Null)
        }
        "ui.home.quickaction" => {
            let label = params.get("label").and_then(Value::as_str).unwrap_or_default();
            let command = params.get("command").and_then(Value::as_str).unwrap_or_default();
            if label.is_empty() || command.is_empty() {
                return protocol::error_response(
                    id,
                    "ui.home.quickaction requires label and command",
                );
            }
            registrations.home_actions.push(crate::registrations::PluginHomeAction {
                plugin_id,
                label: label.to_owned(),
                command: command.to_owned(),
            });
            protocol::response(id, Value::Null)
        }
        // The v2 TUI-plugin slot claim (`ctx.ui.slot`, api.tsx:298-312):
        // exactly one placement kind + a non-empty target (the guest
        // already enforces the single-kind rule; the host re-checks —
        // registration surfaces validate both sides).
        "ui.slot.claim" => {
            let kind = params.get("kind").and_then(Value::as_str).unwrap_or_default();
            let target = params.get("target").and_then(Value::as_str).unwrap_or_default();
            let key = params.get("key").and_then(Value::as_str).unwrap_or_default();
            let valid_kind = matches!(kind, "prepend" | "append" | "before" | "after" | "replace");
            if !valid_kind || target.is_empty() || key.is_empty() {
                return protocol::error_response(
                    id,
                    "ui.slot.claim requires exactly one placement kind and a target",
                );
            }
            registrations.slot_claims.push(crate::registrations::PluginSlotClaim {
                plugin_id,
                key: key.to_owned(),
                kind: kind.to_owned(),
                target: target.to_owned(),
            });
            protocol::response(id, Value::Null)
        }
        // The plugin route registration (`ctx.ui.router.register`,
        // api.tsx:204-212). A duplicate name from ANOTHER plugin is
        // rejected with the ref's error (`Route already registered`).
        "ui.router.register" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.is_empty() {
                return protocol::error_response(id, "ui.router.register requires a name");
            }
            if let Some(existing) = registrations.routes.get(name) {
                if existing.plugin_id != plugin_id {
                    return protocol::error_response(
                        id,
                        &format!("Route already registered: {name}"),
                    );
                }
            }
            registrations.routes.insert(
                name.to_owned(),
                crate::registrations::PluginRoute { plugin_id, name: name.to_owned() },
            );
            protocol::response(id, Value::Null)
        }
        // The keymap layer (`ctx.keymap.layer(factory)`,
        // api.tsx:179-199): the factory's command definitions land as
        // native entries (the `run` callbacks stay in the guest).
        "keymap.layer" => {
            let Some(commands) = params.get("commands").and_then(Value::as_array) else {
                return protocol::error_response(id, "keymap.layer requires commands");
            };
            for command in commands {
                let id_field =
                    command.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
                let bind =
                    command.get("bind").and_then(Value::as_str).unwrap_or_default().to_owned();
                if id_field.is_empty() || bind.is_empty() {
                    return protocol::error_response(
                        id,
                        "keymap.layer commands require an id and a bind",
                    );
                }
                registrations.keymap_commands.push(crate::registrations::PluginKeymapCommand {
                    plugin_id: plugin_id.clone(),
                    id: id_field,
                    title: command
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    group: command
                        .get("group")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    bind,
                    palette: command.get("palette").and_then(Value::as_bool).unwrap_or(false),
                    slash: command
                        .get("slash")
                        .and_then(Value::as_str)
                        .filter(|slash| !slash.is_empty())
                        .map(str::to_owned),
                    mode: command
                        .get("mode")
                        .and_then(Value::as_str)
                        .unwrap_or("global")
                        .to_owned(),
                });
            }
            protocol::response(id, Value::Null)
        }
        other => protocol::error_response(id, &format!("unknown ui registration {other:?}")),
    }
}

/// A running supervisor handle (cloneable, kept alive by the facade).
pub struct Supervisor {
    pub shared: Arc<Shared>,
    /// Set true → the supervise loop kills the child and exits.
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub tap_tx: mpsc::Sender<TapEvent>,
    /// Set once the supervise loop confirmed the CURRENT child is dead
    /// (killed-and-reaped by handle — never a naked-PID signal). The
    /// deterministic teardown for [`Supervisor::shutdown`]: by the time
    /// it returns, no live guest remains (incident 2026-10-22).
    child_reaped: Arc<AtomicBool>,
}

impl Supervisor {
    /// Spawn the host, run the initial load pass, and start every
    /// background task. Must be called inside a tokio runtime.
    pub async fn start(config: SupervisorConfig) -> Result<Arc<Self>, String> {
        let (tap_tx, tap_rx) = mpsc::channel::<TapEvent>(1024);
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<String>();
        let (kill_tx, kill_rx) = tokio::sync::watch::channel(0u64);
        let shared = Arc::new(Shared {
            config: config.clone(),
            outbound: RwLock::new(outbound_tx),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            generation: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
            codemode_kill_pending: AtomicBool::new(false),
            kill_tx,
            registrations: RwLock::new(Registrations::default()),
            crash_counts: Mutex::new(HashMap::new()),
            busy_plugins: Mutex::new(HashSet::new()),
            digests: Mutex::new(HashMap::new()),
            ready: Arc::new((Mutex::new(false), Condvar::new())),
            dispatched_events: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            child_pid: AtomicI64::new(-1),
            ui_requests: Mutex::new(VecDeque::new()),
            sidebar_slots: Mutex::new(HashSet::new()),
            plugin_faults: Mutex::new(Vec::new()),
            runtime_handle: Some(tokio::runtime::Handle::current()),
        });
        // Prime plugin statuses before any IO so crash attribution
        // always sees the entries.
        {
            let mut registrations = shared.registrations.write().expect("registrations poisoned");
            for entry in &config.entries {
                registrations.plugins.insert(
                    entry.to_string_lossy().into_owned(),
                    PluginStatus {
                        entry: entry.clone(),
                        plugin_id: String::new(),
                        state: PluginState::Loading,
                    },
                );
            }
        }

        let (child, stdin, stdout) = spawn_process(&config).map_err(|error| {
            eprintln!("opencode plugin-host: spawn failed ({error}); plugin subsystem disabled");
            error
        })?;
        if let Some(pid) = child.id() {
            shared.child_pid.store(i64::from(pid), Ordering::Relaxed);
        }
        spawn_writer(outbound_rx, stdin);
        spawn_reader(Arc::clone(&shared), stdout, 1);

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let child_reaped = Arc::new(AtomicBool::new(false));
        let supervisor = Arc::new(Self {
            shared: Arc::clone(&shared),
            shutdown_tx,
            tap_tx,
            child_reaped: Arc::clone(&child_reaped),
        });

        // The supervision loop owns the child from here (the kill-signal
        // receiver rides along — a codemode deadline asks it to kill the
        // wedged child; the wait arm below handles the restart).
        tokio::spawn(supervise_loop(
            Arc::clone(&shared),
            child,
            shutdown_rx,
            kill_rx,
            child_reaped,
        ));
        // The event dispatch pump (post-ingest tap consumer).
        tokio::spawn(dispatch_pump(Arc::clone(&shared), tap_rx));

        // Initial handshake + load pass.
        shared
            .request("initialize", json!({}), config.registration_timeout)
            .await
            .map_err(|error| format!("initialize: {error}"))?;
        for entry in &config.entries {
            load_entry(&shared, entry).await;
        }
        shared.scan_all().await;
        mark_ready(&shared);
        Ok(supervisor)
    }

    /// Signal shutdown; the supervise loop kills the child. Returns
    /// once that kill has been CONFIRMED (the child reaped through the
    /// owned handle) or the bounded wait elapsed — callers (the facade
    /// join, test sandboxes) never observe a live guest afterwards.
    pub fn shutdown(&self) {
        self.shared.shutting_down.store(true, Ordering::Relaxed);
        let _ = self.shutdown_tx.send(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !self.child_reaped.load(Ordering::Relaxed) {
            if std::time::Instant::now() >= deadline {
                // The supervise loop is wedged past the bound; the
                // kill_on_drop belt (and the guest's EOF self-exit +
                // PDEATHSIG) still own the child's death.
                eprintln!("opencode plugin-host: shutdown wait for child reaped timed out");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

async fn load_entry(shared: &Arc<Shared>, entry: &std::path::Path) {
    let key = entry.to_string_lossy().into_owned();
    {
        let mut registrations = shared.registrations.write().expect("registrations poisoned");
        if let Some(status) = registrations.plugins.get_mut(&key) {
            status.state = PluginState::Loading;
        } else {
            registrations.plugins.insert(
                key.clone(),
                PluginStatus {
                    entry: entry.to_owned(),
                    plugin_id: String::new(),
                    state: PluginState::Loading,
                },
            );
        }
    }
    let generation = shared.generation.load(Ordering::Relaxed);
    let options = shared.config.entry_options.get(&key).cloned().unwrap_or(Value::Null);
    let outcome = shared
        .request(
            "load",
            json!({
                "entry": key,
                "generation": generation,
                "seed": shared.config.host_state.to_seed_json(),
                "options": options,
            }),
            shared.config.registration_timeout,
        )
        .await;
    // The guard scope: registrations never live across an await
    // (Send futures — the status update happens, then the block
    // ends before the TUI-companion load below).
    let loaded = match &outcome {
        Ok(result) => {
            let plugin_id =
                result.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
            let mut registrations = shared.registrations.write().expect("registrations poisoned");
            if let Some(status) = registrations.plugins.get_mut(&key) {
                status.plugin_id.clone_from(&plugin_id);
                status.state = PluginState::Active;
            }
            plugin_id
        }
        Err(error) => {
            // A load that merely failed/timed out is contained (no
            // crash strike — that is process-death attribution).
            let mut registrations = shared.registrations.write().expect("registrations poisoned");
            if let Some(status) = registrations.plugins.get_mut(&key) {
                status.state = PluginState::Failed(error.to_string());
            }
            eprintln!("opencode plugin-host: load {key} failed: {error}");
            return;
        }
    };
    // The v2 TUI-plugin companion (beta.13 bug 1): a package with an
    // `exports["./tui"]` entry (oh-my-opencode-slim's `dist/tui2.js`)
    // drives its sidebar through `slots.register({slots:
    // {sidebar_content: render}})` — load it next so the real
    // plugin's sections reach the native sidebar bridge. A missing
    // entry or a failed load is contained (the server entry stays
    // active).
    if let Some(tui_entry) = probe_tui_entry(entry) {
        let tui_outcome = shared
            .request(
                "load.tui",
                json!({
                    "entry": tui_entry.display().to_string(),
                    "options": options,
                }),
                shared.config.registration_timeout,
            )
            .await;
        match tui_outcome {
            Ok(result) => {
                let tui_id =
                    result.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
                let sidebar = result.get("sidebar").and_then(Value::as_bool).unwrap_or(false);
                eprintln!(
                    "opencode plugin-host: tui entry loaded for {loaded} ({tui_id}, sidebar slot: {sidebar})"
                );
            }
            Err(error) => {
                eprintln!(
                    "opencode plugin-host: tui entry for {loaded} failed to load (contained): {error}"
                );
            }
        }
    }
}

/// The package's `exports["./tui"]` entry, when the loaded entry
/// sits inside a package that ships one (the flagship's companion
/// TUI plugin). Walks up from the entry to the package root
/// (the nearest `package.json`) and probes the subpath.
fn probe_tui_entry(entry: &Path) -> Option<PathBuf> {
    let mut package_root = entry.parent()?;
    loop {
        if package_root.join("package.json").is_file() {
            break;
        }
        package_root = package_root.parent()?;
    }
    crate::context::install::resolve_subpath_entry(package_root, "./tui").ok()
}

fn mark_ready(shared: &Arc<Shared>) {
    let (lock, condvar) = &*shared.ready;
    let mut ready = lock.lock().expect("ready lock poisoned");
    *ready = true;
    condvar.notify_all();
}

impl Shared {
    /// Refresh digests for every non-disabled entry (used at start
    /// and by [`Shared::reload_changed`]).
    async fn scan_all(&self) {
        let entries: Vec<PathBuf> = self.loadable_entries();
        for entry in entries {
            let key = entry.to_string_lossy().into_owned();
            if let Ok(result) =
                self.request("scan", json!({ "entry": key }), self.config.dispatch_timeout).await
            {
                if let Some(digests) = decode_digests(&result) {
                    self.digests.lock().expect("digests poisoned").insert(key, digests);
                }
            }
        }
    }

    fn loadable_entries(&self) -> Vec<PathBuf> {
        self.registrations
            .read()
            .expect("registrations poisoned")
            .plugins
            .values()
            .filter(|status| !matches!(status.state, PluginState::Disabled { .. }))
            .map(|status| status.entry.clone())
            .collect()
    }

    /// Compare current digests against the stored ones; reload every
    /// plugin whose graph changed. Returns the reloaded count.
    pub async fn reload_changed(&self) -> Result<usize, HostError> {
        let entries = self.loadable_entries();
        let mut reloaded = 0usize;
        for entry in entries {
            let key = entry.to_string_lossy().into_owned();
            let Ok(result) =
                self.request("scan", json!({ "entry": key }), self.config.dispatch_timeout).await
            else {
                continue; // entry vanished — next poll / restart handles it
            };
            let Some(fresh) = decode_digests(&result) else { continue };
            let changed = {
                let digests = self.digests.lock().expect("digests poisoned");
                digests.get(&key).is_none_or(|stored| *stored != fresh)
            };
            if !changed {
                continue;
            }
            // Unload the OLD plugin id (registrations cleared), bump
            // the generation, re-load, store the new digests.
            let old_id = {
                let registrations = self.registrations.read().expect("registrations poisoned");
                registrations.plugins.get(&key).map(|status| status.plugin_id.clone())
            };
            if let Some(old_id) = old_id.filter(|id| !id.is_empty()) {
                self.registrations.write().expect("registrations poisoned").clear_plugin(&old_id);
                let _ = self
                    .request(
                        "unload",
                        json!({ "pluginId": old_id }),
                        self.config.registration_timeout,
                    )
                    .await;
            }
            let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
            let reload_options =
                self.config.entry_options.get(&key).cloned().unwrap_or(Value::Null);
            let outcome = self
                .request(
                    "load",
                    json!({
                        "entry": key,
                        "generation": generation,
                        "seed": self.config.host_state.to_seed_json(),
                        "options": reload_options,
                    }),
                    self.config.registration_timeout,
                )
                .await;
            self.digests.lock().expect("digests poisoned").insert(key.clone(), fresh);
            match outcome {
                Ok(result) => {
                    let plugin_id =
                        result.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
                    let mut registrations =
                        self.registrations.write().expect("registrations poisoned");
                    if let Some(status) = registrations.plugins.get_mut(&key) {
                        status.plugin_id = plugin_id;
                        status.state = PluginState::Active;
                    }
                    reloaded += 1;
                    eprintln!("opencode plugin-host: hot reload {key}");
                }
                Err(error) => {
                    eprintln!("opencode plugin-host: hot reload {key} failed: {error}");
                }
            }
        }
        Ok(reloaded)
    }

    /// Look up the owning plugin of a command name.
    pub fn plugin_for_command(&self, name: &str) -> Option<String> {
        self.registrations
            .read()
            .expect("registrations poisoned")
            .commands
            .get(name)
            .map(|command| command.plugin_id.clone())
    }

    /// Look up the winning RPC registration for an id (PLUG-D14).
    pub fn rpc_registration(&self, rpc_id: &str) -> Option<crate::registrations::PluginRpc> {
        self.registrations.read().expect("registrations poisoned").rpcs.get(rpc_id).cloned()
    }

    /// Look up the owning plugin of a dialog provider id (raw or
    /// `plugin:id`-qualified).
    pub fn plugin_for_dialog(&self, dialog_id: &str) -> Option<String> {
        let registrations = self.registrations.read().expect("registrations poisoned");
        if let Some(dialog) = registrations.dialogs.get(dialog_id) {
            return Some(dialog.plugin_id.clone());
        }
        // Raw id: exactly one provider may own it locally.
        let mut found: Option<String> = None;
        for dialog in registrations.dialogs.values() {
            if dialog.id.ends_with(&format!(":{dialog_id}")) {
                if found.is_some() {
                    return None; // ambiguous — caller must qualify
                }
                found = Some(dialog.plugin_id.clone());
            }
        }
        found
    }

    /// Drain the pending plugin → TUI UI requests (between frames).
    pub fn drain_ui_requests(&self) -> Vec<UiRequest> {
        let mut queue = self.ui_requests.lock().expect("ui request queue poisoned");
        queue.drain(..).collect()
    }

    /// TUI-25: settle a deferred `ctx.ui.alert`/`ctx.ui.confirm` —
    /// send the stored guest request id its reply (the promise's
    /// resolution value; `null` for an unresolved close). Idempotent
    /// per settle-once semantics only through the TUI's queue; a
    /// second resolve for the same id just answers an already-settled
    /// guest request (the JS side ignores it).
    pub fn resolve_ui_request(&self, request_id: u64, result: Value) {
        let frame = protocol::response(request_id, result);
        let outbound = self.outbound.read().expect("outbound lock poisoned").clone();
        let _ = outbound.send(frame);
    }

    /// Push the TUI's live route to the guest (the `tui.route`
    /// notification — the flagship sidebar's render reads it at every
    /// materialize; `None` is the home route).
    pub fn notify_tui_route(&self, session: Option<&str>) {
        let params = match session {
            Some(session) => json!({ "sessionID": session }),
            None => json!({ "sessionID": null }),
        };
        self.send_notification("tui.route", params);
    }

    /// Push the live theme view (the flat `TuiThemeCurrent` sidebar
    /// subset the guest's render consumes).
    pub fn notify_tui_theme(&self, view: serde_json::Value) {
        self.send_notification("tui.theme", view);
    }

    /// PLUG-D07 — push the LATEST activated location into the guest
    /// (the `location.update` notification): every live `ctx`'s
    /// `directory`/`location` view follows, the one-guest approximation
    /// of v2's per-instance plugin hosts.
    pub fn push_location(&self, directory: &str, project_id: &str) {
        self.send_notification(
            "location.update",
            json!({ "directory": directory, "project": { "id": project_id } }),
        );
    }

    /// Push the sidebar's layout budget (the `tui.layout` notification
    /// — SA-F5: the guest's sidebar flattener lays `space-between` /
    /// flexGrow rows out against this width so the plugin's padded
    /// columns materialize like its own renderer's output).
    pub fn notify_tui_layout(&self, sidebar_content_width: usize) {
        self.send_notification(
            "tui.layout",
            json!({ "sidebarContentWidth": sidebar_content_width }),
        );
    }

    fn send_notification(&self, method: &str, params: Value) {
        let frame = protocol::notification(method, params);
        let outbound = self.outbound.read().expect("outbound lock poisoned");
        let _ = outbound.send(frame);
    }

    /// Re-materialize one plugin's sidebar slot NOW (host → guest
    /// `slot.materialize`, hard-timeout-bounded) and land the result
    /// as a native [`UiRequest::SidebarSection`]. Spawned off the
    /// reader task (deadlock rule 1); a failure logs and keeps the
    /// last section (contained like every plugin surface).
    pub fn materialize_sidebar_slot(self: &Arc<Self>, plugin_id: &str) {
        let Some(handle) = self.runtime_handle.clone() else { return };
        let shared = Arc::clone(self);
        let plugin_id = plugin_id.to_owned();
        handle.spawn(async move {
            let outcome = shared
                .request(
                    "slot.materialize",
                    json!({ "pluginId": plugin_id }),
                    Duration::from_secs(2),
                )
                .await;
            match outcome {
                Ok(result) => {
                    let (title, rows, clickable) = parse_sidebar_section(&result);
                    shared
                        .sidebar_slots
                        .lock()
                        .expect("sidebar slots poisoned")
                        .insert(plugin_id.clone());
                    shared
                        .ui_requests
                        .lock()
                        .expect("ui request queue poisoned")
                        .push_back(UiRequest::SidebarSection { plugin_id, title, rows, clickable });
                }
                Err(error) => {
                    eprintln!(
                        "opencode plugin-host: sidebar slot {plugin_id} materialize: {error}"
                    );
                }
            }
        });
    }

    /// The steady sidebar refresh (the flagship's own 1 s renderTimer
    /// analog): one bounded materialize per second while the slot
    /// stays registered — keeps agent rows/status fresh between the
    /// plugin's explicit rerender asks.
    fn start_sidebar_refresh_loop(self: &Arc<Self>, plugin_id: &str) {
        {
            let mut slots = self.sidebar_slots.lock().expect("sidebar slots poisoned");
            if slots.contains(plugin_id) {
                return;
            }
            slots.insert(plugin_id.to_owned());
        }
        let Some(handle) = self.runtime_handle.clone() else { return };
        let shared = Arc::clone(self);
        let plugin_id = plugin_id.to_owned();
        handle.spawn(async move {
            let mut misses = 0u32;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let still_registered = shared
                    .sidebar_slots
                    .lock()
                    .expect("sidebar slots poisoned")
                    .contains(&plugin_id);
                if !still_registered || shared.shutting_down.load(Ordering::Relaxed) {
                    return;
                }
                let outcome = shared
                    .request(
                        "slot.materialize",
                        json!({ "pluginId": plugin_id }),
                        Duration::from_secs(2),
                    )
                    .await;
                match outcome {
                    Ok(result) => {
                        misses = 0;
                        let (title, rows, clickable) = parse_sidebar_section(&result);
                        shared
                            .ui_requests
                            .lock()
                            .expect("ui request queue poisoned")
                            .push_back(UiRequest::SidebarSection {
                                plugin_id: plugin_id.clone(),
                                title,
                                rows,
                                clickable,
                            });
                    }
                    Err(error) => {
                        misses += 1;
                        if misses >= 3 {
                            eprintln!(
                                "opencode plugin-host: sidebar refresh for {plugin_id} stopped: {error}"
                            );
                            shared
                                .sidebar_slots
                                .lock()
                                .expect("sidebar slots poisoned")
                                .remove(&plugin_id);
                            return;
                        }
                    }
                }
            }
        });
    }
}

/// Parse one `slot.materialize` reply: `{title, rows:
/// [{text, click}]}` (the v2 TUI-plugin surface — the plugin's own
/// rows; `click` marks rows carrying an `onMouseUp` handler).
fn parse_sidebar_section(result: &Value) -> (String, Vec<String>, Vec<bool>) {
    let title = result.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut rows = Vec::new();
    let mut clickable = Vec::new();
    if let Some(entries) = result.get("rows").and_then(Value::as_array) {
        for entry in entries {
            // The object form ({text, click}); a bare string degrades
            // to a plain row.
            if let Some(text) = entry.as_str() {
                rows.push(text.to_owned());
                clickable.push(false);
            } else {
                let text = entry.get("text").and_then(Value::as_str).unwrap_or_default();
                let click = entry.get("click").and_then(Value::as_bool).unwrap_or(false);
                rows.push(text.to_owned());
                clickable.push(click);
            }
        }
    }
    (title, rows, clickable)
}

fn decode_digests(result: &Value) -> Option<HashMap<String, String>> {
    let digests = result.get("digests")?.as_object()?;
    Some(
        digests
            .iter()
            .filter_map(|(path, hash)| Some((path.clone(), hash.as_str()?.to_owned())))
            .collect(),
    )
}

fn spawn_process(
    config: &SupervisorConfig,
) -> Result<(Child, tokio::process::ChildStdin, tokio::process::ChildStdout), String> {
    let mut command = Command::new(&config.bun);
    command
        .arg(&config.host_js)
        .current_dir(&config.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // Belt for the no-Drop paths (incident 2026-10-22): if the host
    // process dies abruptly — SIGKILL of the supervisor's process, a
    // test binary exiting mid-flight — no Rust teardown can run. The
    // guest self-exits on IPC EOF (host.js `hostGone`), and on Linux
    // the kernel additionally SIGKILLs it the moment its parent
    // thread dies (PR_SET_PDEATHSIG). Empirically verified to fire
    // even when the parent is SIGKILLed.
    #[cfg(target_os = "linux")]
    parent_death::arm(&mut command);
    for (key, value) in &config.env {
        command.env(key, value);
    }
    let mut child =
        command.spawn().map_err(|error| format!("{}: {error}", config.bun.display()))?;
    let stdin = child.stdin.take().ok_or_else(|| "no stdin".to_owned())?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_owned())?;
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(log_stderr(stderr));
    }
    Ok((child, stdin, stdout))
}

/// The Linux parent-death belt (see [`spawn_process`]). Extracted to a
/// module so the raw syscall sits behind the same contained-unsafe
/// pattern as oc-server's `safekill`.
#[cfg(target_os = "linux")]
mod parent_death {
    #![allow(unsafe_code)] // contained raw syscall (prctl), reviewed per-site

    /// Arm `PR_SET_PDEATHSIG(SIGKILL)` on the command's forked child:
    /// the kernel kills the guest the moment its parent thread dies —
    /// graceful exit, abrupt exit, or SIGKILL alike. The `pre_exec`
    /// closure only performs async-signal-safe calls (prctl,
    /// getppid, _exit) and touches no locks or allocations.
    pub fn arm(command: &mut tokio::process::Command) {
        let parent_pid = libc::pid_t::try_from(std::process::id()).unwrap_or(-1);
        // SAFETY: runs between fork and exec on the child side; only
        // async-signal-safe syscalls, no shared-state access.
        unsafe {
            command.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
                // The classic fork/prctl race: if the parent died
                // between fork and prctl, the signal can never arrive —
                // exit now instead of living on as an orphan.
                if libc::getppid() != parent_pid {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
}

async fn log_stderr(stderr: tokio::process::ChildStderr) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("opencode plugin-host [guest-stderr] {line}");
    }
}

/// The writer task: sole owner of stdin. Everyone else sends frames
/// through the unbounded channel — nobody blocks holding the write
/// path (deadlock rule 2).
fn spawn_writer(
    mut inbound: mpsc::UnboundedReceiver<String>,
    mut stdin: tokio::process::ChildStdin,
) {
    tokio::spawn(async move {
        while let Some(frame) = inbound.recv().await {
            if stdin.write_all(frame.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                // Pipe broke (process died): drain silently and exit.
                while inbound.try_recv().is_ok() {}
                break;
            }
        }
        let _ = stdin.shutdown().await;
    });
}

/// The reader task (one per process generation): reads lines,
/// completes pending requests, answers guest requests INLINE (deadlock
/// rule 1), and fails all pending calls on EOF.
fn spawn_reader(shared: Arc<Shared>, stdout: tokio::process::ChildStdout, generation: u64) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            let Ok(Some(line)) = lines.next_line().await else { break };
            let Some(incoming) = protocol::parse_line(&line) else {
                if !line.trim().is_empty() {
                    eprintln!("opencode plugin-host: unparseable frame: {line:.200}");
                }
                continue;
            };
            match incoming {
                Incoming::Response { id, result } => {
                    if let Some(sender) =
                        shared.pending.lock().expect("pending map poisoned").remove(&id)
                    {
                        let _ = sender.send(result);
                    }
                    // Unknown ids: a late reply after timeout — dropped.
                }
                Incoming::Request { id, method, params } => {
                    if let Some(reply) = shared.answer_guest(id, &method, &params) {
                        let outbound =
                            shared.outbound.read().expect("outbound lock poisoned").clone();
                        let _ = outbound.send(reply);
                    }
                }
                Incoming::Notification { method, params } => shared.note(&method, &params),
            }
        }
        // Always sweep: a request raced onto a dying generation's
        // channel must fail fast, not wait out its timeout.
        shared.fail_pending("plugin host process exited");
        if !shared.shutting_down.load(Ordering::Relaxed)
            && generation == shared.generation.load(Ordering::Relaxed)
        {
            // A deliberate codemode-timeout kill is NOT a plugin crash:
            // consume the exemption instead of attributing strikes.
            if shared.codemode_kill_pending.swap(false, Ordering::Relaxed) {
                eprintln!("opencode plugin-host: deliberate codemode-timeout kill; crash attribution skipped");
            } else {
                shared.attribute_crash();
            }
        }
    });
}

/// The supervise loop: owns the child; on unexpected exit, restart
/// with backoff and reload every non-disabled plugin. Every exit path
/// confirms the child dead (kill by handle + reap) and flips
/// `child_reaped` — the deterministic teardown seam for
/// [`Supervisor::shutdown`].
async fn supervise_loop(
    shared: Arc<Shared>,
    mut child: Child,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    mut kill_rx: tokio::sync::watch::Receiver<u64>,
    child_reaped: Arc<AtomicBool>,
) {
    /// Kill the child THROUGH ITS HANDLE (never a naked-PID signal)
    /// and confirm the death. Idempotent: an already-exited child just
    /// reaps.
    async fn kill_and_reap(child: &mut Child) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let mut consecutive = 0u32;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    kill_and_reap(&mut child).await;
                    child_reaped.store(true, Ordering::Relaxed);
                    return;
                }
            }
            // A codemode deadline expired: kill the CURRENT child. The
            // deliberate kill is strike-exempt; if the child already
            // died on its own, cancel the exemption (the reader's EOF
            // sweep attributed that death correctly).
            _ = kill_rx.changed() => {
                let _ = kill_rx.borrow_and_update();
                if matches!(child.try_wait(), Ok(None)) {
                    kill_and_reap(&mut child).await;
                    // The child.wait() arm below observes the death and
                    // runs the normal restart path.
                } else {
                    shared.codemode_kill_pending.store(false, Ordering::Relaxed);
                }
            }
            status = child.wait() => {
                // This generation's child is confirmed dead.
                child_reaped.store(true, Ordering::Relaxed);
                if *shutdown.borrow_and_update() {
                    return;
                }
                let how = match status {
                    Ok(code) => format!("exit {code:?}"),
                    Err(error) => error.to_string(),
                };
                consecutive += 1;
                if consecutive > shared.config.max_restarts {
                    eprintln!(
                        "opencode plugin-host: {how}; giving up after {consecutive} restarts (plugin subsystem disabled)"
                    );
                    shared.fail_pending("plugin host gave up");
                    return;
                }
                let backoff = backoff(shared.config.restart_backoff_base, shared.config.restart_backoff_max, consecutive);
                eprintln!(
                    "opencode plugin-host: guest died ({how}); restart #{consecutive} in {backoff:?}"
                );
                tokio::time::sleep(backoff).await;
                // Shutdown may have been requested while we slept in
                // the restart arm (the select above was not armed):
                // never respawn a guest the host is tearing down.
                if shared.shutting_down.load(Ordering::Relaxed) {
                    eprintln!("opencode plugin-host: shutdown during restart backoff; not respawning");
                    return;
                }
                match spawn_process(&shared.config) {
                    Ok((new_child, stdin, stdout)) => {
                        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
                        shared.rotate_outbound(outbound_tx);
                        let generation = shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(pid) = new_child.id() {
                            shared.child_pid.store(i64::from(pid), Ordering::Relaxed);
                        }
                        spawn_writer(outbound_rx, stdin);
                        spawn_reader(Arc::clone(&shared), stdout, generation);
                        child = new_child;
                        // A live guest exists again; the reaped flag
                        // describes the CURRENT child.
                        child_reaped.store(false, Ordering::Relaxed);
                        if shared
                            .request("initialize", json!({}), shared.config.registration_timeout)
                            .await
                            .is_err()
                        {
                            continue; // died again immediately — loop re-enters wait()
                        }
                        // Reload every enabled entry.
                        for entry in shared.loadable_entries() {
                            load_entry(&shared, &entry).await;
                        }
                        let () = shared.scan_all().await;
                        mark_ready(&shared);
                        consecutive = 0;
                    }
                    Err(error) => {
                        eprintln!("opencode plugin-host: respawn failed: {error}; giving up");
                        shared.fail_pending("plugin host respawn failed");
                        return;
                    }
                }
            }
        }
    }
}

/// base · 2^(attempt-1), capped.
fn backoff(base: Duration, max: Duration, attempt: u32) -> Duration {
    let factor = 2u64.saturating_pow(attempt.saturating_sub(1).min(10));
    let millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX / 2).saturating_mul(factor);
    let cap = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(millis.min(cap))
}

/// The dispatch pump: the post-ingest tap consumer. Sequential — tap
/// order is plugin order. A dispatch that raced a crash/restart
/// (send onto the dying generation's channel) is retried briefly so
/// restarts stay transparent — the window (4 × 500 ms) covers the
/// backoff + respawn + reload even under load; a timeout drops THAT
/// event (logged) and never blocks the stream.
async fn dispatch_pump(shared: Arc<Shared>, mut tap_rx: mpsc::Receiver<TapEvent>) {
    while let Some(event) = tap_rx.recv().await {
        let payload = json!({
            "event": {
                "id": event.id,
                "type": event.typ,
                "data": event.data,
                "created": event.created,
            }
        });
        let mut outcome =
            shared.request("dispatch", payload.clone(), shared.config.dispatch_timeout).await;
        for _ in 0..4 {
            match outcome {
                Err(HostError::Exited(_)) => {
                    // The guest is between generations (crash +
                    // backoff + respawn): the retry window covers the
                    // transition without stalling the stream.
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    outcome = shared
                        .request("dispatch", payload.clone(), shared.config.dispatch_timeout)
                        .await;
                }
                _ => break,
            }
        }
        match outcome {
            Ok(_) => {
                shared.dispatched_events.fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => {
                shared.dropped_events.fetch_add(1, Ordering::Relaxed);
                eprintln!("opencode plugin-host: dispatch of {} dropped ({error})", event.typ);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let base = Duration::from_millis(100);
        let max = Duration::from_secs(4);
        assert_eq!(backoff(base, max, 1), Duration::from_millis(100));
        assert_eq!(backoff(base, max, 2), Duration::from_millis(200));
        assert_eq!(backoff(base, max, 3), Duration::from_millis(400));
        assert_eq!(backoff(base, max, 100), Duration::from_secs(4));
    }

    #[test]
    fn storage_port_defaults_to_none() {
        // The declining default: no backend bound → guest ctx.storage
        // calls reject with a clear error (the DB-backed port is the
        // embedder's binding, PLUG-D18).
        let config = SupervisorConfig::default();
        assert!(config.storage_port.is_none(), "storage port must be opt-in");
    }
}
