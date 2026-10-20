//! Plugin-agent application seam (wave 2, flagship bring-up).
//!
//! `oc-plugin-host` materializes the guest's `ctx.agent.transform`
//! result into its registration snapshot as v2 `Agent.Info` JSON.
//! This module applies those definitions into the oc-core
//! [`AgentRegistry`] — the additive seam the engine/TUI layers (and
//! the bring-up test) call after the host's registration pass. The
//! host crate stays dependency-free; the parse lives on the core side
//! where the schema types are.
//!
//! The wave-12 production seam: [`LocationPluginSource`] is the
//! trait the ENGINE's per-location assembly consults — the embedder
//! (oc-server's boot) implements it over the booted plugin host, and
//! every location's graph applies the plugin agents + tool
//! registrations natively (registry rows + executors; the synthesized
//! `execute` surface then hides the code-mode-eligible ones exactly
//! like MCP tools).

use crate::hooks::{HookError, HookHandler, HookOptions, HookRegistry};
use crate::location::{AgentRegistry, ToolExecute, ToolRegistryState};
use oc_schema::id::AgentId;
use serde_json::Value;
use std::sync::Arc;

/// Apply plugin-agent `Agent.Info` JSON documents into `registry`
/// (`AgentRegistry::set` — insert or replace). Unparseable entries are
/// skipped (a defective plugin never blocks the rest); the count of
/// applied agents is returned.
#[must_use]
pub fn apply_plugin_agents(registry: &AgentRegistry, agents: &[Value]) -> usize {
    let mut applied = 0usize;
    for info in agents {
        match serde_json::from_value::<oc_schema::agent::Info>(info.clone()) {
            Ok(parsed) => {
                registry.set(parsed);
                applied += 1;
            }
            Err(error) => {
                eprintln!(
                    "opencode plugin-apply: skipping malformed agent {:?}: {error}",
                    info.get("id").and_then(serde_json::Value::as_str)
                );
            }
        }
    }
    applied
}

/// One plugin-transformed agent as the engine sees it (the snapshot's
/// `PluginAgent` mirrored: v2 `Agent.Info` JSON + the introducing
/// plugin's id for per-location activation filtering).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginAgentDef {
    pub plugin_id: String,
    /// v2 `Agent.Info` JSON.
    pub info: Value,
}

/// Apply the plugin table's default agent (`draft.default(id)`):
/// `AgentRegistry::set_default`. `None` clears it.
pub fn apply_plugin_agent_default(registry: &AgentRegistry, default: Option<&str>) {
    registry.set_default(default.filter(|id| !id.is_empty()).map(AgentId::parse));
}

/// One plugin-registered tool as the engine sees it (the registration
/// snapshot's `PluginTool` mirrored; oc-plugin-host stays dep-light so
/// the definition crosses as plain data).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginToolDef {
    /// Owning plugin id (PLUG-D07 per-location activation filtering).
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    /// JSON-Schema input.
    pub input: Value,
    /// `options.codemode !== false` (default true).
    pub codemode: bool,
    /// `options.namespace`.
    pub namespace: Option<String>,
}

/// The per-location plugin source the engine's `assemble` consults
/// (the wave-12 production seam): the embedder's implementation is
/// backed by the booted plugin host's registration snapshot. The
/// engine calls it once per location graph build, AFTER the native
/// registrations and BEFORE the Code Mode surface (so the snapshot
/// shaping sees the plugin tools); it must be cheap and synchronous
/// (a resolved snapshot read, never an IPC round-trip — the host's
/// registration pass settles before boot flips ready).
pub trait LocationPluginSource: Send + Sync {
    /// v2 `Agent.Info` JSON documents + provenance (applied via
    /// [`apply_plugin_agents`]; the plugin id drives PLUG-D07's
    /// per-location activation filtering).
    fn agents(&self) -> Vec<PluginAgentDef> {
        Vec::new()
    }

    /// The plugin table's default agent id, when set.
    fn default_agent(&self) -> Option<String> {
        None
    }

    /// Plugin-registered tools (rows + executor pairs).
    fn tools(&self) -> Vec<PluginToolDef> {
        Vec::new()
    }

    /// Plugin-declared MCP servers (`ctx.mcp.transform` — PLUG-ENGINE-
    /// MERGE: v2 registers them IN the server process, host.ts:374-403,
    /// so a headless server sees them too; the TUI wire seam of
    /// PLUG-D33 stays as an idempotent re-push over the same rows).
    fn mcps(&self) -> Vec<PluginMcpDef> {
        Vec::new()
    }

    /// Plugin-registered skills (PLUG-D35): v2 `Skill.Info` JSON
    /// documents from the host snapshot's `ctx.skill.transform`
    /// materializations — applied via
    /// [`apply_plugin_skills`] into the location's
    /// [`crate::engine::skills::ConfigSkillTable`] (insert-or-replace;
    /// the plugin entry wins an id collision, matching v2's
    /// plugins-after-config activation order).
    fn skills(&self) -> Vec<PluginSkillDef> {
        Vec::new()
    }

    /// Plugin-registered commands (`command.add` registrations: name +
    /// description; execution dispatches back into the guest through
    /// the host's `command.execute` — CD-34's unsurfaced surface).
    fn commands(&self) -> Vec<PluginCommandDef> {
        Vec::new()
    }

    /// The executor dispatching one plugin tool by name (guest
    /// `tool.execute`, timeout-bounded by the implementation).
    fn tool_executor(&self, name: &str) -> Option<Arc<dyn ToolExecute>> {
        let _ = name;
        None
    }

    /// Guest hook subscriptions (the PLUG-D21 seam): each lands in the
    /// location's [`HookRegistry`] behind a forwarder that crosses the
    /// process boundary through [`GuestHookDispatcher`]. Registration
    /// order is the snapshot's order (v2 fires in registration order).
    fn hooks(&self) -> Vec<PluginHookDef> {
        Vec::new()
    }

