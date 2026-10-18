//! Config/plugin command table — the v2 `Command.Service` surface.
//!
//! Three sources merge in document order (later overwrites on name
//! collision, mirroring the ref's `editor.add` Map.set):
//!
//! 1. config `commands` maps (`opencode.json` documents),
//! 2. `{command,commands}/**/*.md` discovery per config directory entry
//!    (ref `config/plugin/command.ts`),
//! 3. plugin `command.add` registrations (the host's registration
//!    snapshot, applied through the engine's plugin seam at every
//!    location assembly — CD-34: the registrations existed but never
//!    reached an API surface).
//!
//! The list shape is v2 `Command.Info` (`{name, description?}`).

use parking_lot::RwLock;

use crate::config::command_md;
use crate::config::Loaded;

/// Where a registered command came from (drives execution dispatch).
#[derive(Debug, Clone, PartialEq)]
pub enum CommandSource {
    /// Config document or markdown file (template-driven).
    Config(oc_schema::config::Command),
    /// A plugin registration (guest `command.add`; dispatched through
    /// the plugin host's `command.execute`).
    Plugin,
}

/// One registered command (v2 `Command.Definition` folded with its
/// `Info` projection).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRegistration {
    pub name: String,
    pub description: Option<String>,
    pub source: CommandSource,
}

/// The per-location command registry (v2 `Command.Service` state).
#[derive(Debug, Default)]
pub struct CommandTable {
    registrations: RwLock<Vec<CommandRegistration>>,
}

impl CommandTable {
    /// No commands.
    #[must_use]
    pub fn empty() -> Self {
        Self { registrations: RwLock::new(Vec::new()) }
    }

    /// Discovers config + markdown commands from a loaded config
    /// snapshot (document order; later sources overwrite on collision).
    #[must_use]
    pub fn discover(loaded: &Loaded) -> Self {
        let table = Self::empty();
        for (name, command) in command_md::load_commands(&loaded.entries) {
            table.insert(CommandRegistration {
                description: command.description.clone(),
                name,
                source: CommandSource::Config(command),
            });
        }
        table
    }

    /// Inserts (or replaces) one registration — `editor.add` semantics.
    pub fn insert(&self, registration: CommandRegistration) {
        let mut registrations = self.registrations.write();
        match registrations.iter_mut().find(|item| item.name == registration.name) {
            Some(slot) => *slot = registration,
            None => registrations.push(registration),
        }
    }

    /// Registers a plugin command (name + description only; execution
    /// dispatches to the owning plugin).
    pub fn register_plugin(&self, name: &str, description: &str) {
        self.insert(CommandRegistration {
            name: name.to_string(),
            description: if description.is_empty() { None } else { Some(description.to_string()) },
            source: CommandSource::Plugin,
        });
    }

    /// Every registration, insertion order (v2 `command.list`).
    #[must_use]
    pub fn list(&self) -> Vec<CommandRegistration> {
        self.registrations.read().clone()
    }

    /// One command by name (v2 `command.get`).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<CommandRegistration> {
        self.registrations.read().iter().find(|item| item.name == name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_table_discovers_config_and_markdown() {
        let dir = std::env::temp_dir().join(format!("oc-cmd-table-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let command_dir = dir.join("command");
        std::fs::create_dir_all(&command_dir).unwrap();
        std::fs::write(command_dir.join("md-cmd.md"), "Markdown body.").unwrap();

        let info = serde_json::from_value::<oc_schema::config::Info>(serde_json::json!({
            "commands": {"cfg-cmd": {"template": "Config body.", "description": "From config"}}
        }))
        .unwrap();
        let loaded = Loaded {
            sources: crate::config::discovery::Sources::default(),
            entries: vec![
                oc_schema::config::Entry::Document(Box::new(oc_schema::config::Document {
                    info,
                    path: None,
                })),
                oc_schema::config::Entry::Directory(oc_schema::config::Directory {
                    path: dir.display().to_string(),
                }),
            ],
            diagnostics: Vec::new(),
        };
        let table = CommandTable::discover(&loaded);
        let list = table.list();
        let names: Vec<&str> = list.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["cfg-cmd", "md-cmd"]);
        let described = table.get("cfg-cmd").and_then(|item| item.description);
        assert_eq!(described.as_deref(), Some("From config"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_table_plugin_registrations_overwrite_and_list() {
        let table = CommandTable::empty();
        table.register_plugin("hw.hello", "Say hello");
        table.register_plugin("hw.hello", "Say hello again");
        table.insert(CommandRegistration {
            name: "cfg".to_string(),
            description: None,
            source: CommandSource::Config(oc_schema::config::Command {
                template: "t".to_string(),
                ..Default::default()
            }),
        });
        assert_eq!(
            table.get("hw.hello").map(|item| item.description),
            Some(Some("Say hello again".to_string()))
        );
        assert_eq!(table.list().len(), 2);
    }
}
