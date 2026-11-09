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

/// `plugin/command/initialize.txt`, verbatim (G-193.5, ref
/// plugin/command.ts:8/31-41 — `${path}` substituted at registration).
pub const COMMAND_INITIALIZE_TEMPLATE: &str = include_str!("command/initialize.txt");

/// `plugin/command/review.txt`, verbatim.
pub const COMMAND_REVIEW_TEMPLATE: &str = include_str!("command/review.txt");

/// Seeds the builtin slash commands (ref `opencode.command` plugin,
/// command.ts:28-54): `init` (guided AGENTS.md setup) and `review`
/// (change review), template bodies verbatim with `${path}` bound to
/// the project directory. `editor.add` semantics — a config command of
/// the same name is REPLACED (the plugin transform runs after config
/// load in v2).
pub fn seed_builtin_commands(table: &CommandTable, project_directory: &std::path::Path) {
    let directory = project_directory.display().to_string();
    for (name, description, template) in [
        ("init", "guided AGENTS.md setup", COMMAND_INITIALIZE_TEMPLATE),
        (
            "review",
            "review changes [commit|branch|pr], defaults to uncommitted",
            COMMAND_REVIEW_TEMPLATE,
        ),
    ] {
        table.insert(CommandRegistration {
            name: name.to_string(),
            description: Some(description.to_string()),
            source: CommandSource::Config(oc_schema::config::Command {
                template: template.replace("${path}", &directory),
                description: None,
                agent: None,
                model: None,
                subagent: None,
                subtask: None,
            }),
        });
    }
}

/// Where a registered command came from (drives execution dispatch).
#[derive(Debug, Clone, PartialEq)]
pub enum CommandSource {
    /// Config document or markdown file (template-driven).
    Config(oc_schema::config::Command),
    /// A plugin registration (guest `command.add`; dispatched through
    /// the plugin host's `command.execute`).
    Plugin,
    /// G-215 — an MCP prompt registered as a `server:prompt` command
    /// (ref CommandPlugin: executing it calls `mcp.prompt` with the
    /// positional arguments parsed from the input text, then prompts
    /// the session with the returned message text).
    Mcp {
        server: String,
        prompt: String,
        /// The prompt's declared argument names (positional wiring).
        arguments: Vec<String>,
    },
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
    /// The location's config `shell` key (G-214: `` !`cmd` `` template
    /// interpolation resolves the config-priority shell at execution).
    config_shell: RwLock<Option<String>>,
}

impl CommandTable {
    /// No commands.
    #[must_use]
    pub fn empty() -> Self {
        Self { registrations: RwLock::new(Vec::new()), config_shell: RwLock::new(None) }
    }

    /// G-214 — the location's config `shell` selection (the tool seam's
    /// `config_shell_of`), consumed by template execution.
    pub fn set_config_shell(&self, shell: Option<String>) {
        *self.config_shell.write() = shell;
    }

