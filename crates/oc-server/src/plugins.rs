//! The production plugin-host boot (the wave-12 integrator glue): the
//! server constructs [`PluginHost`] from the resolved config's
//! `plugins[]` entries and attaches its surfaces to the engine —
//!
//! * **agents + tools** — the host's registration snapshot applies
//!   into every location graph through
//!   [`oc_core::plugin_apply::LocationPluginSource`] (the engine's
//!   `assemble` consults it);
//! * **Code Mode** — `EngineOptions::code_mode_runtime` carries the
//!   host-backed interpreter. v2's snapshot logic (tool.ts:225-241 +
//!   mcp.ts:45) synthesizes the `execute` tool over
//!   codemode-default-TRUE tools (MCP, `opencode.*`, plugin tools) with
//!   ZERO plugins loaded — Code Mode is core, not plugin-gated — so the
//!   runtime attaches whenever the host came up, plugin-less boots
//!   included;
//! * **`ctx.session.*`** — [`SessionPort`] over the real engine
//!   (prompt/get/list/messages/interrupt at the boot location), filled
//!   into [`EngineSessionSlot`] once the engine exists (the host starts
//!   BEFORE `Engine::boot` because the engine options need its seams);
//! * **nested `tools.*` calls** — [`ToolPort`] over the per-evaluation
//!   executor closure (token-routed: the guest echoes the
//!   `codemode.evaluate` token on every `codemode.tool` request, the
//!   router resolves the REQUEST's location registry + session
//!   context, exactly v2's per-request `executeTool` closure);
//! * **`/api/plugin`** — [`PluginBoot::list_info`] renders the loaded
//!   plugins (id, source, status incl. resolution failures and
//!   crash-disabled entries) in v2's `Plugin.Info` shape.
//!
//! Boot ordering (the boot module's ready-race rule): the host starts
//! and its registration pass SETTLES before `Engine::boot`, the
//! listener bind, and readiness — a location assembled by the first
//! request always sees the settled snapshot. A bun-less box degrades
//! gracefully (bunman's managed download first, then one clear log +
//! the flat tool surface — never a boot failure).

use std::collections::BTreeMap;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use oc_core::config::Loaded;
use oc_core::engine::Engine;
use oc_core::hooks::PluginStorage;
use oc_core::location::{LocationRef, ToolExecute, ToolRunFailure, ToolRunResult};
use oc_core::plugin_apply::{
    GuestHookDispatcher, LocationPluginSource, PluginHookDef, PluginToolDef,
};
use oc_core::tool::codemode::{CallRecord, CodeModeRuntime, RunOutcome, ToolCallFn};
use oc_plugin_host::registrations::PluginState;
use oc_plugin_host::{
    Bridge, CodeModeToolSpec, HostState, PluginHost, PluginHostConfig, Resolution, ResolvedPlugin,
    SessionPort, StoragePort, StorageScanQuery, ToolPort,
};
use oc_schema::mcp::ServerConfig;
use oc_schema::misc::{PluginFeatures, PluginInfo, PluginSource, PluginState as WireState};

