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

/// One tool registered through `ctx.tool.transform` (v2 `Tool.Info`
/// at the JS boundary: name/description/input JSON-Schema plus
/// `options {codemode, namespace}`; execution dispatches to the guest
/// on call — the wave-4 `tool.execute` seam).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginTool {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    /// JSON-Schema input (Effect-Schema-equivalent at the JS boundary).
    pub input: Value,
    /// `options.codemode !== false` (code-mode-eligible by default).
    pub codemode: bool,
    /// `options.namespace` (dot-path; prefixed onto the catalog path).
    pub namespace: Option<String>,
}

/// `normalizedName` — non `[A-Za-z0-9_-]` characters become `_`.
fn normalized_tool_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// `registrationError` (tool.ts:298-323) over the plugin-tool fields:
/// invalid namespace, invalid name (`^[A-Za-z0-9_-]{1,128}$` on the
/// normalized name), and the reserved `execute` id for direct tools.
fn tool_registration_error(name: &str, namespace: Option<&str>, codemode: bool) -> Option<String> {
    if let Some(namespace) = namespace {
        let invalid = namespace.is_empty()
            || namespace.split('.').any(|segment| {
                segment.is_empty()
                    || segment.len() > 64
                    || !segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            });
        if invalid {
            return Some(format!("Invalid tool namespace: {namespace:?}"));
        }
    }
    let normalized = normalized_tool_name(name);
    if normalized.is_empty() || normalized.len() > 128 {
        return Some(format!("Invalid tool name: {normalized}"));
    }
    let effective = match namespace {
        Some(namespace) => format!("{}_{}", namespace.replace('.', "_"), normalized),
        None => normalized.clone(),
    };
    if !codemode && effective == "execute" {
        return Some("Tool name \"execute\" is reserved for CodeMode".to_string());
    }
    None
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

/// One plugin-provided message-grouping rule (the open rules table
/// behind `GroupKind`): tools matching `tool` (exact) or `tool_prefix`
/// group under the plugin's custom header. Native grouping
/// (reasoning/exploration/instructions) wins first — a rule may claim
/// any other tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginGroupRule {
    pub plugin_id: String,
    /// Exact tool-name match.
    pub tool: Option<String>,
    /// Prefix match (`omo_` claims every `omo_*` tool).
    pub tool_prefix: Option<String>,
    /// The group's header label.
    pub header: String,
}

/// One plugin-contributed home quick action (registration-time only):
/// a label rendered on the home footer row that dispatches `command`
/// (a plugin or builtin command id) through the palette dispatcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHomeAction {
    pub plugin_id: String,
    pub label: String,
    pub command: String,
}

/// Coverage entry for a plugin-registered dialog provider
/// (`ctx.ui.dialog.register({id, build})`): the TUI materializes the
/// content at OPEN time through [`crate::PluginHost::materialize_dialog`]
/// — the guest builds the spec synchronously, bounded by the hard
/// timeout, and the dialog then renders natively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDialog {
    pub plugin_id: String,
    /// The dialog id (unique per host; `plugin:dialog` qualified by
    /// the supervisor on registration).
    pub id: String,
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
    /// Plugin-provided message-grouping rules (wave 3's open rules
    /// table).
    pub group_rules: Vec<PluginGroupRule>,
    /// Plugin-contributed home quick actions.
    pub home_actions: Vec<PluginHomeAction>,
    /// Registered dialog providers (materialize-at-open coverage).
    pub dialogs: Vec<PluginDialog>,
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
    /// Plugin group rules (registration order stable across snapshots).
    pub group_rules: Vec<PluginGroupRule>,
    /// Plugin home quick actions.
    pub home_actions: Vec<PluginHomeAction>,
    /// Dialog providers: qualified id (`plugin:id`) → entry.
    pub dialogs: BTreeMap<String, PluginDialog>,
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
        self.group_rules.retain(|rule| rule.plugin_id != plugin_id);
        self.home_actions.retain(|action| action.plugin_id != plugin_id);
        self.dialogs.retain(|_, dialog| dialog.plugin_id != plugin_id);
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
    /// tool name). Invalid registrations are skipped with v2's
    /// "Skipping invalid tool registration" log (`tool.ts` notify) —
    /// healthy neighbors register.
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
            let description =
                tool.get("description").and_then(Value::as_str).unwrap_or_default().to_owned();
            let input = tool
                .get("input")
                .cloned()
                .unwrap_or(serde_json::json!({ "type": "object", "properties": {} }));
            let options = tool.get("options").cloned().unwrap_or(Value::Null);
            let codemode = options.get("codemode").and_then(Value::as_bool).unwrap_or(true);
            let namespace = options
                .get("namespace")
                .and_then(Value::as_str)
                .filter(|namespace| !namespace.is_empty())
                .map(str::to_owned);
            if let Some(error) = tool_registration_error(&name, namespace.as_deref(), codemode) {
                eprintln!(
                    "opencode plugin-host: Skipping invalid tool registration: name={name:?} namespace={namespace:?} error={error}"
                );
                continue;
            }
            let provenance =
                self.tools.get(&name).map_or_else(|| plugin_id.to_owned(), |t| t.plugin_id.clone());
            self.tools.insert(
                name.clone(),
                PluginTool { plugin_id: provenance, name, description, input, codemode, namespace },
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
            group_rules: self.group_rules.clone(),
            home_actions: self.home_actions.clone(),
            dialogs: self.dialogs.values().cloned().collect(),
            plugins: self.plugins.values().cloned().collect(),
            default_agent: self.agent_default.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_tools_parses_options_and_skips_invalid_registrations() {
        let mut registrations = Registrations::default();
        registrations.merge_tools(
            "corp",
            &[
                json!({ "name": "greet", "description": "Greet", "input": { "type": "object", "properties": {} } }),
                json!({ "name": "greet", "description": "Greet (direct)", "options": { "codemode": false } }),
                json!({ "name": "sales", "description": "Read sales", "options": { "namespace": "registry.search" } }),
                // Invalid: empty name, bad namespace, reserved direct id.
                json!({ "name": "", "description": "nope" }),
                json!({ "name": "nested", "options": { "namespace": "slack..admin" } }),
                json!({ "name": "execute", "options": { "codemode": false } }),
                // `execute` code-mode-eligible is allowed (v2 reserves it
                // only for direct tools).
                json!({ "name": "execute", "description": "their own", "options": {} }),
            ],
        );
        let tools: Vec<(String, bool, Option<String>)> = registrations
            .tools
            .values()
            .map(|tool| (tool.name.clone(), tool.codemode, tool.namespace.clone()))
            .collect();
        assert_eq!(
            tools,
            vec![
                ("execute".into(), true, None),
                ("greet".into(), false, None),
                ("sales".into(), true, Some("registry.search".into())),
            ]
        );
    }

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
