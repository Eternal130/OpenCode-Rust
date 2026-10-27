//! # oc-plugin-host — the bun-subprocess JS plugin host (wave 1 spine)
//!
//! Productionization of the `/tmp/opencode/spike-bun` prototype
//! (spike-plugin-runtime.md option B): plugins are JS/TS ESM modules
//! run by a supervised `bun host.js` child process, speaking NDJSON
//! JSON-RPC over stdio in **both directions**.
//!
//! * [`PluginHost`] — the facade: bun resolution (absent bun ⇒ the
//!   subsystem disables itself with one clear log, every op no-ops),
//!   spawn/supervision, registration snapshot, command execution,
//!   hot-reload check, shutdown.
//! * [`BroadcastTap`] — the post-ingest event tap the TUI store
//!   publishes through after its reducers ran.
//! * [`RegistrationSnapshot`] — the ONLY synchronous surface:
//!   commands (with owned keymap entries), theme JSON documents,
//!   agents/skills/tools/MCP servers (wave 2 transform-domain
//!   materialization) and runtime-hook coverage resolve into native
//!   Rust structures at registration time; nothing crosses IPC
//!   per-frame.
//! * [`context`] (wave 2) — the minimal flagship-driven Context
//!   backing: [`context::HostState`] (the host-owned agent/MCP tables
//!   seeded INTO the guest at load), [`context::SessionPort`] (the
//!   `ctx.session.*` engine seam — answered asynchronously off the IO
//!   reader so ports may dispatch hooks back into the guest through
//!   [`context::Bridge::dispatch_hook`]), and
//!   [`context::install`] (the pinned, frozen npm/bun install path
//!   behind `opencode plugin add`).
//!
//! ## 1. Deadlock protocol (binding)
//!
//! The spike hit a real 30 s deadlock: the supervisor held the stdin
//! write lock while blocking on a `load` reply that required answering
//! the guest's nested `config.get` request first. The rules below are
//! the structural fix and are enforced here — the stress test
//! (`tests/storm.rs`) hammers them.
//!
//! 1. **The supervisor IO reader never blocks on anything but the
//!    pipe.** Guest→host requests (`command.add`, `theme.register`,
//!    `event.subscribe`, `ping`) are answered *on the reader task*:
//!    the handler is synchronous (a registration insert, a log line)
//!    and the reply is *enqueued* through the writer channel. No lock
//!    on the write path is ever held across a wait.
//! 2. **Nobody holds the write channel while awaiting a reply.**
//!    [`supervisor::Shared::request`] moves the frame into the
//!    writer-owned channel, *then* awaits a oneshot keyed by the
//!    request id. Concurrent callers never serialize behind each
//!    other's pending replies.
//! 3. **Every bridge call is timeout-bounded** — 2 s registration /
//!    5 s dispatch by default ([`PluginHostConfig`]). A timed-out call
//!    is removed from the pending map; a late reply is discarded by
//!    id. A hung plugin costs exactly its own dispatch (logged,
//!    counted via [`PluginHost::dispatch_counters`]), never the host.
//! 4. **The guest mirrors the rules.** Its read loop never awaits a
//!    handler before reading the next line: handlers run detached,
//!    guest→host calls are Promises settled by supervisor replies
//!    that arrive on the same stdin stream the loop keeps draining.
//!
//! ## 2. Supervision
//!
//! The guest is monitored: an unexpected exit restarts it with
//! exponential backoff (100 ms doubling, 4 s cap). Crashes are
//! *attributed* — the plugin whose load was in flight, or which was
//! mid-dispatch (guest `plugin.busy` markers), gets a strike; three
//! strikes disable that plugin while the rest reload. Eight
//! consecutive unattributable restarts give up entirely (subsystem
//! disabled, logged). One deliberate kill is EXEMPT: a Code Mode
//! evaluation that outruns its limits-aligned deadline wedges the
//! guest (a sync loop is not interruptible in-process), so the
//! supervisor kills and restarts it WITHOUT strike attribution.
//!
//! ## 3. Hot reload
//!
//! Digest-scan parity with v2 (`source.bun.ts`): sha256 of the entry
//! plus its relative import graph (`Bun.Transpiler.scanImports`), CJS
//! `require.cache` eviction under the plugin dir, re-import with a
//! generation cache-bust (`entry?t=N`). [`PluginHost::reload_now`]
//! compares digests and reloads changed plugins, refreshing the
//! registration snapshot.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub mod bun;
pub mod context;
pub mod host_js;
pub mod protocol;
pub mod registrations;
pub mod resolve;
pub mod supervisor;
pub mod tap;
pub mod ui;

pub use context::{
    install::{default_root as install_root, install, InstallOptions, Installed},
    Bridge, HostState, NoSessionEngine, NoToolEngine, PermissionPort, SessionPort, ToolPort,
    SESSION_METHODS,
};
pub use registrations::{
    PluginAgent, PluginCommand, PluginDialog, PluginGroupRule, PluginHomeAction, PluginHook,
    PluginKeymapCommand, PluginMcp, PluginRoute, PluginRpc, PluginSkill, PluginSlotClaim,
    PluginState, PluginStatus, PluginTheme, PluginTool, RegistrationSnapshot,
};
pub use resolve::{
    discover_directory_ops, parse_ops, resolve, PluginOp, PluginSourceKind, Resolution,
    ResolvedPlugin, PLUGIN_SOURCE_DIRECTORIES,
};
pub use supervisor::{HostError, StoragePort, StorageScanQuery, SupervisorConfig};
pub use tap::{BroadcastTap, TapEvent};
pub use ui::{PluginDialogOption, PluginDialogSpec, PluginSectionSpec, UiRequest};