/// How long boot waits for the registration pass to settle (the
/// flagship loads in ~2-5 s; slow disks get the headroom).
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The hard bound on one guest tool dispatch (`tool.transform`
/// execute) from an engine executor.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// The hard bound on one `execute` program (matches the engine's
/// `code_mode_timeout` default).
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(120);

/// The hard bound on one guest RPC dispatch (`/api/rpc`).
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// The booted plugin subsystem (attached to [`crate::state::AppState`]).
pub struct PluginBoot {
    pub host: Arc<PluginHost>,
    /// The boot-time resolution (entries + recorded failures).
    pub resolution: Resolution,
    /// Whether the host came up and its registration pass settled (the
    /// engine seams only attach when `true`).
    pub ready: bool,
    /// The nested-call router the Code Mode runtime + tool port share.
    pub router: Arc<TokenRouter>,
    /// The session-port slot (filled with the engine handle after
    /// `Engine::boot`).
    pub session_slot: Arc<EngineSessionSlot>,
}

/// One `/api/rpc` call's failure (PLUG-D14): the v2 error channel —
/// `Failure` maps to `RpcError` 400, `Internal` to `RpcInternalError`
/// 500 (`rpc.internal` / `rpc.invalid_output`).
#[derive(Debug, Clone, PartialEq)]
pub enum RpcCallError {
    /// A declared or registry failure with its v2 `type` (the 400 leg,
    /// unless the type is one of the internal two).
    Failure { type_: String, message: String, data: Option<Value> },
    /// The 500 leg (`rpc.internal` / `rpc.invalid_output`).
    Internal(String),
}

impl RpcCallError {
    /// v2's unavailable / method-not-found failures arrive as plain
    /// strings from the host facade; classify by their `type` prefix.
    fn unregistered(message: String) -> Self {
        if message.starts_with("RPC is unavailable:") {
            Self::Failure { type_: "rpc.unavailable".into(), message, data: None }
        } else if message.starts_with("Unknown RPC method:") {
            Self::Failure { type_: "rpc.method_not_found".into(), message, data: None }
        } else {
            Self::Internal(message)
        }
    }
}

impl PluginBoot {
    /// A disabled, empty boot (the panic fallback / embedders that boot
    /// without plugins).
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            host: Arc::new(PluginHost::detect()),
            resolution: Resolution::default(),
            ready: false,
            router: Arc::new(TokenRouter::default()),
            session_slot: Arc::new(EngineSessionSlot::default()),
        }
    }

    /// `/api/rpc/{rpcID}/{method}` dispatch (PLUG-D14): route the call
    /// into the guest through the host's `rpc.call`. The error strings
    /// mirror v2's failure channel exactly (`rpc.unavailable` /
    /// `rpc.method_not_found` carry their v2 messages; the caller maps
    /// them onto `RpcError` 400).
    pub async fn rpc_call(
        &self,
        rpc_id: &str,
        method: &str,
        input: Value,
    ) -> Result<Option<Value>, RpcCallError> {
        match self.host.rpc_call_async(rpc_id, method, input, RPC_TIMEOUT).await {
            Ok(oc_plugin_host::RpcOutcome::Output(output)) => Ok(output),
            Ok(oc_plugin_host::RpcOutcome::Failure { type_, message, data }) => {
                Err(RpcCallError::Failure { type_, message, data })
            }
            Ok(oc_plugin_host::RpcOutcome::Internal(message)) => {
                Err(RpcCallError::Internal(message))
            }
            // Unregistered id / method: v2's failure(type, message).
            Err(message) => Err(RpcCallError::unregistered(message)),
        }
    }

    /// `/api/plugin`'s list: every resolved plugin (host status merged
    /// in — active, failed, crash-disabled) plus the resolution
    /// failures, in v2's `Plugin.Info` shape.
    #[must_use]
    pub fn list_info(&self) -> Vec<PluginInfo> {
        let mut out = Vec::new();
        let statuses = self.host.registrations().plugins;
        for plugin in &self.resolution.plugins {
            let status = statuses.iter().find(|status| status.entry == plugin.entry);
            let raw_id = status.map(|status| status.plugin_id.clone()).unwrap_or_default();
            let state = match status.map(|status| &status.state) {
                Some(PluginState::Active | PluginState::Loading) => WireState::Active,
                Some(PluginState::Disabled { .. }) => WireState::Failed {
                    error: "plugin disabled after repeated crashes".to_owned(),
                    ref_: None,
                },
                Some(PluginState::Failed(error)) => {
                    WireState::Failed { error: error.clone(), ref_: None }
                }
                None => WireState::Failed { error: "plugin did not load".to_owned(), ref_: None },
            };
            out.push(PluginInfo {
                // The loaded plugin id (e.g. `oh-my-opencode-slim`);
                // pre-load statuses fall back to the configured target.
                id: Some(oc_schema::id::PluginId::parse(if raw_id.is_empty() {
                    plugin.target.clone()
                } else {
                    raw_id.clone()
                })),
                source: wire_source(plugin),
                features: PluginFeatures { server: Some(true), tui: None, rpc: None },
                state,
            });
        }
        for (target, error) in &self.resolution.failures {
            out.push(PluginInfo {
                id: Some(oc_schema::id::PluginId::parse(target.clone())),
                source: PluginSource::Package {
                    target: target.clone(),
                    version: None,
                    outdated: None,
                    updating: None,
                },
                features: PluginFeatures { server: Some(true), tui: None, rpc: None },
                state: WireState::Failed { error: error.clone(), ref_: None },
            });
        }
        out
    }

    /// The engine seams for `EngineOptions`: the Code Mode runtime and
    /// the per-location plugin source (both absent unless `ready`).
    #[must_use]
    pub fn engine_seams(&self) -> EngineSeams {
        if !self.ready {
            return EngineSeams { code_mode_runtime: None, plugin_source: None };
        }
        let runtime = Arc::new(HostCodeModeRuntime {
            host: Arc::clone(&self.host),
            router: Arc::clone(&self.router),
            timeout: EXECUTE_TIMEOUT,
        });
        let source = Arc::new(HostPluginSource { host: Arc::clone(&self.host) });
        EngineSeams {
            code_mode_runtime: Some(runtime as Arc<dyn CodeModeRuntime>),
            plugin_source: Some(source as Arc<dyn LocationPluginSource>),
        }
    }
}

/// The engine seams a booted subsystem offers (consumed by
/// `boot_and_serve`'s `EngineOptions`).
pub struct EngineSeams {
    pub code_mode_runtime: Option<Arc<dyn CodeModeRuntime>>,
    pub plugin_source: Option<Arc<dyn LocationPluginSource>>,
}

/// The v2 `Plugin.Source` for one resolved plugin.
fn wire_source(plugin: &ResolvedPlugin) -> PluginSource {
    match &plugin.source {
        oc_plugin_host::PluginSourceKind::Package { target, version } => PluginSource::Package {
            target: target.clone(),
            version: version.clone(),
            outdated: None,
            updating: None,
        },
        oc_plugin_host::PluginSourceKind::Local { path } => {
            PluginSource::Local { path: path.clone() }
        }
    }
}

/// The kill switch for the plugin subsystem (sandboxes / CI /
/// debugging): `OPENCODE_DISABLE_PLUGINS=1` keeps every server boot
/// plugin-less and flat (no bun spawn, no Code Mode runtime).
fn plugins_disabled() -> bool {
    std::env::var("OPENCODE_DISABLE_PLUGINS").ok().as_deref() == Some("1")
}

