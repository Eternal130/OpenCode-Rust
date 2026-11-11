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

    /// G-24 — plugin-registered websearch providers
    /// (`ctx.websearch.transform`'s `editor.add`, ref host.ts:489-504).
    fn websearch_providers(&self) -> Vec<PluginWebsearchProviderDef> {
        Vec::new()
    }

    /// G-64 — plugin provider definitions (`ctx.provider.transform`'s
    /// editor table, ref host.ts:196-223). Applied into every
    /// location's provider overlay by [`apply_plugin_providers`].
    fn provider_defs(&self) -> Vec<PluginProviderDef> {
        Vec::new()
    }

    /// G-64 — plugin reference sources (`ctx.reference.transform`'s
    /// editor table, ref host.ts:432-443). Applied into the engine's
    /// `ReferenceService` by [`apply_plugin_references`].
    fn reference_defs(&self) -> Vec<PluginReferenceDef> {
        Vec::new()
    }

    /// G-24 — plugin-registered worktree strategies
    /// (`ctx.worktree.transform`'s `editor.add`, ref host.ts:505-522).
    fn worktree_strategies(&self) -> Vec<PluginWorktreeStrategyDef> {
        Vec::new()
    }

    /// G-24 — plugin-registered integration methods (ref host.ts:631
    /// `methodImplementation` — `ctx.integration.method.update`).
    fn integration_methods(&self) -> Vec<PluginIntegrationMethodDef> {
        Vec::new()
    }

    /// G-384 — plugin-registered integration rows (the
    /// `ctx.integration.transform` editor table: upserts with
    /// create-if-missing name = id).
    fn integrations(&self) -> Vec<PluginIntegrationDef> {
        Vec::new()
    }

    /// G-384 — the plugin integration removal markers
    /// (`editor.remove`): the engine retracts these ids.
    fn integration_removals(&self) -> Vec<PluginIntegrationRemovalDef> {
        Vec::new()
    }

    /// G-384 — the plugin integration METHOD removal markers
    /// (`editor.method.remove`).
    fn integration_method_removals(&self) -> Vec<PluginIntegrationMethodRemovalDef> {
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

    /// G-5 — plugin-scoped state transforms over the assembled
    /// config-derived AGENT state (the ref's `ctx.agent.transform`
    /// rewrite surface: update/remove/default on the agent table).
    /// Runs in `apply_location_plugins` AFTER the additive snapshot
    /// application, in registration order, per
    /// [`crate::state_transform`] semantics (grouped failure disables
    /// the plugin's transform group; host failure aborts). Default:
    /// none.
    fn agent_state_transforms(&self) -> Vec<crate::state_transform::AgentTransform> {
        Vec::new()
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
    /// `session` | `tool` | `shell` | `permission` | `aisdk` (the
    /// guest-bridge domains).
    pub domain: String,
    /// e.g. `prompt`, `execute.before`, `create.before`.
    pub name: String,
}

/// One plugin provider definition as the engine sees it (G-64 — ref
/// host.ts:196-223 `provider.transform`'s editor table): the raw
/// definition JSON (`{id, name?, baseURL?, apiKey?, models?}`); the
/// location overlay folds it into an openai-compatible custom provider
/// (`apply_plugin_providers`).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginProviderDef {
    pub plugin_id: String,
    pub id: String,
    pub definition: Value,
}

/// One plugin reference source as the engine sees it (G-64 — ref
/// host.ts:432-443 `reference.transform`'s editor table): a
/// `Reference.Source` JSON (`{type: "local", path}` / `{type: "git",
/// repository, ref?, path?}`).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginReferenceDef {
    pub plugin_id: String,
    pub name: String,
    pub source: Value,
}

/// One plugin-registered websearch provider as the engine sees it
/// (G-24 — ref host.ts:489-504 `websearch.transform`'s
/// `editor.add({id, name, execute})`): the execute closure stays in
/// the GUEST; queries dispatch back through [`GuestHookDispatcher`]
/// (domain `websearch`, name `execute`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginWebsearchProviderDef {
    pub plugin_id: String,
    pub id: String,
    pub name: String,
}

/// One plugin-registered worktree strategy as the engine sees it
/// (G-24/G-4 — ref host.ts:505-522 `worktree.transform`'s
/// `editor.add({id, create, remove, list})`): the closures stay in
/// the GUEST; ops dispatch back through [`GuestHookDispatcher`]
/// (domain `worktree`, names `create`/`remove`/`list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginWorktreeStrategyDef {
    pub plugin_id: String,
    pub id: String,
}

/// One plugin-registered integration method (G-24 — ref host.ts:631
/// `methodImplementation`): the v2 `IntegrationMethodRegistration`
/// JSON (`{integrationID, method, authorize?, refresh?, label?}`);
/// the OAuth closures dispatch back into the guest through
/// [`GuestHookDispatcher`] (domain `integration`, names
/// `authorize`/`callback`/`refresh`).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginIntegrationMethodDef {
    pub plugin_id: String,
    /// v2 `IntegrationMethodRegistration` JSON.
    pub registration: Value,
}

/// G-384 — one plugin-registered integration row as the engine sees
/// it (the `ctx.integration.transform` editor table): an upsert of
/// `{id, name, metadata?}` into the engine's `IntegrationHost`
/// (create-if-missing with the name defaulting to the id — ref
/// integration.ts:300-308).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginIntegrationDef {
    pub plugin_id: String,
    pub id: String,
    pub name: String,
    /// `None` = leave the entry's metadata untouched (the guest row
    /// carried none); `Some(map)` sets/clears it.
    pub metadata: Option<std::collections::BTreeMap<String, Value>>,
}

/// G-384 — one integration removal marker (`editor.remove`): the
/// engine drops the whole entry (methods + implementations go with
/// it; credential rows stay owned by the store, as in the ref).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginIntegrationRemovalDef {
    pub plugin_id: String,
    pub id: String,
}

/// G-384 — one integration METHOD removal marker
/// (`editor.method.remove`): the raw `Integration.Method` JSON; the
/// identity is `Method::same_slot` (type + id).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginIntegrationMethodRemovalDef {
    pub plugin_id: String,
    pub integration_id: String,
    pub method: Value,
}

/// One plugin-registered websearch provider bridged onto
/// [`crate::tool::websearch_providers::SearchProvider`]: `execute`
/// crosses the process boundary as a `websearch.execute` guest-hook
/// dispatch (`{provider, query}` → `{results: [...]}`, 120 s class —
/// a guest search rides a network round trip).
pub struct GuestWebSearchProvider {
    dispatcher: Arc<dyn GuestHookDispatcher>,
    plugin_id: String,
    id: String,
    name: String,
}

impl GuestWebSearchProvider {
    #[must_use]
    pub fn new(
        dispatcher: Arc<dyn GuestHookDispatcher>,
        definition: &PluginWebsearchProviderDef,
    ) -> Self {
        Self {
            dispatcher,
            plugin_id: definition.plugin_id.clone(),
            id: definition.id.clone(),
            name: definition.name.clone(),
        }
    }
}