/// The embedded guest script (materialized into the sandbox at start).
const HOST_JS: &str = include_str!("host/host.js");

/// Sandbox root for host processes (gate constraint: `/tmp/opencode`,
/// PID-unique top-level names).
const SANDBOX_ROOT: &str = "/tmp/opencode";

/// Caller-facing configuration (timeouts tunable for tests).
#[derive(Clone)]
pub struct PluginHostConfig {
    /// Extra env for the guest (test fixtures).
    pub env: Vec<(String, String)>,
    pub registration_timeout: Duration,
    pub dispatch_timeout: Duration,
    pub restart_backoff_base: Duration,
    pub restart_backoff_max: Duration,
    pub max_plugin_crashes: u32,
    /// Consecutive restart attempts before the host gives up.
    pub max_restarts: u32,
    /// Host-owned transform-domain base state (the agent/MCP tables
    /// seeded into the guest; `ctx.directory`/`ctx.location`).
    pub host_state: HostState,
    /// `ctx.session.*` routing target (the session-engine seam).
    pub session_port: Option<Arc<dyn SessionPort>>,
    /// `tools.<name>(input)` routing from Code Mode programs (the
    /// oc-core registry seam; wave 4).
    pub tool_port: Option<Arc<dyn ToolPort>>,
    /// `ctx.storage.*` routing target (PLUG-D18): the durable KV
    /// backend. `None` declines every call.
    pub storage_port: Option<Arc<dyn supervisor::StoragePort>>,
    /// `ctx.permission.*` routing target (PLUG-D11): the engine's
    /// permission machinery. `None` declines every call.
    pub permission_port: Option<Arc<dyn PermissionPort>>,
    /// Config-sourced plugin options keyed by entry path (`ctx.options`,
    /// v2 plugin `{package, options}` entries).
    pub entry_options: BTreeMap<String, Value>,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            env: Vec::new(),
            registration_timeout: Duration::from_secs(2),
            dispatch_timeout: Duration::from_secs(5),
            restart_backoff_base: Duration::from_millis(100),
            restart_backoff_max: Duration::from_secs(4),
            max_plugin_crashes: 3,
            max_restarts: 8,
            host_state: HostState::default(),
            session_port: None,
            tool_port: None,
            storage_port: None,
            permission_port: None,
            entry_options: BTreeMap::new(),
        }
    }
}

/// One catalog entry handed to a Code Mode program (the wire shape of
/// oc-core's `CatalogEntry` at the guest bridge).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeModeToolSpec {
    /// Qualified dotted path (`namespace.name`).
    pub path: String,
    pub description: String,
    pub signature: String,
    /// The ref-shaped `searchText` (v2 `toSearchEntry`): path +
    /// description + input property names/descriptions, lowercased —
    /// the match surface of the guest's `search` builtin. Defaults
    /// empty (older hosts) and the guest falls back to its local
    /// approximation.
    #[serde(default)]
    pub search_text: String,
}

/// One observed tool call inside a Code Mode program (v2 `ExecuteCall`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CodeModeCallRecord {
    pub tool: String,
    /// `running` | `completed` | `error`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
}

/// A finished program (the `codemode.evaluate` result).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CodeModeRun {
    /// The completion value (`None` = `undefined`).
    pub value: Option<Value>,
    pub error: Option<String>,
    pub tool_calls: Vec<CodeModeCallRecord>,
    /// Set by the guest when an output budget truncated the value (v2
    /// `Result.truncated`) — the leaf skips re-bounding a pre-bounded
    /// value.
    pub truncated: bool,
}

/// Execution budgets for one `codemode.evaluate` (v2 `ExecutionLimits`,
/// codemode.ts:22-41 — ALL optional; absent = unlimited, no defaults).
/// The wire form is camelCase with absent fields omitted, so the
/// unlimited default serializes to `{}`. The engine-side twin (with
/// `resolve()` validation) is oc-core's `tool::codemode::ExecutionLimits`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeModeLimits {
    /// Wall-clock milliseconds before interruption (guest-enforced
    /// between event-loop turns; supervisor kill at the deadline for a
    /// wedged guest).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Maximum number of tool calls admitted by the runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    /// Maximum UTF-8 bytes retained from the result and logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<u64>,
}

/// The guest's own error message passes through VERBATIM (a plugin's
/// thrown error is the model-visible text); transport failures keep
/// their display forms.
fn guest_error_text(error: HostError) -> String {
    match error {
        HostError::Guest(why) => why,
        other => other.to_string(),
    }
}

/// The plugin host facade. Constructed via [`PluginHost::detect`]
/// (bun-resolving; a missing bun yields a DISABLED host whose every
/// operation is a no-op) and [`PluginHost::start_detached`].
///
/// All methods are synchronous and safe from the TUI thread: the
/// runtime lives on a dedicated OS thread; bridge calls park the
/// caller for at most the configured hard timeout.
pub struct PluginHost {
    facade: Option<Arc<Facade>>,
    /// The resolved bun binary (diagnostics; `None` = disabled).
    bun: Option<PathBuf>,
}

