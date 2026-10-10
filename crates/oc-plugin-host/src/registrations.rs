//! Registration-time native structures — the ONLY synchronous surface
//! (oracle-gate11 §4 wave 1: "anyone expecting a synchronous answer at
//! stroke/render time must pre-resolve to native structures at
//! registration; per-frame IPC round-trips are a latency disaster").
//!
//! Plugin `setup(ctx)` runs once at load; the host calls it makes
//! (`command.add`, `theme.register`, `event.subscribe`) land here as
//! plain Rust data. The TUI consumes the snapshot at registration
//! time; nothing crosses IPC per-frame afterwards.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;

/// One plugin-registered command (also carrying its optional keymap
/// entries — the v2 `ctx.command.add({name, description, execute})`
/// surface, extended with `keybinds` for the owned-keymap seam).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    /// Owning plugin id (`helloworld`).
    pub plugin_id: String,
    /// Dot command id (`hw.hello`).
    pub name: String,
    pub description: String,
    /// Binding specs (`"ctrl+x h"`) — land in the TUI keymap's owned
    /// layer, looked up AFTER the static table.
    pub keybinds: Vec<String>,
    /// Palette visibility (default true).
    pub palette: bool,
}

/// One agent in the plugin-transformed agent table (the materialized
/// `ctx.agent.transform` result — v2 `Agent.Info` JSON). The table is
/// host-global (every plugin's editors run over the same state); the
/// `plugin_id` records the transform that introduced the agent.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginAgent {
    pub plugin_id: String,
    /// v2 `Agent.Info` (id/name/mode/model/request/permissions/…).
    pub info: Value,
}

/// One MCP server contributed (or re-configured) through
/// `ctx.mcp.transform` (v2 `Mcp.ServerConfig` JSON).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMcp {
    pub plugin_id: String,
    pub name: String,
    pub config: Value,
}

/// One skill registered through `ctx.skill.transform` (v2 `Skill.Info`
/// JSON — the flagship's bundled `SKILL.md` documents).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginSkill {
    pub plugin_id: String,
    /// Skill id (`reflect`).
    pub id: String,
    pub description: String,
    /// The full `Skill.Info` (path/content carried for the skill tool).
    pub info: Value,
}

/// One tool registered through `ctx.tool.transform` (metadata only —
/// name, description, JSON-Schema input; execution dispatches to the
/// guest on call, a later-wave surface).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginTool {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    /// JSON-Schema input (Effect-Schema-equivalent at the JS boundary).
    pub input: Value,
}

/// One runtime hook subscription (`ctx.session.hook(name, cb)` /
/// `ctx.tool.hook(name, cb)` coverage — the registration-time surface;
/// dispatch is engine-driven through `Bridge::dispatch_hook`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginHook {
    pub plugin_id: String,
    /// `session` | `tool`.
    pub domain: String,
    /// e.g. `context`, `prompt`, `execute.before`.
    pub name: String,
}

/// One plugin-registered theme document (feeds the oc-tui
/// `ThemeRegistry` discovery path, registration-time only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTheme {
    pub plugin_id: String,
    /// Registry key (theme name).
    pub name: String,
    /// The theme JSON document (v1 `{"theme":…}` or v2), serialized.
    pub document: String,
}

/// Per-plugin lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Load in flight.
    Loading,
    /// Loaded and registered.
    Active,
    /// Load failed (module error, shape error, timeout) — contained,
    /// the host keeps running.
    Failed(String),
    /// Disabled after repeated crashes (the supervision policy).
    Disabled { crashes: u32 },
}

/// One plugin's tracked lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    /// Entrypoint on disk (absolute).
    pub entry: PathBuf,
    pub plugin_id: String,
    pub state: PluginState,
}

/// The immutable snapshot the TUI reads after the load pass (and after
/// each hot reload): commands, agents, skills, tools, MCP servers, hook
/// coverage, themes, plugin statuses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegistrationSnapshot {
    pub commands: Vec<PluginCommand>,
    pub agents: Vec<PluginAgent>,
    pub skills: Vec<PluginSkill>,
    pub tools: Vec<PluginTool>,
    pub mcps: Vec<PluginMcp>,
    pub hooks: Vec<PluginHook>,
    pub themes: Vec<PluginTheme>,
    pub plugins: Vec<PluginStatus>,
    /// The table-global default agent (`draft.default(id)`), when set.
    pub default_agent: Option<String>,
}

/// The mutable registration store behind the snapshot (interior to the
/// supervisor; shared between the IO reader — guest `command.add`
/// requests — and handle reads).
#[derive(Debug, Default)]
pub struct Registrations {
    /// command name → definition (registration order preserved by the
    /// `BTreeMap`'s key sort; stable for snapshot diffing).
    pub commands: BTreeMap<String, PluginCommand>,
    /// agent id → `Agent.Info` (the host-global transform table).
    pub agents: BTreeMap<String, PluginAgent>,
    /// skill id → info.
    pub skills: BTreeMap<String, PluginSkill>,
    /// tool name → metadata.
    pub tools: BTreeMap<String, PluginTool>,
    /// MCP server name → config.
    pub mcps: BTreeMap<String, PluginMcp>,
    /// Hook coverage (plugin, domain, name).
    pub hooks: BTreeSet<PluginHook>,
    pub themes: BTreeMap<String, PluginTheme>,
    /// entry path (string) → status.
    pub plugins: BTreeMap<String, PluginStatus>,
    /// The table-global default agent id.
    pub agent_default: Option<String>,
}

