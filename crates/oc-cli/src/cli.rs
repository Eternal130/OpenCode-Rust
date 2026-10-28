//! The clap command tree — a 1:1 port of `cli/src/commands/commands.ts`
//! (names, aliases, flags, hidden flags). Effect-CLI per-command params map
//! onto clap args; the six shared flags (`--print-logs`, `--standalone`,
//! `--server`, `--auto`, hidden `--yolo`/`--dangerously-skip-permissions`)
//! are declared once as globals so every subcommand surface that v2 gives
//! them accepts them.

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

/// Shared connection params (v2 `ServerParams`) — filled from the global
/// `--standalone` / `--server` flags.
#[derive(Debug, Clone, Default)]
pub struct ServerParams {
    pub standalone: bool,
    pub server: Option<String>,
}

#[derive(Parser, Debug)]
#[allow(clippy::struct_excessive_bools)] // the v2 flag surface, mirrored
#[command(
    name = "opencode-rust",
    version = concat!("v", env!("CARGO_PKG_VERSION")),
    about = "OpenCode command line interface",
    disable_help_subcommand = true,
    arg_required_else_help = false
)]
pub struct Cli {
    /// Print the Rust-port build stamp (version + git hash + profile)
    #[arg(long, hide = true)]
    pub version_rust: bool,

    /// Print logs to stderr (server logs require --standalone)
    #[arg(long, global = true)]
    pub print_logs: bool,

    /// Run with a private server instead of the background service
    #[arg(long, global = true)]
    pub standalone: bool,

    /// Connect to a server URL instead of the background service
    #[arg(long, global = true, value_name = "URL")]
    pub server: Option<String>,

    #[command(flatten)]
    pub permission: PermissionParamsGlobal,

    /// Directory to start `OpenCode` in
    pub directory: Option<String>,

    /// Continue the last session
    #[arg(short = 'c', long)]
    pub continue_: bool,

    /// Session ID to continue, or to create if it does not exist
    #[arg(short = 's', long, value_name = "ID")]
    pub session: Option<String>,

    /// Prompt to use
    #[arg(long, value_name = "PROMPT")]
    pub prompt: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Global form of the permission flags (clap requires globals at the root).
#[derive(clap::Args, Debug, Clone, Default)]
pub struct PermissionParamsGlobal {
    /// Auto-approve permissions that are not explicitly denied
    #[arg(long, global = true)]
    pub auto: bool,

    #[arg(long, global = true, hide = true)]
    pub yolo: bool,

    #[arg(long, global = true, hide = true)]
    pub dangerously_skip_permissions: bool,
}

impl PermissionParamsGlobal {
    #[must_use]
    pub fn effective_auto(&self) -> bool {
        self.auto || self.yolo || self.dangerously_skip_permissions
    }
}

/// `upgrade --method` — v2's seven installation methods (updater.ts:14)
/// plus the port's two documented rust-extra extensions
/// (`local-file`, `github` — see `services::updater::EXTRA_METHODS`;
/// restricting to the seven would strand the extension paths).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum UpgradeMethod {
    Curl,
    Npm,
    Pnpm,
    Bun,
    Yarn,
    Vp,
    Brew,
    /// rust-extra: install from a tarball/zip/binary on disk.
    LocalFile,
    /// rust-extra: download this port's own release tarball.
    Github,
}

impl UpgradeMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Curl => "curl",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
            Self::Yarn => "yarn",
            Self::Vp => "vp",
            Self::Brew => "brew",
            Self::LocalFile => "local-file",
            Self::Github => "github",
        }
    }
}

/// `--format` for `run` and `auth list` (commands.ts:151/:404):
/// `default` or `json`, default `default`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Default,
    Json,
}

impl OutputFormat {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Json => "json",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `session list --format` (commands.ts:431): `table` or `json`,
/// default `table`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum SessionListFormat {
    #[default]
    Table,
    Json,
}

impl SessionListFormat {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Json => "json",
        }
    }
}

impl std::fmt::Display for SessionListFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `debug paths <name>` (commands.ts:125-136): the ten v2 path keys.
/// The ref handler index-crashes on an unknown key; the port validates
/// at parse time (graceful clap invalid-value error).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PathName {
    Db,
    Home,
    Data,
    Config,
    Cache,
    State,
    Tmp,
    Bin,
    Log,
    Repos,
}