struct Facade {
    shared: Arc<supervisor::Shared>,
    runtime: Option<tokio::runtime::Handle>,
    tap: BroadcastTap,
    stop: Option<tokio::sync::watch::Sender<bool>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    start_error: Mutex<Option<String>>,
}

impl PluginHost {
    /// Resolve bun and prepare a host. No process spawns yet;
    /// absent bun ⇒ disabled host (one clear log line).
    #[must_use]
    pub fn detect() -> Self {
        if let Some(path) = bun::resolve_bun() {
            Self { facade: None, bun: Some(path) }
        } else {
            eprintln!(
                "opencode plugin-host: bun not found (looked in ~/.cache/opencode-rust/bin, /usr/local/bin/bun, PATH, ~/.bun/bin/bun); plugin subsystem disabled"
            );
            Self { facade: None, bun: None }
        }
    }

    /// Whether the subsystem is (or can be) live.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.bun.is_some()
    }

    /// Spawn the host on a dedicated thread and run the initial
    /// registration pass over `entries`. On spawn failure the host
    /// degrades to disabled (logged; [`PluginHost::wait_ready`]
    /// returns `false`).
    #[must_use]
    pub fn start_detached(self, entries: &[PathBuf]) -> Self {
        self.start_detached_with(PluginHostConfig::default(), entries)
    }

    /// [`PluginHost::start_detached`] with explicit timeouts/env.
    ///
    /// When bun is absent but plugin `entries` were requested, the
    /// managed pinned download runs first (MIGRATION §2.3: pinned
    /// bun 1.3.10 + sha256 into `~/.cache/opencode/bin/`); any
    /// failure degrades to disabled with one clear message.
    #[must_use]
    pub fn start_detached_with(
        mut self,
        mut config: PluginHostConfig,
        entries: &[PathBuf],
    ) -> Self {
        let bun = match self.bun.clone() {
            Some(bun) => bun,
            None if entries.is_empty() => {
                eprintln!(
                    "opencode plugin-host: start requested but bun is absent; staying disabled"
                );
                return self;
            }
            None => match self.bootstrap_bun() {
                Some(bun) => bun,
                None => return self,
            },
        };
        let sandbox = sandbox_dir();
        if let Err(error) = host_js::materialize(&sandbox, HOST_JS) {
            eprintln!(
                "opencode plugin-host: cannot materialize host.js into {}: {error}; plugin subsystem disabled",
                sandbox.display()
            );
            return self;
        }
        let supervisor_config = supervisor_config(&mut config, bun, &sandbox, entries);
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let (parts_tx, parts_rx) = std::sync::mpsc::channel::<FacadeParts>();
        let thread = std::thread::Builder::new()
            .name("oc-plugin-host".into())
            .spawn(move || {
                // A multi-thread runtime: bridge calls arrive from the
                // caller's thread via `Handle::block_on` while this
                // thread keeps every spawned task (reader/writer/pump/
                // supervise) driving — a current-thread runtime parked
                // on a stop signal would freeze them all.
                let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                else {
                    return; // parts_tx dropped → caller records the failure
                };
                let supervisor = runtime.block_on(supervisor::Supervisor::start(supervisor_config));
                match supervisor {
                    Ok(supervisor) => {
                        let _ = parts_tx.send(FacadeParts {
                            shared: Some(Arc::clone(&supervisor.shared)),
                            tap: Some(supervisor.tap_tx.clone()),
                            handle: Some(runtime.handle().clone()),
                            stop: Some(stop_tx),
                            error: None,
                        });
                        // Drive the runtime until shutdown: leaving the
                        // loop drops it (and the child, via
                        // kill_on_drop).
                        runtime.block_on(async move {
                            while stop_rx.changed().await.is_ok() {
                                if *stop_rx.borrow() {
                                    break;
                                }
                            }
                        });
                        supervisor.shutdown();
                    }
                    Err(error) => {
                        let _ = parts_tx.send(FacadeParts {
                            shared: None,
                            tap: None,
                            handle: Some(runtime.handle().clone()),
                            stop: Some(stop_tx),
                            error: Some(error),
                        });
                    }
                }
            })
            .ok();
        let parts = parts_rx.recv_timeout(Duration::from_secs(30)).ok();
        if let Some(parts) = parts {
            let shared = parts.shared.unwrap_or_else(|| {
                let (outbound, receiver) = tokio::sync::mpsc::unbounded_channel();
                drop(receiver); // requests fail fast with Exited
                Arc::new(supervisor::Shared::dead(outbound))
            });
            let tap = parts.tap.map_or_else(BroadcastTap::disabled, BroadcastTap::live);
            self.facade = Some(Arc::new(Facade {
                shared,
                runtime: parts.handle,
                tap,
                stop: parts.stop,
                thread: Mutex::new(thread),
                start_error: Mutex::new(parts.error),
            }));
        } else {
            // The host thread died or never started (runtime build
            // failure): dead facade, `wait_ready` stays false.
            let (outbound, receiver) = tokio::sync::mpsc::unbounded_channel();
            drop(receiver);
            self.facade = Some(Arc::new(Facade {
                shared: Arc::new(supervisor::Shared::dead(outbound)),
                runtime: None,
                tap: BroadcastTap::disabled(),
                stop: None,
                thread: Mutex::new(thread),
                start_error: Mutex::new(Some("plugin host thread failed to start".into())),
            }));
        }
        self
    }

    /// The managed-bun bootstrap ([`PluginHost::start_detached_with`]'s
    /// absent-bun arm): pinned download per MIGRATION §2.3, graceful
    /// disable on any failure.
    fn bootstrap_bun(&mut self) -> Option<PathBuf> {
        match oc_util::bunman::ensure() {
            Ok(bun) => {
                self.bun = Some(bun.clone());
                eprintln!(
                    "opencode plugin-host: plugins requested without a system bun; managed bun bootstrapped"
                );
                Some(bun)
            }
            Err(error) => {
                eprintln!(
                    "opencode plugin-host: bun bootstrap failed ({error}); plugin subsystem disabled. Install bun or set {}=1 to silence this, then retry.",
                    oc_util::bunman::DISABLE_DOWNLOAD_ENV
                );
                None
            }
        }
    }

    /// Block until the initial registration pass finished (spawn +
    /// initialize + every entry loaded or failed). `false` = the host
    /// never came up (or is disabled).
    #[must_use]
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let Some(facade) = &self.facade else { return false };
        if facade.start_error.lock().map_or(true, |error| error.is_some()) {
            return false;
        }
        let deadline = Instant::now() + timeout;
        let (lock, condvar) = &*facade.shared.ready;
        let mut ready = lock.lock().expect("ready lock poisoned");
        while !*ready {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, wait) =
                condvar.wait_timeout(ready, deadline - now).expect("ready lock poisoned");
            ready = guard;
            if wait.timed_out() && !*ready {
                return false;
            }
        }
        true
    }

    /// The registration snapshot (commands incl. owned keymap entries,
    /// theme documents, plugin statuses). Empty on the disabled host.
    #[must_use]
    pub fn registrations(&self) -> RegistrationSnapshot {
        let Some(facade) = &self.facade else { return RegistrationSnapshot::default() };
        facade.shared.registrations.read().expect("registrations poisoned").snapshot()
    }

    /// The post-ingest broadcast tap (a no-op tap on the disabled
    /// host).
    #[must_use]
    pub fn tap(&self) -> BroadcastTap {
        let Some(facade) = &self.facade else { return BroadcastTap::disabled() };
        facade.tap.clone()
    }

    /// Execute one plugin command by name (the palette dispatch path).
    /// Blocks at most `timeout`; a hung plugin times out cleanly.
    pub fn execute_command(
        &self,
        name: &str,
        args: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.execute_command_in_session(name, args, "", timeout)
    }

    /// [`PluginHost::execute_command`] with the invoking session id
    /// (v2 `CommandInvocation.sessionID` — marker-style commands
    /// submit their prompt into that session).
    pub fn execute_command_in_session(
        &self,
        name: &str,
        args: Option<&str>,
        session_id: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(plugin_id) = facade.shared.plugin_for_command(name) else {
            return Err(format!("{name}: no such plugin command"));
        };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let args = match args {
            Some(text) => json!(text),
            None => Value::Null,
        };
        runtime
            .block_on(facade.shared.request(
                "command.execute",
                json!({
                    "pluginId": plugin_id,
                    "name": name,
                    "args": args,
                    "sessionID": session_id,
                }),
                timeout,
            ))
            .map(|result| result.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }

    /// [`PluginHost::execute_command_in_session`] without parking a
    /// runtime worker — the async-server handler path (called from
    /// inside the tokio runtime, where `block_on` is illegal).
    pub async fn execute_command_in_session_async(
        &self,
        name: &str,
        args: Option<&str>,
        session_id: &str,
        timeout: Duration,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(plugin_id) = facade.shared.plugin_for_command(name) else {
            return Err(format!("{name}: no such plugin command"));
        };
        let args = match args {
            Some(text) => json!(text),
            None => Value::Null,
        };
        facade
            .shared
            .request(
                "command.execute",
                json!({
                    "pluginId": plugin_id,
                    "name": name,
                    "args": args,
                    "sessionID": session_id,
                }),
                timeout,
            )
            .await
            .map(|result| result.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }

    /// Digest-scan + reload changed plugins. Returns the reloaded
    /// count. Errors degrade to `Err` — the host keeps running.
    pub fn reload_now(&self) -> Result<usize, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        runtime.block_on(facade.shared.reload_changed()).map_err(|error| error.to_string())
    }

    /// G-05: load one new plugin entry into the live guest (dynamic
    /// add — a watched plugin source gained a file). The load is
    /// contained: a failing entry is logged and marked failed, and
    /// existing generations are untouched. Errors degrade to `Err` —
    /// the host keeps running.
    pub fn add_entry(&self, entry: &std::path::Path) -> Result<(), String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        runtime.block_on(facade.shared.add_entry(entry));
        Ok(())
    }

    /// G-05: tear down one entry's generation (dynamic remove — a
    /// watched plugin source lost a file): registrations, slot claims
    /// and digest bookkeeping drop, and the guest unloads the plugin
    /// id. Returns whether the entry was loaded (idempotent).
    /// Errors degrade to `Err` — the host keeps running.
    pub fn remove_entry(&self, entry: &std::path::Path) -> Result<bool, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        Ok(runtime.block_on(facade.shared.remove_entry(entry)))
    }

    /// Execute one plugin-registered tool by name (the `tool.transform`
    /// executor dispatch — wave 4's wave-2 TODO). Blocks at most
    /// `timeout`.
    pub fn execute_tool(
        &self,
        name: &str,
        input: &Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        runtime
            .block_on(facade.shared.request(
                "tool.execute",
                json!({ "name": name, "input": input }),
                timeout,
            ))
            .map(|result| result.get("result").cloned().unwrap_or(Value::Null))
            .map_err(guest_error_text)
    }

    /// [`PluginHost::execute_tool`] without parking a runtime worker —
    /// the port/executor path (called from tasks that may live on the
    /// supervisor's own runtime).
    pub async fn execute_tool_async(
        &self,
        name: &str,
        input: &Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        facade
            .shared
            .request("tool.execute", json!({ "name": name, "input": input }), timeout)
            .await
            .map(|result| result.get("result").cloned().unwrap_or(Value::Null))
            .map_err(guest_error_text)
    }

    /// Run one Code Mode program in the guest runtime over `catalog`
    /// (`tools.<path>` calls come back through the tool port). The
    /// `timeout` is the HARD bound on the whole execution.
    pub fn codemode_evaluate(
        &self,
        code: &str,
        catalog: &[CodeModeToolSpec],
        timeout: Duration,
    ) -> Result<CodeModeRun, String> {
        self.codemode_evaluate_token(code, catalog, 0, timeout)
    }

    /// [`PluginHost::codemode_evaluate`] with an evaluation token: the
    /// guest echoes `token` on every nested `codemode.tool` request, so
    /// the tool port can route the call onto THIS evaluation's executor
    /// closure (the production engine adapter's per-request context).
    pub fn codemode_evaluate_token(
        &self,
        code: &str,
        catalog: &[CodeModeToolSpec],
        token: u64,
        timeout: Duration,
    ) -> Result<CodeModeRun, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        runtime.block_on(self.codemode_evaluate_token_async(code, catalog, token, timeout))
    }

    /// [`PluginHost::codemode_evaluate`] without parking a worker.
    pub async fn codemode_evaluate_async(
        &self,
        code: &str,
        catalog: &[CodeModeToolSpec],
        timeout: Duration,
    ) -> Result<CodeModeRun, String> {
        self.codemode_evaluate_token_async(code, catalog, 0, timeout).await
    }

    /// [`PluginHost::codemode_evaluate_token`] without parking a worker.
    pub async fn codemode_evaluate_token_async(
        &self,
        code: &str,
        catalog: &[CodeModeToolSpec],
        token: u64,
        timeout: Duration,
    ) -> Result<CodeModeRun, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let params = json!({
            "code": code,
            "tools": catalog,
            "token": token,
        });
        let result = facade
            .shared
            .request("codemode.evaluate", params, timeout)
            .await
            .map_err(|error| error.to_string())?;
        Ok(Self::parse_run(&result))
    }

    /// [`PluginHost::codemode_evaluate_token_async`] under v2
    /// `ExecutionLimits` ([`CodeModeLimits`], camelCase on the wire;
    /// absent fields = unlimited): the guest validates and enforces
    /// them (soft `timeoutMs` between event-loop turns, the tool-call
    /// admission counter, the value output bound, the depth guard).
    ///
    /// The transport deadline is the limits-aligned bound — `timeoutMs`
    /// plus a grace margin so the guest's soft timeout answers first,
    /// capped by the hard `timeout`. A deadline miss means the guest is
    /// wedged (a sync loop is not interruptible in-process), so the
    /// supervisor KILLS the child and restarts it; that deliberate kill
    /// is exempt from the crash-strike attribution.
    pub async fn codemode_evaluate_limits_async(
        &self,
        code: &str,
        catalog: &[CodeModeToolSpec],
        token: u64,
        timeout: Duration,
        limits: &CodeModeLimits,
    ) -> Result<CodeModeRun, String> {
        /// Grace over `timeoutMs` before the transport gives up on the
        /// guest: the guest's soft timeout (a timer on the event loop)
        /// must get the chance to answer first.
        const TIMEOUT_GRACE: Duration = Duration::from_millis(1_000);
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let deadline = match limits.timeout_ms {
            Some(timeout_ms) => timeout.min(Duration::from_millis(timeout_ms) + TIMEOUT_GRACE),
            None => timeout,
        };
        let params = json!({
            "code": code,
            "tools": catalog,
            "token": token,
            "limits": limits,
        });
        let outcome = facade.shared.request("codemode.evaluate", params, deadline).await;
        let result = match outcome {
            Ok(result) => result,
            Err(HostError::Timeout(why)) => {
                // The guest never answered: it is wedged (sync loop) or
                // past its budget — either way the only interruption is
                // process death. Kill + restart, strike-exempt.
                facade.shared.request_codemode_kill();
                return Err(HostError::Timeout(why).to_string());
            }
            // Guest-thrown errors pass VERBATIM (the guest's
            // RangeError text IS the ref-identical validation message);
            // transport failures keep their display forms.
            Err(error) => return Err(guest_error_text(error)),
        };
        Ok(Self::parse_run(&result))
    }

    /// The `codemode.evaluate` result shape → [`CodeModeRun`]
    /// (`value` absent = `undefined`; explicit `null` stays null).
    fn parse_run(result: &Value) -> CodeModeRun {
        let error = result
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|text| !text.is_empty());
        let value = result.get("value").cloned();
        let truncated = result.get("truncated").and_then(Value::as_bool).unwrap_or(false);
        let tool_calls = result
            .get("calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        Some(CodeModeCallRecord {
                            tool: call.get("tool")?.as_str()?.to_string(),
                            status: call.get("status")?.as_str()?.to_string(),
                            input: call.get("input").cloned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        CodeModeRun { value, error, tool_calls, truncated }
    }
}