impl Registrations {
    /// Drop one plugin's registrations (unload / hot reload / crash).
    /// The agent/MCP/skill/tool tables are host-global derived state:
    /// entries the plugin introduced are removed (provenance merge in
    /// [`Registrations::merge_agents`] keeps other plugins' entries).
    pub fn clear_plugin(&mut self, plugin_id: &str) {
        self.commands.retain(|_, command| command.plugin_id != plugin_id);
        self.themes.retain(|_, theme| theme.plugin_id != plugin_id);
        self.skills.retain(|_, skill| skill.plugin_id != plugin_id);
        self.tools.retain(|_, tool| tool.plugin_id != plugin_id);
        self.hooks.retain(|hook| hook.plugin_id != plugin_id);
        self.agents.retain(|_, agent| agent.plugin_id != plugin_id);
        self.mcps.retain(|_, mcp| mcp.plugin_id != plugin_id);
        if let Some(default) = self.agent_default.as_ref() {
            if !self.agents.contains_key(default) {
                self.agent_default = None;
            }
        }
    }

    /// Merge one `ctx.agent.transform` materialization: the guest sends
    /// the WHOLE derived table; entries keep their existing provenance,
    /// new entries are attributed to the transforming plugin, missing
    /// entries are dropped (v2 table semantics).
    pub fn merge_agents(&mut self, plugin_id: &str, agents: &[Value], default: Option<&str>) {
        let mut incoming: BTreeMap<String, Value> = BTreeMap::new();
        for info in agents {
            if let Some(id) = info.get("id").and_then(Value::as_str) {
                incoming.insert(id.to_owned(), info.clone());
            }
        }
        self.agents.retain(|id, _| incoming.contains_key(id));
        for (id, info) in incoming {
            let provenance =
                self.agents.get(&id).map_or_else(|| plugin_id.to_owned(), |a| a.plugin_id.clone());
            self.agents.insert(id, PluginAgent { plugin_id: provenance, info });
        }
        if let Some(default) = default.filter(|id| !id.is_empty()) {
            self.agent_default = Some(default.to_owned());
        }
    }

    /// Merge one `ctx.mcp.transform` materialization (whole-table, by
    /// server name).
    pub fn merge_mcps(&mut self, plugin_id: &str, servers: &BTreeMap<String, Value>) {
        self.mcps.retain(|name, _| servers.contains_key(name));
        for (name, config) in servers {
            let provenance =
                self.mcps.get(name).map_or_else(|| plugin_id.to_owned(), |m| m.plugin_id.clone());
            self.mcps.insert(
                name.clone(),
                PluginMcp { plugin_id: provenance, name: name.clone(), config: config.clone() },
            );
        }
    }

    /// Merge one `ctx.skill.transform` materialization (whole-table, by
    /// skill id).
    pub fn merge_skills(&mut self, plugin_id: &str, skills: &[Value]) {
        let ids: Vec<String> = skills
            .iter()
            .filter_map(|info| {
                info.get("id")
                    .or_else(|| info.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        self.skills.retain(|id, _| ids.contains(id));
        for info in skills {
            let Some(id) = info
                .get("id")
                .or_else(|| info.get("name"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            let provenance =
                self.skills.get(&id).map_or_else(|| plugin_id.to_owned(), |s| s.plugin_id.clone());
            let description =
                info.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
            self.skills.insert(
                id.clone(),
                PluginSkill { plugin_id: provenance, id, description, info: info.clone() },
            );
        }
    }

    /// Merge one `ctx.tool.transform` materialization (whole-table, by
    /// tool name).
    pub fn merge_tools(&mut self, plugin_id: &str, tools: &[Value]) {
        let names: Vec<String> = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect();
        self.tools.retain(|name, _| names.contains(name));
        for tool in tools {
            let Some(name) = tool.get("name").and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            let provenance =
                self.tools.get(&name).map_or_else(|| plugin_id.to_owned(), |t| t.plugin_id.clone());
            let description =
                tool.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
            let input = tool
                .get("input")
                .cloned()
                .unwrap_or(serde_json::json!({ "type": "object", "properties": {} }));
            self.tools.insert(
                name.clone(),
                PluginTool { plugin_id: provenance, name, description, input },
            );
        }
    }

    /// Snapshot in stable order.
    #[must_use]
    pub fn snapshot(&self) -> RegistrationSnapshot {
        RegistrationSnapshot {
            commands: self.commands.values().cloned().collect(),
            agents: self.agents.values().cloned().collect(),
            skills: self.skills.values().cloned().collect(),
            tools: self.tools.values().cloned().collect(),
            mcps: self.mcps.values().cloned().collect(),
            hooks: self.hooks.iter().cloned().collect(),
            themes: self.themes.values().cloned().collect(),
            plugins: self.plugins.values().cloned().collect(),
            default_agent: self.agent_default.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_plugin_removes_its_registrations_only() {
        let mut registrations = Registrations::default();
        registrations.commands.insert(
            "hw.hello".into(),
            PluginCommand {
                plugin_id: "helloworld".into(),
                name: "hw.hello".into(),
                description: "Say hello".into(),
                keybinds: vec![],
                palette: true,
            },
        );
        registrations.commands.insert(
            "other.cmd".into(),
            PluginCommand {
                plugin_id: "other".into(),
                name: "other.cmd".into(),
                description: String::new(),
                keybinds: vec![],
                palette: true,
            },
        );
        registrations.clear_plugin("helloworld");
        assert_eq!(registrations.commands.len(), 1);
        assert!(registrations.commands.contains_key("other.cmd"));
    }
}