impl PathName {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Db => "db",
            Self::Home => "home",
            Self::Data => "data",
            Self::Config => "config",
            Self::Cache => "cache",
            Self::State => "state",
            Self::Tmp => "tmp",
            Self::Bin => "bin",
            Self::Log => "log",
            Self::Repos => "repos",
        }
    }
}

/// `completion <shell>` (rust-extra, RC-11 — the v2 ref has no
/// completion command): the four clap_complete 4 shells the port
/// generates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
}

impl CompletionShell {
    #[must_use]
    pub fn clap_complete_shell(self) -> clap_complete::Shell {
        match self {
            Self::Bash => clap_complete::Shell::Bash,
            Self::Zsh => clap_complete::Shell::Zsh,
            Self::Fish => clap_complete::Shell::Fish,
            Self::Elvish => clap_complete::Shell::Elvish,
        }
    }
}

/// RC-12 — the ref Schema's `maxItems: 100` on the api header list;
/// clap has no multi-occurrence count value_parser, so the check runs
/// as a post-clap parse-time validation (see [`Cli::validate_extra`]).
pub const MAX_API_HEADERS: usize = 100;

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Upgrade `OpenCode` to the latest or a specific version
    #[command(alias = "update")]
    Upgrade {
        /// Version to upgrade to (with or without a leading v)
        target: Option<String>,
        /// Installation method to use
        #[arg(short = 'm', long)]
        method: Option<UpgradeMethod>,
    },
    /// Uninstall `OpenCode` and remove all related files
    Uninstall {
        /// Keep configuration files
        #[arg(short = 'c', long)]
        keep_config: bool,
        /// Keep session data and snapshots
        #[arg(short = 'd', long)]
        keep_data: bool,
        /// Show what would be removed without removing
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompts
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Start an Agent Client Protocol server
    Acp,
    /// Make a request to the running server
    Api {
        /// `OpenAPI` operation ID, or an HTTP method followed by a path
        #[arg(value_name = "operation | method path", num_args = 1..=2, required = true)]
        request: Vec<String>,
        /// Request body
        #[arg(short = 'd', long)]
        data: Option<String>,
        /// Request header in name:value form
        #[arg(short = 'H', long, value_name = "HEADER", action = ArgAction::Append)]
        header: Vec<String>,
        /// `OpenAPI` path or query parameter
        #[arg(long, value_name = "KEY=VALUE", action = ArgAction::Append)]
        param: Vec<String>,
    },
    /// Debugging and troubleshooting tools
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    /// manage integrations and credentials
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// Manage MCP (Model Context Protocol) servers
    Mcp {
        #[command(subcommand)]
        command: McpCommands,
    },
    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
    /// List all available models
    Models,
    /// Show shareable usage statistics
    Stats {
        /// Show the last N days; 0 means today
        #[arg(long)]
        days: Option<u32>,
        /// Show a calendar year (1970-9999; ref Schema range)
        #[arg(long, value_parser = clap::value_parser!(u32).range(1970..=9999))]
        year: Option<u32>,
        /// Show lifetime statistics
        #[arg(long)]
        all: bool,
        /// Filter by project ID, or use "." for the current project
        #[arg(long)]
        project: Option<String>,
        /// Show model usage
        #[arg(long)]
        models: bool,
        /// Show tool reliability
        #[arg(long)]
        tools: bool,
        /// Show cost and token details
        #[arg(long)]
        cost: bool,
        /// Show every detailed section
        #[arg(long)]
        full: bool,
        /// Number of rows in detailed sections
        #[arg(long, default_value_t = 5)]
        limit: u32,
        /// Output statistics as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the minimal interactive interface
    Mini {
        /// Continue the last session
        #[arg(short = 'c', long)]
        continue_: bool,
        /// Session ID to continue, or to create if it does not exist
        #[arg(short = 's', long, value_name = "ID")]
        session: Option<String>,
        /// Fork the session when continuing
        #[arg(long)]
        fork: bool,
        /// Restore session history on resume and resize (disable with
        /// --no-replay). Absent means "consult `mini.replay` in
        /// cli.json, then default on" (v2
        /// `input.replay ?? resolved.mini?.replay ?? true`).
        #[arg(long, overrides_with = "mini_no_replay")]
        replay: bool,
        /// `--no-replay` (v2 `Flag.boolean("replay").optional`'s
        /// negation form).
        #[arg(long = "no-replay", hide = true)]
        mini_no_replay: bool,
        /// Limit replay to the newest N messages (default: 200)
        #[arg(long)]
        replay_limit: Option<u32>,
        /// Model to use in the format provider/model
        #[arg(short = 'm', long)]
        model: Option<String>,
        /// Agent to use
        #[arg(long)]
        agent: Option<String>,
        /// Prompt to use
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long, hide = true)]
        demo: bool,
    },
    /// Run `OpenCode` with a message
    Run {
        /// Message to send
        #[arg(value_name = "MESSAGE")]
        message: Vec<String>,
        /// Continue the last session
        #[arg(short = 'c', long)]
        continue_: bool,
        /// Session ID to continue, or to create if it does not exist
        #[arg(short = 's', long, value_name = "ID")]
        session: Option<String>,
        /// Fork the session before continuing
        #[arg(long)]
        fork: bool,
        /// Model to use in the format provider/model#variant
        #[arg(short = 'm', long)]
        model: Option<String>,
        /// Agent to use
        #[arg(long)]
        agent: Option<String>,
        /// Output format
        #[arg(long, default_value_t = OutputFormat::Default)]
        format: OutputFormat,
        /// File to attach to the message
        #[arg(short = 'f', long, value_name = "FILE", action = ArgAction::Append)]
        file: Vec<String>,
        /// Session title
        #[arg(long)]
        title: Option<String>,
        /// Show thinking blocks
        #[arg(long)]
        thinking: bool,
    },
    /// Manage sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Manage the background server
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    /// Reload configuration
    Reload,
    /// Print one-time links to connect a browser or app
    Pair {
        /// Use an external HTTP(S) server URL in pairing links
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// Generate shell completion scripts (rust-extra: the v2 ref has
    /// no completion command)
    Completion {
        /// Shell to generate the completion script for
        shell: CompletionShell,
    },
    /// Start the v2 API and web server
    Serve {
        #[arg(long)]
        hostname: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// Additional allowed CORS origin (repeat for multiple origins)
        #[arg(long, action = ArgAction::Append)]
        cors: Vec<String>,
        #[arg(long)]
        service: bool,
        #[arg(long)]
        stdio: bool,
    },
}