/// The outcome of one guest RPC call (PLUG-D14).
#[derive(Debug, Clone, PartialEq)]
pub enum RpcOutcome {
    /// The handler's output (`None` = `undefined`).
    Output(Option<Value>),
    /// A declared failure (the handler's `ctx.error(type, message,
    /// data)`): `type`, `message`, optional `data`.
    Failure { type_: String, message: String, data: Option<Value> },
    /// A transport/host failure (timeout, dead guest, handler
    /// defect): the v2 `rpc.internal` shape.
    Internal(String),
}

impl PluginHost {
    /// Dispatch one runtime hook event into the guest for `plugin_id`
    /// through the production bridge ([`Bridge::dispatch_hook`] — the
    /// PLUG-D21 seam the engine's `HookRegistry` forwarders call).
    /// Returns the possibly-mutated event.
    pub async fn dispatch_hook_async(
        &self,
        plugin_id: &str,
        domain: &str,
        name: &str,
        event: Value,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else {
            return Err("plugin subsystem disabled".into());
        };
        let bridge = Bridge { shared: Arc::clone(&facade.shared) };
        bridge
            .dispatch_hook(plugin_id, domain, name, event)
            .await
            .map_err(|error| error.to_string())
    }

    /// Dispatch one `/api/rpc/{rpcID}/{method}` call into the guest
    /// (PLUG-D14): resolves the registration, gates unknown ids/methods
    /// (the caller maps those to v2's `rpc.unavailable` /
    /// `rpc.method_not_found`), and routes the call through the guest's
    /// `rpc.call` handler.
    pub async fn rpc_call_async(
        &self,
        rpc_id: &str,
        method: &str,
        input: Value,
        timeout: Duration,
    ) -> Result<RpcOutcome, String> {
        let Some(facade) = &self.facade else {
            return Err(format!("RPC is unavailable: {rpc_id}"));
        };
        let Some(registration) = facade.shared.rpc_registration(rpc_id) else {
            return Err(format!("RPC is unavailable: {rpc_id}"));
        };
        if !registration.methods.iter().any(|name| name == method) {
            return Err(format!("Unknown RPC method: {rpc_id}.{method}"));
        }
        let result = match facade
            .shared
            .request(
                "rpc.call",
                json!({
                    "pluginId": registration.plugin_id,
                    "rpcId": rpc_id,
                    "method": method,
                    "input": input,
                }),
                timeout,
            )
            .await
        {
            Ok(result) => result,
            // Transport/host failures are the v2 `rpc.internal` leg
            // (handler defects normalize guest-side; timeouts and a
            // dead guest never echo internals).
            Err(HostError::Guest(why)) => return Ok(RpcOutcome::Internal(why)),
            Err(HostError::Timeout(_) | HostError::Exited(_)) => {
                return Ok(RpcOutcome::Internal("RPC call failed".to_owned()))
            }
        };
        // The guest answers `{ok: true, output?}` or `{ok: false,
        // failure: {type, message, data?}}`.
        if result.get("ok").and_then(Value::as_bool) == Some(false) {
            let failure = result.get("failure").cloned().unwrap_or(Value::Null);
            let type_ =
                failure.get("type").and_then(Value::as_str).unwrap_or("rpc.internal").to_owned();
            let message = failure
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("RPC call failed")
                .to_owned();
            let data = failure.get("data").cloned().filter(|data| !data.is_null());
            if type_ == "rpc.internal" || type_ == "rpc.invalid_output" {
                return Ok(RpcOutcome::Internal(message));
            }
            return Ok(RpcOutcome::Failure { type_, message, data });
        }
        Ok(RpcOutcome::Output(result.get("output").cloned()))
    }

