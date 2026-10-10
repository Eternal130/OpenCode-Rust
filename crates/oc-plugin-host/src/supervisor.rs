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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::context::{
    Bridge, HostState, SessionPort, ToolPort, CODEMODE_METHODS, GENERATE_METHODS, SESSION_METHODS,
};
use crate::protocol::{self, Incoming};
use crate::registrations::{PluginState, PluginStatus, Registrations};
use crate::tap::TapEvent;
use crate::ui::UiRequest;

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
            runtime_handle: None,
        }
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
            // -- plugin-ID-scoped durable KV (file per plugin under the
            //    sandbox; small synchronous IO on the reader task).
            "storage.get" | "storage.set" | "storage.remove" | "storage.scan" => {
                Some(answer_storage(self, id, method, params))
            }
            "permission.reply" => {
                // Registration-time acknowledgment only; the permission
                // prompt flow is the permission lane's seam (wave 4).
                Some(protocol::response(id, Value::Null))
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
            "ui.dialog.register" | "ui.group.rule" | "ui.home.quickaction" => {
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
            "log" => log_guest(params),
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

/// The `storage.*` guest-request arms (one helper — `answer_guest`'s
/// match stays under the complexity ceiling). Synchronous file IO on
/// the reader task; the KV is small by construction (value/key caps).
fn answer_storage(shared: &Shared, id: u64, method: &str, params: &Value) -> String {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or_default().to_owned();
    let store = StorageFile::for_plugin(&shared.config.cwd, &plugin_id);
    match method {
        "storage.get" => {
            let Some((_, key)) = storage_key(params) else {
                return protocol::error_response(id, "storage.get requires a key");
            };
            let value = store.get(&key);
            protocol::response(id, json!({ "value": value }))
        }
        "storage.set" => {
            let Some((_, key)) = storage_key(params) else {
                return protocol::error_response(id, "storage.set requires a key");
            };
            let Some(value) = params.get("value") else {
                return protocol::error_response(id, "storage.set requires a value");
            };
            match store.set(&key, value) {
                Ok(()) => protocol::response(id, Value::Null),
                Err(error) => protocol::error_response(id, &error),
            }
        }
        "storage.remove" => {
            let Some((_, key)) = storage_key(params) else {
                return protocol::error_response(id, "storage.remove requires a key");
            };
            store.remove(&key);
            protocol::response(id, Value::Null)
        }
        "storage.scan" => {
            let prefix = params.get("prefix").and_then(Value::as_str).unwrap_or_default();
            let after = params.get("after").and_then(Value::as_str);
            let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 500);
            let result = store.scan(prefix, after, limit as usize);
            protocol::response(id, result)
        }
        other => protocol::error_response(id, &format!("unknown storage method {other:?}")),
    }
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
            // drops the plugin's section).
            UiRequest::SidebarSection { plugin_id, title, rows }
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

/// The registration-time `ui.*` arms: dialog-provider coverage, group
/// rules, home quick actions — all native entries in
/// [`Registrations`] (the snapshot is the TUI's only synchronous
/// surface; nothing here crosses IPC again).
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
        other => protocol::error_response(id, &format!("unknown ui registration {other:?}")),
    }
}

/// A running supervisor handle (cloneable, kept alive by the facade).
pub struct Supervisor {
    pub shared: Arc<Shared>,
    /// Set true → the supervise loop kills the child and exits.
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    pub tap_tx: mpsc::Sender<TapEvent>,
}