/// Boot the plugin host from the resolved config's `plugins[]` ops.
///
/// The engine seams ([`PluginBoot::engine_seams`]) are offered ONLY
/// when the host came up and settled — a bun-less or dead host keeps
/// today's flat surface (logged once, never fatal). With ZERO plugins
/// the host still starts when bun is available: the Code Mode runtime
/// is core, not plugin-gated (v2 synthesizes `execute` over MCP tools
/// with no plugins loaded).
///
/// `data_root` is the engine's database root: the plugin KV shares the
/// SAME `opencode.db` `kv` table (PLUG-D18 — v2 `PluginHost.storage(kv,
/// pluginID)`), so plugin state survives process restarts exactly like
/// every other durable row. A second connection over the same file is
/// safe (WAL + busy timeout; migrations idempotent under the bootstrap
/// lock).
///
/// Blocking: the registration pass parks the caller up to
/// [`READY_TIMEOUT`] — `boot_and_serve` runs this inside
/// `spawn_blocking`.
#[must_use]
pub fn boot(
    ops: &[oc_plugin_host::PluginOp],
    loaded: &Loaded,
    home: &std::path::Path,
    directory: &std::path::Path,
    data_root: &std::path::Path,
    app_version: &str,
) -> Arc<PluginBoot> {
    if plugins_disabled() {
        return Arc::new(PluginBoot::disabled());
    }
    let resolution = oc_plugin_host::resolve(ops, None);
    for (target, error) in &resolution.failures {
        eprintln!("opencode plugin: {target} failed to resolve: {error}");
    }
    let detected = PluginHost::detect();
    let router = Arc::new(TokenRouter::default());
    let session_slot = Arc::new(EngineSessionSlot::default());
    if !detected.enabled() && resolution.entries().is_empty() {
        eprintln!(
            "opencode plugin: no plugins configured and bun is unavailable; staying flat (no Code Mode runtime)"
        );
        return Arc::new(PluginBoot {
            host: Arc::new(detected),
            resolution,
            ready: false,
            router,
            session_slot,
        });
    }

    // The host-state seed: the native agent table (builtins + config
    // markdown, the same table every location's graph builds) and the
    // config-sourced MCP servers (codemode defaults applied — the
    // guest's editors snapshot the defaulted base).
    let documents = oc_core::config::agent_md::load_documents(&loaded.entries);
    let agents = oc_core::engine::native_agent_seed(&documents, home);
    let mut mcps: BTreeMap<String, ServerConfig> = BTreeMap::new();
    for document in &loaded.entries {
        if let oc_schema::config::Entry::Document(document) = document {
            if let Some(mcp) = &document.info.mcp {
                if let Some(servers) = &mcp.servers {
                    mcps.extend(
                        servers.iter().map(|(name, config)| (name.clone(), config.clone())),
                    );
                }
            }
        }
    }
    oc_core::mcp::registry::apply_codemode_defaults(&mut mcps);
    let host_state = HostState {
        app_version: app_version.to_owned(),
        project_id: String::new(),
        directory: directory.display().to_string(),
        agents,
        mcps: mcps
            .into_iter()
            .map(|(name, config)| (name, serde_json::to_value(&config).unwrap_or(Value::Null)))
            .collect(),
    };

    // PLUG-D18: the plugin KV lives in the ENGINE's database (the same
    // `opencode.db` under `data_root` — the v2 shape, one shared `kv`
    // table with `plugin:<hex(id)>:` namespaced keys). The host boots
    // BEFORE the engine creates the root, so create it here. A failed
    // open degrades the port (calls decline with a clear error) — never
    // a boot failure.
    let storage_port: Option<Arc<dyn StoragePort>> =
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _ = std::fs::create_dir_all(data_root);
            match handle.block_on(oc_core::database::Database::open_location(
                oc_core::database::engine_location(data_root),
                oc_core::database::MigrationOptions::default(),
            )) {
                Ok(db) => Some(Arc::new(DbStoragePort { db: Arc::new(db) })),
                Err(error) => {
                    eprintln!(
                    "opencode plugin: storage database unavailable ({error}); ctx.storage disabled"
                );
                    None
                }
            }
        } else {
            eprintln!(
                "opencode plugin: no async runtime for the storage database; ctx.storage disabled"
            );
            None
        };

    let config = PluginHostConfig {
        host_state,
        session_port: Some(Arc::clone(&session_slot) as Arc<dyn SessionPort>),
        tool_port: Some(Arc::new(RouterToolPort { router: Arc::clone(&router) })),
        storage_port,
        entry_options: resolution.entry_options(),
        // The registration pass spans bun cold start + every entry's
        // setup; 2 s (the crate default) flakes under load.
        registration_timeout: Duration::from_secs(10),
        ..PluginHostConfig::default()
    };
    let host = Arc::new(detected.start_detached_with(config, &resolution.entries()));
    let ready = host.enabled() && host.wait_ready(READY_TIMEOUT);
    if !ready {
        eprintln!(
            "opencode plugin: host did not come up ({:?}); plugins disabled, Code Mode runtime not attached",
            host.start_error()
        );
    }
    Arc::new(PluginBoot { host, resolution, ready, router, session_slot })
}