#[async_trait::async_trait]
impl crate::tool::websearch_providers::SearchProvider for GuestWebSearchProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn execute(
        &self,
        query: &str,
    ) -> Result<
        Vec<oc_schema::misc::WebSearchResult>,
        crate::tool::websearch_providers::ProviderError,
    > {
        let event = serde_json::json!({ "provider": self.id, "query": query });
        let reply = self
            .dispatcher
            .dispatch(&self.plugin_id, "websearch", "execute", event)
            .await
            .map_err(|message| crate::tool::websearch_providers::ProviderError {
                status: None,
                message,
                retry_after: None,
            })?;
        let results = reply.get("results").cloned().unwrap_or(serde_json::Value::Null);
        serde_json::from_value::<Vec<oc_schema::misc::WebSearchResult>>(results).map_err(|error| {
            crate::tool::websearch_providers::ProviderError {
                status: None,
                message: format!("guest websearch provider {}: {error}", self.id),
                retry_after: None,
            }
        })
    }
}

/// G-64 — apply plugin provider definitions (ref host.ts:196-223's
/// `provider.transform` table) into one location's provider overlay:
/// every definition with a `baseURL` becomes an openai-compatible
/// [`CustomProvider`] (insert-or-replace by id, plugins AFTER config —
/// the v2 activation order). Definitions without a `baseURL` land as
/// connection overlays when they carry an `apiKey` (the CRED-A4 fold).
pub fn apply_plugin_providers(
    overlay: &mut crate::provider_catalog::overlay::ConfigProviders,
    defs: &[PluginProviderDef],
) {
    for def in defs {
        let Some(custom) = custom_provider_from_def(&def.id, &def.definition) else {
            continue;
        };
        // apiKey-only entries (no baseURL): the connection overlay.
        if custom.base_url.is_empty() {
            if let Some(api_key) = custom.api_key.clone() {
                let entry = overlay.connections.entry(custom.id.clone()).or_default();
                entry.api_key = Some(api_key);
            }
            continue;
        }
        match overlay.custom.iter().position(|existing| existing.id == custom.id) {
            Some(at) => overlay.custom[at] = custom,
            None => overlay.custom.push(custom),
        }
    }
}

