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
//!   crash-disabled entries) in v2's `Plugin.Info` shape;
//! * **`ctx.event.subscribe`** — PLUG-D13: one bus listener forwards
//!   every public (server+rpc) event into the host's broadcast tap, so
//!   plugin event subscriptions see the same stream the SSE feed serves
//!   (v2 host.ts:255-265 — headless boots included).
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
    Bridge, CodeModeToolSpec, HostState, PluginHost, PluginHostConfig, PluginSourceKind,
    Resolution, ResolvedPlugin, SessionPort, StoragePort, StorageScanQuery, ToolPort,
    PLUGIN_SOURCE_DIRECTORIES,
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
    /// The boot-time resolution (entries + recorded failures), kept as
    /// a live cell: G-05 hot reload swaps it when the plugin source
    /// set changes (added/removed generations).
    pub resolution: Arc<parking_lot::Mutex<Resolution>>,
    /// Whether the host came up and its registration pass settled (the
    /// engine seams only attach when `true`).
    pub ready: bool,
    /// The nested-call router the Code Mode runtime + tool port share.
    pub router: Arc<TokenRouter>,
    /// The session-port slot (filled with the engine handle after
    /// `Engine::boot`).
    pub session_slot: Arc<EngineSessionSlot>,
    /// The permission-port slot (PLUG-D11; filled after `Engine::boot`).
    pub permission_slot: Arc<EnginePermissionSlot>,
    /// The websearch-port slot (G-24; filled after `Engine::boot`).
    pub websearch_slot: Arc<EngineWebsearchSlot>,
    /// The vcs-port slot (G-24; filled after `Engine::boot`).
    pub vcs_slot: Arc<EngineVcsSlot>,
    /// The provider-port slot (G-64; filled after `Engine::boot`).
    pub provider_slot: Arc<EngineProviderSlot>,
    /// The model-port slot (G-147; filled after `Engine::boot`).
    pub model_slot: Arc<EngineModelSlot>,
    /// The reference-port slot (G-64; filled after `Engine::boot`).
    pub reference_slot: Arc<EngineReferenceSlot>,
    /// The terminal-port slot (G-64; filled after `Engine::boot`).
    pub terminal_slot: Arc<EngineTerminalSlot>,
    /// The integration-port slot (G-64; filled after `Engine::boot`).
    pub integration_slot: Arc<EngineIntegrationSlot>,
    /// G-222 — serializes the reload passes: the ConfigReloadBridge's
    /// config leg and the [`PluginSupervisor`]'s fs/timer/sdk legs share
    /// it (the ref merges all four trigger sources into ONE supervisor
    /// queue, supervisor.ts:196-237; the lock is the port's equivalent).
    pub reload_lock: Arc<tokio::sync::Mutex<()>>,
    /// G-222 — the reload supervisor handle, filled by the boot module
    /// once the engine bus exists; tests read it for pass-count
    /// observability.
    supervisor: parking_lot::RwLock<Option<Arc<PluginSupervisor>>>,
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
            resolution: Arc::new(parking_lot::Mutex::new(Resolution::default())),
            ready: false,
            router: Arc::new(TokenRouter::default()),
            session_slot: Arc::new(EngineSessionSlot::default()),
            permission_slot: Arc::new(EnginePermissionSlot::default()),
            websearch_slot: Arc::new(EngineWebsearchSlot::default()),
            vcs_slot: Arc::new(EngineVcsSlot::default()),
            provider_slot: Arc::new(EngineProviderSlot::default()),
            model_slot: Arc::new(EngineModelSlot::default()),
            reference_slot: Arc::new(EngineReferenceSlot::default()),
            terminal_slot: Arc::new(EngineTerminalSlot::default()),
            integration_slot: Arc::new(EngineIntegrationSlot::default()),
            reload_lock: Arc::new(tokio::sync::Mutex::new(())),
            supervisor: parking_lot::RwLock::new(None),
        }
    }

    /// G-222 — the reload supervisor handle (set by the boot module
    /// after `Engine::boot`; `None` until then or for a flat boot).
    #[must_use]
    pub fn supervisor(&self) -> Option<Arc<PluginSupervisor>> {
        self.supervisor.read().clone()
    }

    /// G-222 — fills the supervisor slot (boot-module seam).
    pub fn set_supervisor(&self, supervisor: Arc<PluginSupervisor>) {
        *self.supervisor.write() = Some(supervisor);
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

    /// The package-source targets in the current inventory
    /// (`plugin.update`'s unknown-target validation base).
    #[must_use]
    pub fn package_targets(&self) -> Vec<String> {
        self.resolution
            .lock()
            .plugins
            .iter()
            .filter(|plugin| {
                matches!(plugin.source, oc_plugin_host::PluginSourceKind::Package { .. })
            })
            .map(|plugin| plugin.target.clone())
            .collect()
    }

    /// `/api/plugin`'s list: every resolved plugin (host status merged
    /// in — active, failed, crash-disabled) plus the resolution
    /// failures, in v2's `Plugin.Info` shape.
    #[must_use]
    pub fn list_info(&self) -> Vec<PluginInfo> {
        let resolution = self.resolution.lock().clone();
        let mut out = Vec::new();
        let statuses = self.host.registrations().plugins;
        for plugin in &resolution.plugins {
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
        for (target, error) in &resolution.failures {
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
        let source = Arc::new(HostPluginSource {
            host: Arc::clone(&self.host),
            resolution: Arc::clone(&self.resolution),
        });
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
    let permission_slot = Arc::new(EnginePermissionSlot::default());
    let websearch_slot = Arc::new(EngineWebsearchSlot::default());
    let vcs_slot = Arc::new(EngineVcsSlot::default());
    let provider_slot = Arc::new(EngineProviderSlot::default());
    let model_slot = Arc::new(EngineModelSlot::default());
    let reference_slot = Arc::new(EngineReferenceSlot::default());
    let terminal_slot = Arc::new(EngineTerminalSlot::default());
    let integration_slot = Arc::new(EngineIntegrationSlot::default());
    if !detected.enabled() && resolution.entries().is_empty() {
        eprintln!(
            "opencode plugin: no plugins configured and bun is unavailable; staying flat (no Code Mode runtime)"
        );
        return Arc::new(PluginBoot {
            host: Arc::new(detected),
            resolution: Arc::new(parking_lot::Mutex::new(resolution)),
            ready: false,
            router,
            session_slot,
            permission_slot,
            websearch_slot,
            vcs_slot,
            provider_slot,
            model_slot,
            reference_slot,
            terminal_slot,
            integration_slot,
            reload_lock: Arc::new(tokio::sync::Mutex::new(())),
            supervisor: parking_lot::RwLock::new(None),
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
        permission_port: Some(
            Arc::clone(&permission_slot) as Arc<dyn oc_plugin_host::PermissionPort>
        ),
        // G-24: ctx.websearch.providers/query + ctx.vcs.get/base over
        // the boot location's engine services (slots filled after
        // Engine::boot, the permission-port pattern).
        websearch_port: Some(Arc::clone(&websearch_slot) as Arc<dyn oc_plugin_host::WebsearchPort>),
        vcs_port: Some(Arc::clone(&vcs_slot) as Arc<dyn oc_plugin_host::VcsPort>),
        // G-64: ctx.provider.*/reference.*/experimental.terminal.read/
        // integration.* over the boot location's engine services (slots
        // filled after Engine::boot, the same pattern).
        provider_port: Some(Arc::clone(&provider_slot) as Arc<dyn oc_plugin_host::ProviderPort>),
        // G-147: ctx.model.list/default/reload over the boot location's
        // engine model catalog (slot filled after Engine::boot).
        model_port: Some(Arc::clone(&model_slot) as Arc<dyn oc_plugin_host::ModelPort>),
        reference_port: Some(Arc::clone(&reference_slot) as Arc<dyn oc_plugin_host::ReferencePort>),
        terminal_port: Some(Arc::clone(&terminal_slot) as Arc<dyn oc_plugin_host::TerminalPort>),
        integration_port: Some(
            Arc::clone(&integration_slot) as Arc<dyn oc_plugin_host::IntegrationPort>
        ),
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
    Arc::new(PluginBoot {
        host,
        resolution: Arc::new(parking_lot::Mutex::new(resolution)),
        ready,
        router,
        session_slot,
        permission_slot,
        websearch_slot,
        vcs_slot,
        provider_slot,
        model_slot,
        reference_slot,
        terminal_slot,
        integration_slot,
        reload_lock: Arc::new(tokio::sync::Mutex::new(())),
        supervisor: parking_lot::RwLock::new(None),
    })
}

/// G-05 hot reload — the post-`config.updated` refresh for the plugin
/// subsystem, mirroring the ref supervisor's generation lifecycle
/// (supervisor.ts:23-94, add/remove only — no hot-swap of existing
/// generations beyond the digest reload):
///
/// * EXISTING sources: `PluginHost::reload_now` digest-scans every
///   loadable entry and reloads the changed ones (a failed reload keeps
///   the previous generation — crash isolation precedent; errors are
///   logged, never propagated).
/// * ADDED targets (a new drop-in under `.opencode/plugin`, a new
///   `plugins[]` entry): re-resolve the current ops and load the new
///   entry's generation into the live guest (`PluginHost::add_entry`).
/// * REMOVED targets: tear the entry's generation down
///   (`PluginHost::remove_entry` — registrations/slot claims drop, the
///   guest unloads the plugin id).
///
/// Every failure is contained: a failing add/remove is logged, the
/// previous generation keeps its place, and the live resolution cell
/// only swaps when the diff applied cleanly.
pub fn refresh_after_config_change(boot: &PluginBoot, loaded: &Loaded) {
    if !boot.ready {
        return;
    }
    let current = config_plugin_ops(loaded);
    let previous_resolution = boot.resolution.lock().clone();
    let previous: Vec<String> =
        previous_resolution.plugins.iter().map(|plugin| plugin.target.clone()).collect();
    let (added, removed) = plugin_source_diff(&previous, &current);

    // Removed first (a remove+re-add round trip collapses to a reload).
    let mut clean = true;
    for target in &removed {
        let entry = previous_resolution.plugins.iter().find(|plugin| &plugin.target == target);
        let Some(plugin) = entry else {
            clean = false;
            eprintln!("opencode plugin: removed target {target} has no entry; skipping");
            continue;
        };
        match boot.host.remove_entry(&plugin.entry) {
            Ok(true) => {
                eprintln!("opencode plugin: source change unloaded {target} generation");
            }
            Ok(false) => {} // never loaded — idempotent
            Err(error) => {
                clean = false;
                eprintln!(
                    "opencode plugin: unload {target} failed ({error}); previous generation kept"
                );
            }
        }
    }

    // Added: resolve the current ops so the new targets map to entries.
    let resolved = oc_plugin_host::resolve(&current, None);
    for (target, error) in &resolved.failures {
        eprintln!("opencode plugin: {target} failed to resolve: {error}");
    }
    for target in &added {
        if let Some(plugin) = resolved.plugins.iter().find(|plugin| &plugin.target == target) {
            match boot.host.add_entry(&plugin.entry) {
                Ok(()) => {
                    eprintln!("opencode plugin: source change loaded {target} generation");
                }
                Err(error) => {
                    clean = false;
                    eprintln!(
                        "opencode plugin: load {target} failed ({error}); previous generations kept"
                    );
                }
            }
        } else {
            clean = false;
            eprintln!("opencode plugin: added target {target} did not resolve; skipping");
        }
    }

    // Digest reload of the surviving sources (existing generations).
    match boot.host.reload_now() {
        Ok(count) if count > 0 => {
            eprintln!("opencode plugin: hot-reloaded {count} changed plugin source(s)");
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("opencode plugin: source reload failed ({error}); previous generations kept");
        }
    }

    // The live resolution cell drives per-location activation
    // (`active_plugins`) and `/api/plugin` — swap only on a clean diff;
    // a partial failure keeps the previous snapshot (conservative).
    if clean {
        *boot.resolution.lock() = resolved;
    }
}

// ============================================================================
// G-222 — the plugin reload supervisor (plugin/supervisor.ts:196-237)
// ============================================================================
//
// The ref reloads plugins from FOUR trigger sources merged into a
// sliding(1) queue with a 100 ms debounce:
//
//   * `sources.changes()` — the plugin-source fs watch (each resolved
//     operation's target, config/plugin/source.ts:60-68) plus config
//     changes filtered to plugin-source paths;
//   * `modules.changes()` — the loaded guest modules' own file changes;
//   * the 24 h periodic reload (supervisor.ts:211);
//   * `bus.subscribe([Event.Updated, SdkPlugins.Updated])`
//     (supervisor.ts:213).
//
// The port's mapping (declared seam): the `Event.Updated`
// (`config.updated`) leg is served by `ConfigReloadBridge` (boot.rs),
// which runs the identical pass immediately on every real config
// change — this supervisor owns the three legs the port lacked: the
// plugin-source fs watch, the 24 h timer, and `sdk.plugin.updated`
// (sdk.ts:9). All passes serialize through `PluginBoot::reload_lock`
// so the bridge's leg and the supervisor's legs never interleave (the
// ref needs no lock — its single queue IS the serialization point).

/// The ref's debounce window (supervisor.ts:236:
/// `Stream.debounce("100 millis")` on the merged trigger queue).
const SUPERVISOR_DEBOUNCE_MS: u64 = 100;
/// The ref's periodic reload (supervisor.ts:211:
/// `Stream.fromEffectRepeat(Effect.sleep("24 hours"))`).
const SUPERVISOR_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);

/// One serialized reload pass — the full [`refresh_after_config_change`]
/// pipeline (drop-in rescan → add/remove diff → digest reload) under the
/// boot's reload lock, off the async workers. Works WITHOUT any config
/// entry change: editing a plugin file changes no config entry, but the
/// rescan + digest scan still observe it (the G-222 gap — the ref's
/// "plugins changed" is separate from "config changed").
pub async fn run_reload_pass(boot: &Arc<PluginBoot>, watched: &Arc<oc_core::config::WatchedConfig>) {
    let lock = Arc::clone(&boot.reload_lock);
    let boot_pass = Arc::clone(boot);
    let loaded = watched.loaded();
    let _ = tokio::task::spawn_blocking(move || {
        let _guard = lock.blocking_lock();
        refresh_after_config_change(&boot_pass, &loaded);
    })
    .await;
}

/// G-222 — the plugin reload supervisor: watches the plugin sources,
/// merges the triggers with ref debounce semantics, and runs
/// [`run_reload_pass`] on every accepted trigger.
pub struct PluginSupervisor {
    boot: Arc<PluginBoot>,
    watched: Arc<oc_core::config::WatchedConfig>,
    /// The sliding(1) trigger queue: `send_modify` collapses a burst to
    /// its latest value (supervisor.ts:201 `Queue.sliding<number>(1)` —
    /// the queue "retains the latest observed request").
    trigger: tokio::sync::watch::Sender<u64>,
    shutdown: tokio::sync::watch::Sender<bool>,
    watcher: Arc<oc_core::filesystem::watcher::FileWatcher>,
    /// One pump task per live watch (abort = unsubscribe: the owned
    /// Subscription drops and detaches from the shared native watch).
    pumps: parking_lot::Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>,
    passes: AtomicU64,
}

impl PluginSupervisor {
    /// Starts the supervisor legs: the debounced pass loop, the 24 h
    /// timer (supervisor.ts:211), the `sdk.plugin.updated` bus
    /// subscription (supervisor.ts:213), and the plugin-source watches
    /// (source.ts:60-68).
    #[must_use]
    pub fn spawn(
        boot: Arc<PluginBoot>,
        watched: Arc<oc_core::config::WatchedConfig>,
        bus: oc_core::bus::Bus,
    ) -> Arc<Self> {
        let (trigger_tx, trigger_rx) = tokio::sync::watch::channel(0u64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let supervisor = Arc::new(Self {
            boot,
            watched,
            trigger: trigger_tx,
            shutdown: shutdown_tx,
            watcher: oc_core::filesystem::watcher::FileWatcher::new(),
            pumps: parking_lot::Mutex::new(std::collections::HashMap::new()),
            passes: AtomicU64::new(0),
        });
        // The merged-queue consumer (the debounce + apply loop).
        let debounce = Arc::clone(&supervisor);
        let debounce_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            debounce.run(trigger_rx, debounce_shutdown).await;
        });
        // supervisor.ts:211 — the 24h periodic reload.
        let timer = Arc::clone(&supervisor);
        let mut timer_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = timer_shutdown.changed() => break,
                    _ = tokio::time::sleep(SUPERVISOR_PERIOD) => timer.notify(),
                }
            }
        });
        // supervisor.ts:213 — the SdkPlugins.Updated bus leg (the
        // Event.Updated half of that subscription is ConfigReloadBridge's,
        // which runs the same pass synchronously — see boot.rs).
        let bus_leg = Arc::clone(&supervisor);
        let mut bus_shutdown = shutdown_rx;
        let mut events = bus.subscribe_types(&[oc_core::bus::registry::SDK_PLUGIN_UPDATED]);
        tokio::spawn(async move {
            use futures::StreamExt as _;
            loop {
                tokio::select! {
                    _ = bus_shutdown.changed() => break,
                    event = events.next() => match event {
                        Some(_) => bus_leg.notify(),
                        None => break,
                    },
                }
            }
        });
        // source.ts:60-68 — the plugin-source watches.
        supervisor.reconcile_watches();
        supervisor
    }

    /// Completed reload passes — the burst-coalescing observable (tests).
    #[must_use]
    pub fn passes(&self) -> u64 {
        self.passes.load(Ordering::SeqCst)
    }

    /// Offers one trigger onto the sliding(1) queue.
    fn notify(&self) {
        self.trigger.send_modify(|count| *count += 1);
    }

    /// supervisor.ts:234-237 — the debounced consumer: ONE pass per
    /// quiet window however many triggers the burst produced (the
    /// window extends while more land), then re-arm the watches (the
    /// pass may have added/removed plugin targets).
    async fn run(
        self: Arc<Self>,
        mut triggers: tokio::sync::watch::Receiver<u64>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                changed = triggers.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
            // Debounce: extend the window while more triggers land.
            loop {
                tokio::time::sleep(Duration::from_millis(SUPERVISOR_DEBOUNCE_MS)).await;
                if !triggers.has_changed().unwrap_or(false) {
                    break;
                }
                let _ = triggers.borrow_and_update();
            }
            run_reload_pass(&self.boot, &self.watched).await;
            self.passes.fetch_add(1, Ordering::SeqCst);
            self.reconcile_watches();
        }
    }

    /// Reconciles the fs watches against the current sources (the ref's
    /// `watchConfiguredSources` re-run, source.ts:78-80).
    fn reconcile_watches(&self) {
        let targets = supervisor_targets(&self.boot, &self.watched.loaded());
        let keys: std::collections::HashSet<String> =
            targets.iter().map(|(key, _)| key.clone()).collect();
        let mut pumps = self.pumps.lock();
        pumps.retain(|key, handle| {
            if keys.contains(key) {
                true
            } else {
                handle.abort();
                false
            }
        });
        for (key, target) in targets {
            if pumps.contains_key(&key) {
                continue;
            }
            let mut subscription = self.watcher.subscribe(&target);
            let trigger = self.trigger.clone();
            let mut shutdown = self.shutdown.subscribe();
            let handle = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => break,
                        update = subscription.recv() => match update {
                            Some(_) => trigger.send_modify(|count| *count += 1),
                            None => break,
                        },
                    }
                }
            });
            pumps.insert(key, handle);
        }
    }
}