    /// The dispatch target guest hook forwarders call (the plugin
    /// host's bridge). `None` = no guest hooks wire up.
    fn hook_dispatcher(&self) -> Option<Arc<dyn GuestHookDispatcher>> {
        None
    }

    /// PLUG-D07 — the per-location activation set: v2 activates plugins
    /// PER location (`plugin/supervisor.ts` is a location node; each
    /// location's config layers resolve that location's plugin
    /// operations). `ops` are THIS location's resolved operations; the
    /// implementation reduces them (dedup/removes) and maps the
    /// surviving targets onto the loaded plugins' ids. `None` (the
    /// default) = process-global: every registration applies, the
    /// pre-D07 behavior for embedders that don't implement it.
    fn active_plugins(&self, ops: &[LocationPluginOp]) -> Option<Vec<String>> {
        let _ = ops;
        None
    }

    /// PLUG-D07 — one location's assembly completed with plugins
    /// active (the embedder updates the guest's `ctx.location` view to
    /// the latest activation — the one-guest approximation of v2's
    /// per-instance hosts). Default: no-op.
    fn location_activated(&self, directory: &str, project_id: &str) {
        let _ = (directory, project_id);
    }
}

/// One config plugin operation as the engine sees it (PLUG-D07 — the
/// location-scoped mirror of the boot pass's ops; oc-plugin-host stays
/// dep-light so the directive crosses as plain data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationPluginOp {
    /// Add a package (spec string or `{package}`).
    Add { target: String },
    /// Remove a previously added target (`-id` / `-*`).
    Remove { target: String },
}

impl LocationPluginOp {
    /// Extract the location's plugin operations from a loaded config
    /// (every document's `plugins` array, priority order — the same
    /// normalization the boot pass applies; discovery drop-ins are the
    /// boot pass's, a location can only add/remove what the process
    /// already knows how to load).
    #[must_use]
    pub fn from_loaded(loaded: &crate::config::Loaded) -> Vec<Self> {
        let mut ops = Vec::new();
        for entry in &loaded.entries {
            let oc_schema::config::Entry::Document(document) = entry else { continue };
            let Some(plugins) = &document.info.plugins else { continue };
            for plugin in plugins {
                match plugin {
                    oc_schema::config::Plugin::Spec(spec) => {
                        if let Some(target) = spec.strip_prefix('-') {
                            if target.is_empty() {
                                continue;
                            }
                            ops.push(Self::Remove { target: target.to_owned() });
                        } else {
                            ops.push(Self::Add { target: spec.clone() });
                        }
                    }
                    oc_schema::config::Plugin::Entry { package, .. } => {
                        ops.push(Self::Add { target: package.clone() });
                    }
                }
            }
        }
        ops
    }
}

/// One plugin-registered command as the engine sees it (the host
/// snapshot's `PluginCommand` mirrored; oc-plugin-host stays dep-light).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommandDef {
    /// Owning plugin id (PLUG-D07 per-location activation filtering).
    pub plugin_id: String,
    pub name: String,
    pub description: String,
}

/// One plugin-registered skill as the engine sees it (the snapshot's
/// `PluginSkill` mirrored: v2 `Skill.Info` JSON + provenance).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginSkillDef {
    pub plugin_id: String,
    /// v2 `Skill.Info` JSON.
    pub info: Value,
}

/// One plugin-declared MCP server as the engine sees it (the host
/// snapshot's `PluginMcp` mirrored; oc-plugin-host stays dep-light —
/// the config crosses as the v2 `Mcp.ServerConfig` JSON the guest's
/// editor produced, and [`apply_location_plugins`] re-validates it
/// against the schema exactly like the ref's
/// `Schema.decodeUnknownSync(ServerConfig)` at host.ts:387-397).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMcpDef {
    /// Owning plugin id (PLUG-D07 per-location activation filtering).
    pub plugin_id: String,
    pub name: String,
    /// v2 `Mcp.ServerConfig` JSON (`{type: "local"|"remote", ...}`).
    pub config: Value,
}

/// One guest hook subscription as the engine sees it: the host
/// snapshot's `PluginHook` mirrored (plugin id + `domain.name` wire
/// key). The registration order is preserved (v2 fires callbacks in
/// registration order, `hooks.ts:89`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHookDef {
    pub plugin_id: String,
    /// `session` | `tool` | `shell` (the guest-bridge domains).
    pub domain: String,
    /// e.g. `prompt`, `execute.before`, `create.before`.
    pub name: String,
}

impl PluginHookDef {
    /// The `domain.name` wire key.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}.{}", self.domain, self.name)
    }
}

/// The engine-facing dispatch target for guest hooks: one call forwards
/// one serialized event to ONE plugin's handlers in the guest process
/// and returns the possibly-mutated event (the copy-in/copy-out
/// documented divergence, ADR-0005). The embedder implements this over
/// the plugin host's `Bridge::dispatch_hook`.
#[async_trait::async_trait]
pub trait GuestHookDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        plugin_id: &str,
        domain: &str,
        name: &str,
        event: Value,
    ) -> Result<Value, String>;
}

/// Typed ↔ v2-JSON codec pair for one hook event (the
/// process-isolation boundary's serialization seam): `encode` mirrors
/// the v2 event shape (camelCase wire fields), `decode` parses the
/// guest's possibly-mutated copy back.
pub type HookCodec<E> = (fn(&E) -> Value, fn(Value) -> Option<E>);

