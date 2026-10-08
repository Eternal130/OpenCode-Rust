//! Port of `config.ts` and `config/*` — the `opencode.json(c)` schema.
//!
//! Naming follows v2: config keys are `snake_case` at the top level
//! (`default_agent`, `tool_output`, …) while nested provider/model settings
//! reuse the runtime camelCase shapes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::permission::Ruleset;

pub mod agent;
pub mod command;
pub mod compaction;
pub mod experimental;
pub mod formatter;
pub mod lsp;
pub mod mcp;
pub mod media;
pub mod model;
pub mod plugin;
pub mod policy;
pub mod provider;
pub mod reference;
pub mod tool_output;
pub mod warming;
pub mod watcher;
pub mod websearch;
pub mod worktree;

pub use agent::Agent;
pub use command::Command;
pub use compaction::Compaction;
pub use experimental::Experimental;
pub use formatter::Formatter;
pub use lsp::Lsp;
pub use mcp::Mcp;
pub use media::Media;
pub use model::ModelSelection;
pub use plugin::{Plugin, Plugins};
pub use policy::Policy;
pub use provider::Provider;
pub use reference::Reference;
pub use tool_output::ToolOutput;
pub use warming::Warming;
pub use watcher::Watcher;
pub use websearch::WebSearchSelection;
pub use worktree::Worktree;

/// `Config.Info` — the top-level `opencode.json` document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    /// JSON schema reference for configuration validation.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Default shell to use for terminal and shell tool execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Default model when no session or agent model is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSelection>,
    /// Default primary agent when no session agent is selected.
    #[serde(rename = "default_agent", default, skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    /// Disable updates, notify when available, or install automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<Update>,
    /// Whether sessions may be shared manually, automatically, or not at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<Share>,
    /// Enterprise sharing service configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise: Option<Enterprise>,
    /// Username displayed in conversations and used for telemetry identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Ordered tool permission rules applied to agent tool use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Ruleset>,
    /// Named built-in agent overrides and custom agent definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<BTreeMap<String, Agent>>,
    /// Enable snapshots used for undo and revert behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    /// Filesystem watcher configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watcher: Option<Watcher>,
    /// Enable built-in formatters or configure formatter overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formatter: Option<Formatter>,
    /// Enable built-in language servers or configure server overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsp: Option<Lsp>,
    /// Media processing configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    /// Tool output truncation thresholds.
    #[serde(rename = "tool_output", default, skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<ToolOutput>,
    /// MCP server configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<Mcp>,
    /// Conversation compaction behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<Compaction>,
    /// Additional paths or URLs to discover skills from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    /// Named slash command definitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<BTreeMap<String, Command>>,
    /// Additional paths or URLs supplying ambient instructions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,
    /// Named local directories or Git repositories available as context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Reference>,
    /// Web search provider selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub websearch: Option<WebSearchSelection>,
    /// Ordered plugin enablement directives and external package declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Plugins>,
    /// Directory defaults for local worktree creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<Worktree>,
    /// Keep recently active sessions warm with transient requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warming: Option<Warming>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<BTreeMap<String, Provider>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Experimental>,
}

/// `Config.Info.update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Update {
    Disable,
    Notify,
    Auto,
}

/// `Config.Info.share`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Share {
    Manual,
    Auto,
    Disabled,
}

/// `Config.Info.enterprise`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Enterprise {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// `Config.Patch` — `shell` is `string | null` (always emitted).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Patch {
    pub shell: Option<String>,
}

/// `Config.Document` — one loaded config file. The `type` tag is supplied by
/// the [`Entry`] union; standalone use re-adds it via the envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Document {
    pub info: Info,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// `Config.Directory` — a plugin/config source directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Directory {
    pub path: String,
}

/// `Config.Entry` — document or directory, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Entry {
    /// A parsed config document.
    Document(Box<Document>),
    /// A directory scanned for config/plugins.
    Directory(Directory),
}

/// Value re-export used by provider overlays.
#[allow(dead_code)]
type _Value = Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_minimal_document() {
        let entry: Entry =
            serde_json::from_value(serde_json::json!({"type": "document", "info": {}})).unwrap();
        assert!(matches!(entry, Entry::Document(ref document) if document.info.model.is_none()));
        assert_eq!(serde_json::to_value(&entry).unwrap()["type"], "document");
    }

    #[test]
    fn entry_union_wire() {
        let value = serde_json::json!({
            "type": "directory",
            "path": "/repo/.opencode"
        });
        let parsed: Entry = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Entry::Directory(_)));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }
}