/// The watch set (source.ts:60-68 — "each resolved operation's target"):
/// (key, target) pairs for
///
/// 1. every config directory source's `plugin`/`plugins` drop-in
///    directories ([`PLUGIN_SOURCE_DIRECTORIES`] — what
///    `discover_directory_ops` scans; watching them makes add/delete
///    of drop-in files observable), and
/// 2. every resolved LOCAL plugin's entry file that does not already
///    live under one of those directories (document `plugins[]` file
///    targets).
///
/// Package installs are not watched: their generations swap through the
/// RF-02 update path (`reload_after_update`), never in place.
fn supervisor_targets(
    boot: &PluginBoot,
    loaded: &Loaded,
) -> Vec<(String, oc_core::config::watch::WatchTarget)> {
    use oc_core::config::watch::WatchTarget;

    let mut directories: Vec<std::path::PathBuf> = Vec::new();
    for entry in &loaded.entries {
        if let oc_schema::config::Entry::Directory(directory) = entry {
            let root = std::path::PathBuf::from(&directory.path);
            for name in PLUGIN_SOURCE_DIRECTORIES {
                let dir = root.join(name);
                if dir.is_dir() {
                    directories.push(dir);
                }
            }
        }
    }
    let mut targets: Vec<(String, WatchTarget)> = directories
        .into_iter()
        .map(|dir| {
            (
                format!("directory:{}", dir.display()),
                WatchTarget::Directory { path: dir, ignore: vec!["node_modules".to_owned()] },
            )
        })
        .collect();
    for plugin in &boot.resolution.lock().plugins {
        let under_watched_dir = targets.iter().any(|(_, target)| match target {
            WatchTarget::Directory { path, .. } => plugin.entry.starts_with(path),
            WatchTarget::File { .. } | WatchTarget::Entries { .. } => false,
        });
        if !matches!(plugin.source, PluginSourceKind::Local { .. })
            || !plugin.entry.is_file()
            || under_watched_dir
        {
            continue;
        }
        let key = format!("file:{}", plugin.entry.display());
        if targets.iter().all(|(existing, _)| *existing != key) {
            targets.push((key, WatchTarget::File { path: plugin.entry.clone() }));
        }
    }
    targets
}

