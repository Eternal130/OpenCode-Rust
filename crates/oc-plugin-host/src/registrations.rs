//! Registration-time native structures — the ONLY synchronous surface
//! (oracle-gate11 §4 wave 1: "anyone expecting a synchronous answer at
//! stroke/render time must pre-resolve to native structures at
//! registration; per-frame IPC round-trips are a latency disaster").
//!
//! Plugin `setup(ctx)` runs once at load; the host calls it makes
//! (`command.add`, `theme.register`, `event.subscribe`) land here as
//! plain Rust data. The TUI consumes the snapshot at registration
//! time; nothing crosses IPC per-frame afterwards.

use std::collections::BTreeMap;
use std::path::PathBuf;

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
/// each hot reload): commands, themes, plugin statuses.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegistrationSnapshot {
    pub commands: Vec<PluginCommand>,
    pub themes: Vec<PluginTheme>,
    pub plugins: Vec<PluginStatus>,
}

/// The mutable registration store behind the snapshot (interior to the
/// supervisor; shared between the IO reader — guest `command.add`
/// requests — and handle reads).
#[derive(Debug, Default)]
pub struct Registrations {
    /// command name → definition (registration order preserved by the
    /// `BTreeMap`'s key sort; stable for snapshot diffing).
    pub commands: BTreeMap<String, PluginCommand>,
    pub themes: BTreeMap<String, PluginTheme>,
    /// entry path (string) → status.
    pub plugins: BTreeMap<String, PluginStatus>,
}

impl Registrations {
    /// Drop one plugin's registrations (unload / hot reload / crash).
    pub fn clear_plugin(&mut self, plugin_id: &str) {
        self.commands.retain(|_, command| command.plugin_id != plugin_id);
        self.themes.retain(|_, theme| theme.plugin_id != plugin_id);
    }

    /// Snapshot in stable order.
    #[must_use]
    pub fn snapshot(&self) -> RegistrationSnapshot {
        RegistrationSnapshot {
            commands: self.commands.values().cloned().collect(),
            themes: self.themes.values().cloned().collect(),
            plugins: self.plugins.values().cloned().collect(),
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