/// Extract the merged `plugins[]` ops from a loaded config (every
/// document's array, priority order — later documents' entries land
/// later so their removes override earlier adds; a rejected list is
/// logged and ignored, never a boot failure).
///
/// PLUG-D01: every DIRECTORY entry's `plugin`/`plugins` children
/// auto-discover FIRST (v2 `ConfigPluginSource.scan` —
/// `[...discovered, ...configured]`), so an explicit document remove
/// drops a discovered drop-in.
#[must_use]
pub fn config_plugin_ops(loaded: &Loaded) -> Vec<oc_plugin_host::PluginOp> {
    let directories: Vec<std::path::PathBuf> = loaded
        .entries
        .iter()
        .filter_map(|entry| match entry {
            oc_schema::config::Entry::Directory(directory) => {
                Some(std::path::PathBuf::from(&directory.path))
            }
            oc_schema::config::Entry::Document(_) => None,
        })
        .collect();
    let mut ops = oc_plugin_host::discover_directory_ops(&directories);
    let mut raw: Vec<Value> = Vec::new();
    for entry in &loaded.entries {
        if let oc_schema::config::Entry::Document(document) = entry {
            if let Some(plugins) = &document.info.plugins {
                for plugin in plugins {
                    if let Ok(value) = serde_json::to_value(plugin) {
                        raw.push(value);
                    }
                }
            }
        }
    }
    match oc_plugin_host::parse_ops(&raw) {
        Ok(document_ops) => ops.extend(document_ops),
        Err(error) => {
            eprintln!("opencode plugin: config plugins[] rejected: {error}");
        }
    }
    ops
}

// ---------------------------------------------------------------------------
// Code Mode (the host-backed runtime + the nested-call router)
// ---------------------------------------------------------------------------

/// The engine-facing [`CodeModeRuntime`] over the guest's
/// `codemode.evaluate`, with per-evaluation token routing.
pub struct HostCodeModeRuntime {
    pub host: Arc<PluginHost>,
    pub router: Arc<TokenRouter>,
    pub timeout: Duration,
}

#[async_trait]
impl CodeModeRuntime for HostCodeModeRuntime {
    async fn evaluate(
        &self,
        code: &str,
        catalog: &[oc_core::tool::codemode::CatalogEntry],
        call: ToolCallFn,
    ) -> RunOutcome {
        let specs: Vec<CodeModeToolSpec> = catalog
            .iter()
            .map(|entry| CodeModeToolSpec {
                path: entry.path.clone(),
                description: entry.description.clone(),
                signature: entry.signature.clone(),
            })
            .collect();
        let token = self.router.insert(call);
        let result =
            self.host.codemode_evaluate_token_async(code, &specs, token, self.timeout).await;
        self.router.remove(token);
        match result {
            Ok(run) => RunOutcome {
                value: run.value,
                error: run.error,
                tool_calls: run
                    .tool_calls
                    .into_iter()
                    .map(|record| CallRecord {
                        tool: record.tool,
                        status: record.status,
                        input: record.input,
                    })
                    .collect(),
            },
            // The hard timeout (or a dead guest) is the program's error.
            Err(message) => RunOutcome { error: Some(message), ..RunOutcome::default() },
        }
    }
}

/// Token → executor-closure routing for nested `tools.*` calls. Each
/// `execute` evaluation registers its closure (capturing the location
/// registry + session context) under a fresh token; the guest echoes
/// the token on every nested call, so concurrent programs on different
/// sessions never cross their contexts (v2's per-request closure
/// semantics).
#[derive(Default)]
pub struct TokenRouter {
    next: AtomicU64,
    slots: std::sync::Mutex<BTreeMap<u64, ToolCallFn>>,
}

impl TokenRouter {
    fn insert(&self, call: ToolCallFn) -> u64 {
        let token = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.slots.lock().expect("token router poisoned").insert(token, call);
        token
    }

    fn remove(&self, token: u64) {
        self.slots.lock().expect("token router poisoned").remove(&token);
    }
}

/// The [`ToolPort`] answering nested guest calls through the enclosing
/// evaluation's closure.
pub struct RouterToolPort {
    pub router: Arc<TokenRouter>,
}

#[async_trait]
impl ToolPort for RouterToolPort {
    async fn call(&self, _bridge: &Bridge, name: &str, _input: &Value) -> Result<Value, String> {
        Err(format!(
            "tool call {name}: no Code Mode evaluation context (token routing requires call_in)"
        ))
    }

    async fn call_in(
        &self,
        _bridge: &Bridge,
        name: &str,
        input: &Value,
        token: u64,
    ) -> Result<Value, String> {
        let call = self
            .router
            .slots
            .lock()
            .expect("token router poisoned")
            .get(&token)
            .cloned()
            .ok_or_else(|| format!("Unknown Code Mode evaluation: {token}"))?;
        call(name, input.clone()).await
    }
}

// ---------------------------------------------------------------------------
// The per-location plugin source (agents + tools at every assembly)
// ---------------------------------------------------------------------------

/// The engine-facing [`LocationPluginSource`]: the host's registration
/// snapshot, applied at every location's assembly.
pub struct HostPluginSource {
    pub host: Arc<PluginHost>,
}

impl LocationPluginSource for HostPluginSource {
    fn agents(&self) -> Vec<Value> {
        self.host.registrations().agents.iter().map(|agent| agent.info.clone()).collect()
    }

    fn default_agent(&self) -> Option<String> {
        self.host.registrations().default_agent
    }

    fn tools(&self) -> Vec<PluginToolDef> {
        self.host
            .registrations()
            .tools
            .iter()
            .map(|tool| PluginToolDef {
                name: tool.name.clone(),
                description: tool.description.clone(),
                input: tool.input.clone(),
                codemode: tool.codemode,
                namespace: tool.namespace.clone(),
            })
            .collect()
    }