/// The [`HookHandler`] forwarder: serialize, dispatch to the guest,
/// merge the mutated copy back. A guest failure fails the dispatch
/// (v2 trigger semantics — later handlers are skipped).
struct GuestHookHandler<E> {
    dispatcher: Arc<dyn GuestHookDispatcher>,
    plugin_id: String,
    domain: &'static str,
    name: &'static str,
    codec: HookCodec<E>,
}

#[async_trait::async_trait]
impl<E: Send + 'static> HookHandler<E> for GuestHookHandler<E> {
    async fn call(&self, event: &mut E) -> Result<(), HookError> {
        let (encode, decode) = self.codec;
        let json = encode(event);
        match self.dispatcher.dispatch(&self.plugin_id, self.domain, self.name, json).await {
            Ok(updated) => {
                if let Some(parsed) = decode(updated) {
                    *event = parsed;
                }
                Ok(())
            }
            Err(message) => Err(HookError::Handler(message)),
        }
    }
}

/// Register one guest hook subscription into `hooks` (registration
/// order preserved; the caller appends in snapshot order).
async fn register_guest_hook(
    hooks: &Arc<HookRegistry>,
    definition: &PluginHookDef,
    dispatcher: &Arc<dyn GuestHookDispatcher>,
) -> bool {
    macro_rules! point {
        ($register:ident, $domain:literal, $name:literal, $encode:ident, $decode:ident) => {{
            // Registration disposal is Drop-based; the guest hook lives
            // for the location's lifetime (the same `mem::forget`
            // precedent the engine's own internal hook registrations
            // use — unload/reload rebuilds the whole graph).
            let registration = hooks
                .$register(
                    Arc::new(GuestHookHandler {
                        dispatcher: Arc::clone(dispatcher),
                        plugin_id: definition.plugin_id.clone(),
                        domain: $domain,
                        name: $name,
                        codec: (wire::$encode, wire::$decode),
                    }),
                    HookOptions::default(),
                )
                .await;
            std::mem::forget(registration);
        }};
    }
    match definition.key().as_str() {
        "session.prompt" => {
            point!(register_session_prompt, "session", "prompt", prompt_to_json, prompt_from_json);
        }
        "session.context" => {
            point!(
                register_session_context,
                "session",
                "context",
                context_to_json,
                context_from_json
            );
        }
        "session.compaction" => point!(
            register_session_compaction,
            "session",
            "compaction",
            compaction_to_json,
            compaction_from_json
        ),
        "session.generate" => {
            point!(
                register_session_generate,
                "session",
                "generate",
                context_to_json,
                context_from_json
            );
        }
        "session.title" => {
            point!(register_session_title, "session", "title", title_to_json, title_from_json);
        }
        "session.model.request" => point!(
            register_session_model_request,
            "session",
            "model.request",
            model_request_to_json,
            model_request_from_json
        ),
        "session.retry" => {
            point!(register_session_retry, "session", "retry", retry_to_json, retry_from_json);
        }
        "tool.execute.before" => point!(
            register_tool_execute_before,
            "tool",
            "execute.before",
            tool_before_to_json,
            tool_before_from_json
        ),
        "tool.execute.after" => point!(
            register_tool_execute_after,
            "tool",
            "execute.after",
            tool_after_to_json,
            tool_after_from_json
        ),
        "shell.create.before" => point!(
            register_shell_create_before,
            "shell",
            "create.before",
            shell_before_to_json,
            shell_before_from_json
        ),
        // http.request/http.response + the experimental ws.* points have
        // no engine dispatch site yet (M-02/C7a lanes own the transport
        // sites); permission/aisdk are not guest-bridge domains.
        other => {
            eprintln!(
                "opencode plugin-apply: skipping guest hook {other} (no production dispatch site yet)"
            );
            return false;
        }
    }
    true
}

/// Apply plugin-registered skills (PLUG-D35): each `Skill.Info` JSON
/// document merges into the location's
/// [`crate::engine::skills::ConfigSkillTable`] (insert-or-replace by
/// id). Unparseable entries are skipped (contained, like the agents
/// arm); the applied count is returned.
#[must_use]
pub fn apply_plugin_skills(
    table: &crate::engine::skills::ConfigSkillTable,
    skills: &[Value],
) -> usize {
    let mut applied = 0usize;
    for info in skills {
        match serde_json::from_value::<oc_schema::skill::Skill>(info.clone()) {
            Ok(parsed) => {
                table.merge_plugin_skill(parsed);
                applied += 1;
            }
            Err(error) => {
                eprintln!(
                    "opencode plugin-apply: skipping malformed skill {:?}: {error}",
                    info.get("id").and_then(serde_json::Value::as_str)
                );
            }
        }
    }
    applied
}

