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
use oc_core::location::{LocationRef, ToolExecute, ToolRunFailure, ToolRunResult};
use oc_core::plugin_apply::{LocationPluginSource, PluginToolDef};
use oc_core::tool::codemode::{CallRecord, CodeModeRuntime, RunOutcome, ToolCallFn};
use oc_plugin_host::registrations::PluginState;
use oc_plugin_host::{
    Bridge, CodeModeToolSpec, HostState, PluginHost, PluginHostConfig, Resolution, ResolvedPlugin,
    SessionPort, ToolPort,
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
/// Blocking: the registration pass parks the caller up to
/// [`READY_TIMEOUT`] — `boot_and_serve` runs this inside
/// `spawn_blocking`.
#[must_use]
pub fn boot(
    ops: &[oc_plugin_host::PluginOp],
    loaded: &Loaded,
    home: &std::path::Path,
    directory: &std::path::Path,
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

    let config = PluginHostConfig {
        host_state,
        session_port: Some(Arc::clone(&session_slot) as Arc<dyn SessionPort>),
        tool_port: Some(Arc::new(RouterToolPort { router: Arc::clone(&router) })),
        entry_options: resolution.entry_options(),
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
#[must_use]
pub fn config_plugin_ops(loaded: &Loaded) -> Vec<oc_plugin_host::PluginOp> {
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
        Ok(ops) => ops,
        Err(error) => {
            eprintln!("opencode plugin: config plugins[] rejected: {error}");
            Vec::new()
        }
    }
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
// The session port (ctx.session.* → the real engine)
// ---------------------------------------------------------------------------

/// The engine-backed session port: `ctx.session.*` calls from plugin
/// code route into the boot location's session machinery (prompt
/// admits through the inbox exactly like `POST /api/session/:id/prompt`).
/// Calls are answered asynchronously off the host's IO reader (deadlock
/// rule 1), so a prompt may dispatch hooks back into the guest through
/// the bridge while the reader keeps draining.
pub struct EngineSessionPort {
    pub engine: Arc<Engine>,
    pub location: LocationRef,
}

#[async_trait]
impl SessionPort for EngineSessionPort {
    async fn call(&self, _bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String> {
        let session_id = args.get("sessionID").and_then(Value::as_str).unwrap_or_default();
        match method {
            "prompt" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let message_id = format!("msg_plugin_{}", uuid::Uuid::new_v4());
                self.engine.prompt(&self.location, &id, &message_id, text).await?;
                Ok(Value::Null)
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
            "messages" => {
                let rows = self
                    .engine
                    .messages(
                        &self.location,
                        oc_core::session::store::MessagesInput {
                            session_id: session_id.to_owned(),
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
                Ok(json!({ "data": data }))
            }
            "interrupt" => {
                let id = oc_schema::id::SessionId(session_id.to_owned());
                let interrupted = self.engine.interrupt(&self.location, &id).await;
                Ok(json!(interrupted))
            }
            other => Err(format!(
                "session.{other}: not wired in the Rust server port yet (engine seam: prompt/get/list/messages/interrupt)"
            )),
        }
    }
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