impl Supervisor {
    /// Spawn the host, run the initial load pass, and start every
    /// background task. Must be called inside a tokio runtime.
    pub async fn start(config: SupervisorConfig) -> Result<Arc<Self>, String> {
        let (tap_tx, tap_rx) = mpsc::channel::<TapEvent>(1024);
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<String>();
        let shared = Arc::new(Shared {
            config: config.clone(),
            outbound: RwLock::new(outbound_tx),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
            generation: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
            registrations: RwLock::new(Registrations::default()),
            crash_counts: Mutex::new(HashMap::new()),
            busy_plugins: Mutex::new(HashSet::new()),
            digests: Mutex::new(HashMap::new()),
            ready: Arc::new((Mutex::new(false), Condvar::new())),
            dispatched_events: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            child_pid: AtomicI64::new(-1),
            ui_requests: Mutex::new(VecDeque::new()),
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
        let supervisor = Arc::new(Self { shared: Arc::clone(&shared), shutdown_tx, tap_tx });

        // The supervision loop owns the child from here.
        tokio::spawn(supervise_loop(Arc::clone(&shared), child, shutdown_rx));
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

    /// Signal shutdown; the supervise loop kills the child.
    pub fn shutdown(&self) {
        self.shared.shutting_down.store(true, Ordering::Relaxed);
        let _ = self.shutdown_tx.send(true);
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
    let mut registrations = shared.registrations.write().expect("registrations poisoned");
    match outcome {
        Ok(result) => {
            let plugin_id =
                result.get("pluginId").and_then(Value::as_str).unwrap_or("?").to_owned();
            if let Some(status) = registrations.plugins.get_mut(&key) {
                status.plugin_id = plugin_id;
                status.state = PluginState::Active;
            }
        }
        Err(error) => {
            // A load that merely failed/timed out is contained (no
            // crash strike — that is process-death attribution).
            if let Some(status) = registrations.plugins.get_mut(&key) {
                status.state = PluginState::Failed(error.to_string());
            }
            eprintln!("opencode plugin-host: load {key} failed: {error}");
        }
    }
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
            shared.attribute_crash();
        }
    });
}

/// The supervise loop: owns the child; on unexpected exit, restart
/// with backoff and reload every non-disabled plugin.
async fn supervise_loop(
    shared: Arc<Shared>,
    mut child: Child,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut consecutive = 0u32;
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    let _ = child.kill().await;
                    return;
                }
            }
            status = child.wait() => {
                if *shutdown.borrow() {
                    let _ = child.kill().await;
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

// =============================================================================
// Plugin storage KV (file per plugin id under the sandbox)
// =============================================================================

/// Cap: a value larger than this is rejected (the KV is for flags,
/// caches and small documents — not blobs).
const STORAGE_VALUE_LIMIT: usize = 256 * 1024;
/// Cap: the store file is an in-memory JSON map; refuse unbounded key
/// growth instead of wedging the reader task.
const STORAGE_MAX_KEYS: usize = 10_000;
/// One file per plugin: keys sorted (`BTreeMap`), scanned in
/// key order for deterministic paging.
type StorageMap = BTreeMap<String, Value>;

fn storage_key(params: &Value) -> Option<(String, String)> {
    let plugin_id = params.get("pluginId").and_then(Value::as_str).unwrap_or_default().to_owned();
    let key = params.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
    Some((plugin_id, key)).filter(|(_, key)| !key.is_empty() && key.len() <= 256)
}

/// One plugin's durable KV: `<sandbox>/plugin-storage/<sanitized>.json`.
struct StorageFile {
    path: PathBuf,
}

impl StorageFile {
    fn for_plugin(cwd: &std::path::Path, plugin_id: &str) -> Self {
        let safe: String = plugin_id
            .chars()
            .map(
                |c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' },
            )
            .collect();
        let safe = safe.chars().take(80).collect::<String>();
        let dir = cwd.join("plugin-storage");
        let _ = std::fs::create_dir_all(&dir);
        Self { path: dir.join(format!("{safe}.json")) }
    }

    fn load(&self) -> StorageMap {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, map: &StorageMap) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = serde_json::to_string(map).map_err(|error| error.to_string())?;
        std::fs::write(&self.path, text).map_err(|error| format!("write storage: {error}"))
    }

    fn get(&self, key: &str) -> Option<Value> {
        self.load().get(key).cloned()
    }

    fn set(&self, key: &str, value: &Value) -> Result<(), String> {
        let encoded = serde_json::to_string(value).map_err(|error| error.to_string())?;
        if encoded.len() > STORAGE_VALUE_LIMIT {
            return Err(format!(
                "storage value for {key:?} exceeds {STORAGE_VALUE_LIMIT} bytes ({} encoded)",
                encoded.len()
            ));
        }
        let mut map = self.load();
        if map.len() >= STORAGE_MAX_KEYS {
            return Err(format!("storage for this plugin holds {} keys; refusing more", map.len()));
        }
        map.insert(key.to_owned(), value.clone());
        self.save(&map)
    }

    fn remove(&self, key: &str) {
        let mut map = self.load();
        map.remove(key);
        let _ = self.save(&map);
    }

    /// v2 `storage.scan`: `{prefix, after?, limit?}` → `{entries,
    /// next?}` in key order.
    fn scan(&self, prefix: &str, after: Option<&str>, limit: usize) -> Value {
        let map = self.load();
        let mut entries = Vec::new();
        let mut next = Value::Null;
        let mut count = 0usize;
        for (key, value) in map.range(prefix.to_owned()..) {
            if !key.starts_with(prefix) {
                break;
            }
            if let Some(after) = after {
                if key.as_str() <= after {
                    continue;
                }
            }
            if count >= limit {
                next = Value::String(key.clone());
                break;
            }
            entries.push(json!({ "key": key, "value": value }));
            count += 1;
        }
        let mut result = json!({ "entries": entries });
        if !next.is_null() {
            result["next"] = next;
        }
        result
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
    fn storage_kv_round_trips_and_scans_in_key_order() {
        let dir = std::env::temp_dir().join(format!("ocph-storage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = StorageFile::for_plugin(&dir, "p@1/odd id");
        assert!(store.path.ends_with("p_1_odd_id.json"), "{}", store.path.display());
        store.set("b/2", &json!(2)).expect("set b/2");
        store.set("a/1", &json!({ "x": 1 })).expect("set a/1");
        store.set("b/1", &json!("v")).expect("set b/1");
        assert_eq!(store.get("a/1"), Some(json!({ "x": 1 })));
        let page = store.scan("b/", None, 10);
        assert_eq!(
            page["entries"],
            json!([{ "key": "b/1", "value": "v" }, { "key": "b/2", "value": 2 }])
        );
        let paged = store.scan("b/", Some("b/1"), 10);
        assert_eq!(paged["entries"], json!([{ "key": "b/2", "value": 2 }]));
        store.remove("b/1");
        assert_eq!(store.get("b/1"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_rejects_oversized_values() {
        let dir = std::env::temp_dir().join(format!("ocph-storage-big-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = StorageFile::for_plugin(&dir, "p");
        let big = "x".repeat(STORAGE_VALUE_LIMIT + 1);
        assert!(store.set("big", &json!(big)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