/// Apply one plugin source into a freshly built location graph's
/// registries (the `assemble` step): the plugin-declared MCP servers
/// into the location's [`McpRegistry`] FIRST (PLUG-ENGINE-MERGE —
/// v2's `ctx.mcp.transform` registers them in the server process,
/// host.ts:374-403; here each lands as a runtime override through
/// `McpRegistry::add`, insert-or-replace, exactly like the wire-side
/// `experimental.mcp.add`, so the async connections start earliest
/// and the tool bridge + Code Mode surface below see the entries),
/// then agents + default, the tool rows + executors, the plugin
/// command registrations into the location's command table, and the
/// guest hook subscriptions into the location's [`HookRegistry`] (the
/// PLUG-D21 seam: every engine dispatch site — `tool.execute.before`
/// in the tool pipeline, `session.*` in the model-request preparation
/// — reaches guest handlers through the bridge forwarder). Returns
/// the applied agent count.
#[allow(clippy::too_many_arguments)] // the assembly's registry set + the location's ops
pub async fn apply_location_plugins(
    source: &dyn LocationPluginSource,
    hooks: &Arc<HookRegistry>,
    agents: &Arc<AgentRegistry>,
    tools: &Arc<ToolRegistryState>,
    commands: Option<&Arc<crate::engine::commands::CommandTable>>,
    mcp: &crate::mcp::McpRegistry,
    skills: &Arc<crate::engine::skills::ConfigSkillTable>,
    ops: &[LocationPluginOp],
) -> usize {
    // PLUG-D07: v2 activates plugins per location — registrations from
    // plugins NOT active for THIS location's resolved operations stay
    // out of its graph (hooks never dispatch, tools/MCPs/agents/skills/
    // commands never register).
    let active = source.active_plugins(ops);
    let is_active =
        |plugin_id: &str| active.as_ref().is_none_or(|ids| ids.iter().any(|id| id == plugin_id));
    // The MCP merge runs over THIS location's registry (per-location
    // overlays, like every other arm below). Idempotent by
    // construction: `add` is insert-or-replace and the reconcile diff
    // no-ops an unchanged entry — a TUI attach re-pushing the same
    // declaration over the wire (PLUG-D33) lands on identical rows.
    // An unparseable config declines THIS server only (contained,
    // logged) — never the whole pass.
    for definition in source.mcps() {
        if !is_active(&definition.plugin_id) {
            continue;
        }
        match serde_json::from_value::<oc_schema::mcp::ServerConfig>(definition.config.clone()) {
            Ok(config) => mcp.add(&definition.name, config).await,
            Err(error) => eprintln!(
                "opencode plugin-apply: skipping malformed mcp config {:?}: {error}",
                definition.name
            ),
        }
    }
    let snapshot_agents: Vec<Value> = source
        .agents()
        .into_iter()
        .filter(|definition| is_active(&definition.plugin_id))
        .map(|definition| definition.info)
        .collect();
    let applied =
        if snapshot_agents.is_empty() { 0 } else { apply_plugin_agents(agents, &snapshot_agents) };
    // The table-global default agent applies while any plugin is active
    // for this location (provenance of the LAST draft.default setter is
    // not tracked separately).
    if active.as_ref().is_none_or(|ids| !ids.is_empty()) {
        apply_plugin_agent_default(agents, source.default_agent().as_deref());
    }
    // PLUG-D35: plugin skills merge into the location's skill table
    // (empty vec = a no-op read, never a lock round-trip).
    let snapshot_skills: Vec<Value> = source
        .skills()
        .into_iter()
        .filter(|definition| is_active(&definition.plugin_id))
        .map(|definition| definition.info)
        .collect();
    if !snapshot_skills.is_empty() {
        let _ = apply_plugin_skills(skills, &snapshot_skills);
    }
    for definition in source.tools() {
        if !is_active(&definition.plugin_id) {
            continue;
        }
        let mut registration = crate::location::ToolRegistration::new(
            &definition.name,
            definition.description.clone(),
            definition.input.clone(),
        );
        registration.codemode = definition.codemode;
        registration.namespace.clone_from(&definition.namespace);
        tools.register(registration);
        if let Some(executor) = source.tool_executor(&definition.name) {
            tools.register_executor(definition.name.clone(), executor);
        }
    }
    if let Some(commands) = commands {
        for definition in source.commands() {
            if !is_active(&definition.plugin_id) {
                continue;
            }
            commands.register_plugin(&definition.name, &definition.description);
        }
    }
    if let Some(dispatcher) = source.hook_dispatcher() {
        for definition in source.hooks() {
            if !is_active(&definition.plugin_id) {
                continue;
            }
            register_guest_hook(hooks, &definition, &dispatcher).await;
        }
    }
    applied
}

// ---------------------------------------------------------------------------
// Guest-hook wire codecs (typed events ↔ v2 JSON, ADR-0005 copy semantics)
// ---------------------------------------------------------------------------

mod wire {
    //! Mirror structs for the v2 hook event shapes (camelCase wire
    //! fields; `session.prompt` per `session/prompt.ts:40-49`, the tool
    //! events per `tool.ts:101-152`, `shell.create.before` per
    //! `shell.ts:263-272`, the request-shaped session events per
    //! `session/model-request.ts`).