/// One definition JSON → [`CustomProvider`] (None when it carries no
/// `baseURL` and no `apiKey`). The model map folds into
/// [`facade::CustomModel`]s; unknown fields are ignored (the editor's
/// JSON is the ref's provider definition shape).
fn custom_provider_from_def(
    id: &str,
    definition: &Value,
) -> Option<crate::provider_catalog::facade::CustomProvider> {
    use crate::provider_catalog::facade::{CustomModel, CustomProvider};
    let str_field = |key: &str| {
        definition
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let base_url = str_field("baseURL").or_else(|| str_field("base_url")).unwrap_or_default();
    let api_key = str_field("apiKey").or_else(|| str_field("api_key"));
    if base_url.is_empty() && api_key.is_none() {
        return None;
    }
    let models = match definition.get("models") {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(model_id, model)| CustomModel {
                id: model_id.clone(),
                name: model.get("name").and_then(Value::as_str).map(str::to_owned),
                headers: None,
                body: None,
                model_id: model.get("modelID").and_then(Value::as_str).map(str::to_owned),
                family: model.get("family").and_then(Value::as_str).map(str::to_owned),
                compatibility: model
                    .get("compatibility")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                capabilities: model
                    .get("capabilities")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                cost: model
                    .get("cost")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                limit: model
                    .get("limit")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                settings: None,
                base_url: model
                    .get("baseURL")
                    .or_else(|| model.get("base_url"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                package: None,
                variants: Vec::new(),
            })
            .collect::<Vec<_>>(),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|model| {
                let model_id = model.get("id").and_then(Value::as_str)?.to_owned();
                Some(CustomModel {
                    id: model_id,
                    name: model.get("name").and_then(Value::as_str).map(str::to_owned),
                    headers: None,
                    body: None,
                    model_id: model.get("modelID").and_then(Value::as_str).map(str::to_owned),
                    family: model.get("family").and_then(Value::as_str).map(str::to_owned),
                    compatibility: model
                        .get("compatibility")
                        .and_then(|value| serde_json::from_value(value.clone()).ok()),
                    capabilities: model
                        .get("capabilities")
                        .and_then(|value| serde_json::from_value(value.clone()).ok()),
                    cost: model
                        .get("cost")
                        .and_then(|value| serde_json::from_value(value.clone()).ok()),
                    limit: model
                        .get("limit")
                        .and_then(|value| serde_json::from_value(value.clone()).ok()),
                    settings: None,
                    base_url: model
                        .get("baseURL")
                        .or_else(|| model.get("base_url"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    package: None,
                    variants: Vec::new(),
                })
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    Some(CustomProvider {
        id: id.to_owned(),
        name: str_field("name").unwrap_or_else(|| id.to_owned()),
        base_url,
        api_key,
        headers: definition
            .get("headers")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        body: definition.get("body").and_then(|value| serde_json::from_value(value.clone()).ok()),
        canonical: None,
        package: None,
        models,
        ..CustomProvider::default()
    })
}

/// G-64 — apply plugin reference sources (ref host.ts:432-443's
/// `reference.transform` table) into the engine's `ReferenceService`
/// (`add`/`remove` by name; insert-or-replace semantics).
pub fn apply_plugin_references(
    service: &Arc<crate::reference::ReferenceService>,
    defs: &[PluginReferenceDef],
) {
    for def in defs {
        let source = reference_source_from_json(&def.source);
        service.add(def.name.clone(), source);
    }
}

/// One `Reference.Source` JSON → [`crate::reference::Source`]
/// (`{type: "local", path}` / `{type: "git", repository, branch?}`; an
/// unknown shape degrades to a local source pointing at the raw value).
fn reference_source_from_json(source: &Value) -> crate::reference::Source {
    let str_field = |key: &str| source.get(key).and_then(Value::as_str).map(str::to_owned);
    let description = str_field("description");
    let hidden = source.get("hidden").and_then(Value::as_bool);
    match str_field("type").as_deref() {
        Some("git") => crate::reference::Source::Git(crate::reference::GitSource {
            repository: str_field("repository").unwrap_or_default(),
            branch: str_field("branch").or_else(|| str_field("ref")),
            description,
            hidden,
        }),
        _ => crate::reference::Source::Local(crate::reference::LocalSource {
            path: std::path::PathBuf::from(str_field("path").unwrap_or_default()),
            description,
            hidden,
        }),
    }
}

/// Apply plugin-registered websearch providers (G-24) into the
/// location's [`crate::tool::websearch::WebSearch`] registry
/// (insert-or-replace by id). Inactive plugins' providers stay out
/// (PLUG-D07). Returns the applied count.
pub fn apply_plugin_websearch_providers(
    websearch: &crate::tool::websearch::WebSearch,
    dispatcher: &Arc<dyn GuestHookDispatcher>,
    defs: &[PluginWebsearchProviderDef],
) -> usize {
    for definition in defs {
        websearch.add_provider(Arc::new(GuestWebSearchProvider::new(
            Arc::clone(dispatcher),
            definition,
        )));
    }
    defs.len()
}

/// Apply plugin-registered integration methods (G-24) into the
/// engine's [`crate::integration::IntegrationHost`] through
/// `update_method` (the `editor.method.update` seam,
/// integration.ts:312-340). The OAuth `authorize`/`refresh` closures
/// dispatch back into the guest; `env`/`command`/`key` methods
/// register without an implementation (the ref's methodImplementation
/// env/command arms). Malformed registrations are skipped (contained,
/// like the agents arm). Returns the applied count.
/// Monolithic port of the plugin integration-method apply pipeline;
/// helper split deferred.
#[allow(clippy::too_many_lines)]
pub fn apply_plugin_integration_methods(
    integration: &crate::integration::IntegrationHost,
    dispatcher: &Arc<dyn GuestHookDispatcher>,
    defs: &[PluginIntegrationMethodDef],
) -> usize {
    use crate::integration::types::{CredentialValue, FormAnswer, Method};
    use crate::integration::{
        AuthMode, Authorization, CodeCallback, OAuthImplementation, RefreshFn,
    };
    use futures::future::BoxFuture;
    use serde_json::Value;

    let mut applied = 0usize;
    for definition in defs {
        let Value::Object(registration) = &definition.registration else { continue };
        let Some(integration_id) = registration.get("integrationID").and_then(Value::as_str) else {
            continue;
        };
        let Some(method_value) = registration.get("method") else { continue };
        let Ok(method) = serde_json::from_value::<Method>(method_value.clone()) else {
            eprintln!(
                "opencode plugin-apply: skipping malformed integration method {integration_id:?}"
            );
            continue;
        };
        // The guest's `authorize`/`refresh` closures cannot cross the
        // boundary as JSON — the guest sends presence markers and
        // serves the calls through the `integration.*` hook dispatches
        // (`authorize` → `{url, instructions, mode, credential?}`,
        // `callback` → `{credential}`, `refresh` → `{credential}`).
        let has_authorize = registration.get("authorize").and_then(Value::as_bool).unwrap_or(false);
        let has_refresh = registration.get("refresh").and_then(Value::as_bool).unwrap_or(false);
        let authorize = has_authorize.then(|| {
            let plugin_id = definition.plugin_id.clone();
            let integration_id = integration_id.to_owned();
            let dispatcher = Arc::clone(dispatcher);
            move |answer: FormAnswer| -> BoxFuture<'static, Result<Authorization, String>> {
                let plugin_id = plugin_id.clone();
                let integration_id = integration_id.clone();
                let dispatcher = Arc::clone(&dispatcher);
                Box::pin(async move {
                    let event =
                        serde_json::json!({ "integrationID": integration_id, "answer": answer });
                    let reply =
                        dispatcher.dispatch(&plugin_id, "integration", "authorize", event).await?;
                    let Some(result) = reply.get("result") else {
                        return Err("integration authorize: no result".to_owned());
                    };
                    let mode_is_auto = result.get("mode").and_then(Value::as_str) == Some("auto");
                    // Both modes resolve the credential through a later
                    // `integration.callback` dispatch (auto omits the
                    // code, matching the ref's auto Effect).
                    let plugin_id_cb = plugin_id.clone();
                    let integration_id_cb = integration_id.clone();
                    let dispatcher_cb = Arc::clone(&dispatcher);
                    let callback = Arc::new(
                        move |code: Option<String>| -> futures::future::BoxFuture<
                            'static,
                            Result<CredentialValue, String>,
                        > {
                            let plugin_id = plugin_id_cb.clone();
                            let integration_id = integration_id_cb.clone();
                            let dispatcher = Arc::clone(&dispatcher_cb);
                            Box::pin(async move {
                                let mut event =
                                    serde_json::json!({ "integrationID": integration_id });
                                if let Some(code) = code {
                                    event["code"] = serde_json::json!(code);
                                }
                                let reply = dispatcher
                                    .dispatch(&plugin_id, "integration", "callback", event)
                                    .await?;
                                serde_json::from_value::<CredentialValue>(
                                    reply.get("credential").cloned().unwrap_or(Value::Null),
                                )
                                .map_err(|error| error.to_string())
                            })
                        },
                    )
                        as Arc<
                            dyn Fn(
                                    Option<String>,
                                ) -> futures::future::BoxFuture<
                                    'static,
                                    Result<CredentialValue, String>,
                                > + Send
                                + Sync,
                        >;
                    let mode = if mode_is_auto {
                        AuthMode::Auto(callback(None))
                    } else {
                        AuthMode::Code(
                            Arc::new(move |code: String| callback(Some(code))) as CodeCallback
                        )
                    };
                    Ok(Authorization {
                        url: result
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("about:blank")
                            .to_owned(),
                        instructions: result
                            .get("instructions")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        expires_at: None,
                        mode,
                    })
                })
            }
        });
        let refresh = has_refresh.then(|| {
            let plugin_id = definition.plugin_id.clone();
            let integration_id = integration_id.to_owned();
            let dispatcher = Arc::clone(dispatcher);
            Arc::new(
                move |value: crate::integration::types::OAuthValue| -> BoxFuture<'static, Result<CredentialValue, String>> {
                    let plugin_id = plugin_id.clone();
                    let integration_id = integration_id.clone();
                    let dispatcher = Arc::clone(&dispatcher);
                    Box::pin(async move {
                        let credential = serde_json::to_value(&value).unwrap_or(Value::Null);
                        let event =
                            serde_json::json!({ "integrationID": integration_id, "credential": credential });
                        let reply = dispatcher
                            .dispatch(&plugin_id, "integration", "refresh", event)
                            .await?;
                        serde_json::from_value::<CredentialValue>(
                            reply.get("credential").cloned().unwrap_or(Value::Null),
                        )
                        .map_err(|error| error.to_string())
                    })
                },
            ) as RefreshFn
        });
        let implementation = authorize.map(|authorize| OAuthImplementation {
            authorize: Arc::new(authorize),
            refresh,
            label: None,
        });
        integration.update_method(integration_id, method, implementation);
        applied += 1;
    }
    applied
}

/// G-384 — apply the `ctx.integration.transform` editor table into
/// the engine's [`crate::integration::IntegrationHost`]: upserts via
/// `update_integration`/`set_integration_metadata` (create-if-missing
/// with name = id — ref integration.ts:300-308), method removals via
/// `remove_method`, whole-entry removals via `remove_integration`
/// (applied LAST so a remove wins over the same id's upsert). Rows
/// whose id carries ANY plugin's removal marker skip their upsert.
/// The caller publishes `integration.updated` afterwards (the ref
/// notifies on every registration AND disposal — see the engine's
/// apply loop). Returns the applied op count.
pub fn apply_plugin_integrations(
    integration: &crate::integration::IntegrationHost,
    defs: &[PluginIntegrationDef],
    removals: &[PluginIntegrationRemovalDef],
    method_removals: &[PluginIntegrationMethodRemovalDef],
) -> usize {
    use crate::integration::types::Method;
    let removed_ids: std::collections::HashSet<&str> =
        removals.iter().map(|removal| removal.id.as_str()).collect();
    let mut applied = 0usize;
    for def in defs {
        if removed_ids.contains(def.id.as_str()) {
            continue;
        }
        integration.update_integration(&def.id, &def.name);
        if let Some(metadata) = &def.metadata {
            integration.set_integration_metadata(&def.id, Some(metadata.clone()));
        }
        applied += 1;
    }
    for removal in method_removals {
        let Ok(method) = serde_json::from_value::<Method>(removal.method.clone()) else {
            eprintln!(
                "opencode plugin-apply: skipping malformed integration method removal {:?}",
                removal.integration_id
            );
            continue;
        };
        integration.remove_method(&removal.integration_id, &method);
        applied += 1;
    }
    for removal in removals {
        integration.remove_integration(&removal.id);
        applied += 1;
    }
    applied
}

/// One plugin-registered worktree strategy bridged onto
/// [`crate::project::worktree_strategy::Strategy`]: every op crosses
/// the process boundary as a `worktree.<op>` guest-hook dispatch
/// carrying `{strategy, input}` and returning the JSON result.
pub struct GuestWorktreeStrategy {
    dispatcher: Arc<dyn GuestHookDispatcher>,
    plugin_id: String,
    id: String,
}

impl GuestWorktreeStrategy {
    #[must_use]
    pub fn new(
        dispatcher: Arc<dyn GuestHookDispatcher>,
        definition: &PluginWorktreeStrategyDef,
    ) -> Self {
        Self { dispatcher, plugin_id: definition.plugin_id.clone(), id: definition.id.clone() }
    }

    fn error(message: String) -> crate::project::worktree_strategy::StrategyError {
        crate::project::worktree_strategy::StrategyError::Failed { message, force_required: false }
    }
}

#[async_trait::async_trait]
impl crate::project::worktree_strategy::Strategy for GuestWorktreeStrategy {
    fn id(&self) -> &str {
        &self.id
    }

    async fn create(
        &self,
        input: &crate::project::worktree_strategy::CreateInput,
    ) -> Result<std::path::PathBuf, crate::project::worktree_strategy::StrategyError> {
        let event = serde_json::json!({
            "strategy": self.id,
            "input": {
                "sourceDirectory": input.source_directory.display().to_string(),
                "directory": input.directory.display().to_string(),
                "branch": input.branch,
            },
        });
        let reply = self
            .dispatcher
            .dispatch(&self.plugin_id, "worktree", "create", event)
            .await
            .map_err(Self::error)?;
        let directory = reply.get("directory").and_then(Value::as_str).ok_or_else(|| {
            Self::error(format!("guest worktree strategy {}: missing directory", self.id))
        })?;
        Ok(std::path::PathBuf::from(directory))
    }

    async fn remove(
        &self,
        input: &crate::project::worktree_strategy::RemoveInput,
    ) -> Result<(), crate::project::worktree_strategy::StrategyError> {
        let event = serde_json::json!({
            "strategy": self.id,
            "input": {
                "directory": input.directory.display().to_string(),
                "force": input.force,
            },
        });
        self.dispatcher
            .dispatch(&self.plugin_id, "worktree", "remove", event)
            .await
            .map_err(Self::error)
            .map(|_| ())
    }

    async fn list(
        &self,
        directory: &std::path::Path,
    ) -> Vec<crate::project::worktree_strategy::ListEntry> {
        let event = serde_json::json!({
            "strategy": self.id,
            "input": { "directory": directory.display().to_string() },
        });
        let Ok(reply) = self.dispatcher.dispatch(&self.plugin_id, "worktree", "list", event).await
        else {
            return Vec::new();
        };
        reply
            .get("entries")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let directory = entry.get("directory")?.as_str()?;
                        let kind = entry.get("kind")?.as_str()?;
                        Some(crate::project::worktree_strategy::ListEntry {
                            directory: std::path::PathBuf::from(directory),
                            kind: if kind == "root" { "root" } else { "worktree" },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Apply plugin-registered worktree strategies (G-24/G-4) into a
/// [`crate::project::worktree_strategy::StrategyRegistry`]
/// (insert-or-replace by id; the LAST registered becomes the selected
/// default, matching v2's `editor.add` select semantics). Returns the
/// applied count.
pub fn apply_plugin_worktree_strategies(
    registry: &mut crate::project::worktree_strategy::StrategyRegistry,
    dispatcher: &Arc<dyn GuestHookDispatcher>,
    defs: &[PluginWorktreeStrategyDef],
) -> usize {
    for definition in defs {
        registry
            .add_strategy(Box::new(GuestWorktreeStrategy::new(Arc::clone(dispatcher), definition)));
    }
    defs.len()
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
/// guest's possibly-mutated copy back — carrying the ORIGINAL event so
/// non-JSON fields (the aisdk handle fields, ADR-0005's documented
/// process-isolation divergence) survive the round trip.
pub type HookCodec<E> = (fn(&E) -> Value, fn(&E, Value) -> Option<E>);

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
                if let Some(parsed) = decode(event, updated) {
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
// One registration arm per wire codec (five hook shapes, each with its
// scope-flattened event mapping from plugin_apply's v2 table); splitting
// per-hook helpers would duplicate the snapshot-order contract, so the
// length is intentional.
#[allow(clippy::too_many_lines)]
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
        // PLUG-M02: the transport-layer domains — the engine dispatch
        // sites live at the route transport seam (model-request.ts:
        // 320-375; oc-ai's intercepted transport + the ws seams). The
        // http pair wraps every request the route executes (the
        // WebSocket fallback included); the ws trio rewrites the
        // durable socket's handshake and frames.
        "session.http.request" => point!(
            register_session_http_request,
            "session",
            "http.request",
            http_request_to_json,
            http_request_from_json
        ),
        "session.http.response" => point!(
            register_session_http_response,
            "session",
            "http.response",
            http_response_to_json,
            http_response_from_json
        ),
        "session.experimental.ws.handshake" => point!(
            register_session_ws_handshake,
            "session",
            "experimental.ws.handshake",
            ws_handshake_to_json,
            ws_handshake_from_json
        ),
        "session.experimental.ws.send" => point!(
            register_session_ws_send,
            "session",
            "experimental.ws.send",
            ws_send_to_json,
            ws_send_from_json
        ),
        "session.experimental.ws.receive" => point!(
            register_session_ws_receive,
            "session",
            "experimental.ws.receive",
            ws_receive_to_json,
            ws_receive_from_json
        ),
        // PLUG-G2: the remaining v2 hook domains (ref packages/plugin/src/promise/aisdk.ts:21
        // `aisdk.hook`, promise/permission.ts:23 `permission.hook`) — the host-side
        // dispatch sites are hooks.rs's permission_evaluate / aisdk_sdk
        // / aisdk_language points; the guest bridge mirrors them.
        "permission.evaluate" => point!(
            register_permission_evaluate,
            "permission",
            "evaluate",
            permission_to_json,
            permission_from_json
        ),
        "aisdk.sdk" => {
            point!(register_aisdk_sdk, "aisdk", "sdk", aisdk_sdk_to_json, aisdk_sdk_from_json);
        }
        "aisdk.language" => point!(
            register_aisdk_language,
            "aisdk",
            "language",
            aisdk_language_to_json,
            aisdk_language_from_json
        ),
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
    // G-5: the state-transform chain — plugin-scoped rewrites over the
    // assembled agent table (config + snapshot lanes), ref
    // state.ts:172-194 semantics (registration order, grouped-failure
    // isolation, fresh-candidate restart). Inactive plugins' transforms
    // stay out of this location's rebuild (PLUG-D07).
    let state_transforms: Vec<crate::state_transform::AgentTransform> = source
        .agent_state_transforms()
        .into_iter()
        .filter(|transform| transform.plugin_id.as_deref().is_none_or(is_active))
        .collect();
    if !state_transforms.is_empty() {
        if let Err(failure) =
            crate::state_transform::apply_agent_state_transforms(agents, &state_transforms)
        {
            // Ungrouped (host) failure — ref propagates the read
            // error; the assembly continues with the pre-transform
            // agent state rather than failing the whole location.
            eprintln!(
                "opencode plugin-apply: agent state transform failed ({}): {} — keeping pre-transform agent state",
                failure.state, failure.cause
            );
        }
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
            // G-291 — the executor key mirrors the registration table's
            // effective key (tool.ts:188), not the bare definition name.
            let effective = crate::tool::registry::effective_name(
                definition.namespace.as_deref(),
                &definition.name,
            );
            tools.register_executor(effective, executor);
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
        AisdkLanguageEvent, AisdkSdkEvent, CompactionResult, PermissionEvaluateEvent,
        RetryDecision, SessionCompactionEvent, SessionContextEvent, SessionModelRequestEvent,
        SessionPromptEvent, SessionRequestEvent, SessionRetryEvent, SessionTitleEvent,
        ShellCreateBeforeEvent, ToolDescriptor, ToolExecuteAfterEvent, ToolExecuteBeforeEvent,
        ToolOutcome,
    };
    use oc_ai::schema::messages::{Message, SystemPart};
    use oc_schema::id::{AgentId, MessageId, SessionId};
    use oc_schema::model::Ref as ModelRef;
    use oc_schema::permission::Effect as PermissionEffect;
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

    /// The plain HTTP parts (v2 carries a `Request`/`Response`; the
    /// Rust contract is the parts plugins actually rewrite).
    #[derive(Default, Serialize, Deserialize)]
    pub struct HttpWire {
        #[serde(default)]
        pub url: String,
        #[serde(default)]
        pub method: String,
        #[serde(default)]
        pub headers: BTreeMap<String, String>,
        #[serde(default)]
        pub body: Value,
    }

    #[derive(Serialize, Deserialize)]
    pub struct HttpResponseWire {
        pub status: u16,
        #[serde(default)]
        pub headers: BTreeMap<String, String>,
        #[serde(default)]
        pub body: Value,
    }

    /// The scope half every transport hook event carries.
    #[derive(Serialize, Deserialize)]
    pub struct TransportScopeWire {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<AgentId>,
        pub model: ModelRef,
        pub kind: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct HttpRequestEvent {
        #[serde(flatten)]
        pub scope: TransportScopeWire,
        pub request: HttpWire,
    }

    #[derive(Serialize, Deserialize)]
    pub struct HttpResponseEvent {
        #[serde(flatten)]
        pub scope: TransportScopeWire,
        #[serde(default)]
        pub request: HttpWire,
        pub response: HttpResponseWire,
    }

    #[derive(Serialize, Deserialize)]
    pub struct WsHandshakeEvent {
        #[serde(flatten)]
        pub scope: TransportScopeWire,
        pub url: String,
        #[serde(default)]
        pub headers: BTreeMap<String, String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct WsFrameEvent {
        #[serde(flatten)]
        pub scope: TransportScopeWire,
        pub frame: String,
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

    pub fn prompt_from_json(
        _original: &SessionPromptEvent,
        json: Value,
    ) -> Option<SessionPromptEvent> {
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

    pub fn context_from_json(
        _original: &SessionContextEvent,
        json: Value,
    ) -> Option<SessionContextEvent> {
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

    pub fn compaction_from_json(
        _original: &SessionCompactionEvent,
        json: Value,
    ) -> Option<SessionCompactionEvent> {
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

    pub fn title_from_json(
        _original: &SessionTitleEvent,
        json: Value,
    ) -> Option<SessionTitleEvent> {
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

    pub fn model_request_from_json(
        _original: &SessionModelRequestEvent,
        json: Value,
    ) -> Option<SessionModelRequestEvent> {
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

    /// The scope half every transport event carries (session, agent,
    /// model, kind — v2 spreads `scope` into each trigger).
    struct TransportScope {
        session_id: SessionId,
        agent: AgentId,
        model: ModelRef,
        kind: crate::hooks::SessionRequestKind,
    }

    fn transport_scope(event: &TransportScopeWire) -> TransportScope {
        let kind = match event.kind.as_str() {
            "compaction" => crate::hooks::SessionRequestKind::Compaction,
            "title" => crate::hooks::SessionRequestKind::Title,
            "generate" => crate::hooks::SessionRequestKind::Generate,
            _ => crate::hooks::SessionRequestKind::Primary,
        };
        TransportScope {
            session_id: event.session_id.clone(),
            agent: event.agent.clone().unwrap_or_else(|| AgentId::parse("build")),
            model: event.model.clone(),
            kind,
        }
    }

    fn scope_wire(
        session_id: SessionId,
        agent: AgentId,
        model: ModelRef,
        kind: crate::hooks::SessionRequestKind,
    ) -> TransportScopeWire {
        TransportScopeWire { session_id, agent: Some(agent), model, kind: kind.as_str().to_owned() }
    }

    fn http_wire_to_parts(wire: &HttpWire) -> crate::hooks::HttpRequest {
        crate::hooks::HttpRequest {
            url: wire.url.clone(),
            method: wire.method.clone(),
            headers: wire.headers.clone(),
            body: wire.body.clone(),
        }
    }

    fn http_parts_to_wire(parts: &crate::hooks::HttpRequest) -> HttpWire {
        HttpWire {
            url: parts.url.clone(),
            method: parts.method.clone(),
            headers: parts.headers.clone(),
            body: parts.body.clone(),
        }
    }

    pub fn http_request_to_json(event: &crate::hooks::SessionHttpRequestEvent) -> Value {
        encode(&HttpRequestEvent {
            scope: scope_wire(
                event.session_id.clone(),
                event.agent.clone(),
                event.model.clone(),
                event.kind,
            ),
            request: http_parts_to_wire(&event.request),
        })
    }

    pub fn http_request_from_json(
        _original: &crate::hooks::SessionHttpRequestEvent,
        json: Value,
    ) -> Option<crate::hooks::SessionHttpRequestEvent> {
        let wire = serde_json::from_value::<HttpRequestEvent>(json).ok()?;
        let scope = transport_scope(&wire.scope);
        Some(crate::hooks::SessionHttpRequestEvent {
            session_id: scope.session_id,
            agent: scope.agent,
            model: scope.model,
            kind: scope.kind,
            request: http_wire_to_parts(&wire.request),
        })
    }

    pub fn http_response_to_json(event: &crate::hooks::SessionHttpResponseEvent) -> Value {
        encode(&HttpResponseEvent {
            scope: scope_wire(
                event.session_id.clone(),
                event.agent.clone(),
                event.model.clone(),
                event.kind,
            ),
            request: http_parts_to_wire(&event.request),
            response: HttpResponseWire {
                status: event.response.status,
                headers: event.response.headers.clone(),
                body: event.response.body.clone(),
            },
        })
    }

    pub fn http_response_from_json(
        _original: &crate::hooks::SessionHttpResponseEvent,
        json: Value,
    ) -> Option<crate::hooks::SessionHttpResponseEvent> {
        let wire = serde_json::from_value::<HttpResponseEvent>(json).ok()?;
        let scope = transport_scope(&wire.scope);
        Some(crate::hooks::SessionHttpResponseEvent {
            session_id: scope.session_id,
            agent: scope.agent,
            model: scope.model,
            kind: scope.kind,
            request: http_wire_to_parts(&wire.request),
            response: crate::hooks::HttpResponse {
                status: wire.response.status,
                headers: wire.response.headers,
                body: wire.response.body,
            },
        })
    }

    pub fn ws_handshake_to_json(event: &crate::hooks::SessionWsHandshakeEvent) -> Value {
        encode(&WsHandshakeEvent {
            scope: scope_wire(
                event.session_id.clone(),
                event.agent.clone(),
                event.model.clone(),
                event.kind,
            ),
            url: event.url.clone(),
            headers: event.headers.clone(),
        })
    }

    pub fn ws_handshake_from_json(
        _original: &crate::hooks::SessionWsHandshakeEvent,
        json: Value,
    ) -> Option<crate::hooks::SessionWsHandshakeEvent> {
        let wire = serde_json::from_value::<WsHandshakeEvent>(json).ok()?;
        let scope = transport_scope(&wire.scope);
        Some(crate::hooks::SessionWsHandshakeEvent {
            session_id: scope.session_id,
            agent: scope.agent,
            model: scope.model,
            kind: scope.kind,
            url: wire.url,
            headers: wire.headers,
        })
    }

    pub fn ws_send_to_json(event: &crate::hooks::SessionWsSendEvent) -> Value {
        encode(&WsFrameEvent {
            scope: scope_wire(
                event.session_id.clone(),
                event.agent.clone(),
                event.model.clone(),
                event.kind,
            ),
            frame: event.frame.clone(),
        })
    }

    pub fn ws_send_from_json(
        _original: &crate::hooks::SessionWsSendEvent,
        json: Value,
    ) -> Option<crate::hooks::SessionWsSendEvent> {
        let wire = serde_json::from_value::<WsFrameEvent>(json).ok()?;
        let scope = transport_scope(&wire.scope);
        Some(crate::hooks::SessionWsSendEvent {
            session_id: scope.session_id,
            agent: scope.agent,
            model: scope.model,
            kind: scope.kind,
            frame: wire.frame,
        })
    }

    pub fn ws_receive_to_json(event: &crate::hooks::SessionWsReceiveEvent) -> Value {
        encode(&WsFrameEvent {
            scope: scope_wire(
                event.session_id.clone(),
                event.agent.clone(),
                event.model.clone(),
                event.kind,
            ),
            frame: event.frame.clone(),
        })
    }

    pub fn ws_receive_from_json(
        _original: &crate::hooks::SessionWsReceiveEvent,
        json: Value,
    ) -> Option<crate::hooks::SessionWsReceiveEvent> {
        let wire = serde_json::from_value::<WsFrameEvent>(json).ok()?;
        let scope = transport_scope(&wire.scope);
        Some(crate::hooks::SessionWsReceiveEvent {
            session_id: scope.session_id,
            agent: scope.agent,
            model: scope.model,
            kind: scope.kind,
            frame: wire.frame,
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

    pub fn retry_from_json(
        _original: &SessionRetryEvent,
        json: Value,
    ) -> Option<SessionRetryEvent> {
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

    pub fn tool_before_from_json(
        _original: &ToolExecuteBeforeEvent,
        json: Value,
    ) -> Option<ToolExecuteBeforeEvent> {
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

    pub fn tool_after_from_json(
        _original: &ToolExecuteAfterEvent,
        json: Value,
    ) -> Option<ToolExecuteAfterEvent> {
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

    pub fn shell_before_from_json(
        _original: &ShellCreateBeforeEvent,
        json: Value,
    ) -> Option<ShellCreateBeforeEvent> {
        let wire = serde_json::from_value::<ShellBeforeEvent>(json).ok()?;
        Some(ShellCreateBeforeEvent {
            command: wire.command,
            cwd: wire.cwd,
            timeout_ms: wire.timeout,
            shell: wire.shell,
            env: wire.env,
        })
    }

    // -- permission.evaluate / aisdk.* (PLUG-G2) ----------------------------
    //
    // Wire shapes per the ref: permission.evaluate is
    // `PermissionEvaluation` (promise/permission.ts:9-18) and the aisdk
    // pair is `AISDKHooks` (promise/aisdk.ts:5-19). The aisdk handle
    // fields (`sdk`, `language`) are process-local objects — they do
    // NOT cross the JSON wire: encode emits a `sdk` only when the
    // current handle IS a JSON value (a previous guest swap), and
    // decode preserves the ORIGINAL handle unless the guest returned a
    // new JSON `sdk` (wrapped as the new handle). The `language`
    // handle always stays the original (a `LanguageModel` carries a
    // live route — a documented process-isolation divergence).

    #[derive(Serialize, Deserialize)]
    pub struct PermissionEvent {
        #[serde(rename = "sessionID")]
        pub session_id: SessionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub agent: Option<AgentId>,
        pub action: String,
        #[serde(default)]
        pub resources: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub metadata: Option<serde_json::Map<String, Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub source: Option<Value>,
        pub effect: PermissionEffect,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct AisdkSdkEventWire {
        pub model: oc_schema::model::Info,
        pub package: String,
        #[serde(default)]
        pub options: serde_json::Map<String, Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sdk: Option<Value>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct AisdkLanguageEventWire {
        pub model: oc_schema::model::Info,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sdk: Option<Value>,
        #[serde(default)]
        pub options: serde_json::Map<String, Value>,
    }

    /// The JSON view of one opaque sdk handle, when it IS JSON (a
    /// previous guest swap); native handles cross as nothing.
    fn sdk_handle_json(handle: Option<&crate::hooks::SdkHandle>) -> Option<Value> {
        let handle = handle?;
        handle.clone().downcast::<Value>().ok().map(|boxed| (*boxed).clone())
    }

    /// Wrap a guest-returned JSON sdk as the new opaque handle.
    fn sdk_handle_of(value: Value) -> crate::hooks::SdkHandle {
        std::sync::Arc::new(value)
    }

    pub fn permission_to_json(event: &PermissionEvaluateEvent) -> Value {
        encode(&PermissionEvent {
            session_id: event.session_id.clone(),
            agent: event.agent.clone(),
            action: event.action.clone(),
            resources: event.resources.clone(),
            metadata: event.metadata.clone(),
            source: event.source.clone(),
            effect: event.effect,
            message: event.message.clone(),
        })
    }

    pub fn permission_from_json(
        _original: &PermissionEvaluateEvent,
        json: Value,
    ) -> Option<PermissionEvaluateEvent> {
        let wire = serde_json::from_value::<PermissionEvent>(json).ok()?;
        Some(PermissionEvaluateEvent {
            session_id: wire.session_id,
            agent: wire.agent,
            action: wire.action,
            resources: wire.resources,
            metadata: wire.metadata,
            source: wire.source,
            effect: wire.effect,
            message: wire.message,
        })
    }

    pub fn aisdk_sdk_to_json(event: &AisdkSdkEvent) -> Value {
        encode(&AisdkSdkEventWire {
            model: event.model.clone(),
            package: event.package.clone(),
            options: event.options.clone(),
            sdk: sdk_handle_json(event.sdk.as_ref()),
        })
    }

    pub fn aisdk_sdk_from_json(original: &AisdkSdkEvent, json: Value) -> Option<AisdkSdkEvent> {
        let wire = serde_json::from_value::<AisdkSdkEventWire>(json).ok()?;
        Some(AisdkSdkEvent {
            model: wire.model,
            package: wire.package,
            options: wire.options,
            sdk: match wire.sdk {
                // The guest swapped the package instance: its JSON
                // value becomes the new handle (host.ts:160-169's
                // `event.sdk = output.sdk` seam).
                // EDGE: an explicit guest `sdk: null` deserializes to
                // `None` here, so it PRESERVES the original handle
                // rather than clearing it — serde cannot distinguish
                // absent from null in `Option<Value>`. Accepted
                // divergence; revisit if a guest ever needs to clear.
                Some(value) => Some(sdk_handle_of(value)),
                None => original.sdk.clone(),
            },
        })
    }

    pub fn aisdk_language_to_json(event: &AisdkLanguageEvent) -> Value {
        encode(&AisdkLanguageEventWire {
            model: event.model.clone(),
            sdk: sdk_handle_json(event.sdk.as_ref()),
            options: event.options.clone(),
        })
    }

    pub fn aisdk_language_from_json(
        original: &AisdkLanguageEvent,
        json: Value,
    ) -> Option<AisdkLanguageEvent> {
        let wire = serde_json::from_value::<AisdkLanguageEventWire>(json).ok()?;
        Some(AisdkLanguageEvent {
            model: wire.model,
            sdk: match wire.sdk {
                Some(value) => Some(sdk_handle_of(value)),
                None => original.sdk.clone(),
            },
            options: wire.options,
            // The `LanguageModel` handle cannot be rebuilt from JSON —
            // the original rides through (ADR-0005 divergence).
            language: original.language.clone(),
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

#[cfg(test)]
mod guest_hook_domain_tests {
    //! PLUG-G2 — the `permission.evaluate` / `aisdk.sdk` /
    //! `aisdk.language` guest-bridge domains: guest registration (the
    //! snapshot's `PluginHook` rows land through
    //! [`register_guest_hook`]) + host dispatch round-trip through a
    //! mock [`GuestHookDispatcher`] (the same copy-in/copy-out seam
    //! the bun bridge rides).

    use super::*;
    use crate::hooks::{AisdkLanguageEvent, AisdkSdkEvent, HookRegistry, PermissionEvaluateEvent};
    use oc_schema::id::{AgentId, SessionId};
    use oc_schema::permission::Effect as PermissionEffect;
    use serde_json::{json, Value};

    /// The mock guest: one recorded dispatch + a canned mutation per
    /// domain (the guest side of the round trip).
    struct MockGuest {
        calls: std::sync::Mutex<Vec<(String, String, String, Value)>>,
    }

    #[async_trait::async_trait]
    impl GuestHookDispatcher for MockGuest {
        async fn dispatch(
            &self,
            plugin_id: &str,
            domain: &str,
            name: &str,
            event: Value,
        ) -> Result<Value, String> {
            self.calls.lock().unwrap().push((
                plugin_id.to_owned(),
                domain.to_owned(),
                name.to_owned(),
                event.clone(),
            ));
            let mut event = event;
            if let Value::Object(map) = &mut event {
                match (domain, name) {
                    ("permission", "evaluate") => {
                        map.insert("effect".into(), json!("deny"));
                        map.insert("message".into(), json!("denied-by-plugin"));
                    }
                    ("aisdk", "sdk") => {
                        map.insert("package".into(), json!("@ai-sdk/swapped"));
                        map.insert("sdk".into(), json!({ "wrapped": true }));
                    }
                    ("aisdk", "language") => {
                        map.insert("options".into(), json!({ "touched": "by-plugin" }));
                    }
                    _ => {}
                }
            }
            Ok(event)
        }
    }

    struct HookedSource {
        dispatcher: Arc<MockGuest>,
    }

    impl LocationPluginSource for HookedSource {
        fn hooks(&self) -> Vec<PluginHookDef> {
            vec![
                PluginHookDef {
                    plugin_id: "plugindemo".into(),
                    domain: "permission".into(),
                    name: "evaluate".into(),
                },
                PluginHookDef {
                    plugin_id: "plugindemo".into(),
                    domain: "aisdk".into(),
                    name: "sdk".into(),
                },
                PluginHookDef {
                    plugin_id: "plugindemo".into(),
                    domain: "aisdk".into(),
                    name: "language".into(),
                },
            ]
        }

        fn hook_dispatcher(&self) -> Option<Arc<dyn GuestHookDispatcher>> {
            Some(Arc::new(MockGuestShim(Arc::clone(&self.dispatcher)))
                as Arc<dyn GuestHookDispatcher>)
        }
    }

    /// Arc<MockGuest> → dispatcher shim (`MockGuest` itself is the
    /// impl; this works around the `CoerceUnsized` split).
    struct MockGuestShim(Arc<MockGuest>);

    #[async_trait::async_trait]
    impl GuestHookDispatcher for MockGuestShim {
        async fn dispatch(
            &self,
            plugin_id: &str,
            domain: &str,
            name: &str,
            event: Value,
        ) -> Result<Value, String> {
            self.0.dispatch(plugin_id, domain, name, event).await
        }
    }

    async fn apply(hooks: &Arc<HookRegistry>, source: &HookedSource) {
        let agents = Arc::new(AgentRegistry::new());
        let tools = Arc::new(ToolRegistryState::new());
        let skills = Arc::new(crate::engine::skills::ConfigSkillTable::empty());
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
        let mcp = crate::mcp::McpRegistry::with_connector(
            bus,
            connector,
            Arc::new(crate::mcp::NoCredentials),
        )
        .with_directory("/tmp");
        apply_location_plugins(source, hooks, &agents, &tools, None, &mcp, &skills, &[]).await;
    }

    fn model_info() -> oc_schema::model::Info {
        serde_json::from_value(json!({
            "id": "glm-5.3", "modelID": "glm-5.3", "providerID": "own",
            "name": "GLM",
            "capabilities": { "tools": true, "input": ["text"], "output": ["text"] },
            "variants": [],
            "time": { "released": 0 }, "cost": [], "status": "beta",
            "enabled": true, "limit": { "context": 1, "input": 1, "output": 1 }
        }))
        .expect("model info")
    }

    /// permission.evaluate: registration lands + the host dispatch
    /// round-trips the guest's effect/message rewrite.
    #[tokio::test]
    async fn permission_evaluate_round_trips() {
        let hooks = Arc::new(HookRegistry::new());
        let source = HookedSource { dispatcher: MockGuest::default_shim() };
        apply(&hooks, &source).await;
        assert!(hooks.has_permission_evaluate(None), "the guest hook registered");

        let event = PermissionEvaluateEvent {
            session_id: SessionId::parse("ses_perm").expect("session id"),
            agent: Some(AgentId::parse("build")),
            action: "read".into(),
            resources: vec![".env".into()],
            metadata: None,
            source: None,
            effect: PermissionEffect::Ask,
            message: None,
        };
        let result = hooks.dispatch_permission_evaluate(event).await.expect("dispatch");
        assert_eq!(result.effect, PermissionEffect::Deny, "the guest's effect rode back");
        assert_eq!(result.message.as_deref(), Some("denied-by-plugin"));
    }

    /// aisdk.sdk: the guest's package swap + the JSON sdk handle swap
    /// ride back (the handle becomes the wrapped guest value).
    #[tokio::test]
    async fn aisdk_sdk_round_trips() {
        let hooks = Arc::new(HookRegistry::new());
        let source = HookedSource { dispatcher: MockGuest::default_shim() };
        apply(&hooks, &source).await;
        assert!(hooks.has_aisdk_sdk(None), "the guest hook registered");

        let event = AisdkSdkEvent {
            model: model_info(),
            package: "@ai-sdk/anthropic".into(),
            options: serde_json::Map::new(),
            sdk: None,
        };
        let result = hooks.dispatch_aisdk_sdk(event).await.expect("dispatch");
        assert_eq!(result.package, "@ai-sdk/swapped", "the guest swapped the package");
        let handle = result.sdk.expect("the guest's sdk value became a handle");
        let downcast = handle.downcast::<Value>().expect("the handle wraps JSON");
        assert_eq!(*downcast, json!({ "wrapped": true }));
    }

    /// aisdk.language: the JSON fields mutate; the non-serializable
    /// `language` handle rides through untouched (documented
    /// divergence — a `LanguageModel` carries a live route).
    #[tokio::test]
    async fn aisdk_language_round_trips() {
        let hooks = Arc::new(HookRegistry::new());
        let source = HookedSource { dispatcher: MockGuest::default_shim() };
        apply(&hooks, &source).await;
        assert!(hooks.has_aisdk_language(None), "the guest hook registered");

        let event = AisdkLanguageEvent {
            model: model_info(),
            sdk: None,
            options: serde_json::Map::new(),
            language: None,
        };
        let result = hooks.dispatch_aisdk_language(event).await.expect("dispatch");
        assert_eq!(result.options.get("touched"), Some(&json!("by-plugin")));
        assert!(result.language.is_none(), "the original handle rides through");
    }

    impl MockGuest {
        fn default_shim() -> Arc<MockGuest> {
            Arc::new(MockGuest { calls: std::sync::Mutex::new(Vec::new()) })
        }
    }
}

#[cfg(test)]
mod g384_tests {
    use super::*;
    use crate::bus::BusOptions;
    use std::collections::BTreeMap;

    async fn test_integration_host(
    ) -> (Arc<crate::integration::IntegrationHost>, Arc<crate::bus::Bus>) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db = std::sync::Arc::new(
            crate::database::Database::open_file(dir.path().join("t.db")).await.expect("db"),
        );
        std::mem::forget(dir);
        let bus = Arc::new(crate::bus::Bus::new(
            (*db).clone(),
            BusOptions { persist: false, ..BusOptions::deterministic() },
        ));
        let host = crate::integration::IntegrationHost::new(
            db,
            bus.clone(),
            crate::provider_catalog::catalog::Catalog::embedded(),
            "opencode-test",
        );
        (host, bus)
    }

    fn row(
        id: &str,
        name: &str,
        metadata: Option<BTreeMap<String, Value>>,
    ) -> PluginIntegrationDef {
        PluginIntegrationDef {
            plugin_id: "p".to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            metadata,
        }
    }

    /// G-384 — upserts (create-if-missing name = id, metadata set),
    /// method removal, and whole-entry removal (the removal wins over
    /// the same id's upsert).
    #[tokio::test]
    async fn apply_plugin_integrations_upserts_and_retracts() {
        let (host, _bus) = test_integration_host().await;
        // A pre-existing integration the editor renames + tags.
        host.update_integration("base", "Base");
        let metadata: BTreeMap<String, Value> =
            [("source".to_owned(), Value::String("plugin".to_owned()))].into();
        let applied = apply_plugin_integrations(
            &host,
            &[row("base", "Renamed", Some(metadata)), row("brandnew", "brandnew", None)],
            &[PluginIntegrationRemovalDef { plugin_id: "p".into(), id: "gone".into() }],
            &[PluginIntegrationMethodRemovalDef {
                plugin_id: "p".into(),
                integration_id: "base".into(),
                method: serde_json::json!({ "type": "key" }),
            }],
        );
        assert_eq!(applied, 4, "three upserts/removals + the method removal");
        let base = host.get("base").await.expect("get").expect("present");
        assert_eq!(base.name, "Renamed");
        assert_eq!(
            base.metadata.as_ref().expect("metadata").get("source"),
            Some(&serde_json::json!("plugin"))
        );
        let brandnew = host.get("brandnew").await.expect("get").expect("created");
        assert_eq!(brandnew.name, "brandnew", "name defaults to the id");
        assert!(host.get("gone").await.expect("get").is_none(), "removal retracted");

        // Method removal: install a key method on `base`, remove it.
        host.update_method(
            "base",
            crate::integration::types::Method::Key { label: None, form: None },
            None,
        );
        assert!(
            host.get("base")
                .await
                .expect("get")
                .expect("present")
                .methods
                .iter()
                .any(|m| matches!(m, crate::integration::types::Method::Key { .. })),
            "key method installed"
        );
        apply_plugin_integrations(
            &host,
            &[],
            &[],
            &[PluginIntegrationMethodRemovalDef {
                plugin_id: "p".into(),
                integration_id: "base".into(),
                method: serde_json::json!({ "type": "key" }),
            }],
        );
        assert!(
            !host
                .get("base")
                .await
                .expect("get")
                .expect("present")
                .methods
                .iter()
                .any(|m| matches!(m, crate::integration::types::Method::Key { .. })),
            "key method removed"
        );

        // A removal marker beats the same id's upsert (remove wins).
        apply_plugin_integrations(
            &host,
            &[row("brandnew", "zombie", None)],
            &[PluginIntegrationRemovalDef { plugin_id: "p".into(), id: "brandnew".into() }],
            &[],
        );
        assert!(host.get("brandnew").await.expect("get").is_none(), "removal wins the upsert");
    }

    /// G-385 — `reload_catalog` re-installs the catalog key/env methods
    /// over a refreshed catalog and `notify_updated` publishes
    /// `integration.updated` (integration.ts:355 — the ref reload
    /// invalidates + publishes; it does NOT refetch models.dev).
    #[tokio::test]
    async fn reload_catalog_reinstalls_methods_and_notify_publishes() {
        use futures::StreamExt as _;
        let (host, bus) = test_integration_host().await;
        let mut subscription = bus.subscribe_types(&[crate::bus::registry::INTEGRATION_UPDATED]);
        let catalog = crate::provider_catalog::catalog::Catalog::embedded();
        // Drop every entry (simulating a first boot over an empty
        // catalog), then reload over the real one: the methods return.
        for info in host.list().await.expect("list") {
            host.remove_integration(&info.id);
        }
        assert!(host.list().await.expect("list").is_empty());
        host.reload_catalog(&catalog);
        assert!(
            !host.list().await.expect("list").is_empty(),
            "catalog methods re-installed after reload"
        );
        host.notify_updated().await;
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), subscription.next())
            .await
            .expect("event within timeout")
            .expect("event present");
        assert_eq!(event.type_, crate::bus::registry::INTEGRATION_UPDATED, "published: {event:?}");
        assert_eq!(event.data, serde_json::json!({}), "ephemeral, data {{}}");
    }
}
