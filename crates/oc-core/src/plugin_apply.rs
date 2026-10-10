//! Plugin-agent application seam (wave 2, flagship bring-up).
//!
//! `oc-plugin-host` materializes the guest's `ctx.agent.transform`
//! result into its registration snapshot as v2 `Agent.Info` JSON.
//! This module applies those definitions into the oc-core
//! [`AgentRegistry`] — the additive seam the engine/TUI layers (and
//! the bring-up test) call after the host's registration pass. The
//! host crate stays dependency-free; the parse lives on the core side
//! where the schema types are.

use crate::location::AgentRegistry;
use oc_schema::id::AgentId;

/// Apply plugin-agent `Agent.Info` JSON documents into `registry`
/// (`AgentRegistry::set` — insert or replace). Unparseable entries are
/// skipped (a defective plugin never blocks the rest); the count of
/// applied agents is returned.
#[must_use]
pub fn apply_plugin_agents(registry: &AgentRegistry, agents: &[serde_json::Value]) -> usize {
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
