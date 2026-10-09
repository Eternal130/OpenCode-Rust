//! The clap command tree — a 1:1 port of `cli/src/commands/commands.ts`
//! (names, aliases, flags, hidden flags). Effect-CLI per-command params map
//! onto clap args; the six shared flags (`--print-logs`, `--standalone`,
//! `--server`, `--auto`, hidden `--yolo`/`--dangerously-skip-permissions`)
//! are declared once as globals so every subcommand surface that v2 gives
//! them accepts them.

use clap::{ArgAction, Parser, Subcommand};

/// Shared connection params (v2 `ServerParams`) — filled from the global
/// `--standalone` / `--server` flags.
#[derive(Debug, Clone, Default)]
pub struct ServerParams {
    pub standalone: bool,
    pub server: Option<String>,
}

#[derive(Parser, Debug)]
#[command(
    name = "opencode",
    version = concat!("v", env!("CARGO_PKG_VERSION")),
    about = "OpenCode command line interface",
    disable_help_subcommand = true,
    arg_required_else_help = false
)]
pub struct Cli {
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

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Upgrade `OpenCode` to the latest or a specific version
    #[command(alias = "update")]
    Upgrade {
        /// Version to upgrade to (with or without a leading v)
        target: Option<String>,
        /// Installation method to use
        #[arg(short = 'm', long)]
        method: Option<String>,
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
        /// Show a calendar year
        #[arg(long)]
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
        /// Restore session history on resume and resize (disable with --no-replay)
        #[arg(long, action = ArgAction::Set, default_value_t = true)]
        replay: bool,
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
        #[arg(long, default_value = "default")]
        format: String,
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

#[derive(Subcommand, Debug)]
pub enum DebugCommands {
    /// List all agents
    Agents,
    /// List configuration sources
    Config,
    /// Show global paths (data, config, cache, state)
    Paths {
        /// Print only one path: db, home, data, config, cache, state, tmp, bin, log, repos
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// list integrations and credentials
    List {
        /// Output format
        #[arg(long, default_value = "default")]
        format: String,
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
        #[arg(long, default_value = "table")]
        format: String,
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