    /// Drain the pending plugin → TUI UI requests (the wave-3 slot
    /// bridge: toasts, sidebar sections, footer hints, dialog opens).
    /// The embedder polls this between frames; empty on the disabled
    /// host.
    #[must_use]
    pub fn drain_ui_requests(&self) -> Vec<UiRequest> {
        let Some(facade) = &self.facade else { return Vec::new() };
        facade.shared.drain_ui_requests()
    }

    /// TUI-25: settle a deferred `ctx.ui.alert`/`ctx.ui.confirm`
    /// request — the drained request's `request_id` resolves the
    /// guest's promise with `result` (null for an unresolved close).
    /// A no-op on the disabled host.
    pub fn resolve_ui_request(&self, request_id: u64, result: serde_json::Value) {
        let Some(facade) = &self.facade else { return };
        facade.shared.resolve_ui_request(request_id, result);
    }

    /// Push the TUI's live route to the guest (the v2 TUI-plugin
    /// surface — the flagship sidebar's render reads the visible
    /// session at every materialize; `None` is home).
    pub fn notify_tui_route(&self, session: Option<&str>) {
        let Some(facade) = &self.facade else { return };
        facade.shared.notify_tui_route(session);
    }

    /// PLUG-D07 — push the latest activated location into the guest
    /// (`ctx.location`/`ctx.directory` follow; the engine calls this
    /// through [`crate::LocationPluginSource::location_activated`]
    /// whenever a location assembles with plugins active).
    pub fn push_location(&self, directory: &str, project_id: &str) {
        let Some(facade) = &self.facade else { return };
        facade.shared.push_location(directory, project_id);
    }