    /// The location's config `shell` key, if any.
    #[must_use]
    pub fn config_shell(&self) -> Option<String> {
        self.config_shell.read().clone()
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

    /// Removes every MCP-prompt registration (G-215 refresh: the
    /// prompts set is replaced wholesale on `mcp.prompts.changed`).
    pub fn clear_mcp_commands(&self) {
        self.registrations.write().retain(|item| !matches!(item.source, CommandSource::Mcp { .. }));
    }
}

/// G-215 — `mcpCommandName` (ref plugin/command.ts:104-106):
/// `sanitize(server) + ":" + sanitize(prompt)`.
#[must_use]
pub fn mcp_command_name(server: &str, prompt: &str) -> String {
    format!("{}:{}", sanitize_command_segment(server), sanitize_command_segment(prompt))
}

/// `sanitize` (command.ts:108-110): every character outside
/// `[a-zA-Z0-9_-]` becomes `_`.
#[must_use]
pub fn sanitize_command_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// G-215 — register the connected MCP prompts as `server:prompt`
/// commands (ref CommandPlugin's `for (const prompt of loaded.prompts)`
/// `editor.add` loop). The refresh semantics: replace the whole MCP
/// slice (`command.reload()` on `mcp.prompts.changed`).
pub fn sync_mcp_prompt_commands(table: &CommandTable, prompts: &[crate::mcp::registry::Prompt]) {
    table.clear_mcp_commands();
    for prompt in prompts {
        table.insert(CommandRegistration {
            name: mcp_command_name(&prompt.server, &prompt.name),
            description: prompt.description.clone(),
            source: CommandSource::Mcp {
                server: prompt.server.clone(),
                prompt: prompt.name.clone(),
                arguments: prompt.arguments.clone(),
            },
        });
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

    /// G-193.5: the builtin `init`/`review` commands register with the
    /// ref template bodies (`${path}` bound to the project directory)
    /// and replace same-name config commands.
    #[test]
    fn builtin_commands_register_with_ref_templates() {
        let table = CommandTable::empty();
        seed_builtin_commands(&table, std::path::Path::new("/work/repo"));

        let init = table.get("init").expect("init registered");
        assert_eq!(init.description.as_deref(), Some("guided AGENTS.md setup"));
        let CommandSource::Config(command) = &init.source else {
            panic!("init is template-driven");
        };
        assert!(command.template.starts_with("Create or update `AGENTS.md` for this repository."));
        assert!(command
            .template
            .contains("If `AGENTS.md` already exists at `/work/repo`, improve it"));
        assert!(command.template.contains("$ARGUMENTS"));

        let review = table.get("review").expect("review registered");
        assert_eq!(
            review.description.as_deref(),
            Some("review changes [commit|branch|pr], defaults to uncommitted")
        );
        let CommandSource::Config(command) = &review.source else {
            panic!("review is template-driven");
        };
        assert!(command
            .template
            .starts_with("You are a code reviewer. Your job is to review code changes and provide actionable feedback."));
        assert!(command
            .template
            .contains("1. **No arguments (default)**: Review all uncommitted changes"));
        assert!(command.template.contains(
            "AVOID flattery, do not give any comments that are not helpful to the reader."
        ));

        // Same-name config command replaced (v2 plugin `editor.add`).
        table.insert(CommandRegistration {
            name: "init".to_string(),
            description: None,
            source: CommandSource::Config(oc_schema::config::Command {
                template: "user override".to_string(),
                ..Default::default()
            }),
        });
        seed_builtin_commands(&table, std::path::Path::new("/work/repo"));
        let CommandSource::Config(command) = &table.get("init").unwrap().source else { panic!() };
        assert!(command.template.starts_with("Create or update `AGENTS.md`"));
    }

    /// G-215 — `server:prompt` naming (`mcpCommandName`: sanitize both
    /// segments, `[^a-zA-Z0-9_-]` → `_`).
    #[test]
    fn mcp_command_names_sanitize_both_segments() {
        assert_eq!(mcp_command_name("github", "review-pr"), "github:review-pr");
        assert_eq!(mcp_command_name("my server!", "a/b c"), "my_server_:a_b_c");
        assert_eq!(sanitize_command_segment("a b/c.d"), "a_b_c_d");
    }

    /// G-215 — prompts register as commands with description and
    /// argument wiring; a re-sync REPLACES the MCP slice wholesale
    /// (ref `command.reload()` on PromptsChanged) without touching
    /// config/plugin registrations.
    #[test]
    fn mcp_prompt_commands_sync_and_reload() {
        use crate::mcp::registry::Prompt;
        let table = CommandTable::empty();
        table.insert(CommandRegistration {
            name: "keep".to_string(),
            description: None,
            source: CommandSource::Config(oc_schema::config::Command::default()),
        });
        sync_mcp_prompt_commands(
            &table,
            &[Prompt {
                server: "fixture".to_string(),
                name: "code review".to_string(),
                description: Some("Review code".to_string()),
                arguments: vec!["focus".to_string()],
            }],
        );
        let command = table.get("fixture:code_review").expect("registered");
        assert_eq!(command.description.as_deref(), Some("Review code"));
        let CommandSource::Mcp { server, prompt, arguments } = &command.source else {
            panic!("mcp source");
        };
        assert_eq!(server, "fixture");
        assert_eq!(prompt, "code review");
        assert_eq!(arguments, &["focus".to_string()]);

        // Reload: the stale prompt goes, the new one arrives, `keep`
        // survives.
        sync_mcp_prompt_commands(
            &table,
            &[Prompt {
                server: "fixture".to_string(),
                name: "deploy".to_string(),
                description: None,
                arguments: Vec::new(),
            }],
        );
        assert!(table.get("fixture:code_review").is_none(), "stale prompt removed");
        assert!(table.get("fixture:deploy").is_some(), "new prompt registered");
        assert!(table.get("keep").is_some(), "non-MCP registrations untouched");
    }
}