impl Cli {
    /// RC-12 — parse `args` with clap, then run the parse-time range
    /// checks clap cannot express as value_parsers (multi-occurrence
    /// counts). Errors are `clap::Error`s rendered like the other
    /// arg errors, so callers just `exit()` on `Err`.
    pub fn try_parse_from_checked<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Self::try_parse_from(args)?;
        cli.validate_extra()?;
        Ok(cli)
    }

    /// RC-12 — `api --header`: the ref Schema caps the header list at
    /// `maxItems: 100`; more than that dies at parse time.
    fn validate_extra(&self) -> Result<(), clap::Error> {
        if let Some(Commands::Api { header, .. }) = &self.command {
            if header.len() > MAX_API_HEADERS {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!(
                        "error: too many --header values ({} given, at most {} allowed)\n",
                        header.len(),
                        MAX_API_HEADERS
                    ),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Subcommand, Debug)]
pub enum DebugCommands {
    /// List all agents
    Agents,
    /// List configuration sources
    Config,
    /// Show global paths (data, config, cache, state)
    Paths {
        /// Print only one path: db, home, data, config, cache, state, tmp, bin, log, repos
        name: Option<PathName>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// list integrations and credentials
    List {
        /// Output format
        #[arg(long, default_value_t = OutputFormat::Default)]
        format: OutputFormat,
    },
    /// connect an integration
    Login {
        /// Integration ID, name, or well-known provider URL
        target: Option<String>,
        /// Authentication method ID
        #[arg(long)]
        method: Option<String>,
        /// Provider form answer (key=value; repeat for multiple fields)
        #[arg(long, value_name = "KEY=VALUE", action = ArgAction::Append)]
        answer: Vec<String>,
    },
    /// log out of a saved account
    Logout {
        /// Integration ID or name
        target: Option<String>,
        /// Credential ID or label (opens an account picker when omitted)
        credential: Option<String>,
    },
    /// print stored credentials, including secrets, as JSON
    Export {
        /// Integration ID or name (exports every integration when omitted)
        target: Option<String>,
    },
    /// import credentials exported by auth export
    Import {
        /// JSON file to import (reads stdin when omitted)
        file: Option<String>,
    },
    /// switch the active account for an integration
    Switch {
        /// Integration ID or name
        target: Option<String>,
        /// Credential ID or label (opens an account picker when omitted)
        credential: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum McpCommands {
    /// List configured MCP servers and their status
    List,
    /// Add an MCP server to your configuration
    Add {
        /// Name of the MCP server
        name: String,
        /// Command and arguments for a local server, passed after --
        #[arg(value_name = "COMMAND", trailing_var_arg = true)]
        command: Vec<String>,
        /// URL for a remote MCP server
        #[arg(long)]
        url: Option<String>,
        /// HTTP header for a remote server, as name=value
        #[arg(long, value_name = "KEY=VALUE")]
        header: Option<String>,
        /// Environment variable for a local server, as name=value
        #[arg(long, value_name = "KEY=VALUE")]
        env: Option<String>,
        /// Write to the global config instead of the project config
        #[arg(long)]
        global: bool,
        /// Discard JSONC comments and rewrite the whole config (the default write preserves comments; rust-extra, v2 has no --force)
        #[arg(long)]
        force: bool,
    },
    /// Authenticate with an OAuth-capable remote MCP server
    Auth {
        /// Name of the MCP server
        name: Option<String>,
    },
    /// Remove stored OAuth credentials for an MCP server
    Logout {
        /// Name of the MCP server
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum PluginCommands {
    /// List plugins
    List {
        /// Include built-in server plugins
        #[arg(long)]
        builtin: bool,
    },
    /// Install a plugin and add it to the global configuration
    Add {
        /// npm registry or Git package specifier
        package: String,
        /// Discard JSONC comments and rewrite the whole config (the default write preserves comments; rust-extra, v2 has no --force)
        #[arg(long)]
        force: bool,
    },
    /// Check package plugins for updates
    Check {
        /// Configured package target
        target: Option<String>,
    },
    /// Update package plugins
    Update {
        /// Configured package target; omit to update all outdated plugins
        target: Option<String>,
    },
    /// Remove a plugin from global configuration
    Remove {
        /// configured package specifier
        package: String,
        /// Discard JSONC comments and rewrite the whole config (the default write preserves comments; rust-extra, v2 has no --force)
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum SessionCommands {
    /// List top-level sessions in the current project, newest first
    List {
        /// Limit to N most recent sessions (default: 100)
        #[arg(short = 'n', long, value_name = "N")]
        max_count: Option<u32>,
        /// Output format
        #[arg(long, default_value_t = SessionListFormat::Table)]
        format: SessionListFormat,
    },
    /// Delete a session and its child sessions
    Delete {
        /// Session ID to delete
        session_id: String,
    },
    /// Export session data as JSON
    Export {
        /// Session ID to export
        session: Option<String>,
        /// Redact sensitive transcript and file data
        #[arg(long)]
        sanitize: bool,
    },
    /// Import session data from a JSON file or URL
    Import {
        /// JSON file or URL to import
        file: String,
        /// Directory in which to import the session
        #[arg(long)]
        directory: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    /// Start the background server
    Start,
    /// Restart the background server
    Restart,
    /// Show background server status
    Status,
    /// Stop the background server
    Stop,
    /// Get service configuration
    Get {
        /// Service setting or env
        key: Option<String>,
        /// Environment variable name
        name: Option<String>,
    },
    /// Set service configuration
    Set {
        /// Service setting or env
        key: String,
        /// Setting value or environment variable name
        value: String,
        /// Environment variable value
        #[arg(value_name = "env-value")]
        nested_value: Option<String>,
    },
    /// Unset service configuration
    Unset {
        /// Service setting or env
        key: String,
        /// Environment variable name
        name: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--prompt`/`--auto` parse onto the root spec (v2
    /// commands.ts:27-33 + :55-57 — the flags BE-1 defined but never
    /// threaded; dogfood3 wires them into the TUI's `BootArgs`).
    #[test]
    fn prompt_and_auto_flags_parse_on_the_root() {
        let cli = Cli::try_parse_from(["opencode", "--prompt", "ship the fix", "--auto", "--yolo"])
            .expect("parse");
        assert_eq!(cli.prompt.as_deref(), Some("ship the fix"));
        assert!(cli.permission.auto, "--auto");
        assert!(cli.permission.effective_auto(), "--yolo folds into effective_auto");
        // Defaults: no prompt, no auto (v2's Flag.withDefault(false)).
        let bare = Cli::try_parse_from(["opencode"]).expect("bare parse");
        assert!(bare.prompt.is_none());
        assert!(!bare.permission.auto);
        assert!(!bare.permission.effective_auto());
    }

    /// G-13: the five former plain-string fields are clap `ValueEnum`s —
    /// invalid values die at PARSE time with clap's invalid-value error
    /// listing the valid choices (the ref choice sets, commands.ts:68/
    /// :151/:404/:431/:125-136).
    #[test]
    fn enum_fields_reject_invalid_values_at_parse_time() {
        let cases: [(&str, Vec<&str>, &[&str]); 5] = [
            (
                "upgrade --method git",
                vec!["opencode", "upgrade", "-m", "git"],
                &["curl", "npm", "pnpm", "bun", "yarn", "vp", "brew", "local-file", "github"],
            ),
            ("run --format xml", vec!["opencode", "run", "--format", "xml"], &["default", "json"]),
            (
                "auth list --format xml",
                vec!["opencode", "auth", "list", "--format", "xml"],
                &["default", "json"],
            ),
            (
                "session list --format xml",
                vec!["opencode", "session", "list", "--format", "xml"],
                &["table", "json"],
            ),
            (
                "debug paths bogus",
                vec!["opencode", "debug", "paths", "bogus"],
                &["db", "home", "data", "config", "cache", "state", "tmp", "bin", "log", "repos"],
            ),
        ];
        for (label, argv, choices) in cases {
            let error = Cli::try_parse_from(argv).unwrap_err();
            let rendered = error.to_string();
            assert!(rendered.contains("invalid value"), "{label}: {rendered}");
            for choice in choices {
                assert!(rendered.contains(choice), "{label} must list {choice}: {rendered}");
            }
        }
    }

    /// G-13: valid values keep parsing exactly as the plain strings did.
    #[test]
    fn enum_fields_accept_the_ref_choice_sets() {
        let upgrade = Cli::try_parse_from(["opencode", "upgrade", "-m", "curl"]).expect("parse");
        let Some(Commands::Upgrade { method, .. }) = upgrade.command else {
            panic!("upgrade subcommand");
        };
        assert_eq!(method, Some(super::UpgradeMethod::Curl));

        let run =
            Cli::try_parse_from(["opencode", "run", "--format", "json", "hi"]).expect("parse");
        let Some(Commands::Run { format, .. }) = run.command else {
            panic!("run subcommand");
        };
        assert_eq!(format, super::OutputFormat::Json);

        let auth =
            Cli::try_parse_from(["opencode", "auth", "list", "--format", "json"]).expect("parse");
        let Some(Commands::Auth { command: AuthCommands::List { format } }) = auth.command else {
            panic!("auth list subcommand");
        };
        assert_eq!(format, super::OutputFormat::Json);

        let session = Cli::try_parse_from(["opencode", "session", "list", "--format", "json"])
            .expect("parse");
        let Some(Commands::Session { command: SessionCommands::List { format, .. } }) =
            session.command
        else {
            panic!("session list subcommand");
        };
        assert_eq!(format, super::SessionListFormat::Json);

        let paths = Cli::try_parse_from(["opencode", "debug", "paths", "repos"]).expect("parse");
        let Some(Commands::Debug { command: DebugCommands::Paths { name } }) = paths.command else {
            panic!("debug paths subcommand");
        };
        assert_eq!(name, Some(super::PathName::Repos));
    }

    /// G-13: the defaults round out the contract (run/auth `default`,
    /// session `table`, every method/`paths` key accepted once).
    #[test]
    fn enum_fields_default_like_the_ref() {
        let run = Cli::try_parse_from(["opencode", "run", "hi"]).expect("parse");
        let Some(Commands::Run { format, .. }) = run.command else { panic!("run") };
        assert_eq!(format, super::OutputFormat::Default);

        let auth = Cli::try_parse_from(["opencode", "auth", "list"]).expect("parse");
        let Some(Commands::Auth { command: AuthCommands::List { format } }) = auth.command else {
            panic!("auth list");
        };
        assert_eq!(format, super::OutputFormat::Default);

        let session = Cli::try_parse_from(["opencode", "session", "list"]).expect("parse");
        let Some(Commands::Session { command: SessionCommands::List { format, .. } }) =
            session.command
        else {
            panic!("session list");
        };
        assert_eq!(format, super::SessionListFormat::Table);

        let bare = Cli::try_parse_from(["opencode", "debug", "paths"]).expect("parse");
        let Some(Commands::Debug { command: DebugCommands::Paths { name } }) = bare.command else {
            panic!("debug paths");
        };
        assert_eq!(name, None);

        for method in ["curl", "npm", "pnpm", "bun", "yarn", "vp", "brew"] {
            let parsed = Cli::try_parse_from(["opencode", "upgrade", "-m", method])
                .unwrap_or_else(|error| panic!("{method}: {error}"));
            assert!(matches!(parsed.command, Some(Commands::Upgrade { .. })), "{method} parses");
        }
    }

    // --- RC-11: completion subcommand --------------------------------

    /// RC-11 (rust-extra): every supported shell parses onto the
    /// command tree; an unknown shell dies with clap's invalid-value
    /// error listing the choices.
    #[test]
    fn completion_parses_every_shell_and_rejects_unknown() {
        for shell in ["bash", "zsh", "fish", "elvish"] {
            let parsed =
                Cli::try_parse_from(["opencode", "completion", shell]).unwrap_or_else(|e| {
                    panic!("{shell}: {e}");
                });
            assert!(matches!(parsed.command, Some(Commands::Completion { .. })), "{shell} parses");
        }
        let error = Cli::try_parse_from(["opencode", "completion", "powershell"]).unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.contains("invalid value"), "{rendered}");
        for choice in ["bash", "zsh", "fish", "elvish"] {
            assert!(rendered.contains(choice), "must list {choice}: {rendered}");
        }
    }

    // --- RC-12: parse-time range validation --------------------------

    /// RC-12: `api --header` accepts at most 100 values (ref Schema
    /// `maxItems: 100`); the boundary is exact and the error names
    /// the limit.
    #[test]
    fn api_header_rejects_more_than_100_at_parse_time() {
        let ok: Vec<String> = (0..MAX_API_HEADERS).map(|i| format!("x-h{i}:v")).collect();
        let mut argv =
            vec!["opencode".to_string(), "api".to_string(), "get".to_string(), "/x".to_string()];
        for header in &ok {
            argv.push("--header".into());
            argv.push(header.clone());
        }
        Cli::try_parse_from_checked(argv).expect("exactly 100 headers parse");

        let mut argv =
            vec!["opencode".to_string(), "api".to_string(), "get".to_string(), "/x".to_string()];
        for header in ok {
            argv.push("--header".into());
            argv.push(format!("{header}extra"));
        }
        argv.push("--header".into());
        argv.push("one:too-many".into());
        let error = Cli::try_parse_from_checked(argv).unwrap_err();
        let rendered = error.to_string();
        assert!(rendered.contains("101"), "reports the count: {rendered}");
        assert!(rendered.contains("100"), "reports the limit: {rendered}");
    }

    /// RC-12: `stats --year` requires 1970..=9999 (ref Schema range)
    /// at the exact boundaries.
    #[test]
    fn stats_year_range_boundaries() {
        for good in [1970u32, 9999] {
            Cli::try_parse_from_checked(["opencode", "stats", "--year", &good.to_string()])
                .unwrap_or_else(|e| panic!("{good} parses: {e}"));
        }
        for bad in [1969u32, 10_000] {
            let error =
                Cli::try_parse_from_checked(["opencode", "stats", "--year", &bad.to_string()])
                    .unwrap_err();
            let rendered = error.to_string();
            assert!(rendered.contains("invalid value"), "{bad}: {rendered}");
            assert!(rendered.contains("1970..=9999"), "{bad} names the range: {rendered}");
        }
    }
}