    fn tool_executor(&self, name: &str) -> Option<Arc<dyn ToolExecute>> {
        Some(Arc::new(PluginToolExecutor {
            host: Arc::clone(&self.host),
            name: name.to_owned(),
            timeout: TOOL_TIMEOUT,
        }))
    }

    /// PLUG-ENGINE-MERGE: the plugin-declared MCP servers
    /// (`ctx.mcp.transform` — the ref registers them IN the server
    /// process, host.ts:374-403). The engine's `assemble` merges them
    /// into every location's `McpRegistry` through
    /// [`oc_core::plugin_apply::apply_location_plugins`], so a
    /// HEADLESS server lists/serves them (`/api/mcp`, the tool bridge)
    /// exactly like the ref — the TUI wire seam (PLUG-D33) stays as an
    /// idempotent re-push of the same rows.
    fn mcps(&self) -> Vec<oc_core::plugin_apply::PluginMcpDef> {
        self.host
            .registrations()
            .mcps
            .iter()
            .map(|mcp| oc_core::plugin_apply::PluginMcpDef {
                name: mcp.name.clone(),
                config: mcp.config.clone(),
            })
            .collect()
    }

    fn commands(&self) -> Vec<oc_core::plugin_apply::PluginCommandDef> {
        self.host
            .registrations()
            .commands
            .iter()
            .map(|command| oc_core::plugin_apply::PluginCommandDef {
                name: command.name.clone(),
                description: command.description.clone(),
            })
            .collect()
    }

    /// PLUG-D35: plugin-registered skills (`ctx.skill.transform`) —
    /// the engine seam merges them into every location's
    /// `ConfigSkillTable`, so a bundled skill is listable and
    /// `@skill`-usable exactly like v2's shared `Skill.Service` state.
    fn skills(&self) -> Vec<Value> {
        self.host.registrations().skills.iter().map(|skill| skill.info.clone()).collect()
    }

    /// PLUG-D21: the guest's hook subscriptions (registration order).
    fn hooks(&self) -> Vec<PluginHookDef> {
        self.host
            .registrations()
            .hooks
            .iter()
            .map(|hook| PluginHookDef {
                plugin_id: hook.plugin_id.clone(),
                domain: hook.domain.clone(),
                name: hook.name.clone(),
            })
            .collect()
    }

    /// PLUG-D21: the bridge forwarder — every engine dispatch site
    /// reaches the guest through `PluginHost::dispatch_hook_async`
    /// (`Bridge::dispatch_hook`).
    fn hook_dispatcher(&self) -> Option<Arc<dyn GuestHookDispatcher>> {
        Some(Arc::new(HostHookDispatcher { host: Arc::clone(&self.host) }))
    }
}

/// The [`GuestHookDispatcher`] over the host's bridge (PLUG-D21): one
/// call forwards one serialized event to one plugin's guest handlers
/// and returns the possibly-mutated copy.
struct HostHookDispatcher {
    host: Arc<PluginHost>,
}

#[async_trait]
impl GuestHookDispatcher for HostHookDispatcher {
    async fn dispatch(
        &self,
        plugin_id: &str,
        domain: &str,
        name: &str,
        event: Value,
    ) -> Result<Value, String> {
        self.host.dispatch_hook_async(plugin_id, domain, name, event).await
    }
}

/// `tool.transform` registrations dispatch to the guest's
/// `tool.execute` (guest errors pass through VERBATIM — a plugin's
/// thrown message is the model-visible text).
struct PluginToolExecutor {
    host: Arc<PluginHost>,
    name: String,
    timeout: Duration,
}

#[async_trait]
impl ToolExecute for PluginToolExecutor {
    async fn execute(&self, input: &Value) -> Result<ToolRunResult, ToolRunFailure> {
        match self.host.execute_tool_async(&self.name, input, self.timeout).await {
            Ok(Value::String(text)) => Ok(ToolRunResult {
                content: vec![oc_schema::tool::Content::Text { text }],
                metadata: None,
            }),
            Ok(other) => Ok(ToolRunResult {
                content: vec![oc_schema::tool::Content::Text {
                    text: serde_json::to_string(&other).unwrap_or_else(|_| other.to_string()),
                }],
                metadata: None,
            }),
            Err(message) => Err(ToolRunFailure::Error { message, metadata: None }),
        }
    }
}

// ---------------------------------------------------------------------------
// The plugin KV port (PLUG-D18: the engine database's `kv` table)
// ---------------------------------------------------------------------------

/// The [`StoragePort`] over oc-core's [`oc_core::hooks::DatabasePluginStorage`]:
/// one namespaced slice of the shared `kv` table per plugin id (the v2
/// `PluginHost.storage(kv, pluginID)` binding). Writes are transactions —
/// a failed write rolls back and the previous value stays.
struct DbStoragePort {
    db: Arc<oc_core::database::Database>,
}

#[async_trait]
impl StoragePort for DbStoragePort {
    async fn get(&self, plugin_id: &str, key: &str) -> Result<Option<Value>, String> {
        oc_core::hooks::DatabasePluginStorage::new(&self.db, plugin_id)
            .get(key)
            .await
            .map_err(|error| error.to_string())
    }

    async fn set(&self, plugin_id: &str, key: &str, value: Value) -> Result<(), String> {
        oc_core::hooks::DatabasePluginStorage::new(&self.db, plugin_id)
            .set(key, value)
            .await
            .map_err(|error| error.to_string())
    }