    /// Push the live theme view (the flat `TuiThemeCurrent` sidebar
    /// subset: `{accent, background, borderActive, text, textMuted}`
    /// as `#rrggbb` strings).
    pub fn notify_tui_theme(&self, view: serde_json::Value) {
        let Some(facade) = &self.facade else { return };
        facade.shared.notify_tui_theme(view);
    }

    /// Push the sidebar's layout budget (`tui.layout` — SA-F5: the
    /// guest flattener's row-layout width; the native painter's cell
    /// span for plugin sections).
    pub fn notify_tui_layout(&self, sidebar_content_width: usize) {
        let Some(facade) = &self.facade else { return };
        facade.shared.notify_tui_layout(sidebar_content_width);
    }

    /// SA-F16: the shielded scheduled guest callbacks' most recent
    /// faults (newest first) — the EACCES-class filesystem probing a
    /// plugin performs periodically, degraded to health results by the
    /// host's timer shields instead of process-level uncaught throws.
    #[must_use]
    pub fn plugin_faults(&self) -> Vec<String> {
        let Some(facade) = &self.facade else { return Vec::new() };
        facade.shared.plugin_faults.lock().expect("plugin faults poisoned").clone()
    }

    /// Run one plugin sidebar row's activate handler (the click side
    /// of the v2 TUI-plugin bridge — beta.13 bug 2's plugin-world
    /// path): the guest invokes the row's `onMouseUp`, whose
    /// navigation flows back as a `tui.route.navigate` UI request.
    /// Bounded by `timeout` (the HARD bound on one plugin callback).
    pub fn click_sidebar_row(
        &self,
        plugin_id: &str,
        row: usize,
        timeout: Duration,
    ) -> Result<bool, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let result = runtime
            .block_on(facade.shared.request(
                "slot.click",
                json!({ "pluginId": plugin_id, "row": row }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(result.get("ok").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Materialize one UI slot claim's content (`ctx.ui.slot`,
    /// api.tsx:298-312): the guest runs the claim's render function
    /// NOW (synchronously, bounded by `timeout`) and the flattened
    /// `{title, rows, clickable}` crosses once — rendering afterwards
    /// is native, no per-frame IPC.
    pub fn render_slot_claim(
        &self,
        plugin_id: &str,
        key: &str,
        timeout: Duration,
    ) -> Result<PluginSectionSpec, String> {
        self.render_slot_claim_with_props(plugin_id, key, &json!({}), timeout)
    }

    /// [`Self::render_slot_claim`] with the slot's render props (the
    /// ref `SlotMap` inputs — e.g. `prompt.footer`'s
    /// `PromptFooterInput{sessionID?, mode, showDetails}`,
    /// context.ts:159-163): the guest runs the claim's render with
    /// `props` instead of `{}`.
    pub fn render_slot_claim_with_props(
        &self,
        plugin_id: &str,
        key: &str,
        props: &Value,
        timeout: Duration,
    ) -> Result<PluginSectionSpec, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let result = runtime
            .block_on(facade.shared.request(
                "slot.claim.render",
                json!({ "pluginId": plugin_id, "key": key, "props": props }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(PluginSectionSpec::from_json(&result))
    }

    /// Run one slot-claim row's `onMouseUp` (the click side of the
    /// claim surface; navigation asks flow back as UI requests).
    pub fn click_slot_claim(
        &self,
        plugin_id: &str,
        key: &str,
        row: usize,
        timeout: Duration,
    ) -> Result<bool, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let result = runtime
            .block_on(facade.shared.request(
                "slot.claim.click",
                json!({ "pluginId": plugin_id, "key": key, "row": row }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(result.get("ok").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Materialize one plugin route page's content
    /// (`ctx.ui.router.register({name, render})`, api.tsx:204-212):
    /// the guest runs the route's render NOW with the navigation
    /// `data`, bounded by `timeout`; the flattened content renders
    /// natively.
    pub fn render_route(
        &self,
        plugin_id: &str,
        name: &str,
        data: Option<&Value>,
        timeout: Duration,
    ) -> Result<PluginSectionSpec, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let result = runtime
            .block_on(facade.shared.request(
                "route.render",
                json!({ "pluginId": plugin_id, "name": name, "data": data }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(PluginSectionSpec::from_json(&result))
    }

    /// Run one keymap-layer command's `run` callback
    /// (`ctx.keymap.layer`, api.tsx:179-199): the TUI dispatches a
    /// resolved plugin bind back into the guest, bounded by `timeout`.
    pub fn run_keymap_command(
        &self,
        plugin_id: &str,
        command_id: &str,
        timeout: Duration,
    ) -> Result<bool, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let result = runtime
            .block_on(facade.shared.request(
                "keymap.command.run",
                json!({ "pluginId": plugin_id, "id": command_id }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(result.get("ok").and_then(Value::as_bool).unwrap_or(false))
    }

    /// Materialize one plugin dialog's content at OPEN time (the
    /// delegation seam behind `DialogKind::Plugin`): the guest builds
    /// the `{title, rows, options}` structure synchronously; this
    /// call is bounded by `timeout` (the HARD bound — a hung plugin
    /// costs exactly this call). The dialog then renders natively; no
    /// per-frame IPC ever happens.
    pub fn materialize_dialog(
        &self,
        dialog_id: &str,
        timeout: Duration,
    ) -> Result<PluginDialogSpec, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let Some(plugin_id) = facade.shared.plugin_for_dialog(dialog_id) else {
            return Err(format!("{dialog_id}: no plugin dialog provider"));
        };
        let result = runtime
            .block_on(facade.shared.request(
                "dialog.materialize",
                json!({ "pluginId": plugin_id, "id": dialog_id }),
                timeout,
            ))
            .map_err(|error| error.to_string())?;
        Ok(PluginDialogSpec::from_json(&result))
    }

    /// Diagnostics: events dispatched to the guest vs dropped
    /// (timeout / full queue).
    #[must_use]
    pub fn dispatch_counters(&self) -> (u64, u64) {
        let Some(facade) = &self.facade else { return (0, 0) };
        (
            facade.shared.dispatched_events.load(AtomicOrdering::Relaxed),
            facade.shared.dropped_events.load(AtomicOrdering::Relaxed),
        )
    }

    /// Why the host never came up, when [`PluginHost::wait_ready`]
    /// returned `false`.
    #[must_use]
    pub fn start_error(&self) -> Option<String> {
        self.facade.as_ref()?.start_error.lock().ok()?.clone()
    }

    /// The CURRENT guest process id (diagnostics; the adversarial
    /// fuzz's kill target). `None` on the disabled/dead host.
    #[must_use]
    pub fn guest_pid(&self) -> Option<i32> {
        let facade = self.facade.as_ref()?;
        let pid = facade.shared.child_pid.load(AtomicOrdering::Relaxed);
        i32::try_from(pid).ok().filter(|pid| *pid > 0)
    }

    /// Shut the guest down and join the host thread.
    pub fn shutdown(&self) {
        let Some(facade) = &self.facade else { return };
        if let Some(stop) = &facade.stop {
            let _ = stop.send(true);
        }
        let thread = facade.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

/// The supervisor config assembled from the caller-facing one (the
/// `start_detached_with` body stayed under clippy's line budget once
/// the port seams landed).
fn supervisor_config(
    config: &mut PluginHostConfig,
    bun: PathBuf,
    sandbox: &std::path::Path,
    entries: &[PathBuf],
) -> SupervisorConfig {
    SupervisorConfig {
        bun,
        host_js: sandbox.join("host.js"),
        cwd: sandbox.to_owned(),
        entries: entries.to_vec(),
        registration_timeout: config.registration_timeout,
        dispatch_timeout: config.dispatch_timeout,
        restart_backoff_base: config.restart_backoff_base,
        restart_backoff_max: config.restart_backoff_max,
        max_plugin_crashes: config.max_plugin_crashes,
        max_restarts: config.max_restarts,
        env: std::mem::take(&mut config.env),
        host_state: std::mem::take(&mut config.host_state),
        session_port: config.session_port.take().or_else(|| Some(Arc::new(NoSessionEngine))),
        tool_port: config.tool_port.take().or_else(|| Some(Arc::new(NoToolEngine))),
        storage_port: config.storage_port.take(),
        permission_port: config.permission_port.take(),
        entry_options: std::mem::take(&mut config.entry_options),
    }
}

struct FacadeParts {
    shared: Option<Arc<supervisor::Shared>>,
    tap: Option<tokio::sync::mpsc::Sender<TapEvent>>,
    handle: Option<tokio::runtime::Handle>,
    stop: Option<tokio::sync::watch::Sender<bool>>,
    error: Option<String>,
}

/// PID-unique sandbox under `/tmp/opencode` (gate constraint).
fn sandbox_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    PathBuf::from(format!("{SANDBOX_ROOT}/oc-plugin-host-{pid}-{n}-{nanos}"))
}

/// The crate's fixtures directory (test-only, hidden from semver).
#[doc(hidden)]
#[must_use]
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_without_bun_disables() {
        // The env override points at a missing binary → the resolver
        // yields None → detect() returns a disabled host.
        let saved = std::env::var(bun::BUN_ENV).ok();
        std::env::set_var(bun::BUN_ENV, "/definitely/not/bun");
        let host = PluginHost::detect();
        if let Some(saved) = saved {
            std::env::set_var(bun::BUN_ENV, saved);
        } else {
            std::env::remove_var(bun::BUN_ENV);
        }
        assert!(!host.enabled(), "missing bun must disable the subsystem");
        assert!(!host.wait_ready(Duration::from_millis(50)));
        assert_eq!(host.registrations().commands, [] as [registrations::PluginCommand; 0]);
        assert!(!host.tap().publish(TapEvent::new("x", serde_json::json!({}))));
        assert!(!host.tap().is_live());
        assert!(host.execute_command("x", None, Duration::from_millis(50)).is_err());
    }

    #[test]
    fn sandbox_names_are_pid_unique() {
        let a = sandbox_dir();
        let b = sandbox_dir();
        assert_ne!(a, b);
        assert!(a.starts_with(SANDBOX_ROOT));
        assert!(a.file_name().expect("name").to_string_lossy().contains("oc-plugin-host"));
    }
}