/// RF-02 — the post-update reload (`plugin/update`'s "notify active
/// locations to reload" leg, plugin.ts's `Plugin.Event.Updated`
/// publish): the update wrote NEW install generations under the cache
/// root, so the current targets are re-resolved (the unchanged specs
/// now point at the new entry paths) and every entry whose path moved
/// is swapped in the live guest (`remove_entry` + `add_entry`), the
/// digest reload runs for the surviving sources, and the resolution
/// cell swaps on a clean diff. Contained like
/// [`refresh_after_config_change`]: every failure is logged and the
/// previous generation keeps its place.
pub fn reload_after_update(boot: &PluginBoot) -> bool {
    if !boot.ready {
        return false;
    }
    let previous = boot.resolution.lock().clone();
    let ops: Vec<oc_plugin_host::PluginOp> = previous
        .plugins
        .iter()
        .map(|plugin| oc_plugin_host::PluginOp::Add {
            target: plugin.target.clone(),
            options: plugin.options.clone(),
        })
        .collect();
    let resolved = oc_plugin_host::resolve(&ops, None);
    let mut clean = true;
    let mut swapped_any = false;
    for plugin in &resolved.plugins {
        let Some(old) = previous.plugins.iter().find(|old| old.target == plugin.target) else {
            continue;
        };
        if old.entry == plugin.entry {
            continue;
        }
        swapped_any = true;
        match boot.host.remove_entry(&old.entry) {
            Ok(true) => {}  // unloaded; the new generation loads below
            Ok(false) => {} // never loaded (a failed boot) — nothing to tear down
            Err(error) => {
                eprintln!("opencode plugin: updated {} unload failed ({error})", plugin.target);
            }
        }
        match boot.host.add_entry(&plugin.entry) {
            Ok(()) => {
                eprintln!("opencode plugin: update swapped {} generation", plugin.target);
            }
            Err(error) => {
                clean = false;
                eprintln!(
                    "opencode plugin: updated {} failed to load ({error}); previous generation kept",
                    plugin.target
                );
            }
        }
    }
    for (target, error) in &resolved.failures {
        clean = false;
        eprintln!("opencode plugin: post-update resolve {target} failed: {error}");
    }
    let mut touched = swapped_any;
    match boot.host.reload_now() {
        Ok(count) if count > 0 => {
            touched = true;
            eprintln!("opencode plugin: post-update hot-reloaded {count} source(s)");
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("opencode plugin: post-update reload failed ({error}); keeping generations");
        }
    }
    if clean {
        *boot.resolution.lock() = resolved;
    }
    touched
}

/// G-29 — `POST /api/plugin/install`'s activation leg: the install
/// wrote a new generation under the cache root and the global config
/// now names the target; load the new entry's generation into the live
/// guest and swap the resolution cell so `/api/plugin` (and every
/// location's activation set) sees it immediately. Contained like
/// [`refresh_after_config_change`]: a failed load is logged, the
/// resolution still swaps (the entry lists with its load status), and
/// a dead host keeps the previous state.
pub fn activate_installed_plugin(boot: &PluginBoot, target: &str) {
    let previous = boot.resolution.lock().clone();
    let mut ops: Vec<oc_plugin_host::PluginOp> = previous
        .plugins
        .iter()
        .map(|plugin| oc_plugin_host::PluginOp::Add {
            target: plugin.target.clone(),
            options: plugin.options.clone(),
        })
        .collect();
    ops.push(oc_plugin_host::PluginOp::Add { target: target.to_owned(), options: Value::Null });
    let resolved = oc_plugin_host::resolve(&ops, None);
    if let Some(plugin) = resolved.plugins.iter().find(|plugin| plugin.target == target) {
        match boot.host.add_entry(&plugin.entry) {
            Ok(()) => {
                eprintln!("opencode plugin: install loaded {target} generation");
            }
            Err(error) => {
                eprintln!(
                    "opencode plugin: install of {target} failed to load ({error}); it activates on the next boot",
                );
            }
        }
    }
    for (failed, error) in &resolved.failures {
        eprintln!("opencode plugin: post-install resolve {failed} failed: {error}");
    }
    *boot.resolution.lock() = resolved;
}