    async fn remove(&self, plugin_id: &str, key: &str) -> Result<(), String> {
        oc_core::hooks::DatabasePluginStorage::new(&self.db, plugin_id)
            .remove(key)
            .await
            .map_err(|error| error.to_string())
    }

    async fn scan(&self, plugin_id: &str, query: StorageScanQuery) -> Result<Value, String> {
        let storage = oc_core::hooks::DatabasePluginStorage::new(&self.db, plugin_id);
        let result = storage
            .scan(oc_core::hooks::ScanOptions {
                prefix: Some(query.prefix),
                cursor: query.after,
                limit: query.limit.map(|limit| u32::try_from(limit).unwrap_or(u32::MAX)),
            })
            .await
            .map_err(|error| error.to_string())?;
        // The v2 guest wire shape: `{entries: [{key, value}], next?}`
        // (namespaced stripped by the storage impl).
        let mut page = json!({
            "entries": result.items.into_iter()
                .map(|(key, value)| json!({ "key": key, "value": value }))
                .collect::<Vec<_>>(),
        });
        if let Some(next) = result.cursor {
            page["next"] = Value::String(next);
        }
        Ok(page)
    }
}

// ---------------------------------------------------------------------------
// The session port (ctx.session.* → the real engine)
// ---------------------------------------------------------------------------

/// The engine-backed session port: `ctx.session.*` calls from plugin
/// code route into the boot location's session machinery (prompt
/// admits through the inbox exactly like `POST /api/session/:id/prompt`).
/// Calls are answered asynchronously off the host's IO reader (deadlock
/// rule 1), so a prompt may dispatch hooks back into the guest through
/// the bridge while the reader keeps draining.
///
/// PLUG-D10: the full v2 `SessionDomain` answers here —
/// create/get/remove/switchAgent/switchModel/prompt/generate/command/
/// compact/synthetic/interrupt/update/move/wait/context (plus the
/// `messages`/`list` reads), each over the SAME engine services the
/// HTTP handlers use.
pub struct EngineSessionPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
    /// The plugin host handle (plugin-source `session.command` guest
    /// dispatch); `None` on plugin-less boots.
    pub host: Option<Arc<oc_plugin_host::PluginHost>>,
}

impl EngineSessionPort {
    /// The method body behind [`SessionPort::call`] (the port ignores
    /// the bridge for these paths — production and tests share it).
    ///
    /// # Errors
    /// v2-shaped error strings (not-found, no-model, command-not-found).
    pub async fn invoke(&self, method: &str, args: &Value) -> Result<Value, String> {
        self.call_inner(method, args).await
    }

