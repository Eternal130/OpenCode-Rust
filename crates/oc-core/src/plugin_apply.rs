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
    /// v2 `Agent.Info` JSON documents (applied via
    /// [`apply_plugin_agents`]).
    fn agents(&self) -> Vec<Value> {
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
}

/// One plugin-registered command as the engine sees it (the host
/// snapshot's `PluginCommand` mirrored; oc-plugin-host stays dep-light).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommandDef {
    pub name: String,
    pub description: String,
}

/// Apply one plugin source into a freshly built location graph's
/// registries (the `assemble` step): agents + default, then the tool
/// rows + executors, then the plugin command registrations into the
/// location's command table. Returns the applied agent count.
#[must_use]
pub fn apply_location_plugins(
    source: &dyn LocationPluginSource,
    agents: &Arc<AgentRegistry>,
    tools: &Arc<ToolRegistryState>,
    commands: Option<&Arc<crate::engine::commands::CommandTable>>,
) -> usize {
    let snapshot_agents = source.agents();
    let applied =
        if snapshot_agents.is_empty() { 0 } else { apply_plugin_agents(agents, &snapshot_agents) };
    apply_plugin_agent_default(agents, source.default_agent().as_deref());
    for definition in source.tools() {
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
            commands.register_plugin(&definition.name, &definition.description);
        }
    }
    applied
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
}