/// The supervisor resolve diff (supervisor.ts:23-94, add/remove only):
/// replay the current ops over the previously-loaded targets; `*` and
/// `name.*` remove selectors match like the boot reduction
/// ([`boot_remove_matches`]). Returns (added, removed) targets — the
/// generations needing a load/unload.
fn plugin_source_diff(
    previous: &[String],
    current: &[oc_plugin_host::PluginOp],
) -> (Vec<String>, Vec<String>) {
    let mut active: Vec<String> = previous.to_vec();
    for op in current {
        match op {
            oc_plugin_host::PluginOp::Add { target, .. } => {
                if !active.contains(target) {
                    active.push(target.clone());
                }
            }
            oc_plugin_host::PluginOp::Remove { target } => {
                if target == "*" {
                    active.clear();
                } else {
                    active.retain(|seen| !boot_remove_matches(seen, target));
                }
            }
        }
    }
    let added: Vec<String> = active
        .iter()
        .filter(|target| !previous.iter().any(|seen| seen == *target))
        .cloned()
        .collect();
    let removed: Vec<String> = previous
        .iter()
        .filter(|target| !active.iter().any(|seen| seen.as_str() == target.as_str()))
        .cloned()
        .collect();
    (added, removed)
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
        self.evaluate_with_limits(
            code,
            catalog,
            &oc_core::tool::codemode::ExecutionLimits::default(),
            call,
        )
        .await
    }

    /// The limits-bearing path: the budgets cross the wire to the
    /// guest (`codemode.evaluate` params.limits), which validates and
    /// enforces them; the transport deadline aligns with `timeoutMs`
    /// so the guest's soft timeout answers first and a wedged guest is
    /// killed (strike-exempt) by the supervisor.
    async fn evaluate_with_limits(
        &self,
        code: &str,
        catalog: &[oc_core::tool::codemode::CatalogEntry],
        limits: &oc_core::tool::codemode::ExecutionLimits,
        call: ToolCallFn,
    ) -> RunOutcome {
        let specs: Vec<CodeModeToolSpec> = catalog
            .iter()
            .map(|entry| CodeModeToolSpec {
                path: entry.path.clone(),
                description: entry.description.clone(),
                signature: entry.signature.clone(),
                search_text: entry.search_text.clone(),
            })
            .collect();
        let token = self.router.insert(call);
        let result = self
            .host
            .codemode_evaluate_limits_async(
                code,
                &specs,
                token,
                self.timeout,
                &oc_plugin_host::CodeModeLimits {
                    timeout_ms: limits.timeout_ms,
                    max_tool_calls: limits.max_tool_calls,
                    max_output_bytes: limits.max_output_bytes,
                },
            )
            .await;
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
                truncated: run.truncated,
                logs: run.logs,
                ..RunOutcome::default()
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
    /// The live resolution (target → entry mapping for PLUG-D07's
    /// per-location activation sets; swapped on G-05 source changes).
    pub resolution: Arc<parking_lot::Mutex<Resolution>>,
}