    use crate::hooks::{
        CompactionResult, RetryDecision, SessionCompactionEvent, SessionContextEvent,
        SessionModelRequestEvent, SessionPromptEvent, SessionRequestEvent, SessionRetryEvent,
        SessionTitleEvent, ShellCreateBeforeEvent, ToolDescriptor, ToolExecuteAfterEvent,
        ToolExecuteBeforeEvent, ToolOutcome,
    };
    use oc_ai::schema::messages::{Message, SystemPart};
    use oc_schema::id::{AgentId, MessageId, SessionId};
    use oc_schema::model::Ref as ModelRef;
    use oc_schema::prompt::Prompt;
    use oc_schema::session_inbox::Delivery;
    use oc_schema::tool::ToolError;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize)]
    pub struct PromptEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(rename = "messageID")]
        pub message_id: MessageId,
        pub prompt: Prompt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub metadata: Option<serde_json::Map<String, Value>>,
        pub delivery: Delivery,
    }

    /// The request-shaped draft (`{sessionID, model, system, messages,
    /// options}` + `agent`/`tools` on context-shaped events).
    #[derive(Serialize, Deserialize)]
    pub struct ContextEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<AgentId>,
        pub model: ModelRef,
        pub system: Vec<SystemPart>,
        pub messages: Vec<Message>,
        pub options: serde_json::Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub tools: Option<BTreeMap<String, DescriptorWire>>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct DescriptorWire {
        pub description: String,
        pub input: Value,
    }

    #[derive(Serialize, Deserialize)]
    pub struct CompactionResultWire {
        pub summary: String,
        #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
        pub provider_state: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub metadata: Option<serde_json::Map<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub tokens: Option<Value>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TitleEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        pub model: ModelRef,
        pub system: Vec<SystemPart>,
        pub messages: Vec<Message>,
        pub options: serde_json::Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub result: Option<String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ModelRequestEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<AgentId>,
        pub model: ModelRef,
        pub kind: String,
        #[serde(rename = "baseURL", default, skip_serializing_if = "Option::is_none")]
        pub base_url: Option<String>,
        #[serde(default)]
        pub headers: BTreeMap<String, String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RetryDecisionWire {
        pub retry: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub delay: Option<u64>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RetryEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<AgentId>,
        pub model: ModelRef,
        pub kind: String,
        pub error: Value,
        pub attempt: u32,
        pub decision: RetryDecisionWire,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ToolBeforeEvent {
        pub tool: String,
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        pub agent: AgentId,
        #[serde(rename = "messageID")]
        pub message_id: MessageId,
        pub id: String,
        pub input: Value,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ToolAfterEvent {
        pub tool: String,
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        pub agent: AgentId,
        #[serde(rename = "messageID")]
        pub message_id: MessageId,
        pub id: String,
        pub input: Value,
        /// `completed` | `error`.
        pub status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub result: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub error: Option<ToolError>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ShellBeforeEvent {
        pub command: String,
        pub cwd: String,
        #[serde(default)]
        pub timeout: u64,
        pub shell: String,
        #[serde(default)]
        pub env: BTreeMap<String, Option<String>>,
    }

    // -- typed ↔ wire conversions ------------------------------------------

    fn encode<T: Serialize>(value: &T) -> Value {
        serde_json::to_value(value).unwrap_or(Value::Null)
    }

    fn request_parts(
        event: &SessionRequestEvent,
    ) -> (SessionId, ModelRef, Vec<SystemPart>, Vec<Message>, serde_json::Map<String, Value>) {
        (
            event.session_id.clone(),
            event.model.clone(),
            event.system.clone(),
            event.messages.clone(),
            event.options.clone(),
        )
    }

    impl From<SessionPromptEvent> for PromptEvent {
        fn from(event: SessionPromptEvent) -> Self {
            Self {
                session_id: event.session_id,
                message_id: event.message_id,
                prompt: event.prompt,
                metadata: event.metadata,
                delivery: event.delivery,
            }
        }
    }

    impl From<PromptEvent> for SessionPromptEvent {
        fn from(wire: PromptEvent) -> Self {
            Self {
                session_id: wire.session_id,
                message_id: wire.message_id,
                prompt: wire.prompt,
                metadata: wire.metadata,
                delivery: wire.delivery,
            }
        }
    }

    pub fn prompt_to_json(event: &SessionPromptEvent) -> Value {
        encode(&PromptEvent::from(event.clone()))
    }

    pub fn prompt_from_json(json: Value) -> Option<SessionPromptEvent> {
        serde_json::from_value::<PromptEvent>(json).ok().map(SessionPromptEvent::from)
    }

    fn context_wire(event: &SessionContextEvent) -> ContextEvent {
        ContextEvent {
            session_id: event.session_id.clone(),
            agent: Some(event.agent.clone()),
            model: event.model.clone(),
            system: event.system.clone(),
            messages: event.messages.clone(),
            options: event.options.clone(),
            tools: Some(
                event
                    .tools
                    .iter()
                    .map(|(name, descriptor)| {
                        (
                            name.clone(),
                            DescriptorWire {
                                description: descriptor.description.clone(),
                                input: descriptor.input.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
        }
    }

    fn context_from_wire(wire: ContextEvent) -> SessionContextEvent {
        SessionContextEvent {
            session_id: wire.session_id,
            agent: wire.agent.unwrap_or_else(|| AgentId::parse("build")),
            model: wire.model,
            system: wire.system,
            messages: wire.messages,
            options: wire.options,
            tools: wire
                .tools
                .unwrap_or_default()
                .into_iter()
                .map(|(name, descriptor)| {
                    (
                        name,
                        ToolDescriptor {
                            description: descriptor.description,
                            input: descriptor.input,
                        },
                    )
                })
                .collect(),
        }
    }

    pub fn context_to_json(event: &SessionContextEvent) -> Value {
        encode(&context_wire(event))
    }

    pub fn context_from_json(json: Value) -> Option<SessionContextEvent> {
        serde_json::from_value::<ContextEvent>(json).ok().map(context_from_wire)
    }

    pub fn compaction_to_json(event: &SessionCompactionEvent) -> Value {
        let mut map = encode(&context_wire(&event.context));
        if let Some(result) = &event.result {
            let wire = CompactionResultWire {
                summary: result.summary.clone(),
                provider_state: result.provider_state.clone(),
                metadata: result.metadata.clone(),
                tokens: result.tokens.clone(),
            };
            if let Some(target) = map.as_object_mut() {
                target.insert("result".into(), encode(&wire));
            }
        }
        map
    }

    pub fn compaction_from_json(json: Value) -> Option<SessionCompactionEvent> {
        let result =
            json.as_object().and_then(|object| object.get("result").cloned()).and_then(|result| {
                serde_json::from_value::<CompactionResultWire>(result).ok().map(
                    |wire: CompactionResultWire| CompactionResult {
                        summary: wire.summary,
                        provider_state: wire.provider_state,
                        metadata: wire.metadata,
                        tokens: wire.tokens,
                    },
                )
            });
        let context = serde_json::from_value::<ContextEvent>(json).ok().map(context_from_wire)?;
        Some(SessionCompactionEvent { context, result })
    }

    pub fn title_to_json(event: &SessionTitleEvent) -> Value {
        let (session_id, model, system, messages, options) = request_parts(&event.request);
        encode(&TitleEvent {
            session_id,
            model,
            system,
            messages,
            options,
            result: event.result.clone(),
        })
    }

    pub fn title_from_json(json: Value) -> Option<SessionTitleEvent> {
        let wire = serde_json::from_value::<TitleEvent>(json).ok()?;
        Some(SessionTitleEvent {
            request: SessionRequestEvent {
                session_id: wire.session_id,
                model: wire.model,
                system: wire.system,
                messages: wire.messages,
                options: wire.options,
            },
            result: wire.result,
        })
    }

    pub fn model_request_to_json(event: &SessionModelRequestEvent) -> Value {
        encode(&ModelRequestEvent {
            session_id: event.session_id.clone(),
            agent: Some(event.agent.clone()),
            model: event.model.clone(),
            kind: event.kind.as_str().to_owned(),
            base_url: event.base_url.clone(),
            headers: event.headers.clone(),
        })
    }

    pub fn model_request_from_json(json: Value) -> Option<SessionModelRequestEvent> {
        let wire = serde_json::from_value::<ModelRequestEvent>(json).ok()?;
        let kind = match wire.kind.as_str() {
            "compaction" => crate::hooks::SessionRequestKind::Compaction,
            "title" => crate::hooks::SessionRequestKind::Title,
            "generate" => crate::hooks::SessionRequestKind::Generate,
            _ => crate::hooks::SessionRequestKind::Primary,
        };
        Some(SessionModelRequestEvent {
            session_id: wire.session_id,
            agent: wire.agent.unwrap_or_else(|| AgentId::parse("build")),
            model: wire.model,
            kind,
            base_url: wire.base_url,
            headers: wire.headers,
        })
    }

    pub fn retry_to_json(event: &SessionRetryEvent) -> Value {
        encode(&RetryEvent {
            session_id: event.session_id.clone(),
            agent: Some(event.agent.clone()),
            model: event.model.clone(),
            kind: "primary".to_owned(),
            error: event.error.clone(),
            attempt: event.attempt,
            decision: match event.decision {
                RetryDecision::NoRetry => RetryDecisionWire { retry: false, delay: None },
                RetryDecision::Retry { delay_ms } => {
                    RetryDecisionWire { retry: true, delay: Some(delay_ms) }
                }
            },
        })
    }

    pub fn retry_from_json(json: Value) -> Option<SessionRetryEvent> {
        let wire = serde_json::from_value::<RetryEvent>(json).ok()?;
        let decision = if wire.decision.retry {
            RetryDecision::Retry { delay_ms: wire.decision.delay.unwrap_or(0) }
        } else {
            RetryDecision::NoRetry
        };
        Some(SessionRetryEvent {
            session_id: wire.session_id,
            agent: wire.agent.unwrap_or_else(|| AgentId::parse("build")),
            model: wire.model,
            error: wire.error,
            attempt: wire.attempt,
            decision,
        })
    }

    pub fn tool_before_to_json(event: &ToolExecuteBeforeEvent) -> Value {
        encode(&ToolBeforeEvent {
            tool: event.tool.clone(),
            session_id: event.session_id.clone(),
            agent: event.agent.clone(),
            message_id: event.message_id.clone(),
            id: event.call_id.clone(),
            input: event.input.clone(),
        })
    }

    pub fn tool_before_from_json(json: Value) -> Option<ToolExecuteBeforeEvent> {
        let wire = serde_json::from_value::<ToolBeforeEvent>(json).ok()?;
        Some(ToolExecuteBeforeEvent {
            tool: wire.tool,
            session_id: wire.session_id,
            agent: wire.agent,
            message_id: wire.message_id,
            call_id: wire.id,
            input: wire.input,
        })
    }

    pub fn tool_after_to_json(event: &ToolExecuteAfterEvent) -> Value {
        // v2's completed result is `{output?, content, metadata?}` — the
        // Rust dispatch site already carries that JSON shape verbatim.
        let (status, result, error) = match &event.outcome {
            ToolOutcome::Completed { result } => ("completed", Some(result.clone()), None),
            ToolOutcome::Error { error } => ("error", None, Some(error.clone())),
        };
        encode(&ToolAfterEvent {
            tool: event.tool.clone(),
            session_id: event.session_id.clone(),
            agent: event.agent.clone(),
            message_id: event.message_id.clone(),
            id: event.call_id.clone(),
            input: event.input.clone(),
            status: status.to_owned(),
            result,
            error,
        })
    }

    pub fn tool_after_from_json(json: Value) -> Option<ToolExecuteAfterEvent> {
        let wire = serde_json::from_value::<ToolAfterEvent>(json).ok()?;
        let outcome = if wire.status == "error" {
            ToolOutcome::Error {
                error: wire.error.unwrap_or(ToolError {
                    message: "tool failed".into(),
                    error: None,
                    metadata: None,
                }),
            }
        } else {
            ToolOutcome::Completed { result: wire.result.unwrap_or(Value::Null) }
        };
        Some(ToolExecuteAfterEvent {
            tool: wire.tool,
            session_id: wire.session_id,
            agent: wire.agent,
            message_id: wire.message_id,
            call_id: wire.id,
            input: wire.input,
            outcome,
        })
    }

    pub fn shell_before_to_json(event: &ShellCreateBeforeEvent) -> Value {
        encode(&ShellBeforeEvent {
            command: event.command.clone(),
            cwd: event.cwd.clone(),
            timeout: event.timeout_ms,
            shell: event.shell.clone(),
            env: event.env.clone(),
        })
    }

    pub fn shell_before_from_json(json: Value) -> Option<ShellCreateBeforeEvent> {
        let wire = serde_json::from_value::<ShellBeforeEvent>(json).ok()?;
        Some(ShellCreateBeforeEvent {
            command: wire.command,
            cwd: wire.cwd,
            timeout_ms: wire.timeout,
            shell: wire.shell,
            env: wire.env,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orchestrator() -> serde_json::Value {
        serde_json::json!({
            "id": "orchestrator",
            "name": "Orchestrator",
            "mode": "primary",
            "hidden": false,
            "model": { "id": "glm-5.3", "providerID": "own" },
            "request": { "settings": {}, "headers": {}, "body": {} },
            "permissions": [{ "action": "*", "resource": "*", "effect": "allow" }]
        })
    }

    #[test]
    fn applies_parseable_agents_and_skips_garbage() {
        let registry = AgentRegistry::new();
        let applied =
            apply_plugin_agents(&registry, &[orchestrator(), serde_json::json!({ "nope": 1 })]);
        assert_eq!(applied, 1);
        let info = registry.get(&AgentId::parse("orchestrator")).expect("orchestrator applied");
        assert_eq!(info.model.as_ref().expect("model").id.0, "glm-5.3");
        assert_eq!(info.model.as_ref().expect("model").provider_id.0, "own");
    }

    #[test]
    fn default_agent_round_trips() {
        let registry = AgentRegistry::new();
        // Two selectable agents: the configured default wins while
        // set; clearing it falls back to the first selectable.
        let applied =
            apply_plugin_agents(&registry, &[orchestrator(), agent("reporter", "primary", false)]);
        assert_eq!(applied, 2);
        apply_plugin_agent_default(&registry, Some("reporter"));
        let selected = registry.selected_default().expect("selectable default");
        assert_eq!(selected.id, AgentId::parse("reporter"));
        apply_plugin_agent_default(&registry, None);
        let fallback = registry.selected_default().expect("fallback default");
        assert_eq!(fallback.id, AgentId::parse("orchestrator"));
    }

    fn agent(id: &str, mode: &str, hidden: bool) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": id,
            "mode": mode,
            "hidden": hidden,
            "request": { "settings": {}, "headers": {}, "body": {} },
            "permissions": []
        })
    }

    // -- PLUG-ENGINE-MERGE (the mcps arm) ----------------------------------

    /// A location registry over an in-memory database whose connector
    /// fails fast: the ROW is the seam under test (listing + override
    /// semantics), so no real server process ever spawns; the failing
    /// connection settles the entry async and never blocks the apply.
    async fn test_mcp_registry() -> crate::mcp::McpRegistry {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let connector: crate::mcp::Connector = Arc::new(|input: crate::mcp::ConnectInput<'_>| {
            let server = input.server.to_string();
            Box::pin(async move {
                Err(crate::mcp::ConnectFailure::Connect(crate::mcp::ConnectError {
                    server,
                    message: "test connector".to_owned(),
                }))
            })
        });
        crate::mcp::McpRegistry::with_connector(bus, connector, Arc::new(crate::mcp::NoCredentials))
            .with_directory("/tmp")
    }

    /// A source declaring only MCP servers (the arm under test).
    struct McpOnlySource(Vec<PluginMcpDef>);

    impl LocationPluginSource for McpOnlySource {
        fn mcps(&self) -> Vec<PluginMcpDef> {
            self.0.clone()
        }
    }

    fn local_mcp(name: &str) -> PluginMcpDef {
        PluginMcpDef {
            plugin_id: "plugindemo".to_owned(),
            name: name.to_owned(),
            config: serde_json::json!({
                "type": "local",
                "command": ["cat"],
                "environment": { "OC_PLUGIN_MCP": "1" },
            }),
        }
    }

    /// The plugin-declared servers land in THIS location's registry;
    /// a malformed config declines its own server only (contained).
    #[tokio::test]
    async fn plugin_mcps_merge_into_the_location_registry() {
        let registry = test_mcp_registry().await;
        let source = McpOnlySource(vec![
            local_mcp("plugindemo"),
            PluginMcpDef {
                plugin_id: "plugindemo".to_owned(),
                name: "remote-demo".to_owned(),
                config: serde_json::json!({ "type": "remote", "url": "https://example.invalid" }),
            },
            PluginMcpDef {
                plugin_id: "plugindemo".to_owned(),
                name: "broken".to_owned(),
                config: serde_json::json!({"type": "?"}),
            },
        ]);
        let hooks = Arc::new(HookRegistry::new());
        let agents = Arc::new(AgentRegistry::new());
        let tools = Arc::new(ToolRegistryState::new());
        let skills_table = Arc::new(crate::engine::skills::ConfigSkillTable::empty());
        apply_location_plugins(
            &source,
            &hooks,
            &agents,
            &tools,
            None,
            &registry,
            &skills_table,
            &[],
        )
        .await;
        let names: Vec<String> = registry.servers().into_iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"plugindemo".to_owned()) && names.contains(&"remote-demo".to_owned()),
            "both valid declarations listed: {names:?}"
        );
        assert!(!names.contains(&"broken".to_owned()), "malformed declined containedly: {names:?}");
    }

    /// The merge is idempotent: re-applying the same declarations (a
    /// location rebuild, or the TUI seam's wire re-push over the same
    /// rows — PLUG-D33) leaves exactly one row per server.
    #[tokio::test]
    async fn plugin_mcp_merge_is_idempotent() {
        let registry = test_mcp_registry().await;
        let source = McpOnlySource(vec![local_mcp("plugindemo")]);
        let hooks = Arc::new(HookRegistry::new());
        let agents = Arc::new(AgentRegistry::new());
        let tools = Arc::new(ToolRegistryState::new());
        let skills_table = Arc::new(crate::engine::skills::ConfigSkillTable::empty());
        apply_location_plugins(
            &source,
            &hooks,
            &agents,
            &tools,
            None,
            &registry,
            &skills_table,
            &[],
        )
        .await;
        apply_location_plugins(
            &source,
            &hooks,
            &agents,
            &tools,
            None,
            &registry,
            &skills_table,
            &[],
        )
        .await;
        let names: Vec<String> = registry.servers().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["plugindemo".to_owned()], "insert-or-replace, no duplicates");
    }
}

#[cfg(test)]
mod per_location_tests {
    //! PLUG-D07 — per-location activation filtering over a provenance-
    //! carrying source (v2 activates plugins per location; the graph of
    //! a location whose config did not activate a plugin never sees its
    //! registrations).

    use super::*;

    struct MultiPluginSource;

    impl LocationPluginSource for MultiPluginSource {
        fn agents(&self) -> Vec<PluginAgentDef> {
            vec![
                PluginAgentDef { plugin_id: "alpha".into(), info: agent("alpha-agent") },
                PluginAgentDef { plugin_id: "beta".into(), info: agent("beta-agent") },
            ]
        }

        fn tools(&self) -> Vec<PluginToolDef> {
            vec![
                PluginToolDef {
                    plugin_id: "alpha".into(),
                    name: "alpha_tool".into(),
                    description: String::new(),
                    input: serde_json::json!({ "type": "object" }),
                    codemode: true,
                    namespace: None,
                },
                PluginToolDef {
                    plugin_id: "beta".into(),
                    name: "beta_tool".into(),
                    description: String::new(),
                    input: serde_json::json!({ "type": "object" }),
                    codemode: true,
                    namespace: None,
                },
            ]
        }

        fn skills(&self) -> Vec<PluginSkillDef> {
            vec![PluginSkillDef {
                plugin_id: "beta".into(),
                info: serde_json::json!({
                    "id": "beta-skill", "name": "Beta",
                    "path": "/beta/SKILL.md", "content": "c",
                }),
            }]
        }

        /// alpha active only when its op survived the reduction.
        fn active_plugins(&self, ops: &[LocationPluginOp]) -> Option<Vec<String>> {
            let mut active: Vec<String> = Vec::new();
            for op in ops {
                match op {
                    LocationPluginOp::Add { target } => active.push(target.clone()),
                    LocationPluginOp::Remove { target } => {
                        active.retain(|id| id != target);
                    }
                }
            }
            Some(active)
        }
    }

    fn agent(id: &str) -> Value {
        serde_json::json!({
            "id": id, "name": id, "mode": "primary", "hidden": false,
            "request": { "settings": {}, "headers": {}, "body": {} }, "permissions": [],
        })
    }

    async fn table() -> (
        Arc<HookRegistry>,
        Arc<AgentRegistry>,
        Arc<ToolRegistryState>,
        Arc<crate::engine::skills::ConfigSkillTable>,
        crate::mcp::McpRegistry,
    ) {
        let database = crate::database::Database::open_in_memory().await.expect("database");
        let bus = crate::bus::Bus::new(database, crate::bus::BusOptions::deterministic());
        let connector: crate::mcp::Connector = Arc::new(|input: crate::mcp::ConnectInput<'_>| {
            let server = input.server.to_string();
            Box::pin(async move {
                Err(crate::mcp::ConnectFailure::Connect(crate::mcp::ConnectError {
                    server,
                    message: "test connector".to_owned(),
                }))
            })
        });
        let registry = crate::mcp::McpRegistry::with_connector(
            bus,
            connector,
            Arc::new(crate::mcp::NoCredentials),
        )
        .with_directory("/tmp");
        (
            Arc::new(HookRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(ToolRegistryState::new()),
            Arc::new(crate::engine::skills::ConfigSkillTable::empty()),
            registry,
        )
    }

    /// A location that activated only alpha gets alpha's registrations;
    /// beta's stay out (agent rows, tools, skills).
    #[tokio::test]
    async fn inactive_plugin_registrations_stay_out() {
        let (hooks, agents, tools, skills, mcp) = table().await;
        apply_location_plugins(
            &MultiPluginSource,
            &hooks,
            &agents,
            &tools,
            None,
            &mcp,
            &skills,
            &[LocationPluginOp::Add { target: "alpha".to_owned() }],
        )
        .await;
        assert!(agents.get(&AgentId::parse("alpha-agent")).is_some(), "alpha applied");
        assert!(agents.get(&AgentId::parse("beta-agent")).is_none(), "beta filtered");
        assert!(tools.get("alpha_tool").is_some(), "alpha tool row registered");
        assert!(tools.get("beta_tool").is_none(), "beta tool filtered");
        assert!(
            crate::tool::skill::SkillTable::get(skills.as_ref(), "beta-skill").is_none(),
            "beta skill filtered"
        );
    }

    /// The remove op drops a previously added plugin (v2's ordered
    /// reduction — explicit config removes drop discovered adds).
    #[tokio::test]
    async fn remove_op_drops_the_activation() {
        let (hooks, agents, tools, skills, mcp) = table().await;
        apply_location_plugins(
            &MultiPluginSource,
            &hooks,
            &agents,
            &tools,
            None,
            &mcp,
            &skills,
            &[
                LocationPluginOp::Add { target: "alpha".to_owned() },
                LocationPluginOp::Remove { target: "alpha".to_owned() },
            ],
        )
        .await;
        assert!(agents.get(&AgentId::parse("alpha-agent")).is_none(), "removed → filtered");
    }

    /// An empty activation set (a plugin-less location) applies nothing.
    #[tokio::test]
    async fn empty_set_applies_nothing() {
        let (hooks, agents, tools, skills, mcp) = table().await;
        apply_location_plugins(
            &MultiPluginSource,
            &hooks,
            &agents,
            &tools,
            None,
            &mcp,
            &skills,
            &[],
        )
        .await;
        assert!(agents.get(&AgentId::parse("alpha-agent")).is_none());
        assert!(tools.get("alpha_tool").is_none());
    }
}