    #[allow(clippy::too_many_lines)] // the v2 SessionDomain dispatch (one arm per method)
    async fn call_inner(&self, method: &str, args: &Value) -> Result<Value, String> {
        let session_id = args.get("sessionID").and_then(Value::as_str).unwrap_or_default();
        let services =
            self.engine.location(self.location.clone()).await.map_err(|error| error.to_string())?;
        match method {
            "create" => {
                let row = self
                    .engine
                    .create_session(
                        &self.location,
                        oc_core::session::store::CreateInput {
                            title: args.get("title").and_then(Value::as_str).map(str::to_owned),
                            location: Some(oc_schema::location::Ref {
                                directory: oc_schema::common::AbsolutePath(
                                    self.location.as_path().display().to_string(),
                                ),
                                workspace_id: None,
                            }),
                            version: "2.0.24".to_owned(),
                            ..oc_core::session::store::CreateInput::default()
                        },
                    )
                    .await?;
                Ok(session_info_json(&row))
            }
            "get" => {
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let row = self
                    .engine
                    .session(&self.location, &id)
                    .await?
                    .ok_or_else(|| format!("session {session_id} not found"))?;
                Ok(session_info_json(&row))
            }
            "list" => {
                let rows = self
                    .engine
                    .sessions(&self.location, oc_core::session::store::ListInput::default())
                    .await?;
                Ok(Value::Array(rows.iter().map(session_info_json).collect()))
            }
            "messages" | "context" => {
                // v2 `session.context` returns the session's message
                // read model (the same rows the HTTP context endpoint
                // serves).
                let rows = self
                    .engine
                    .messages(
                        &self.location,
                        oc_core::session::store::MessagesInput {
                            session_id: session_id.to_owned(),
                            order: Some(oc_core::session::store::ListOrder::Asc),
                            ..oc_core::session::store::MessagesInput::default()
                        },
                    )
                    .await?;
                let data: Vec<Value> = rows
                    .iter()
                    .map(|row| {
                        json!({
                            "id": row.id,
                            "sessionID": row.session_id,
                            "type": row.type_,
                            "time": { "created": row.time_created },
                        })
                    })
                    .collect();
                if method == "context" {
                    Ok(Value::Array(data))
                } else {
                    Ok(json!({ "data": data }))
                }
            }
            "prompt" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine.prompt(&self.location, &id, &message_id, text).await?;
                Ok(Value::Null)
            }
            "interrupt" => {
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let interrupted = self.engine.interrupt(&self.location, &id).await;
                Ok(json!(interrupted))
            }
            "remove" => {
                let bus = (*services.graph.bus).clone();
                services.store.remove(&bus, session_id).await.map_err(|error| error.to_string())?;
                Ok(Value::Null)
            }
            "update" => {
                // v2 SessionUpdateInput: title/metadata/permissions each
                // publish their session event; the projection follows.
                if let Some(title) = args.get("title").and_then(Value::as_str) {
                    publish_event(
                        &services,
                        "session.renamed",
                        json!({
                            "sessionID": session_id, "title": title,
                        }),
                    )
                    .await?;
                }
                if let Some(metadata) = args.get("metadata") {
                    publish_event(
                        &services,
                        "session.metadata.updated",
                        json!({
                            "sessionID": session_id, "metadata": metadata,
                        }),
                    )
                    .await?;
                }
                if let Some(permissions) = args.get("permissions") {
                    publish_event(
                        &services,
                        "session.permissions",
                        json!({
                            "sessionID": session_id, "permissions": permissions,
                        }),
                    )
                    .await?;
                }
                Ok(Value::Null)
            }
            "switchAgent" => {
                let agent = args.get("agent").and_then(Value::as_str).unwrap_or_default();
                if services.graph.agents.get(&oc_schema::id::AgentId(agent.to_owned())).is_none() {
                    return Err(format!("Agent not found: {agent}"));
                }
                publish_event(
                    &services,
                    "session.agent.selected",
                    json!({
                        "sessionID": session_id, "agent": agent,
                    }),
                )
                .await?;
                Ok(Value::Null)
            }
            "switchModel" => {
                publish_event(
                    &services,
                    "session.model.selected",
                    json!({
                        "sessionID": session_id,
                        "model": args.get("model").cloned().unwrap_or(Value::Null),
                    }),
                )
                .await?;
                Ok(Value::Null)
            }
            "compact" => {
                let id = oc_schema::id::SessionId(session_id.to_owned());
                self.engine.compact(&self.location, &id).await?;
                Ok(Value::Null)
            }
            "move" => {
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let directory =
                    args.pointer("/location/directory").and_then(Value::as_str).unwrap_or_default();
                if directory.is_empty() {
                    return Err("session.move requires location.directory".to_owned());
                }
                let destination = oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath::parse(directory),
                    workspace_id: args
                        .pointer("/location/workspace")
                        .and_then(Value::as_str)
                        .and_then(|id| oc_schema::id::WorkspaceId::parse(id.to_owned()).ok()),
                };
                // The destination project resolves through the
                // destination location's own assembly (the HTTP move
                // handler's path).
                let destination_services = self
                    .engine
                    .location(LocationRef::new(std::path::PathBuf::from(directory), None))
                    .await
                    .map_err(|error| error.to_string())?;
                let project =
                    oc_schema::id::ProjectId::parse(destination_services.project.id.clone());
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine
                    .move_session(&self.location, &id, &message_id, destination, project, None)
                    .await?;
                Ok(Value::Null)
            }
            "synthetic" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                let delivery = match args.get("delivery").and_then(Value::as_str) {
                    Some("queue") => oc_schema::session_inbox::Delivery::Queue,
                    _ => oc_schema::session_inbox::Delivery::Steer,
                };
                let info = services
                    .inbox
                    .admit(
                        &message_id,
                        session_id,
                        oc_schema::session_inbox::Item::Synthetic {
                            payload: oc_schema::session_inbox::SyntheticPayload {
                                text: text.to_owned(),
                                description: args
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                                metadata: args.get("metadata").and_then(Value::as_object).map(
                                    |metadata| {
                                        metadata
                                            .iter()
                                            .map(|(key, value)| (key.clone(), value.clone()))
                                            .collect()
                                    },
                                ),
                            },
                            delivery,
                        },
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                let _ = info;
                Ok(Value::Null)
            }
            "wait" => {
                services.execution.await_idle(session_id).await;
                Ok(Value::Null)
            }
            "command" => {
                let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
                self.invoke_session_command(&services, session_id, name, text).await
            }
            "generate" => self.invoke_generate(&services, session_id, args).await,
            other => Err(format!(
                "session.{other}: not a v2 SessionDomain method (allowlist: {:?})",
                oc_plugin_host::SESSION_METHODS
            )),
        }
    }
}

#[async_trait]
impl SessionPort for EngineSessionPort {
    async fn call(&self, _bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String> {
        self.call_inner(method, args).await
    }
}

impl EngineSessionPort {
    /// v2 `session.command` (SessionCommandInput): a registered command
    /// dispatches — plugin-source through the guest, config-source
    /// through template expansion + prompt admission (the subagent
    /// fork is the HTTP handler's; the plugin port answers the
    /// non-subagent semantics).
    async fn invoke_session_command(
        &self,
        services: &Arc<oc_core::engine::LocationServices>,
        session_id: &str,
        name: &str,
        text: &str,
    ) -> Result<Value, String> {
        let Some(registration) = services.commands.get(name) else {
            return Err(format!("Command not found: {name}"));
        };
        match registration.source {
            oc_core::engine::commands::CommandSource::Plugin => {
                let Some(host) = &self.host else {
                    return Err(format!("command execution {name}: plugin subsystem disabled"));
                };
                host.execute_command_in_session_async(
                    name,
                    Some(text),
                    session_id,
                    Duration::from_secs(30),
                )
                .await
                .map(|_| Value::Null)
            }
            oc_core::engine::commands::CommandSource::Config(command) => {
                let expanded =
                    oc_core::config::command_md::evaluate_template(&command.template, text);
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine.prompt(&self.location, &id, &message_id, &expanded).await?;
                Ok(Value::Null)
            }
        }
    }