impl LocationPluginSource for HostPluginSource {
    fn agents(&self) -> Vec<oc_core::plugin_apply::PluginAgentDef> {
        self.host
            .registrations()
            .agents
            .iter()
            .map(|agent| oc_core::plugin_apply::PluginAgentDef {
                plugin_id: agent.plugin_id.clone(),
                info: agent.info.clone(),
            })
            .collect()
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
                plugin_id: tool.plugin_id.clone(),
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
                plugin_id: mcp.plugin_id.clone(),
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
                plugin_id: command.plugin_id.clone(),
                name: command.name.clone(),
                description: command.description.clone(),
            })
            .collect()
    }

    /// PLUG-D35: plugin-registered skills (`ctx.skill.transform`) —
    /// the engine seam merges them into every location's
    /// `ConfigSkillTable`, so a bundled skill is listable and
    /// `@skill`-usable exactly like v2's shared `Skill.Service` state.
    fn skills(&self) -> Vec<oc_core::plugin_apply::PluginSkillDef> {
        self.host
            .registrations()
            .skills
            .iter()
            .map(|skill| oc_core::plugin_apply::PluginSkillDef {
                plugin_id: skill.plugin_id.clone(),
                info: skill.info.clone(),
            })
            .collect()
    }

    /// G-64 — plugin provider definitions (`ctx.provider.transform`'s
    /// editor table): the engine's `assemble` folds them into every
    /// location's provider overlay (`apply_plugin_providers`).
    fn provider_defs(&self) -> Vec<oc_core::plugin_apply::PluginProviderDef> {
        self.host
            .registrations()
            .providers
            .iter()
            .map(|def| oc_core::plugin_apply::PluginProviderDef {
                plugin_id: def.plugin_id.clone(),
                id: def.id.clone(),
                definition: def.definition.clone(),
            })
            .collect()
    }

    /// G-64 — plugin reference sources (`ctx.reference.transform`'s
    /// editor table): the engine's `assemble` applies them into the
    /// `ReferenceService` (`apply_plugin_references`).
    fn reference_defs(&self) -> Vec<oc_core::plugin_apply::PluginReferenceDef> {
        self.host
            .registrations()
            .references
            .iter()
            .map(|def| oc_core::plugin_apply::PluginReferenceDef {
                plugin_id: def.plugin_id.clone(),
                name: def.name.clone(),
                source: def.source.clone(),
            })
            .collect()
    }

    /// G-24 — plugin-registered websearch providers
    /// (`ctx.websearch.transform`): the engine's `assemble` merges them
    /// into every location's WebSearch registry (the execute closure
    /// stays in the guest).
    fn websearch_providers(&self) -> Vec<oc_core::plugin_apply::PluginWebsearchProviderDef> {
        self.host
            .registrations()
            .websearch_providers
            .iter()
            .map(|provider| oc_core::plugin_apply::PluginWebsearchProviderDef {
                plugin_id: provider.plugin_id.clone(),
                id: provider.id.clone(),
                name: provider.name.clone(),
            })
            .collect()
    }

    /// G-24 — plugin-registered worktree strategies
    /// (`ctx.worktree.transform`): boot applies them into the server's
    /// shared [`oc_core::project::worktree_strategy::StrategyRegistry`].
    fn worktree_strategies(&self) -> Vec<oc_core::plugin_apply::PluginWorktreeStrategyDef> {
        self.host
            .registrations()
            .worktree_strategies
            .iter()
            .map(|strategy| oc_core::plugin_apply::PluginWorktreeStrategyDef {
                plugin_id: strategy.plugin_id.clone(),
                id: strategy.id.clone(),
            })
            .collect()
    }

    /// G-24 — plugin-registered integration methods
    /// (`ctx.integration.method.update`, ref host.ts:631).
    fn integration_methods(&self) -> Vec<oc_core::plugin_apply::PluginIntegrationMethodDef> {
        self.host
            .registrations()
            .integration_methods
            .iter()
            .map(|method| oc_core::plugin_apply::PluginIntegrationMethodDef {
                plugin_id: method.plugin_id.clone(),
                registration: method.registration.clone(),
            })
            .collect()
    }

    /// PLUG-D07: reduce THIS location's operations (the boot pass's
    /// dedup/remove semantics) and map the surviving targets onto the
    /// loaded plugins' ids — a location's registrations come only from
    /// plugins its own config layers activated. A target the boot pass
    /// never loaded contributes nothing (loading new guests per
    /// location is the per-instance-host divergence, noted in the
    /// ledger).
    fn active_plugins(
        &self,
        ops: &[oc_core::plugin_apply::LocationPluginOp],
    ) -> Option<Vec<String>> {
        let mut targets: Vec<String> = Vec::new();
        for op in ops {
            match op {
                oc_core::plugin_apply::LocationPluginOp::Add { target } => {
                    if targets.iter().any(|seen| seen == target) {
                        continue;
                    }
                    targets.push(target.clone());
                }
                oc_core::plugin_apply::LocationPluginOp::Remove { target } => {
                    if target == "*" {
                        targets.clear();
                    } else {
                        targets.retain(|seen| !boot_remove_matches(seen, target));
                    }
                }
            }
        }
        let resolution = self.resolution.lock().clone();
        let snapshot = self.host.registrations();
        let mut ids: Vec<String> = Vec::new();
        for target in targets {
            // Direct: the boot resolution carries this target.
            if let Some(plugin) = resolution.plugins.iter().find(|p| p.target == target) {
                if let Some(status) = snapshot.plugins.iter().find(|s| s.entry == plugin.entry) {
                    if !status.plugin_id.is_empty() {
                        ids.push(status.plugin_id.clone());
                        continue;
                    }
                }
            }
            // Fallback: the loaded plugin's own id matches the target.
            if let Some(status) = snapshot.plugins.iter().find(|s| s.plugin_id == target) {
                ids.push(status.plugin_id.clone());
            }
        }
        Some(ids)
    }

    /// PLUG-D07: push the latest activated location into the guest
    /// (the host updates `ctx.location`/`ctx.directory`).
    fn location_activated(&self, directory: &str, project_id: &str) {
        self.host.push_location(directory, project_id);
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

/// v2 remove matching over boot targets (mirrors
/// `oc_plugin_host::resolve`'s reduction): the directive matches the
/// spec, its package name, or a local path stem.
fn boot_remove_matches(target: &str, directive: &str) -> bool {
    if target == directive {
        return true;
    }
    let package = target.rsplit_once('@').map_or(target, |(name, _)| name);
    if package == directive {
        return true;
    }
    std::path::Path::new(target)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == directive)
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
                output: None,
            }),
            Ok(other) => Ok(ToolRunResult {
                content: vec![oc_schema::tool::Content::Text {
                    text: serde_json::to_string(&other).unwrap_or_else(|_| other.to_string()),
                }],
                metadata: None,
                // TOOL-D1: plugin tools returning structured values can
                // adopt the widened seam later; the flattened text stays
                // the model-facing content.
                output: Some(other),
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

    /// PLUG-D13: the serve-mode event source for `ctx.event.subscribe`.
    ///
    /// v2 (host.ts:255-265) wires `ctx.event.subscribe` onto the process
    /// bus, filtered to `EventManifest.isServer(event) || isRpcEvent(event)`
    /// — the same predicate the SSE feed applies (`sse::is_public_event`).
    /// The Rust port's ONLY feed into the guest's dispatch pump is the
    /// host's [`BroadcastTap`]; the TUI store publishes into it after its
    /// reducer runs, but serve mode had no publisher at all — the tap was
    /// live yet nothing fed it, so every plugin subscription starved.
    ///
    /// This attaches one bus listener (non-blocking `try_send` semantics;
    /// a slow guest drops events with one log line, never stalling the
    /// publish path) for the lifetime of the process: the guard is
    /// deliberately kept alive past this port (the bus, the host, and the
    /// server share the process lifetime; after a host death the tap's
    /// channel is closed and publishes degrade to logged no-ops).
    pub fn attach_event_tap(&self) {
        let Some(host) = &self.host else { return };
        let tap = host.tap();
        if !tap.is_live() {
            return;
        }
        let forwarder: oc_core::bus::Listener = forward_public_events(tap);
        std::mem::forget(self.engine.bus.listen(forwarder));
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
                            // ref session.ts:271 — unconditional
                            // `projects.resolve`: the caller-resolved
                            // project for the location (identity by
                            // project id, not the directory-hash
                            // fallback).
                            project: Some(oc_core::session::ResolvedProject {
                                id: services.project.id.clone(),
                                directory: services
                                    .project
                                    .canonical
                                    .to_string_lossy()
                                    .into_owned(),
                            }),
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
                // G-107: subpath relative to the destination project root
                // (ref move.ts:89).
                let subpath = std::path::Path::new(directory)
                    .strip_prefix(&destination_services.project.directory)
                    .ok()
                    .map(|rest| rest.to_string_lossy().replace('\\', "/"))
                    .filter(|rest| !rest.is_empty())
                    .map(oc_schema::common::RelativePath);
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine
                    .move_session(
                        &self.location,
                        &id,
                        &message_id,
                        destination,
                        project,
                        subpath,
                        // G-108①: steer default (ref move.ts:127).
                        oc_schema::session_inbox::Delivery::Steer,
                    )
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
            oc_core::engine::commands::CommandSource::Mcp { server, prompt, arguments } => {
                // G-215 — the plugin port mirrors the HTTP handler's MCP
                // prompt execution (ref CommandPlugin execute).
                let parsed = oc_core::config::command_md::parse_arguments(text);
                let mut args = std::collections::BTreeMap::new();
                for (index, argument) in arguments.iter().enumerate() {
                    args.insert(argument.clone(), parsed.get(index).cloned().unwrap_or_default());
                }
                let result = services
                    .mcp
                    .prompt(&server, &prompt, Some(args))
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("MCP prompt not found: {server}:{prompt}"))?;
                let expanded =
                    crate::handlers::session::mcp_prompt_message_text(&result);
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine.prompt(&self.location, &id, &message_id, &expanded).await?;
                Ok(Value::Null)
            }
            oc_core::engine::commands::CommandSource::Config(command) => {
                // G-214: `` !`cmd` `` shell blocks interpolate AFTER
                // $N/$ARGUMENTS (cwd = location directory,
                // config-priority shell).
                let expanded = oc_core::config::command_md::evaluate_template_shell(
                    &command.template,
                    text,
                    &self.location.as_path().to_path_buf(),
                    services.commands.config_shell().as_deref(),
                )
                .await?;
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

/// The bus listener body behind [`EngineSessionPort::attach_event_tap`]
/// (PLUG-D13): one public event → one [`oc_plugin_host::TapEvent`] (the
/// envelope id/type/data/created wire shape the dispatch pump forwards
/// into the guest's subscriptions).
fn forward_public_events(tap: oc_plugin_host::BroadcastTap) -> oc_core::bus::Listener {
    Arc::new(move |event: &oc_core::bus::Event| {
        if crate::sse::is_public_event(&event.type_) {
            tap.publish(oc_plugin_host::TapEvent {
                id: event.id.as_str().to_owned(),
                typ: event.type_.clone(),
                data: event.data.clone(),
                created: Some(event.created),
            });
        }
        Ok(())
    })
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
    /// Fill the slot (idempotent; the first engine wins). The fill is
    /// the first moment the engine exists — the PLUG-D13 event-source
    /// tap attaches here (v2 wires `ctx.event.subscribe` onto the
    /// process bus at host construction; the engine boots after).
    pub fn fill(&self, port: Arc<EngineSessionPort>) {
        let mut slot = self.0.write().expect("session slot poisoned");
        if slot.is_none() {
            port.attach_event_tap();
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

    use super::plugin_source_diff;
    use oc_plugin_host::PluginOp;

    fn ops(values: &[(&str, bool)]) -> Vec<PluginOp> {
        values
            .iter()
            .map(|&(target, add)| {
                if add {
                    PluginOp::Add { target: target.to_owned(), options: serde_json::Value::Null }
                } else {
                    PluginOp::Remove { target: target.to_owned() }
                }
            })
            .collect()
    }

    /// G-05: the supervisor resolve diff (add/remove with the port's
    /// remove-selector semantics — exact/package/stem + `*`) over
    /// previously-loaded targets. (The ref supervisor.ts:26-28 also
    /// accepts `name.*` prefix selectors; the port's `resolve` does
    /// not — the diff mirrors the port.)
    #[test]
    fn plugin_source_diff_adds_and_removes_with_selectors() {
        let previous = vec!["@scope/one".to_owned(), "local/two".to_owned()];
        // No change -> empty diff.
        let (added, removed) =
            plugin_source_diff(&previous, &ops(&[("@scope/one", true), ("local/two", true)]));
        assert_eq!(added, Vec::<String>::new());
        assert_eq!(removed, Vec::<String>::new());
        // A new drop-in appears.
        let (added, removed) = plugin_source_diff(
            &previous,
            &ops(&[("@scope/one", true), ("local/two", true), ("local/three", true)]),
        );
        assert_eq!(added, vec!["local/three".to_owned()]);
        assert_eq!(removed, Vec::<String>::new());
        // `name.*` selector removes the matching package generation
        // (the port's remove reduction: exact / package / stem, plus
        // `*` — resolve.rs `remove_matches`).
        let (added, removed) =
            plugin_source_diff(&previous, &ops(&[("local/two", true), ("one", false)]));
        assert_eq!(added, Vec::<String>::new());
        assert_eq!(removed, vec!["@scope/one".to_owned()]);
        // `*` clears everything.
        let (_, removed) = plugin_source_diff(&previous, &ops(&[("*", false)]));
        assert_eq!(removed, vec!["@scope/one".to_owned(), "local/two".to_owned()]);
    }
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

// ---------------------------------------------------------------------------
// The permission port (PLUG-D11: ctx.permission.* → the engine)
// ---------------------------------------------------------------------------

/// The engine-backed [`oc_plugin_host::PermissionPort`]: v2's
/// `PermissionDomain` — `list`/`get`/`reply` over THIS location's
/// permission machinery (the same `Permission` service the tool
/// pipeline's asks park on).
pub struct EnginePermissionPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
}

impl EnginePermissionPort {
    /// The pending requests for one session, serialized in the v2
    /// `Permission.Request` wire shape.
    fn pending(&self, session_id: &str) -> Vec<Value> {
        self.engine
            .permission_pending(&self.location, &oc_schema::id::SessionId(session_id.to_owned()))
            .into_iter()
            .map(|request| serde_json::to_value(&request).unwrap_or(Value::Null))
            .collect()
    }

    /// One pending request (v2: unknown id or wrong session →
    /// `Permission request not found`).
    fn one(&self, session_id: &str, request_id: &str) -> Result<Value, String> {
        self.pending(session_id)
            .into_iter()
            .find(|request| request.get("id").and_then(Value::as_str) == Some(request_id))
            .ok_or_else(|| format!("Permission request not found: {request_id}"))
    }
}

#[async_trait]
impl oc_plugin_host::PermissionPort for EnginePermissionPort {
    async fn list(&self, session_id: &str) -> Result<Value, String> {
        Ok(Value::Array(self.pending(session_id)))
    }

    async fn get(&self, session_id: &str, request_id: &str) -> Result<Value, String> {
        self.one(session_id, request_id)
    }

    async fn reply(
        &self,
        session_id: &str,
        request_id: &str,
        decision: &str,
        message: Option<String>,
    ) -> Result<(), String> {
        // v2 validates the request exists AND belongs to the session
        // before replying (host.ts permission.reply).
        self.one(session_id, request_id)?;
        let reply = match decision {
            "once" => oc_schema::permission::Reply::Once,
            "always" => oc_schema::permission::Reply::Always,
            "reject" => oc_schema::permission::Reply::Reject,
            other => return Err(format!("Invalid permission decision: {other}")),
        };
        self.engine
            .permission_reply(
                &self.location,
                oc_core::permission::ReplyInput {
                    request_id: oc_schema::id::PermissionId(request_id.to_owned()),
                    reply,
                    message,
                },
            )
            .await
    }
}

/// The pre-engine permission-port placeholder (the same slot pattern
/// as [`EngineSessionSlot`]): empty until `boot_and_serve` fills it.
#[derive(Default)]
pub struct EnginePermissionSlot(std::sync::RwLock<Option<Arc<EnginePermissionPort>>>);

impl EnginePermissionSlot {
    /// Fill the slot (idempotent; the first engine wins).
    pub fn fill(&self, port: Arc<EnginePermissionPort>) {
        let mut slot = self.0.write().expect("permission slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::PermissionPort for EnginePermissionSlot {
    async fn list(&self, session_id: &str) -> Result<Value, String> {
        self.port()?.list(session_id).await
    }

    async fn get(&self, session_id: &str, request_id: &str) -> Result<Value, String> {
        self.port()?.get(session_id, request_id).await
    }

    async fn reply(
        &self,
        session_id: &str,
        request_id: &str,
        decision: &str,
        message: Option<String>,
    ) -> Result<(), String> {
        self.port()?.reply(session_id, request_id, decision, message).await
    }
}

impl EnginePermissionSlot {
    fn port(&self) -> Result<Arc<EnginePermissionPort>, String> {
        self.0.read().expect("permission slot poisoned").clone().ok_or_else(|| {
            "permission: the engine is not attached to the plugin host yet".to_owned()
        })
    }
}

/// The engine-backed [`oc_plugin_host::WebsearchPort`] (G-24 —
/// `ctx.websearch.providers/query` over the boot location's WebSearch
/// service).
pub struct EngineWebsearchPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
}

#[async_trait]
impl oc_plugin_host::WebsearchPort for EngineWebsearchPort {
    async fn providers(&self) -> Result<Value, String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(
            services
                .websearch
                .providers()
                .into_iter()
                .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
                .collect::<Vec<_>>(),
        )
        .unwrap_or(Value::Null))
    }

    async fn query(&self, query: &str, provider_id: Option<&str>) -> Result<Value, String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        let response = services
            .websearch
            .query(query, None, provider_id)
            .await
            .map_err(|error| error.to_string())?;
        serde_json::to_value(&response).map_err(|error| error.to_string())
    }

    /// G-24 residual — `editor.default.get`: the persisted selection.
    async fn default_provider(&self) -> Result<Value, String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        Ok(services.websearch.selection_json().await)
    }

    /// G-24 residual — `editor.default.set(selection)`: persisted
    /// through the engine KV (`WebSearch.select`, the same store the
    /// ref's auth/config selection lands in).
    async fn set_default(&self, selection: Value) -> Result<(), String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        let parsed = match &selection {
            Value::String(id) if id == "random" => oc_core::tool::websearch::Selection::Random,
            Value::Bool(false) => oc_core::tool::websearch::Selection::Disabled,
            Value::String(id) => oc_core::tool::websearch::Selection::Provider(id.clone()),
            other => {
                return Err(format!(
                "websearch default selection must be a provider id, \"random\", or false: {other}"
            ))
            }
        };
        services.websearch.select(&parsed).await;
        Ok(())
    }
}

/// The pre-engine websearch-port placeholder (the same slot pattern).
#[derive(Default)]
pub struct EngineWebsearchSlot(std::sync::RwLock<Option<Arc<EngineWebsearchPort>>>);

impl EngineWebsearchSlot {
    pub fn fill(&self, port: Arc<EngineWebsearchPort>) {
        let mut slot = self.0.write().expect("websearch slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::WebsearchPort for EngineWebsearchSlot {
    async fn providers(&self) -> Result<Value, String> {
        self.port()?.providers().await
    }

    async fn query(&self, query: &str, provider_id: Option<&str>) -> Result<Value, String> {
        self.port()?.query(query, provider_id).await
    }
}

impl EngineWebsearchSlot {
    fn port(&self) -> Result<Arc<EngineWebsearchPort>, String> {
        self.0.read().expect("websearch slot poisoned").clone().ok_or_else(|| {
            "websearch: the engine is not attached to the plugin host yet".to_owned()
        })
    }
}

/// The engine-backed [`oc_plugin_host::VcsPort`] (G-24 —
/// `ctx.vcs.get/base` over the boot location's resolved project; the
/// git adapter's branch/base shapes mirror handlers/project.rs's
/// vcs_get/vcs_base).
pub struct EngineVcsPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
}

impl EngineVcsPort {
    fn git_output(directory: &std::path::Path, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(directory)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[async_trait]
impl oc_plugin_host::VcsPort for EngineVcsPort {
    async fn get(&self) -> Result<Value, String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        let Some(vcs) = &services.project.vcs else { return Ok(Value::Null) };
        let scope = services.project.canonical.as_path();
        // The hg adapter's literal default branch (handlers/project.rs
        // hg_branch); git reads the checked-out ref.
        let (current, default) = if vcs.kind() == "hg" {
            (
                Self::git_output(scope, &["branch"]).map(|o| o.trim().to_owned()),
                "default".to_owned(),
            )
        } else {
            (
                Self::git_output(scope, &["rev-parse", "--abbrev-ref", "HEAD"])
                    .map(|o| o.trim().to_owned()),
                "main".to_owned(),
            )
        };
        Ok(serde_json::json!({
            "branch": { "current": current.unwrap_or_default(), "default": default },
            "provider": vcs.kind(),
        }))
    }

    async fn base(&self) -> Result<Value, String> {
        let services =
            self.engine.location(self.location.clone()).await.map_err(|e| e.to_string())?;
        if services.project.vcs.as_ref().is_some_and(|vcs| vcs.kind() == "hg") {
            // The hg adapter exposes no base op (vcs.ts:198-201 → null).
            return Ok(Value::Null);
        }
        let scope = services.project.canonical.as_path();
        let branch = Self::git_output(scope, &["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|o| o.trim().to_owned());
        // vcs.base (plugin/vcs/git.ts:286-317): the reflog's
        // "branch: Created from <base>" first, the default-branch
        // fallback second, `null` when neither resolves.
        if let Some(branch) = branch.as_deref() {
            if let Some(reflog) =
                Self::git_output(scope, &["reflog", "show", "-1", "--format=%gs", branch])
            {
                if let Some(base) = reflog.trim().strip_prefix("branch: Created from ") {
                    return Ok(serde_json::json!(base));
                }
            }
        }
        for candidate in ["main", "master"] {
            if Self::git_output(scope, &["rev-parse", "--verify", candidate]).is_some() {
                return Ok(serde_json::json!(candidate));
            }
        }
        Ok(Value::Null)
    }
}

/// The pre-engine vcs-port placeholder (the same slot pattern).
#[derive(Default)]
pub struct EngineVcsSlot(std::sync::RwLock<Option<Arc<EngineVcsPort>>>);

impl EngineVcsSlot {
    pub fn fill(&self, port: Arc<EngineVcsPort>) {
        let mut slot = self.0.write().expect("vcs slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::VcsPort for EngineVcsSlot {
    async fn get(&self) -> Result<Value, String> {
        self.port()?.get().await
    }

    async fn base(&self) -> Result<Value, String> {
        self.port()?.base().await
    }
}

impl EngineVcsSlot {
    fn port(&self) -> Result<Arc<EngineVcsPort>, String> {
        self.0
            .read()
            .expect("vcs slot poisoned")
            .clone()
            .ok_or_else(|| "vcs: the engine is not attached to the plugin host yet".to_owned())
    }
}

/// The engine-backed [`oc_plugin_host::ProviderPort`] (G-64 —
/// `ctx.provider.list/get/reload` over the live provider catalog).
pub struct EngineProviderPort {
    pub engine: Arc<Engine>,
}

impl EngineProviderPort {
    fn snapshot_json(snapshot: &oc_core::provider_catalog::catalog::Snapshot) -> Value {
        serde_json::to_value(&snapshot.info).unwrap_or(Value::Null)
    }
}

#[async_trait]
impl oc_plugin_host::ProviderPort for EngineProviderPort {
    async fn list(&self) -> Result<Value, String> {
        let catalog = self.engine.catalog_service.get();
        Ok(serde_json::to_value(
            catalog.providers().iter().map(Self::snapshot_json).collect::<Vec<_>>(),
        )
        .unwrap_or(Value::Null))
    }

    async fn get(&self, id: &str) -> Result<Value, String> {
        let catalog = self.engine.catalog_service.get();
        Ok(catalog.provider(id).map(Self::snapshot_json).unwrap_or(Value::Null))
    }

    async fn reload(&self) -> Result<(), String> {
        // `provider.reload` (ref providers.reload): a fresh catalog
        // refresh; a failure surfaces (the next location assembly
        // reads the live service catalog either way).
        self.engine.catalog_service.refresh(true).await.map(|_| ()).map_err(|e| e)
    }
}

/// The pre-engine provider-port placeholder (the slot pattern).
#[derive(Default)]
pub struct EngineProviderSlot(std::sync::RwLock<Option<Arc<EngineProviderPort>>>);

impl EngineProviderSlot {
    pub fn fill(&self, port: Arc<EngineProviderPort>) {
        let mut slot = self.0.write().expect("provider slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::ProviderPort for EngineProviderSlot {
    async fn list(&self) -> Result<Value, String> {
        self.port()?.list().await
    }

    async fn get(&self, id: &str) -> Result<Value, String> {
        self.port()?.get(id).await
    }

    async fn reload(&self) -> Result<(), String> {
        self.port()?.reload().await
    }
}

impl EngineProviderSlot {
    fn port(&self) -> Result<Arc<EngineProviderPort>, String> {
        self.0
            .read()
            .expect("provider slot poisoned")
            .clone()
            .ok_or_else(|| "provider: the engine is not attached to the plugin host yet".to_owned())
    }
}

/// The engine-backed [`oc_plugin_host::ModelPort`] (G-147 —
/// `ctx.model.list/default/reload` over the boot location's model
/// catalog: the same `location_model_entries` ordering `/api/model`
/// serves, and the configured default with the available-first
/// fallback `/api/model/default` takes).
pub struct EngineModelPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
}

#[async_trait]
impl oc_plugin_host::ModelPort for EngineModelPort {
    async fn list(&self, provider_id: Option<&str>) -> Result<Value, String> {
        let entries =
            crate::handlers::model::location_model_entries(&self.engine, &self.location).await;
        let filtered = match provider_id {
            Some(provider) => entries
                .into_iter()
                .filter(|entry| entry.get("providerID").and_then(Value::as_str) == Some(provider))
                .collect(),
            None => entries,
        };
        Ok(serde_json::to_value(filtered).unwrap_or(Value::Null))
    }

    async fn default_model(&self) -> Result<Value, String> {
        let configured = crate::boot::location_default_model(&self.engine, &self.location);
        if let Some(selection) = configured {
            return Ok(serde_json::to_value(&selection).unwrap_or(Value::Null));
        }
        // `/api/model/default`'s available-first fallback
        // (`available.find(supportsText)`, model.ts:260-266).
        let entries =
            crate::handlers::model::location_model_entries(&self.engine, &self.location).await;
        Ok(entries
            .into_iter()
            .find(|entry| entry.get("id").and_then(Value::as_str).is_some())
            .map(|entry| {
                serde_json::json!({
                    "providerID": entry.get("providerID").cloned().unwrap_or(Value::Null),
                    "modelID": entry.get("id").cloned().unwrap_or(Value::Null),
                })
            })
            .unwrap_or(Value::Null))
    }

    async fn reload(&self) -> Result<(), String> {
        // `model.reload` — the catalog refresh `provider.reload` drives
        // (the model rows derive from it).
        self.engine.catalog_service.refresh(true).await.map(|_| ()).map_err(|e| e)
    }
}

/// The pre-engine model-port placeholder (the slot pattern).
#[derive(Default)]
pub struct EngineModelSlot(std::sync::RwLock<Option<Arc<EngineModelPort>>>);

impl EngineModelSlot {
    pub fn fill(&self, port: Arc<EngineModelPort>) {
        let mut slot = self.0.write().expect("model slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::ModelPort for EngineModelSlot {
    async fn list(&self, provider_id: Option<&str>) -> Result<Value, String> {
        self.port()?.list(provider_id).await
    }

    async fn default_model(&self) -> Result<Value, String> {
        self.port()?.default_model().await
    }

    async fn reload(&self) -> Result<(), String> {
        self.port()?.reload().await
    }
}

impl EngineModelSlot {
    fn port(&self) -> Result<Arc<EngineModelPort>, String> {
        self.0
            .read()
            .expect("model slot poisoned")
            .clone()
            .ok_or_else(|| "model: the engine is not attached to the plugin host yet".to_owned())
    }
}

/// The engine-backed [`oc_plugin_host::ReferencePort`] (G-64 —
/// `ctx.reference.list/reload` over the engine's `ReferenceService`).
pub struct EngineReferencePort {
    pub engine: Arc<Engine>,
}

#[async_trait]
impl oc_plugin_host::ReferencePort for EngineReferencePort {
    async fn list(&self) -> Result<Value, String> {
        Ok(serde_json::to_value(
            self.engine
                .references
                .list()
                .into_iter()
                .map(|info| {
                    serde_json::json!({
                        "name": info.name,
                        "path": info.path.display().to_string(),
                        "description": info.description,
                        "hidden": info.hidden,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or(Value::Null))
    }

    async fn reload(&self) -> Result<(), String> {
        // `reference.reload` (ref reference.ts:104-108): refresh every
        // source (a forked refresh-all).
        self.engine.references.refresh_all();
        Ok(())
    }
}

/// The pre-engine reference-port placeholder.
#[derive(Default)]
pub struct EngineReferenceSlot(std::sync::RwLock<Option<Arc<EngineReferencePort>>>);

impl EngineReferenceSlot {
    pub fn fill(&self, port: Arc<EngineReferencePort>) {
        let mut slot = self.0.write().expect("reference slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::ReferencePort for EngineReferenceSlot {
    async fn list(&self) -> Result<Value, String> {
        self.port()?.list().await
    }

    async fn reload(&self) -> Result<(), String> {
        self.port()?.reload().await
    }
}

impl EngineReferenceSlot {
    fn port(&self) -> Result<Arc<EngineReferencePort>, String> {
        self.0.read().expect("reference slot poisoned").clone().ok_or_else(|| {
            "reference: the engine is not attached to the plugin host yet".to_owned()
        })
    }
}

/// The persistent-pty-backed [`oc_plugin_host::TerminalPort`] (G-64 —
/// `ctx.experimental.terminal.read(sessionID, lines)` over the
/// frontend's session terminal).
pub struct EngineTerminalPort {
    pub pty: oc_core::persistent_pty::frontend::Frontend,
}

#[async_trait]
impl oc_plugin_host::TerminalPort for EngineTerminalPort {
    async fn read(&self, session_id: &str, lines: Option<u32>) -> Result<Value, String> {
        let session_id = oc_schema::id::SessionId::parse(session_id).map_err(|e| e.to_string())?;
        let result = self.pty.read(&session_id, lines).await.map_err(|error| error.to_string())?;
        Ok(result
            .map(|read| {
                serde_json::json!({
                    "id": read.pty_id,
                    "title": read.title,
                    "cwd": read.cwd,
                    "foregroundProcess": read.foreground_process,
                    "text": read.screen.text,
                    "cursor": { "x": read.screen.cursor.0, "y": read.screen.cursor.1 },
                })
            })
            .unwrap_or(Value::Null))
    }
}

/// The pre-engine terminal-port placeholder.
#[derive(Default)]
pub struct EngineTerminalSlot(std::sync::RwLock<Option<Arc<EngineTerminalPort>>>);

impl EngineTerminalSlot {
    pub fn fill(&self, port: Arc<EngineTerminalPort>) {
        let mut slot = self.0.write().expect("terminal slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::TerminalPort for EngineTerminalSlot {
    async fn read(&self, session_id: &str, lines: Option<u32>) -> Result<Value, String> {
        self.port()?.read(session_id, lines).await
    }
}

impl EngineTerminalSlot {
    fn port(&self) -> Result<Arc<EngineTerminalPort>, String> {
        self.0.read().expect("terminal slot poisoned").clone().ok_or_else(|| {
            "experimental.terminal: the terminal service is not attached to the plugin host yet"
                .to_owned()
        })
    }
}

/// The engine-backed [`oc_plugin_host::IntegrationPort`] (G-64 — the
/// ref host.ts:274-357 method channel over the engine's
/// `IntegrationHost`; JSON args map onto the typed surface).
pub struct EngineIntegrationPort {
    pub engine: Arc<Engine>,
}

#[async_trait]
impl oc_plugin_host::IntegrationPort for EngineIntegrationPort {
    async fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        use oc_core::integration::types::{
            ConnectionInfo, ConnectionStatus, FormAnswer, IntegrationError,
        };
        let host = self.engine.integration.clone();
        let field =
            |key: &str| args.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        let answer: FormAnswer = args
            .get("answer")
            .and_then(Value::as_object)
            .map(|answer| answer.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
            .unwrap_or_default();
        let label = args.get("label").and_then(Value::as_str).map(str::to_owned);
        let err = |error: IntegrationError| error.to_string();
        match method {
            "list" => {
                let infos = host.list().await.map_err(|e| e.to_string())?;
                serde_json::to_value(&infos).map_err(|e| e.to_string())
            }
            "get" => {
                let integration_id = field("integrationID");
                if integration_id.is_empty() {
                    return Err("integration.get requires an integrationID".into());
                }
                let info = host.get(&integration_id).await.map_err(|e| e.to_string())?;
                serde_json::to_value(&info).map_err(|e| e.to_string())
            }
            "connect.key" => {
                let integration_id = field("integrationID");
                let key = field("key");
                if integration_id.is_empty() || key.is_empty() {
                    return Err("integration.connect.key requires integrationID and key".into());
                }
                host.connect_key(&integration_id, &key, &answer, label.as_deref())
                    .await
                    .map_err(err)?;
                Ok(Value::Null)
            }
            "oauth.connect" => {
                let integration_id = field("integrationID");
                let method_id = field("methodID");
                if integration_id.is_empty() || method_id.is_empty() {
                    return Err(
                        "integration.oauth.connect requires integrationID and methodID".into()
                    );
                }
                let attempt = host
                    .oauth_connect(&integration_id, &method_id, &answer, label.as_deref())
                    .await
                    .map_err(err)?;
                serde_json::to_value(&attempt).map_err(|e| e.to_string())
            }
            "oauth.status" => {
                let status = host
                    .oauth_status(&field("integrationID"), &field("attemptID"))
                    .await
                    .map_err(err)?;
                serde_json::to_value(&status).map_err(|e| e.to_string())
            }
            "oauth.complete" => {
                let code = args.get("code").and_then(Value::as_str).map(str::to_owned);
                host.oauth_complete(&field("integrationID"), &field("attemptID"), code.as_deref())
                    .await
                    .map_err(err)?;
                Ok(Value::Null)
            }
            "oauth.cancel" => {
                host.oauth_cancel(&field("integrationID"), &field("attemptID")).await;
                Ok(Value::Null)
            }
            "command.connect" => {
                let integration_id = field("integrationID");
                let method_id = field("methodID");
                if integration_id.is_empty() || method_id.is_empty() {
                    return Err(
                        "integration.command.connect requires integrationID and methodID".into()
                    );
                }
                let attempt = host
                    .command_connect(&integration_id, &method_id, label.as_deref())
                    .await
                    .map_err(err)?;
                serde_json::to_value(&attempt).map_err(|e| e.to_string())
            }
            "command.status" => {
                let status = host
                    .command_status(&field("integrationID"), &field("attemptID"))
                    .await
                    .map_err(err)?;
                serde_json::to_value(&status).map_err(|e| e.to_string())
            }
            "command.cancel" => {
                host.command_cancel(&field("integrationID"), &field("attemptID")).await;
                Ok(Value::Null)
            }
            "connection.active" => {
                let id = field("id");
                if id.is_empty() {
                    return Err("integration.connection.active requires an id".into());
                }
                let connection = host.connection_active(&id).await.map_err(|e| e.to_string())?;
                serde_json::to_value(&connection).map_err(|e| e.to_string())
            }
            "connection.resolve" => {
                let connection: ConnectionInfo =
                    serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
                let value = host.connection_resolve(&connection).await.map_err(err)?;
                serde_json::to_value(&value).map_err(|e| e.to_string())
            }
            "connection.status" => {
                let integration_id = field("integrationID");
                if integration_id.is_empty() {
                    return Err("integration.connection.status requires an integrationID".into());
                }
                let connection: ConnectionInfo = args
                    .get("connection")
                    .cloned()
                    .ok_or_else(|| "integration.connection.status requires a connection".to_owned())
                    .and_then(|value| serde_json::from_value(value).map_err(|e| e.to_string()))?;
                let status = match args.get("status") {
                    Some(Value::Object(map)) => Some(
                        serde_json::from_value::<ConnectionStatus>(Value::Object(map.clone()))
                            .map_err(|e| e.to_string())?,
                    ),
                    _ => None,
                };
                host.connection_status(&integration_id, &connection, status).await;
                Ok(Value::Null)
            }
            other => Err(format!("unknown integration method {other:?}")),
        }
    }
}

/// The pre-engine integration-port placeholder.
#[derive(Default)]
pub struct EngineIntegrationSlot(std::sync::RwLock<Option<Arc<EngineIntegrationPort>>>);

impl EngineIntegrationSlot {
    pub fn fill(&self, port: Arc<EngineIntegrationPort>) {
        let mut slot = self.0.write().expect("integration slot poisoned");
        if slot.is_none() {
            *slot = Some(port);
        }
    }
}

#[async_trait]
impl oc_plugin_host::IntegrationPort for EngineIntegrationSlot {
    async fn call(&self, method: &str, args: &Value) -> Result<Value, String> {
        self.port()?.call(method, args).await
    }
}

impl EngineIntegrationSlot {
    fn port(&self) -> Result<Arc<EngineIntegrationPort>, String> {
        self.0.read().expect("integration slot poisoned").clone().ok_or_else(|| {
            "integration: the engine is not attached to the plugin host yet".to_owned()
        })
    }
}

/// G-24 — build the server's shared worktree-strategy registry from the
/// plugin host's snapshot (git + every plugin-registered
/// `ctx.worktree.transform` strategy, guest-dispatched).
pub fn plugin_worktree_registry(
    host: &Arc<PluginHost>,
    resolution: &Arc<parking_lot::Mutex<Resolution>>,
) -> std::sync::Arc<oc_core::project::worktree_strategy::StrategyRegistry> {
    let source = HostPluginSource { host: Arc::clone(host), resolution: Arc::clone(resolution) };
    let mut registry = oc_core::project::worktree_strategy::StrategyRegistry::new();
    if let Some(dispatcher) = source.hook_dispatcher() {
        let defs = source.worktree_strategies();
        if !defs.is_empty() {
            oc_core::plugin_apply::apply_plugin_worktree_strategies(
                &mut registry,
                &dispatcher,
                &defs,
            );
        }
    }
    std::sync::Arc::new(registry)
}