    /// v2 `session.generate` (`SessionGenerateInput` → `{text}`): one
    /// stateless generation over the session's resolved model — the
    /// same resolver + LLM seam the `/api/experimental/generate`
    /// handler rides.
    async fn invoke_generate(
        &self,
        services: &Arc<oc_core::engine::LocationServices>,
        session_id: &str,
        args: &Value,
    ) -> Result<Value, String> {
        use futures::StreamExt as _;
        let Some(prompt) = args.get("prompt").and_then(Value::as_str) else {
            return Err("session.generate requires a prompt".to_owned());
        };
        let row = services
            .store
            .get(session_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("session {session_id} not found"))?;
        let resolved = services.resolver.resolve(&row).map_err(|error| error.to_string())?;
        let mut request = oc_ai::schema::LLMRequest::new(resolved.model);
        request.messages = vec![oc_ai::schema::messages::Message::user(prompt)];
        let mut stream = services
            .runner
            .llm()
            .stream(request)
            .await
            .map_err(|error| format!("Text generation is not available: {error}"))?;
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event.map_err(|error| format!("Text generation failed: {error}"))? {
                oc_ai::schema::LLMEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
                oc_ai::schema::LLMEvent::ProviderError { message, .. } => {
                    return Err(format!("Text generation failed: {message}"));
                }
                _ => {}
            }
        }
        Ok(json!({ "text": text }))
    }
}

/// Publish one session event through the location bus (the same
/// envelope the HTTP session handlers use).
async fn publish_event(
    services: &Arc<oc_core::engine::LocationServices>,
    type_: &str,
    data: Value,
) -> Result<(), String> {
    let Some(definition) = oc_core::bus::registry::by_type_latest(type_) else { return Ok(()) };
    let location = oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(
            services.graph.location.directory.to_string_lossy().into_owned(),
        ),
        workspace_id: services
            .graph
            .location
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    };
    services
        .graph
        .bus
        .publish(
            definition,
            data,
            oc_core::bus::PublishOptions {
                location: Some(location),
                ..oc_core::bus::PublishOptions::default()
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The v2 `Session.Info`-shaped JSON for one row (the plugin surface's
/// read model — the fields guests probe: id/title/directory/time).
fn session_info_json(row: &oc_core::database::schema::SessionRow) -> Value {
    json!({
        "id": row.id,
        "title": row.title,
        "directory": row.directory,
        "parentID": row.parent_id,
        "time": { "created": row.time_created, "updated": row.time_updated },
    })
}

/// The pre-engine session-port placeholder: the host starts before
/// `Engine::boot` (its options carry the host's seams), so the port is
/// this slot — empty until `boot_and_serve` fills it with the engine
/// handle. Calls before the fill (or on a plugin-less host) reject
/// with a clear message.
#[derive(Default)]
pub struct EngineSessionSlot(std::sync::RwLock<Option<Arc<EngineSessionPort>>>);

impl EngineSessionSlot {
    /// Fill the slot (idempotent; the first engine wins).
    pub fn fill(&self, port: Arc<EngineSessionPort>) {
        let mut slot = self.0.write().expect("session slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl SessionPort for EngineSessionSlot {
    async fn call(&self, bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String> {
        let port = self.0.read().expect("session slot poisoned").clone();
        match port {
            Some(port) => port.call(bridge, method, args).await,
            None => Err(format!(
                "session.{method}: the session engine is not attached to the plugin host yet"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_schema::config::{Directory, Entry, Plugin};

    /// PLUG-D01: the server boot's ops assembly discovers `plugin`/
    /// `plugins` drop-ins from every config DIRECTORY entry first, then
    /// appends the document `plugins[]` ops (explicit removes drop
    /// discovered entries — v2 `ConfigPluginSource.scan` ordering).
    #[test]
    fn config_plugin_ops_discover_directories_before_documents() {
        let root = std::env::temp_dir().join(format!(
            "oc-server-plugin-ops-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        let _ = std::fs::remove_dir_all(&root);
        let layer = root.join("layer");
        std::fs::create_dir_all(layer.join("plugins").join("dropin")).expect("dropin dir");
        std::fs::write(layer.join("plugins").join("dropin").join("index.ts"), "// dropin")
            .expect("dropin entry");
        std::fs::create_dir_all(&layer).expect("layer");

        let info = oc_schema::config::Info {
            plugins: Some(vec![Plugin::Spec("-dropin".to_owned())]),
            ..oc_schema::config::Info::default()
        };
        let loaded = Loaded {
            sources: oc_core::config::discovery::Sources::default(),
            entries: vec![
                Entry::Directory(Directory { path: layer.display().to_string() }),
                Entry::Document(Box::new(oc_schema::config::Document {
                    info,
                    path: Some(layer.join("opencode.json").display().to_string()),
                })),
            ],
            diagnostics: Vec::new(),
        };
        let ops = config_plugin_ops(&loaded);
        let targets: Vec<&str> = ops
            .iter()
            .map(|op| match op {
                oc_plugin_host::PluginOp::Add { target, .. }
                | oc_plugin_host::PluginOp::Remove { target } => target.as_str(),
            })
            .collect();
        assert_eq!(
            targets,
            vec![layer.join("plugins").join("dropin").display().to_string().as_str(), "dropin"],
            "discovered first, document remove last: {targets:?}"
        );
        let resolution = oc_plugin_host::resolve(&ops, None);
        assert!(
            resolution.plugins.is_empty(),
            "the explicit remove drops the discovered drop-in (failures: {:?})",
            resolution.failures
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
