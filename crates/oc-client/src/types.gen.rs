// Generated file — DO NOT EDIT by hand.
// Regenerate with: cargo run -p openapi-gen
// Source: tools/openapi-gen/openapi.json (sha256 0612418392961ef22f0337ce0e1e520270917665fc85f29c4af1c94d1e1873ee)
//! Wire types for the opencode HttpApi (764 generated types).
//! Field names use exact wire spellings via `#[serde(rename)]` — camelCase,
//! including `ID` suffixes (`sessionID`, `parentID`, ...).
//! Unions: discriminated where the schema provides a singleton-enum tag,
//! `#[serde(untagged)]` otherwise (variant order follows the spec).
#![allow(clippy::large_enum_variant, clippy::struct_excessive_bools)]
#![allow(clippy::doc_markdown, clippy::must_use_candidate)]

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueryLocation {
    /// Wire field: `directory`.
    pub directory: Option<String>,
}

/// Wire schema: `Agent.Color`.
pub type AgentColor = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum AgentInfoMode {
    #[serde(rename = "subagent")]
    Subagent,
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "all")]
    All,
}

impl AgentInfoMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subagent => "subagent",
            Self::Primary => "primary",
            Self::All => "all",
        }
    }
}

/// Wire schema: `Model.Ref`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelRef {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `variant`.
    #[serde(default, rename = "variant", skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Wire schema: `Permission.Effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PermissionEffect {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

impl PermissionEffect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

/// Wire schema: `Permission.Rule`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionRule {
    /// Wire field: `action`.
    #[serde(rename = "action")]
    pub action: String,
    /// Wire field: `effect`.
    #[serde(rename = "effect")]
    pub effect: PermissionEffect,
    /// Wire field: `resource`.
    #[serde(rename = "resource")]
    pub resource: String,
}

/// Wire schema: `Permission.Ruleset`.
pub type PermissionRuleset = Vec<PermissionRule>;

/// Untagged union; serde matches variants in declared order (wire schema `ProviderSettingsChunkTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ProviderSettingsChunkTimeout {
    Number(f64),
    Boolean(bool),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ProviderCompactionVariant1Type {
    #[default]
    #[serde(rename = "summary")]
    Summary,
}

impl ProviderCompactionVariant1Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Summary => "summary",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCompactionVariant1 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ProviderCompactionVariant1Type,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ProviderCompactionVariant2Type {
    #[default]
    #[serde(rename = "native")]
    Native,
}

impl ProviderCompactionVariant2Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Native => "native",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderCompactionVariant2 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ProviderCompactionVariant2Type,
}

/// Wire schema: `Provider.Compaction`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Provider.Compaction`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ProviderCompaction {
    Summary(ProviderCompactionVariant1),
    Native(ProviderCompactionVariant2),
}

/// Untagged union; serde matches variants in declared order (wire schema `ProviderSettingsHeaderTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ProviderSettingsHeaderTimeout {
    Number(f64),
    Boolean(bool),
}

/// Untagged union; serde matches variants in declared order (wire schema `ProviderSettingsTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ProviderSettingsTimeout {
    Number(f64),
    Boolean(bool),
}

/// Wire schema: `Provider.Transport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProviderTransport {
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "websocket")]
    Websocket,
}

impl ProviderTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Websocket => "websocket",
        }
    }
}

/// Wire schema: `Provider.Settings`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderSettings {
    /// Wire field: `chunkTimeout`.
    #[serde(default, rename = "chunkTimeout", skip_serializing_if = "Option::is_none")]
    pub chunk_timeout: Option<ProviderSettingsChunkTimeout>,
    /// Wire field: `compaction`.
    #[serde(default, rename = "compaction", skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ProviderCompaction>,
    /// Wire field: `headerTimeout`.
    #[serde(default, rename = "headerTimeout", skip_serializing_if = "Option::is_none")]
    pub header_timeout: Option<ProviderSettingsHeaderTimeout>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<ProviderSettingsTimeout>,
    /// Wire field: `transport`.
    #[serde(default, rename = "transport", skip_serializing_if = "Option::is_none")]
    pub transport: Option<ProviderTransport>,
}

/// Wire schema: `Provider.Request`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRequest {
    /// Wire field: `body`.
    #[serde(rename = "body")]
    pub body: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `headers`.
    #[serde(rename = "headers")]
    pub headers: std::collections::BTreeMap<String, String>,
    /// Wire field: `settings`.
    #[serde(rename = "settings")]
    pub settings: ProviderSettings,
}

/// Wire schema: `Agent.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentInfo {
    /// Wire field: `color`.
    #[serde(default, rename = "color", skip_serializing_if = "Option::is_none")]
    pub color: Option<AgentColor>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(rename = "hidden")]
    pub hidden: bool,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `mode`.
    #[serde(rename = "mode")]
    pub mode: AgentInfoMode,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `permissions`.
    #[serde(rename = "permissions")]
    pub permissions: PermissionRuleset,
    /// Wire field: `request`.
    #[serde(rename = "request")]
    pub request: ProviderRequest,
    /// Wire field: `steps`.
    #[serde(default, rename = "steps", skip_serializing_if = "Option::is_none")]
    pub steps: Option<i64>,
    /// Wire field: `system`.
    #[serde(default, rename = "system", skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AgentNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "AgentNotFoundError")]
    AgentNotFoundError,
}

impl AgentNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentNotFoundError => "AgentNotFoundError",
        }
    }
}

/// Wire schema: `AgentNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: AgentNotFoundErrorEncodedTag,
    /// Wire field: `agentID`.
    #[serde(rename = "agentID")]
    pub agent_id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

/// Wire schema: `Command.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandInfo {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CommandExecutionErrorEncodedTag {
    #[default]
    #[serde(rename = "CommandExecutionError")]
    CommandExecutionError,
}

impl CommandExecutionErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CommandExecutionError => "CommandExecutionError",
        }
    }
}

/// Wire schema: `CommandExecutionErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandExecutionErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: CommandExecutionErrorEncodedTag,
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CommandNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "CommandNotFoundError")]
    CommandNotFoundError,
}

impl CommandNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CommandNotFoundError => "CommandNotFoundError",
        }
    }
}

/// Wire schema: `CommandNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: CommandNotFoundErrorEncodedTag,
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConfigAgentEncodedMode {
    #[serde(rename = "subagent")]
    Subagent,
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "all")]
    All,
}

impl ConfigAgentEncodedMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Subagent => "subagent",
            Self::Primary => "primary",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigAgentEncodedModelVariant2 {
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `variant`.
    #[serde(default, rename = "variant", skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigAgentEncodedModel`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigAgentEncodedModel {
    String(String),
    ConfigAgentEncodedModelVariant2(ConfigAgentEncodedModelVariant2),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigAgentEncodedRequest {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
}

/// Wire schema: `Config.AgentEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigAgentEncoded {
    /// Wire field: `color`.
    #[serde(default, rename = "color", skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `mode`.
    #[serde(default, rename = "mode", skip_serializing_if = "Option::is_none")]
    pub mode: Option<ConfigAgentEncodedMode>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ConfigAgentEncodedModel>,
    /// Wire field: `permissions`.
    #[serde(default, rename = "permissions", skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionRuleset>,
    /// Wire field: `request`.
    #[serde(default, rename = "request", skip_serializing_if = "Option::is_none")]
    pub request: Option<ConfigAgentEncodedRequest>,
    /// Wire field: `steps`.
    #[serde(default, rename = "steps", skip_serializing_if = "Option::is_none")]
    pub steps: Option<i64>,
    /// Wire field: `system`.
    #[serde(default, rename = "system", skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigCommandEncodedModelVariant2 {
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `variant`.
    #[serde(default, rename = "variant", skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigCommandEncodedModel`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigCommandEncodedModel {
    String(String),
    ConfigCommandEncodedModelVariant2(ConfigCommandEncodedModelVariant2),
}

/// Wire schema: `Config.CommandEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigCommandEncoded {
    /// Wire field: `agent`.
    #[serde(default, rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ConfigCommandEncodedModel>,
    /// Wire field: `subagent`.
    #[serde(default, rename = "subagent", skip_serializing_if = "Option::is_none")]
    pub subagent: Option<bool>,
    /// Deprecated alias for subagent.
    #[serde(default, rename = "subtask", skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
    /// Wire field: `template`.
    #[serde(rename = "template")]
    pub template: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConfigDirectoryEncodedType {
    #[default]
    #[serde(rename = "directory")]
    Directory,
}

impl ConfigDirectoryEncodedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Directory => "directory",
        }
    }
}

/// Wire schema: `Config.DirectoryEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigDirectoryEncoded {
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ConfigDirectoryEncodedType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedCompactionKeep {
    /// Wire field: `tokens`.
    #[serde(default, rename = "tokens", skip_serializing_if = "Option::is_none")]
    pub tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedCompaction {
    /// Wire field: `auto`.
    #[serde(default, rename = "auto", skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    /// Wire field: `buffer`.
    #[serde(default, rename = "buffer", skip_serializing_if = "Option::is_none")]
    pub buffer: Option<i64>,
    /// Wire field: `keep`.
    #[serde(default, rename = "keep", skip_serializing_if = "Option::is_none")]
    pub keep: Option<ConfigInfoEncodedCompactionKeep>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedEnterprise {
    /// Wire field: `url`.
    #[serde(default, rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConfigInfoEncodedExperimentalPoliciesItemAction {
    #[serde(rename = "provider.use")]
    ProviderUse,
    #[serde(rename = "permission")]
    Permission,
}

impl ConfigInfoEncodedExperimentalPoliciesItemAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProviderUse => "provider.use",
            Self::Permission => "permission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConfigInfoEncodedExperimentalPoliciesItemEffect {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
}

impl ConfigInfoEncodedExperimentalPoliciesItemEffect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedExperimentalPoliciesItem {
    /// Wire field: `action`.
    #[serde(rename = "action")]
    pub action: ConfigInfoEncodedExperimentalPoliciesItemAction,
    /// Wire field: `effect`.
    #[serde(rename = "effect")]
    pub effect: ConfigInfoEncodedExperimentalPoliciesItemEffect,
    /// Wire field: `resource`.
    #[serde(rename = "resource")]
    pub resource: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedExperimental {
    /// Wire field: `policies`.
    #[serde(default, rename = "policies", skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<ConfigInfoEncodedExperimentalPoliciesItem>>,
    /// Wire field: `portable_shell_scanner`.
    #[serde(default, rename = "portable_shell_scanner", skip_serializing_if = "Option::is_none")]
    pub portable_shell_scanner: Option<bool>,
    /// Wire field: `subagent_depth`.
    #[serde(default, rename = "subagent_depth", skip_serializing_if = "Option::is_none")]
    pub subagent_depth: Option<i64>,
}

/// Wire schema: `Config.Formatter.EntryEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigFormatterEntryEncoded {
    /// Wire field: `command`.
    #[serde(default, rename = "command", skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `environment`.
    #[serde(default, rename = "environment", skip_serializing_if = "Option::is_none")]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `extensions`.
    #[serde(default, rename = "extensions", skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedFormatter`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedFormatter {
    Boolean(bool),
    Map(std::collections::BTreeMap<String, ConfigFormatterEntryEncoded>),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedLspVariant2ValueVariant1 {
    /// Wire field: `disabled`.
    #[serde(rename = "disabled")]
    pub disabled: bool,
}

/// Wire schema: `Config.LSP.ServerEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigLspServerEncoded {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: Vec<String>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `env`.
    #[serde(default, rename = "env", skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `extensions`.
    #[serde(default, rename = "extensions", skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    /// Wire field: `initialization`.
    #[serde(default, rename = "initialization", skip_serializing_if = "Option::is_none")]
    pub initialization: Option<std::collections::BTreeMap<String, serde_json::Value>>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedLspVariant2Value`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedLspVariant2Value {
    ConfigInfoEncodedLspVariant2ValueVariant1(ConfigInfoEncodedLspVariant2ValueVariant1),
    ServerEncoded(ConfigLspServerEncoded),
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedLsp`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedLsp {
    Boolean(bool),
    Map(std::collections::BTreeMap<String, ConfigInfoEncodedLspVariant2Value>),
}

/// MCP protocol negotiation. "legacy" (default) opens with the initialize handshake and speaks protocol revisions up to 2025-11-25. "auto" probes for the 2026-07-28 revision and falls...
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum McpProtocol {
    #[serde(rename = "legacy")]
    Legacy,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "2026-07-28")]
    X20260728,
}

impl McpProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Auto => "auto",
            Self::X20260728 => "2026-07-28",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpLocalConfigEncodedTimeout {
    /// Wire field: `catalog`.
    #[serde(default, rename = "catalog", skip_serializing_if = "Option::is_none")]
    pub catalog: Option<i64>,
    /// Wire field: `execution`.
    #[serde(default, rename = "execution", skip_serializing_if = "Option::is_none")]
    pub execution: Option<i64>,
    /// Wire field: `startup`.
    #[serde(default, rename = "startup", skip_serializing_if = "Option::is_none")]
    pub startup: Option<i64>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpLocalConfigEncodedType {
    #[default]
    #[serde(rename = "local")]
    Local,
}

impl McpLocalConfigEncodedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
        }
    }
}

/// Wire schema: `Mcp.LocalConfigEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpLocalConfigEncoded {
    /// Wire field: `codemode`.
    #[serde(default, rename = "codemode", skip_serializing_if = "Option::is_none")]
    pub codemode: Option<bool>,
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: Vec<String>,
    /// Wire field: `cwd`.
    #[serde(default, rename = "cwd", skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `environment`.
    #[serde(default, rename = "environment", skip_serializing_if = "Option::is_none")]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `protocol`.
    #[serde(default, rename = "protocol", skip_serializing_if = "Option::is_none")]
    pub protocol: Option<McpProtocol>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<McpLocalConfigEncodedTimeout>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: McpLocalConfigEncodedType,
}

/// Wire schema: `Mcp.OAuthConfigEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpOAuthConfigEncoded {
    /// Wire field: `auth_server_metadata_url`.
    #[serde(default, rename = "auth_server_metadata_url", skip_serializing_if = "Option::is_none")]
    pub auth_server_metadata_url: Option<String>,
    /// Wire field: `callback_port`.
    #[serde(default, rename = "callback_port", skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<i64>,
    /// Wire field: `client_id`.
    #[serde(default, rename = "client_id", skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Wire field: `client_secret`.
    #[serde(default, rename = "client_secret", skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    /// Wire field: `redirect_uri`.
    #[serde(default, rename = "redirect_uri", skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    /// Wire field: `scope`.
    #[serde(default, rename = "scope", skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Untagged union; serde matches variants in declared order (wire schema `McpRemoteConfigEncodedOauth`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum McpRemoteConfigEncodedOauth {
    OAuthConfigEncoded(McpOAuthConfigEncoded),
    Boolean(bool),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpRemoteConfigEncodedTimeout {
    /// Wire field: `catalog`.
    #[serde(default, rename = "catalog", skip_serializing_if = "Option::is_none")]
    pub catalog: Option<i64>,
    /// Wire field: `execution`.
    #[serde(default, rename = "execution", skip_serializing_if = "Option::is_none")]
    pub execution: Option<i64>,
    /// Wire field: `startup`.
    #[serde(default, rename = "startup", skip_serializing_if = "Option::is_none")]
    pub startup: Option<i64>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpRemoteConfigEncodedType {
    #[default]
    #[serde(rename = "remote")]
    Remote,
}

impl McpRemoteConfigEncodedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Remote => "remote",
        }
    }
}

/// Wire schema: `Mcp.RemoteConfigEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpRemoteConfigEncoded {
    /// Wire field: `codemode`.
    #[serde(default, rename = "codemode", skip_serializing_if = "Option::is_none")]
    pub codemode: Option<bool>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `oauth`.
    #[serde(default, rename = "oauth", skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpRemoteConfigEncodedOauth>,
    /// Wire field: `protocol`.
    #[serde(default, rename = "protocol", skip_serializing_if = "Option::is_none")]
    pub protocol: Option<McpProtocol>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<McpRemoteConfigEncodedTimeout>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: McpRemoteConfigEncodedType,
    /// Wire field: `url`.
    #[serde(rename = "url")]
    pub url: String,
}

/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `ConfigInfoEncodedMcpServersValue`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedMcpServersValue {
    LocalConfigEncoded(McpLocalConfigEncoded),
    RemoteConfigEncoded(McpRemoteConfigEncoded),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedMcpTimeout {
    /// Wire field: `catalog`.
    #[serde(default, rename = "catalog", skip_serializing_if = "Option::is_none")]
    pub catalog: Option<i64>,
    /// Wire field: `execution`.
    #[serde(default, rename = "execution", skip_serializing_if = "Option::is_none")]
    pub execution: Option<i64>,
    /// Wire field: `startup`.
    #[serde(default, rename = "startup", skip_serializing_if = "Option::is_none")]
    pub startup: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedMcp {
    /// Wire field: `servers`.
    #[serde(default, rename = "servers", skip_serializing_if = "Option::is_none")]
    pub servers: Option<std::collections::BTreeMap<String, ConfigInfoEncodedMcpServersValue>>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<ConfigInfoEncodedMcpTimeout>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedMediaImage {
    /// Wire field: `auto_resize`.
    #[serde(default, rename = "auto_resize", skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    /// Wire field: `max_base64_bytes`.
    #[serde(default, rename = "max_base64_bytes", skip_serializing_if = "Option::is_none")]
    pub max_base64_bytes: Option<i64>,
    /// Wire field: `max_height`.
    #[serde(default, rename = "max_height", skip_serializing_if = "Option::is_none")]
    pub max_height: Option<i64>,
    /// Wire field: `max_width`.
    #[serde(default, rename = "max_width", skip_serializing_if = "Option::is_none")]
    pub max_width: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedMedia {
    /// Wire field: `image`.
    #[serde(default, rename = "image", skip_serializing_if = "Option::is_none")]
    pub image: Option<ConfigInfoEncodedMediaImage>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedModelVariant2 {
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `variant`.
    #[serde(default, rename = "variant", skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedModel`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedModel {
    String(String),
    ConfigInfoEncodedModelVariant2(ConfigInfoEncodedModelVariant2),
}

/// Wire schema: `Config.Plugin.EntryEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigPluginEntryEncoded {
    /// Wire field: `options`.
    #[serde(default, rename = "options", skip_serializing_if = "Option::is_none")]
    pub options: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `package`.
    #[serde(rename = "package")]
    pub package: String,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedPluginsItem`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedPluginsItem {
    String(String),
    EntryEncoded(ConfigPluginEntryEncoded),
}

/// Wire schema: `Config.Model.Capabilities`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelCapabilities {
    /// Wire field: `input`.
    #[serde(default, rename = "input", skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<String>>,
    /// Wire field: `output`.
    #[serde(default, rename = "output", skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<String>>,
    /// Wire field: `tools`.
    #[serde(default, rename = "tools", skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
}

/// Wire schema: `Model.MaxTokensField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelMaxTokensField {
    #[serde(rename = "max_completion_tokens")]
    MaxCompletionTokens,
    #[serde(rename = "max_tokens")]
    MaxTokens,
}

impl ModelMaxTokensField {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MaxCompletionTokens => "max_completion_tokens",
            Self::MaxTokens => "max_tokens",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelReasoningFieldVariant1 {
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "reasoning_content")]
    ReasoningContent,
    #[serde(rename = "reasoning_text")]
    ReasoningText,
}

impl ModelReasoningFieldVariant1 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::ReasoningContent => "reasoning_content",
            Self::ReasoningText => "reasoning_text",
        }
    }
}

/// Wire schema: `Model.ReasoningField`.
/// Untagged union; serde matches variants in declared order (wire schema `Model.ReasoningField`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ModelReasoningField {
    ModelReasoningFieldVariant1(ModelReasoningFieldVariant1),
    String(String),
}

/// Wire schema: `Model.Compatibility`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelCompatibility {
    /// Wire field: `maxTokensField`.
    #[serde(default, rename = "maxTokensField", skip_serializing_if = "Option::is_none")]
    pub max_tokens_field: Option<ModelMaxTokensField>,
    /// Wire field: `reasoningField`.
    #[serde(default, rename = "reasoningField", skip_serializing_if = "Option::is_none")]
    pub reasoning_field: Option<ModelReasoningField>,
    /// Wire field: `requireAssistantAfterTool`.
    #[serde(
        default,
        rename = "requireAssistantAfterTool",
        skip_serializing_if = "Option::is_none"
    )]
    pub require_assistant_after_tool: Option<bool>,
    /// Wire field: `requireFinishReason`.
    #[serde(default, rename = "requireFinishReason", skip_serializing_if = "Option::is_none")]
    pub require_finish_reason: Option<bool>,
    /// Wire field: `requireReasoning`.
    #[serde(default, rename = "requireReasoning", skip_serializing_if = "Option::is_none")]
    pub require_reasoning: Option<bool>,
    /// Wire field: `supportsPromptCacheKey`.
    #[serde(default, rename = "supportsPromptCacheKey", skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache_key: Option<bool>,
}

/// Wire schema: `Money.USDPerMillionTokens`.
pub type MoneyUsdPerMillionTokens = f64;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelCostEncodedCache {
    /// Wire field: `read`.
    #[serde(default, rename = "read", skip_serializing_if = "Option::is_none")]
    pub read: Option<MoneyUsdPerMillionTokens>,
    /// Wire field: `write`.
    #[serde(default, rename = "write", skip_serializing_if = "Option::is_none")]
    pub write: Option<MoneyUsdPerMillionTokens>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConfigModelCostEncodedTierType {
    #[default]
    #[serde(rename = "context")]
    Context,
}

impl ConfigModelCostEncodedTierType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Context => "context",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelCostEncodedTier {
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: i64,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ConfigModelCostEncodedTierType,
}

/// Wire schema: `Config.Model.CostEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelCostEncoded {
    /// Wire field: `cache`.
    #[serde(default, rename = "cache", skip_serializing_if = "Option::is_none")]
    pub cache: Option<ConfigModelCostEncodedCache>,
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: MoneyUsdPerMillionTokens,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: MoneyUsdPerMillionTokens,
    /// Wire field: `tier`.
    #[serde(default, rename = "tier", skip_serializing_if = "Option::is_none")]
    pub tier: Option<ConfigModelCostEncodedTier>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigModelEncodedCost`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigModelEncodedCost {
    CostEncoded(ConfigModelCostEncoded),
    Array(Vec<ConfigModelCostEncoded>),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelEncodedLimit {
    /// Wire field: `context`.
    #[serde(default, rename = "context", skip_serializing_if = "Option::is_none")]
    pub context: Option<i64>,
    /// Wire field: `input`.
    #[serde(default, rename = "input", skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    /// Wire field: `output`.
    #[serde(default, rename = "output", skip_serializing_if = "Option::is_none")]
    pub output: Option<i64>,
}

/// Wire schema: `Config.Model.Settings`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelSettings {
    /// Wire field: `compaction`.
    #[serde(default, rename = "compaction", skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ProviderCompaction>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelEncodedVariantsItem {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ConfigModelSettings>,
}

/// Wire schema: `Config.ModelEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigModelEncoded {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `capabilities`.
    #[serde(default, rename = "capabilities", skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ConfigModelCapabilities>,
    /// Wire field: `compatibility`.
    #[serde(default, rename = "compatibility", skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<ModelCompatibility>,
    /// Wire field: `cost`.
    #[serde(default, rename = "cost", skip_serializing_if = "Option::is_none")]
    pub cost: Option<ConfigModelEncodedCost>,
    /// Wire field: `disabled`.
    #[serde(default, rename = "disabled", skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Wire field: `family`.
    #[serde(default, rename = "family", skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `limit`.
    #[serde(default, rename = "limit", skip_serializing_if = "Option::is_none")]
    pub limit: Option<ConfigModelEncodedLimit>,
    /// Wire field: `modelID`.
    #[serde(default, rename = "modelID", skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `package`.
    #[serde(default, rename = "package", skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ConfigModelSettings>,
    /// Wire field: `variants`.
    #[serde(default, rename = "variants", skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<ConfigModelEncodedVariantsItem>>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigProviderSettingsChunkTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigProviderSettingsChunkTimeout {
    Number(f64),
    Boolean(bool),
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigProviderSettingsHeaderTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigProviderSettingsHeaderTimeout {
    Number(f64),
    Boolean(bool),
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigProviderSettingsTimeout`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigProviderSettingsTimeout {
    Number(f64),
    Boolean(bool),
}

/// Wire schema: `Config.Provider.Settings`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigProviderSettings {
    /// Wire field: `chunkTimeout`.
    #[serde(default, rename = "chunkTimeout", skip_serializing_if = "Option::is_none")]
    pub chunk_timeout: Option<ConfigProviderSettingsChunkTimeout>,
    /// Wire field: `compaction`.
    #[serde(default, rename = "compaction", skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ProviderCompaction>,
    /// Wire field: `headerTimeout`.
    #[serde(default, rename = "headerTimeout", skip_serializing_if = "Option::is_none")]
    pub header_timeout: Option<ConfigProviderSettingsHeaderTimeout>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<ConfigProviderSettingsTimeout>,
    /// Wire field: `transport`.
    #[serde(default, rename = "transport", skip_serializing_if = "Option::is_none")]
    pub transport: Option<ProviderTransport>,
}

/// Wire schema: `Config.ProviderEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigProviderEncoded {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `canonical`.
    #[serde(default, rename = "canonical", skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    /// Wire field: `env`.
    #[serde(default, rename = "env", skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `models`.
    #[serde(default, rename = "models", skip_serializing_if = "Option::is_none")]
    pub models: Option<std::collections::BTreeMap<String, ConfigModelEncoded>>,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `package`.
    #[serde(default, rename = "package", skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ConfigProviderSettings>,
}

/// Wire schema: `Config.Reference.GitEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigReferenceGitEncoded {
    /// Wire field: `branch`.
    #[serde(default, rename = "branch", skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `repository`.
    #[serde(rename = "repository")]
    pub repository: String,
}

/// Wire schema: `Config.Reference.LocalEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigReferenceLocalEncoded {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedReferencesValue`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedReferencesValue {
    String(String),
    GitEncoded(ConfigReferenceGitEncoded),
    LocalEncoded(ConfigReferenceLocalEncoded),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConfigInfoEncodedShare {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "disabled")]
    Disabled,
}

impl ConfigInfoEncodedShare {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedToolOutput {
    /// Wire field: `max_bytes`.
    #[serde(default, rename = "max_bytes", skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<i64>,
    /// Wire field: `max_lines`.
    #[serde(default, rename = "max_lines", skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConfigInfoEncodedUpdate {
    #[serde(rename = "disable")]
    Disable,
    #[serde(rename = "notify")]
    Notify,
    #[serde(rename = "auto")]
    Auto,
}

impl ConfigInfoEncodedUpdate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Notify => "notify",
            Self::Auto => "auto",
        }
    }
}

/// Wire schema: `Config.WarmingEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigWarmingEncoded {
    /// Wire field: `duration`.
    #[serde(default, rename = "duration", skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    /// Wire field: `interval`.
    #[serde(default, rename = "interval", skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    /// Wire field: `prompt`.
    #[serde(default, rename = "prompt", skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedWarming`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedWarming {
    Boolean(bool),
    WarmingEncoded(ConfigWarmingEncoded),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncodedWatcher {
    /// Wire field: `ignore`.
    #[serde(default, rename = "ignore", skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
}

/// Wire schema: `ConfigWebSearch.InfoEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigWebSearchInfoEncoded {
    /// Wire field: `provider`.
    #[serde(rename = "provider")]
    pub provider: String,
}

/// Untagged union; serde matches variants in declared order (wire schema `ConfigInfoEncodedWebsearch`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigInfoEncodedWebsearch {
    Boolean(bool),
    InfoEncoded(ConfigWebSearchInfoEncoded),
}

/// Wire schema: `Config.Worktree`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigWorktree {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
}

/// Wire schema: `Config.InfoEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfoEncoded {
    /// Wire field: `$schema`.
    #[serde(default, rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Wire field: `agents`.
    #[serde(default, rename = "agents", skip_serializing_if = "Option::is_none")]
    pub agents: Option<std::collections::BTreeMap<String, ConfigAgentEncoded>>,
    /// Wire field: `commands`.
    #[serde(default, rename = "commands", skip_serializing_if = "Option::is_none")]
    pub commands: Option<std::collections::BTreeMap<String, ConfigCommandEncoded>>,
    /// Wire field: `compaction`.
    #[serde(default, rename = "compaction", skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ConfigInfoEncodedCompaction>,
    /// Wire field: `default_agent`.
    #[serde(default, rename = "default_agent", skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    /// Wire field: `enterprise`.
    #[serde(default, rename = "enterprise", skip_serializing_if = "Option::is_none")]
    pub enterprise: Option<ConfigInfoEncodedEnterprise>,
    /// Wire field: `experimental`.
    #[serde(default, rename = "experimental", skip_serializing_if = "Option::is_none")]
    pub experimental: Option<ConfigInfoEncodedExperimental>,
    /// Wire field: `formatter`.
    #[serde(default, rename = "formatter", skip_serializing_if = "Option::is_none")]
    pub formatter: Option<ConfigInfoEncodedFormatter>,
    /// Wire field: `instructions`.
    #[serde(default, rename = "instructions", skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<String>>,
    /// Wire field: `lsp`.
    #[serde(default, rename = "lsp", skip_serializing_if = "Option::is_none")]
    pub lsp: Option<ConfigInfoEncodedLsp>,
    /// Wire field: `mcp`.
    #[serde(default, rename = "mcp", skip_serializing_if = "Option::is_none")]
    pub mcp: Option<ConfigInfoEncodedMcp>,
    /// Wire field: `media`.
    #[serde(default, rename = "media", skip_serializing_if = "Option::is_none")]
    pub media: Option<ConfigInfoEncodedMedia>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ConfigInfoEncodedModel>,
    /// Wire field: `permissions`.
    #[serde(default, rename = "permissions", skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionRuleset>,
    /// Wire field: `plugins`.
    #[serde(default, rename = "plugins", skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<ConfigInfoEncodedPluginsItem>>,
    /// Wire field: `providers`.
    #[serde(default, rename = "providers", skip_serializing_if = "Option::is_none")]
    pub providers: Option<std::collections::BTreeMap<String, ConfigProviderEncoded>>,
    /// Wire field: `references`.
    #[serde(default, rename = "references", skip_serializing_if = "Option::is_none")]
    pub references: Option<std::collections::BTreeMap<String, ConfigInfoEncodedReferencesValue>>,
    /// Wire field: `share`.
    #[serde(default, rename = "share", skip_serializing_if = "Option::is_none")]
    pub share: Option<ConfigInfoEncodedShare>,
    /// Wire field: `shell`.
    #[serde(default, rename = "shell", skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Wire field: `skills`.
    #[serde(default, rename = "skills", skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    /// Wire field: `snapshots`.
    #[serde(default, rename = "snapshots", skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    /// Wire field: `tool_output`.
    #[serde(default, rename = "tool_output", skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<ConfigInfoEncodedToolOutput>,
    /// Wire field: `update`.
    #[serde(default, rename = "update", skip_serializing_if = "Option::is_none")]
    pub update: Option<ConfigInfoEncodedUpdate>,
    /// Wire field: `username`.
    #[serde(default, rename = "username", skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Wire field: `warming`.
    #[serde(default, rename = "warming", skip_serializing_if = "Option::is_none")]
    pub warming: Option<ConfigInfoEncodedWarming>,
    /// Wire field: `watcher`.
    #[serde(default, rename = "watcher", skip_serializing_if = "Option::is_none")]
    pub watcher: Option<ConfigInfoEncodedWatcher>,
    /// Wire field: `websearch`.
    #[serde(default, rename = "websearch", skip_serializing_if = "Option::is_none")]
    pub websearch: Option<ConfigInfoEncodedWebsearch>,
    /// Wire field: `worktree`.
    #[serde(default, rename = "worktree", skip_serializing_if = "Option::is_none")]
    pub worktree: Option<ConfigWorktree>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConfigDocumentEncodedType {
    #[default]
    #[serde(rename = "document")]
    Document,
}

impl ConfigDocumentEncodedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Document => "document",
        }
    }
}

/// Wire schema: `Config.DocumentEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigDocumentEncoded {
    /// Wire field: `info`.
    #[serde(rename = "info")]
    pub info: ConfigInfoEncoded,
    /// Wire field: `path`.
    #[serde(default, rename = "path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ConfigDocumentEncodedType,
}

/// Wire schema: `Config.Entry`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Config.Entry`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConfigEntry {
    DocumentEncoded(ConfigDocumentEncoded),
    DirectoryEncoded(ConfigDirectoryEncoded),
}

/// Wire schema: `Config.Patch`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigPatch {
    /// Wire field: `shell`.
    #[serde(rename = "shell")]
    pub shell: Option<String>,
}

/// Wire schema: `ConfigShell.Option`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfigShellOption {
    /// Wire field: `acceptable`.
    #[serde(rename = "acceptable")]
    pub acceptable: bool,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConflictErrorEncodedTag {
    #[default]
    #[serde(rename = "ConflictError")]
    ConflictError,
}

impl ConflictErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConflictError => "ConflictError",
        }
    }
}

/// Wire schema: `ConflictErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConflictErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ConflictErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `resource`.
    #[serde(default, rename = "resource", skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConnectionCredentialInfoMethod {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "oauth")]
    Oauth,
}

impl ConnectionCredentialInfoMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Oauth => "oauth",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConnectionStatusStatus {
    #[default]
    #[serde(rename = "needs_auth")]
    NeedsAuth,
}

impl ConnectionStatusStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NeedsAuth => "needs_auth",
        }
    }
}

/// Wire schema: `Connection.Status`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionStatus {
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: Option<ConnectionStatusStatus>,
    /// Wire field: `url`.
    #[serde(default, rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConnectionCredentialInfoType {
    #[default]
    #[serde(rename = "credential")]
    Credential,
}

impl ConnectionCredentialInfoType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Credential => "credential",
        }
    }
}

/// Wire schema: `Connection.CredentialInfo`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionCredentialInfo {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `method`.
    #[serde(rename = "method")]
    pub method: ConnectionCredentialInfoMethod,
    /// Wire field: `status`.
    #[serde(default, rename = "status", skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ConnectionCredentialInfoType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ConnectionEnvInfoType {
    #[default]
    #[serde(rename = "env")]
    Env,
}

impl ConnectionEnvInfoType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Env => "env",
        }
    }
}

/// Wire schema: `Connection.EnvInfo`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConnectionEnvInfo {
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `status`.
    #[serde(default, rename = "status", skip_serializing_if = "Option::is_none")]
    pub status: Option<ConnectionStatus>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ConnectionEnvInfoType,
}

/// Wire schema: `Connection.Info`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Connection.Info`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ConnectionInfo {
    CredentialInfo(ConnectionCredentialInfo),
    EnvInfo(ConnectionEnvInfo),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CredentialOAuthType {
    #[default]
    #[serde(rename = "oauth")]
    Oauth,
}

impl CredentialOAuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
        }
    }
}

/// Wire schema: `Credential.OAuth`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CredentialOAuth {
    /// Wire field: `access`.
    #[serde(rename = "access")]
    pub access: String,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: i64,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `methodID`.
    #[serde(rename = "methodID")]
    pub method_id: String,
    /// Wire field: `refresh`.
    #[serde(rename = "refresh")]
    pub refresh: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: CredentialOAuthType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormValueVariant2Variant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormValueVariant2Variant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormValueVariant2`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormValueVariant2 {
    Number(f64),
    FormValueVariant2Variant2(FormValueVariant2Variant2),
}

/// Wire schema: `Form.Value`.
/// Untagged union; serde matches variants in declared order (wire schema `Form.Value`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormValue {
    String(String),
    FormValueVariant2(FormValueVariant2),
    Boolean(bool),
    Array(Vec<String>),
}

/// Wire schema: `Form.Answer_1`.
pub type FormAnswer1 = std::collections::BTreeMap<String, FormValue>;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CredentialKeyType {
    #[default]
    #[serde(rename = "key")]
    Key,
}

impl CredentialKeyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Key => "key",
        }
    }
}

/// Wire schema: `Credential.Key`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CredentialKey {
    /// Wire field: `configuration`.
    #[serde(default, rename = "configuration", skip_serializing_if = "Option::is_none")]
    pub configuration: Option<FormAnswer1>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: CredentialKeyType,
}

/// Wire schema: `Credential.Value`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Credential.Value`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CredentialValue {
    OAuth(CredentialOAuth),
    Key(CredentialKey),
}

/// Wire schema: `Credential.CreateInput`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CredentialCreateInput {
    /// Wire field: `activate`.
    #[serde(default, rename = "activate", skip_serializing_if = "Option::is_none")]
    pub activate: Option<bool>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `integrationID`.
    #[serde(rename = "integrationID")]
    pub integration_id: String,
    /// Wire field: `label`.
    #[serde(default, rename = "label", skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Wire field: `value`.
    #[serde(rename = "value")]
    pub value: CredentialValue,
}

/// Wire schema: `Credential.Entry`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CredentialEntry {
    /// Wire field: `active`.
    #[serde(rename = "active")]
    pub active: bool,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `integrationID`.
    #[serde(rename = "integrationID")]
    pub integration_id: String,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `value`.
    #[serde(rename = "value")]
    pub value: CredentialValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileDiffInfoStatus {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "modified")]
    Modified,
}

impl FileDiffInfoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Modified => "modified",
        }
    }
}

/// Wire schema: `FileDiff.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileDiffInfo {
    /// Wire field: `additions`.
    #[serde(rename = "additions")]
    pub additions: i64,
    /// Wire field: `deletions`.
    #[serde(rename = "deletions")]
    pub deletions: i64,
    /// Wire field: `file`.
    #[serde(rename = "file")]
    pub file: String,
    /// Wire field: `patch`.
    #[serde(rename = "patch")]
    pub patch: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: FileDiffInfoStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FileNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "FileNotFoundError")]
    FileNotFoundError,
}

impl FileNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FileNotFoundError => "FileNotFoundError",
        }
    }
}

/// Wire schema: `FileNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: FileNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileSystemEntryType {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
}

impl FileSystemEntryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

/// Wire schema: `FileSystem.Entry`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSystemEntry {
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FileSystemEntryType,
}

/// Wire schema: `FileSystem.Write`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSystemWrite {
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ForbiddenErrorEncodedTag {
    #[default]
    #[serde(rename = "ForbiddenError")]
    ForbiddenError,
}

impl ForbiddenErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ForbiddenError => "ForbiddenError",
        }
    }
}

/// Wire schema: `ForbiddenErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ForbiddenErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ForbiddenErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

/// Wire schema: `Form.Answer`.
pub type FormAnswer = std::collections::BTreeMap<String, FormValue>;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormBooleanFieldType {
    #[default]
    #[serde(rename = "boolean")]
    Boolean,
}

impl FormBooleanFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormWhenOp {
    #[serde(rename = "eq")]
    Eq,
    #[serde(rename = "neq")]
    Neq,
}

impl FormWhenOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormWhenValueVariant2Variant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormWhenValueVariant2Variant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormWhenValueVariant2`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormWhenValueVariant2 {
    Number(f64),
    FormWhenValueVariant2Variant2(FormWhenValueVariant2Variant2),
}

/// Untagged union; serde matches variants in declared order (wire schema `FormWhenValue`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormWhenValue {
    String(String),
    FormWhenValueVariant2(FormWhenValueVariant2),
    Boolean(bool),
}

/// Wire schema: `Form.When`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormWhen {
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `op`.
    #[serde(rename = "op")]
    pub op: FormWhenOp,
    /// Wire field: `value`.
    #[serde(rename = "value")]
    pub value: FormWhenValue,
}

/// Wire schema: `Form.BooleanField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormBooleanField {
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `required`.
    #[serde(default, rename = "required", skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormBooleanFieldType,
    /// Wire field: `when`.
    #[serde(default, rename = "when", skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormWhen>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormStringFieldFormat {
    #[serde(rename = "email")]
    Email,
    #[serde(rename = "uri")]
    Uri,
    #[serde(rename = "date")]
    Date,
    #[serde(rename = "date-time")]
    DateTime,
}

impl FormStringFieldFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Uri => "uri",
            Self::Date => "date",
            Self::DateTime => "date-time",
        }
    }
}

/// Wire schema: `Form.Option`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormOption {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `value`.
    #[serde(rename = "value")]
    pub value: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormStringFieldType {
    #[default]
    #[serde(rename = "string")]
    String,
}

impl FormStringFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
        }
    }
}

/// Wire schema: `Form.StringField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormStringField {
    /// Wire field: `custom`.
    #[serde(default, rename = "custom", skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `format`.
    #[serde(default, rename = "format", skip_serializing_if = "Option::is_none")]
    pub format: Option<FormStringFieldFormat>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `maxLength`.
    #[serde(default, rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<i64>,
    /// Wire field: `minLength`.
    #[serde(default, rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<i64>,
    /// Wire field: `options`.
    #[serde(default, rename = "options", skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FormOption>>,
    /// Wire field: `pattern`.
    #[serde(default, rename = "pattern", skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Wire field: `placeholder`.
    #[serde(default, rename = "placeholder", skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// Wire field: `required`.
    #[serde(default, rename = "required", skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormStringFieldType,
    /// Wire field: `when`.
    #[serde(default, rename = "when", skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormWhen>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormNumberFieldDefaultVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormNumberFieldDefaultVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormNumberFieldDefault`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormNumberFieldDefault {
    Number(f64),
    FormNumberFieldDefaultVariant2(FormNumberFieldDefaultVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormNumberFieldMaximumVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormNumberFieldMaximumVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormNumberFieldMaximum`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormNumberFieldMaximum {
    Number(f64),
    FormNumberFieldMaximumVariant2(FormNumberFieldMaximumVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormNumberFieldMinimumVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormNumberFieldMinimumVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormNumberFieldMinimum`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormNumberFieldMinimum {
    Number(f64),
    FormNumberFieldMinimumVariant2(FormNumberFieldMinimumVariant2),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormNumberFieldType {
    #[default]
    #[serde(rename = "number")]
    Number,
}

impl FormNumberFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Number => "number",
        }
    }
}

/// Wire schema: `Form.NumberField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormNumberField {
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<FormNumberFieldDefault>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `maximum`.
    #[serde(default, rename = "maximum", skip_serializing_if = "Option::is_none")]
    pub maximum: Option<FormNumberFieldMaximum>,
    /// Wire field: `minimum`.
    #[serde(default, rename = "minimum", skip_serializing_if = "Option::is_none")]
    pub minimum: Option<FormNumberFieldMinimum>,
    /// Wire field: `required`.
    #[serde(default, rename = "required", skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormNumberFieldType,
    /// Wire field: `when`.
    #[serde(default, rename = "when", skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormWhen>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormIntegerFieldDefaultVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormIntegerFieldDefaultVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormIntegerFieldDefault`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormIntegerFieldDefault {
    Number(f64),
    FormIntegerFieldDefaultVariant2(FormIntegerFieldDefaultVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormIntegerFieldMaximumVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormIntegerFieldMaximumVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormIntegerFieldMaximum`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormIntegerFieldMaximum {
    Number(f64),
    FormIntegerFieldMaximumVariant2(FormIntegerFieldMaximumVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FormIntegerFieldMinimumVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl FormIntegerFieldMinimumVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `FormIntegerFieldMinimum`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormIntegerFieldMinimum {
    Number(f64),
    FormIntegerFieldMinimumVariant2(FormIntegerFieldMinimumVariant2),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormIntegerFieldType {
    #[default]
    #[serde(rename = "integer")]
    Integer,
}

impl FormIntegerFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
        }
    }
}

/// Wire schema: `Form.IntegerField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormIntegerField {
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<FormIntegerFieldDefault>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `maximum`.
    #[serde(default, rename = "maximum", skip_serializing_if = "Option::is_none")]
    pub maximum: Option<FormIntegerFieldMaximum>,
    /// Wire field: `minimum`.
    #[serde(default, rename = "minimum", skip_serializing_if = "Option::is_none")]
    pub minimum: Option<FormIntegerFieldMinimum>,
    /// Wire field: `required`.
    #[serde(default, rename = "required", skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormIntegerFieldType,
    /// Wire field: `when`.
    #[serde(default, rename = "when", skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormWhen>>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormMultiselectFieldType {
    #[default]
    #[serde(rename = "multiselect")]
    Multiselect,
}

impl FormMultiselectFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Multiselect => "multiselect",
        }
    }
}

/// Wire schema: `Form.MultiselectField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormMultiselectField {
    /// Wire field: `custom`.
    #[serde(default, rename = "custom", skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<Vec<String>>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `maxItems`.
    #[serde(default, rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<i64>,
    /// Wire field: `minItems`.
    #[serde(default, rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<i64>,
    /// Wire field: `options`.
    #[serde(rename = "options")]
    pub options: Vec<FormOption>,
    /// Wire field: `required`.
    #[serde(default, rename = "required", skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormMultiselectFieldType,
    /// Wire field: `when`.
    #[serde(default, rename = "when", skip_serializing_if = "Option::is_none")]
    pub when: Option<Vec<FormWhen>>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormExternalFieldType {
    #[default]
    #[serde(rename = "external")]
    External,
}

impl FormExternalFieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::External => "external",
        }
    }
}

/// Wire schema: `Form.ExternalField`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormExternalField {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: FormExternalFieldType,
    /// Wire field: `url`.
    #[serde(rename = "url")]
    pub url: String,
}

/// Wire schema: `Form.Field`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Form.Field`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormField {
    StringField(FormStringField),
    NumberField(FormNumberField),
    IntegerField(FormIntegerField),
    BooleanField(FormBooleanField),
    MultiselectField(FormMultiselectField),
    ExternalField(FormExternalField),
}

/// Wire schema: `Form.Fields`.
pub type FormFields = Vec<FormField>;

/// Wire schema: `Form.Metadata`.
pub type FormMetadata = std::collections::BTreeMap<String, serde_json::Value>;

/// Wire schema: `Form.CreatePayload`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormCreatePayload {
    /// Wire field: `fields`.
    #[serde(rename = "fields")]
    pub fields: FormFields,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FormMetadata>,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormStateVariant1Status {
    #[default]
    #[serde(rename = "pending")]
    Pending,
}

impl FormStateVariant1Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormStateVariant1 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: FormStateVariant1Status,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormStateVariant2Status {
    #[default]
    #[serde(rename = "answered")]
    Answered,
}

impl FormStateVariant2Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Answered => "answered",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormStateVariant2 {
    /// Wire field: `answer`.
    #[serde(rename = "answer")]
    pub answer: FormAnswer,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: FormStateVariant2Status,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormStateVariant3Status {
    #[default]
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl FormStateVariant3Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormStateVariant3 {
    /// Wire field: `message`.
    #[serde(default, rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: FormStateVariant3Status,
}

/// Wire schema: `Form.State`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Form.State`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FormState {
    Pending(FormStateVariant1),
    Answered(FormStateVariant2),
    Cancelled(FormStateVariant3),
}

/// Wire schema: `Form.Detail`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormDetail {
    /// Wire field: `fields`.
    #[serde(rename = "fields")]
    pub fields: FormFields,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FormMetadata>,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `state`.
    #[serde(rename = "state")]
    pub state: FormState,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

/// Wire schema: `Form.Fields_1`.
pub type FormFields1 = Vec<FormField>;

/// Wire schema: `Form.Fields_2`.
pub type FormFields2 = Vec<FormField>;

/// Wire schema: `Form.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormInfo {
    /// Wire field: `fields`.
    #[serde(rename = "fields")]
    pub fields: FormFields,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FormMetadata>,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

/// Wire schema: `Form.Reply`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormReply {
    /// Wire field: `answer`.
    #[serde(rename = "answer")]
    pub answer: FormAnswer,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormAlreadySettledErrorEncodedTag {
    #[default]
    #[serde(rename = "FormAlreadySettledError")]
    FormAlreadySettledError,
}

impl FormAlreadySettledErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FormAlreadySettledError => "FormAlreadySettledError",
        }
    }
}

/// Wire schema: `FormAlreadySettledErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormAlreadySettledErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: FormAlreadySettledErrorEncodedTag,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormInvalidAnswerErrorEncodedTag {
    #[default]
    #[serde(rename = "FormInvalidAnswerError")]
    FormInvalidAnswerError,
}

impl FormInvalidAnswerErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FormInvalidAnswerError => "FormInvalidAnswerError",
        }
    }
}

/// Wire schema: `FormInvalidAnswerErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormInvalidAnswerErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: FormInvalidAnswerErrorEncodedTag,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FormNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "FormNotFoundError")]
    FormNotFoundError,
}

impl FormNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FormNotFoundError => "FormNotFoundError",
        }
    }
}

/// Wire schema: `FormNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: FormNotFoundErrorEncodedTag,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenerateTextResponseData {
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Wire schema: `GenerateTextResponse`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenerateTextResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: GenerateTextResponseData,
}

/// Instruction entry key (lowercase alphanumerics plus . _ -)
pub type InstructionEntryKey = String;

/// Wire schema: `InstructionEntry.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InstructionEntryInfo {
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: InstructionEntryKey,
    /// JSON value attached to the session's instructions
    #[serde(rename = "value")]
    pub value: serde_json::Value,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum InstructionEntryValueTooLargeErrorEncodedTag {
    #[default]
    #[serde(rename = "InstructionEntryValueTooLargeError")]
    InstructionEntryValueTooLargeError,
}

impl InstructionEntryValueTooLargeErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InstructionEntryValueTooLargeError => "InstructionEntryValueTooLargeError",
        }
    }
}

/// Wire schema: `InstructionEntryValueTooLargeErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InstructionEntryValueTooLargeErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: InstructionEntryValueTooLargeErrorEncodedTag,
    /// Wire field: `actualBytes`.
    #[serde(rename = "actualBytes")]
    pub actual_bytes: i64,
    /// Wire field: `maxBytes`.
    #[serde(rename = "maxBytes")]
    pub max_bytes: i64,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptEncodedMode {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "code")]
    Code,
}

impl IntegrationAttemptEncodedMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Code => "code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptEncodedTimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptEncodedTimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptEncodedTimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptEncodedTimeCreated {
    Number(f64),
    IntegrationAttemptEncodedTimeCreatedVariant2(IntegrationAttemptEncodedTimeCreatedVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptEncodedTimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptEncodedTimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptEncodedTimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptEncodedTimeExpires {
    Number(f64),
    IntegrationAttemptEncodedTimeExpiresVariant2(IntegrationAttemptEncodedTimeExpiresVariant2),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptEncodedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationAttemptEncodedTimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationAttemptEncodedTimeExpires,
}

/// Wire schema: `Integration.AttemptEncoded`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptEncoded {
    /// Wire field: `attemptID`.
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    /// Wire field: `instructions`.
    #[serde(rename = "instructions")]
    pub instructions: String,
    /// Wire field: `mode`.
    #[serde(rename = "mode")]
    pub mode: IntegrationAttemptEncodedMode,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationAttemptEncodedTime,
    /// Wire field: `url`.
    #[serde(rename = "url")]
    pub url: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationAttemptStatusVariant1Status {
    #[default]
    #[serde(rename = "pending")]
    Pending,
}

impl IntegrationAttemptStatusVariant1Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant1TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant1TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant1TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant1TimeCreated {
    Number(f64),
    IntegrationAttemptStatusVariant1TimeCreatedVariant2(
        IntegrationAttemptStatusVariant1TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant1TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant1TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant1TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant1TimeExpires {
    Number(f64),
    IntegrationAttemptStatusVariant1TimeExpiresVariant2(
        IntegrationAttemptStatusVariant1TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant1Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationAttemptStatusVariant1TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationAttemptStatusVariant1TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant1 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationAttemptStatusVariant1Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationAttemptStatusVariant1Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationAttemptStatusVariant2Status {
    #[default]
    #[serde(rename = "complete")]
    Complete,
}

impl IntegrationAttemptStatusVariant2Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant2TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant2TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant2TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant2TimeCreated {
    Number(f64),
    IntegrationAttemptStatusVariant2TimeCreatedVariant2(
        IntegrationAttemptStatusVariant2TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant2TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant2TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant2TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant2TimeExpires {
    Number(f64),
    IntegrationAttemptStatusVariant2TimeExpiresVariant2(
        IntegrationAttemptStatusVariant2TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant2Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationAttemptStatusVariant2TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationAttemptStatusVariant2TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant2 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationAttemptStatusVariant2Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationAttemptStatusVariant2Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationAttemptStatusVariant3Status {
    #[default]
    #[serde(rename = "failed")]
    Failed,
}

impl IntegrationAttemptStatusVariant3Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant3TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant3TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant3TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant3TimeCreated {
    Number(f64),
    IntegrationAttemptStatusVariant3TimeCreatedVariant2(
        IntegrationAttemptStatusVariant3TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant3TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant3TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant3TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant3TimeExpires {
    Number(f64),
    IntegrationAttemptStatusVariant3TimeExpiresVariant2(
        IntegrationAttemptStatusVariant3TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant3Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationAttemptStatusVariant3TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationAttemptStatusVariant3TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant3 {
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationAttemptStatusVariant3Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationAttemptStatusVariant3Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationAttemptStatusVariant4Status {
    #[default]
    #[serde(rename = "expired")]
    Expired,
}

impl IntegrationAttemptStatusVariant4Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant4TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant4TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant4TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant4TimeCreated {
    Number(f64),
    IntegrationAttemptStatusVariant4TimeCreatedVariant2(
        IntegrationAttemptStatusVariant4TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationAttemptStatusVariant4TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationAttemptStatusVariant4TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationAttemptStatusVariant4TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatusVariant4TimeExpires {
    Number(f64),
    IntegrationAttemptStatusVariant4TimeExpiresVariant2(
        IntegrationAttemptStatusVariant4TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant4Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationAttemptStatusVariant4TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationAttemptStatusVariant4TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptStatusVariant4 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationAttemptStatusVariant4Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationAttemptStatusVariant4Time,
}

/// Wire schema: `Integration.AttemptStatus`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Integration.AttemptStatus`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationAttemptStatus {
    Pending(IntegrationAttemptStatusVariant1),
    Complete(IntegrationAttemptStatusVariant2),
    Failed(IntegrationAttemptStatusVariant3),
    Expired(IntegrationAttemptStatusVariant4),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptTimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptTimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptTimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptTimeCreated {
    Number(f64),
    IntegrationCommandAttemptTimeCreatedVariant2(IntegrationCommandAttemptTimeCreatedVariant2),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptTimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptTimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptTimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptTimeExpires {
    Number(f64),
    IntegrationCommandAttemptTimeExpiresVariant2(IntegrationCommandAttemptTimeExpiresVariant2),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationCommandAttemptTimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationCommandAttemptTimeExpires,
}

/// Wire schema: `Integration.CommandAttempt`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttempt {
    /// Wire field: `attemptID`.
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationCommandAttemptTime,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationCommandAttemptStatusVariant1Status {
    #[default]
    #[serde(rename = "pending")]
    Pending,
}

impl IntegrationCommandAttemptStatusVariant1Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant1TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant1TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant1TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant1TimeCreated {
    Number(f64),
    IntegrationCommandAttemptStatusVariant1TimeCreatedVariant2(
        IntegrationCommandAttemptStatusVariant1TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant1TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant1TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant1TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant1TimeExpires {
    Number(f64),
    IntegrationCommandAttemptStatusVariant1TimeExpiresVariant2(
        IntegrationCommandAttemptStatusVariant1TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant1Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationCommandAttemptStatusVariant1TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationCommandAttemptStatusVariant1TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant1 {
    /// Wire field: `message`.
    #[serde(default, rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationCommandAttemptStatusVariant1Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationCommandAttemptStatusVariant1Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationCommandAttemptStatusVariant2Status {
    #[default]
    #[serde(rename = "complete")]
    Complete,
}

impl IntegrationCommandAttemptStatusVariant2Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant2TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant2TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant2TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant2TimeCreated {
    Number(f64),
    IntegrationCommandAttemptStatusVariant2TimeCreatedVariant2(
        IntegrationCommandAttemptStatusVariant2TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant2TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant2TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant2TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant2TimeExpires {
    Number(f64),
    IntegrationCommandAttemptStatusVariant2TimeExpiresVariant2(
        IntegrationCommandAttemptStatusVariant2TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant2Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationCommandAttemptStatusVariant2TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationCommandAttemptStatusVariant2TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant2 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationCommandAttemptStatusVariant2Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationCommandAttemptStatusVariant2Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationCommandAttemptStatusVariant3Status {
    #[default]
    #[serde(rename = "failed")]
    Failed,
}

impl IntegrationCommandAttemptStatusVariant3Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant3TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant3TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant3TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant3TimeCreated {
    Number(f64),
    IntegrationCommandAttemptStatusVariant3TimeCreatedVariant2(
        IntegrationCommandAttemptStatusVariant3TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant3TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant3TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant3TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant3TimeExpires {
    Number(f64),
    IntegrationCommandAttemptStatusVariant3TimeExpiresVariant2(
        IntegrationCommandAttemptStatusVariant3TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant3Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationCommandAttemptStatusVariant3TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationCommandAttemptStatusVariant3TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant3 {
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationCommandAttemptStatusVariant3Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationCommandAttemptStatusVariant3Time,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationCommandAttemptStatusVariant4Status {
    #[default]
    #[serde(rename = "expired")]
    Expired,
}

impl IntegrationCommandAttemptStatusVariant4Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant4TimeCreatedVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant4TimeCreatedVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant4TimeCreated`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant4TimeCreated {
    Number(f64),
    IntegrationCommandAttemptStatusVariant4TimeCreatedVariant2(
        IntegrationCommandAttemptStatusVariant4TimeCreatedVariant2,
    ),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum IntegrationCommandAttemptStatusVariant4TimeExpiresVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl IntegrationCommandAttemptStatusVariant4TimeExpiresVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `IntegrationCommandAttemptStatusVariant4TimeExpires`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatusVariant4TimeExpires {
    Number(f64),
    IntegrationCommandAttemptStatusVariant4TimeExpiresVariant2(
        IntegrationCommandAttemptStatusVariant4TimeExpiresVariant2,
    ),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant4Time {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: IntegrationCommandAttemptStatusVariant4TimeCreated,
    /// Wire field: `expires`.
    #[serde(rename = "expires")]
    pub expires: IntegrationCommandAttemptStatusVariant4TimeExpires,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandAttemptStatusVariant4 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: IntegrationCommandAttemptStatusVariant4Status,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: IntegrationCommandAttemptStatusVariant4Time,
}

/// Wire schema: `Integration.CommandAttemptStatus`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Integration.CommandAttemptStatus`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationCommandAttemptStatus {
    Pending(IntegrationCommandAttemptStatusVariant1),
    Complete(IntegrationCommandAttemptStatusVariant2),
    Failed(IntegrationCommandAttemptStatusVariant3),
    Expired(IntegrationCommandAttemptStatusVariant4),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationCommandMethodType {
    #[default]
    #[serde(rename = "command")]
    Command,
}

impl IntegrationCommandMethodType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Command => "command",
        }
    }
}

/// Wire schema: `Integration.CommandMethod`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandMethod {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: Vec<String>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: IntegrationCommandMethodType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationEnvMethodType {
    #[default]
    #[serde(rename = "env")]
    Env,
}

impl IntegrationEnvMethodType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Env => "env",
        }
    }
}

/// Wire schema: `Integration.EnvMethod`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationEnvMethod {
    /// Wire field: `names`.
    #[serde(rename = "names")]
    pub names: Vec<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: IntegrationEnvMethodType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationOAuthMethodType {
    #[default]
    #[serde(rename = "oauth")]
    Oauth,
}

impl IntegrationOAuthMethodType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Oauth => "oauth",
        }
    }
}

/// Wire schema: `Integration.OAuthMethod`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationOAuthMethod {
    /// Wire field: `form`.
    #[serde(default, rename = "form", skip_serializing_if = "Option::is_none")]
    pub form: Option<FormFields1>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: IntegrationOAuthMethodType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationKeyMethodType {
    #[default]
    #[serde(rename = "key")]
    Key,
}

impl IntegrationKeyMethodType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Key => "key",
        }
    }
}

/// Wire schema: `Integration.KeyMethod`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationKeyMethod {
    /// Wire field: `form`.
    #[serde(default, rename = "form", skip_serializing_if = "Option::is_none")]
    pub form: Option<FormFields2>,
    /// Wire field: `label`.
    #[serde(default, rename = "label", skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: IntegrationKeyMethodType,
}

/// Wire schema: `Integration.Method`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Integration.Method`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntegrationMethod {
    OAuthMethod(IntegrationOAuthMethod),
    CommandMethod(IntegrationCommandMethod),
    KeyMethod(IntegrationKeyMethod),
    EnvMethod(IntegrationEnvMethod),
}

/// Wire schema: `Integration.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationInfo {
    /// Wire field: `connections`.
    #[serde(rename = "connections")]
    pub connections: Vec<ConnectionInfo>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `methods`.
    #[serde(rename = "methods")]
    pub methods: Vec<IntegrationMethod>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationAttemptNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "IntegrationAttemptNotFoundError")]
    IntegrationAttemptNotFoundError,
}

impl IntegrationAttemptNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IntegrationAttemptNotFoundError => "IntegrationAttemptNotFoundError",
        }
    }
}

/// Wire schema: `IntegrationAttemptNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationAttemptNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: IntegrationAttemptNotFoundErrorEncodedTag,
    /// Wire field: `attemptID`.
    #[serde(rename = "attemptID")]
    pub attempt_id: String,
    /// Wire field: `integrationID`.
    #[serde(rename = "integrationID")]
    pub integration_id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationMethodNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "IntegrationMethodNotFoundError")]
    IntegrationMethodNotFoundError,
}

impl IntegrationMethodNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IntegrationMethodNotFoundError => "IntegrationMethodNotFoundError",
        }
    }
}

/// Wire schema: `IntegrationMethodNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationMethodNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: IntegrationMethodNotFoundErrorEncodedTag,
    /// Wire field: `integrationID`.
    #[serde(rename = "integrationID")]
    pub integration_id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `methodID`.
    #[serde(rename = "methodID")]
    pub method_id: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum IntegrationNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "IntegrationNotFoundError")]
    IntegrationNotFoundError,
}

impl IntegrationNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IntegrationNotFoundError => "IntegrationNotFoundError",
        }
    }
}

/// Wire schema: `IntegrationNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: IntegrationNotFoundErrorEncodedTag,
    /// Wire field: `integrationID`.
    #[serde(rename = "integrationID")]
    pub integration_id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum InvalidCursorErrorEncodedTag {
    #[default]
    #[serde(rename = "InvalidCursorError")]
    InvalidCursorError,
}

impl InvalidCursorErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidCursorError => "InvalidCursorError",
        }
    }
}

/// Wire schema: `InvalidCursorErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidCursorErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: InvalidCursorErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum InvalidRequestErrorEncodedTag {
    #[default]
    #[serde(rename = "InvalidRequestError")]
    InvalidRequestError,
}

impl InvalidRequestErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequestError => "InvalidRequestError",
        }
    }
}

/// Wire schema: `InvalidRequestErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InvalidRequestErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: InvalidRequestErrorEncodedTag,
    /// Wire field: `field`.
    #[serde(default, rename = "field", skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Wire field: `kind`.
    #[serde(default, rename = "kind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationPublicInfoProject {
    /// Wire field: `canonical`.
    #[serde(rename = "canonical")]
    pub canonical: String,
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
}

/// Wire schema: `Location.PublicInfo`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationPublicInfo {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
    /// Wire field: `project`.
    #[serde(rename = "project")]
    pub project: LocationPublicInfoProject,
}

/// Wire schema: `Location.PublicRef`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationPublicRef {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
}

/// Wire schema: `Mcp.Resource`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpResource {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `mimeType`.
    #[serde(default, rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `server`.
    #[serde(rename = "server")]
    pub server: String,
    /// Wire field: `uri`.
    #[serde(rename = "uri")]
    pub uri: String,
}

/// Wire schema: `Mcp.ResourceTemplate`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpResourceTemplate {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `mimeType`.
    #[serde(default, rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `server`.
    #[serde(rename = "server")]
    pub server: String,
    /// Wire field: `uriTemplate`.
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
}

/// Wire schema: `Mcp.ResourceCatalog`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpResourceCatalog {
    /// Wire field: `resources`.
    #[serde(rename = "resources")]
    pub resources: Vec<McpResource>,
    /// Wire field: `templates`.
    #[serde(rename = "templates")]
    pub templates: Vec<McpResourceTemplate>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpStatusConnectedStatus {
    #[default]
    #[serde(rename = "connected")]
    Connected,
}

impl McpStatusConnectedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
        }
    }
}

/// Wire schema: `Mcp.Status.Connected`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpStatusConnected {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpStatusConnectedStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpStatusPendingStatus {
    #[default]
    #[serde(rename = "pending")]
    Pending,
}

impl McpStatusPendingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
        }
    }
}

/// Wire schema: `Mcp.Status.Pending`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpStatusPending {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpStatusPendingStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpStatusDisabledStatus {
    #[default]
    #[serde(rename = "disabled")]
    Disabled,
}

impl McpStatusDisabledStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
        }
    }
}

/// Wire schema: `Mcp.Status.Disabled`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpStatusDisabled {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpStatusDisabledStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpStatusFailedStatus {
    #[default]
    #[serde(rename = "failed")]
    Failed,
}

impl McpStatusFailedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Failed => "failed",
        }
    }
}

/// Wire schema: `Mcp.Status.Failed`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpStatusFailed {
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpStatusFailedStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpStatusNeedsAuthStatus {
    #[default]
    #[serde(rename = "needs_auth")]
    NeedsAuth,
}

impl McpStatusNeedsAuthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NeedsAuth => "needs_auth",
        }
    }
}

/// Wire schema: `Mcp.Status.NeedsAuth`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpStatusNeedsAuth {
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpStatusNeedsAuthStatus,
}

/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `McpServerStatus`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum McpServerStatus {
    Connected(McpStatusConnected),
    Pending(McpStatusPending),
    Disabled(McpStatusDisabled),
    Failed(McpStatusFailed),
    NeedsAuth(McpStatusNeedsAuth),
}

/// Wire schema: `Mcp.Server`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpServer {
    /// Wire field: `integrationID`.
    #[serde(default, rename = "integrationID", skip_serializing_if = "Option::is_none")]
    pub integration_id: Option<String>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: McpServerStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum McpServerNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "McpServerNotFoundError")]
    McpServerNotFoundError,
}

impl McpServerNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::McpServerNotFoundError => "McpServerNotFoundError",
        }
    }
}

/// Wire schema: `McpServerNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpServerNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: McpServerNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `server`.
    #[serde(rename = "server")]
    pub server: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MessageNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "MessageNotFoundError")]
    MessageNotFoundError,
}

impl MessageNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MessageNotFoundError => "MessageNotFoundError",
        }
    }
}

/// Wire schema: `MessageNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MessageNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: MessageNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// Wire schema: `Model.Capabilities`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelCapabilities {
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: Vec<String>,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: Vec<String>,
    /// Wire field: `tools`.
    #[serde(rename = "tools")]
    pub tools: bool,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelCostCache {
    /// Wire field: `read`.
    #[serde(rename = "read")]
    pub read: MoneyUsdPerMillionTokens,
    /// Wire field: `write`.
    #[serde(rename = "write")]
    pub write: MoneyUsdPerMillionTokens,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ModelCostTierType {
    #[default]
    #[serde(rename = "context")]
    Context,
}

impl ModelCostTierType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Context => "context",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelCostTier {
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: i64,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ModelCostTierType,
}

/// Wire schema: `Model.Cost`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelCost {
    /// Wire field: `cache`.
    #[serde(rename = "cache")]
    pub cache: ModelCostCache,
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: MoneyUsdPerMillionTokens,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: MoneyUsdPerMillionTokens,
    /// Wire field: `tier`.
    #[serde(default, rename = "tier", skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelCostTier>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfoLimit {
    /// Wire field: `context`.
    #[serde(rename = "context")]
    pub context: i64,
    /// Wire field: `input`.
    #[serde(default, rename = "input", skip_serializing_if = "Option::is_none")]
    pub input: Option<i64>,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: i64,
}

/// Wire schema: `Model.Settings`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelSettings {
    /// Wire field: `compaction`.
    #[serde(default, rename = "compaction", skip_serializing_if = "Option::is_none")]
    pub compaction: Option<ProviderCompaction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelInfoStatus {
    #[serde(rename = "alpha")]
    Alpha,
    #[serde(rename = "beta")]
    Beta,
    #[serde(rename = "deprecated")]
    Deprecated,
    #[serde(rename = "active")]
    Active,
}

impl ModelInfoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Beta => "beta",
            Self::Deprecated => "deprecated",
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfoTime {
    /// Wire field: `released`.
    #[serde(rename = "released")]
    pub released: f64,
}

/// Wire schema: `Model.Variant`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelVariant {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ModelSettings>,
}

/// Wire schema: `Model.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `canonical`.
    #[serde(default, rename = "canonical", skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    /// Wire field: `capabilities`.
    #[serde(rename = "capabilities")]
    pub capabilities: ModelCapabilities,
    /// Wire field: `compatibility`.
    #[serde(default, rename = "compatibility", skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<ModelCompatibility>,
    /// Wire field: `cost`.
    #[serde(rename = "cost")]
    pub cost: Vec<ModelCost>,
    /// Wire field: `enabled`.
    #[serde(rename = "enabled")]
    pub enabled: bool,
    /// Wire field: `family`.
    #[serde(default, rename = "family", skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `limit`.
    #[serde(rename = "limit")]
    pub limit: ModelInfoLimit,
    /// Wire field: `modelID`.
    #[serde(rename = "modelID")]
    pub model_id: String,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `package`.
    #[serde(default, rename = "package", skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ModelSettings>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: ModelInfoStatus,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: ModelInfoTime,
    /// Wire field: `variants`.
    #[serde(rename = "variants")]
    pub variants: Vec<ModelVariant>,
}

/// Wire schema: `Money.USD`.
pub type MoneyUsd = f64;

/// Wire schema: `PairingCode`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairingCode {
    /// Wire field: `code`.
    #[serde(rename = "code")]
    pub code: String,
    /// Wire field: `expires_in`.
    #[serde(rename = "expires_in")]
    pub expires_in: i64,
}

/// Wire schema: `PairingSession`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PairingSession {
    /// Wire field: `token`.
    #[serde(rename = "token")]
    pub token: String,
}

/// Wire schema: `Permission.Reply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PermissionReply {
    #[serde(rename = "once")]
    Once,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "reject")]
    Reject,
}

impl PermissionReply {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Reject => "reject",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PermissionSourceVariant1Type {
    #[default]
    #[serde(rename = "tool")]
    Tool,
}

impl PermissionSourceVariant1Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionSourceVariant1 {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PermissionSourceVariant1Type,
}

/// Wire schema: `Permission.Source`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Permission.Source`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PermissionSource {
    Tool(PermissionSourceVariant1),
}

/// Wire schema: `Permission.Request`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionRequest {
    /// Wire field: `action`.
    #[serde(rename = "action")]
    pub action: String,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `message`.
    #[serde(default, rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `resources`.
    #[serde(rename = "resources")]
    pub resources: Vec<String>,
    /// Wire field: `save`.
    #[serde(default, rename = "save", skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `source`.
    #[serde(default, rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<PermissionSource>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PermissionNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "PermissionNotFoundError")]
    PermissionNotFoundError,
}

impl PermissionNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PermissionNotFoundError => "PermissionNotFoundError",
        }
    }
}

/// Wire schema: `PermissionNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: PermissionNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `requestID`.
    #[serde(rename = "requestID")]
    pub request_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionSavedInfoTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
    /// Wire field: `updated`.
    #[serde(rename = "updated")]
    pub updated: f64,
}

/// Wire schema: `PermissionSaved.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionSavedInfo {
    /// Wire field: `action`.
    #[serde(rename = "action")]
    pub action: String,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
    /// Wire field: `resource`.
    #[serde(rename = "resource")]
    pub resource: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: PermissionSavedInfoTime,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyCreateInputSize {
    /// Wire field: `cols`.
    #[serde(rename = "cols")]
    pub cols: i64,
    /// Wire field: `rows`.
    #[serde(rename = "rows")]
    pub rows: i64,
}

/// Wire schema: `PersistentPty.CreateInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyCreateInput {
    /// Wire field: `args`.
    #[serde(rename = "args")]
    pub args: Vec<String>,
    /// Wire field: `command`.
    #[serde(default, rename = "command", skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Wire field: `cwd`.
    #[serde(default, rename = "cwd", skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Wire field: `env`.
    #[serde(rename = "env")]
    pub env: std::collections::BTreeMap<String, String>,
    /// Wire field: `size`.
    #[serde(default, rename = "size", skip_serializing_if = "Option::is_none")]
    pub size: Option<PersistentPtyCreateInputSize>,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PersistentPtyHandoffExpiresAtVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl PersistentPtyHandoffExpiresAtVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `PersistentPtyHandoffExpiresAt`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PersistentPtyHandoffExpiresAt {
    Number(f64),
    PersistentPtyHandoffExpiresAtVariant2(PersistentPtyHandoffExpiresAtVariant2),
}

/// Wire schema: `PersistentPty.Handoff`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyHandoff {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
    /// Wire field: `expiresAt`.
    #[serde(rename = "expiresAt")]
    pub expires_at: PersistentPtyHandoffExpiresAt,
    /// Wire field: `instanceID`.
    #[serde(rename = "instanceID")]
    pub instance_id: String,
    /// Wire field: `ticket`.
    #[serde(rename = "ticket")]
    pub ticket: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyInfoOutput {
    /// Wire field: `head`.
    #[serde(rename = "head")]
    pub head: i64,
    /// Wire field: `tail`.
    #[serde(rename = "tail")]
    pub tail: i64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyInfoSize {
    /// Wire field: `cols`.
    #[serde(rename = "cols")]
    pub cols: i64,
    /// Wire field: `rows`.
    #[serde(rename = "rows")]
    pub rows: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PersistentPtyInfoStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "exited")]
    Exited,
}

impl PersistentPtyInfoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
        }
    }
}

/// Wire schema: `PersistentPty.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyInfo {
    /// Wire field: `args`.
    #[serde(rename = "args")]
    pub args: Vec<String>,
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `cwd`.
    #[serde(rename = "cwd")]
    pub cwd: String,
    /// Wire field: `exitCode`.
    #[serde(default, rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Wire field: `foregroundProcess`.
    #[serde(rename = "foregroundProcess")]
    pub foreground_process: Option<String>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: PersistentPtyInfoOutput,
    /// Wire field: `pid`.
    #[serde(rename = "pid")]
    pub pid: i64,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: PersistentPtyInfoSize,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: PersistentPtyInfoStatus,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

/// Wire schema: `PersistentPty.ReadLinesEncoded`.
pub type PersistentPtyReadLinesEncoded = String;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyReadResultScreenCursor {
    /// Wire field: `x`.
    #[serde(rename = "x")]
    pub x: i64,
    /// Wire field: `y`.
    #[serde(rename = "y")]
    pub y: i64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyReadResultScreen {
    /// Wire field: `cols`.
    #[serde(rename = "cols")]
    pub cols: i64,
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: PersistentPtyReadResultScreenCursor,
    /// Wire field: `rows`.
    #[serde(rename = "rows")]
    pub rows: i64,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Wire schema: `PersistentPty.ReadResult`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyReadResult {
    /// Wire field: `cwd`.
    #[serde(rename = "cwd")]
    pub cwd: String,
    /// Wire field: `foregroundProcess`.
    #[serde(rename = "foregroundProcess")]
    pub foreground_process: Option<String>,
    /// Wire field: `ptyID`.
    #[serde(rename = "ptyID")]
    pub pty_id: String,
    /// Wire field: `screen`.
    #[serde(rename = "screen")]
    pub screen: PersistentPtyReadResultScreen,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtySnapshotCursor {
    /// Wire field: `x`.
    #[serde(rename = "x")]
    pub x: i64,
    /// Wire field: `y`.
    #[serde(rename = "y")]
    pub y: i64,
}

/// Wire schema: `PersistentPty.Snapshot`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtySnapshot {
    /// Wire field: `checkpoint`.
    #[serde(rename = "checkpoint")]
    pub checkpoint: String,
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: PersistentPtySnapshotCursor,
    /// Wire field: `info`.
    #[serde(rename = "info")]
    pub info: PersistentPtyInfo,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyUpdateInputSize {
    /// Wire field: `cols`.
    #[serde(rename = "cols")]
    pub cols: i64,
    /// Wire field: `rows`.
    #[serde(rename = "rows")]
    pub rows: i64,
}

/// Wire schema: `PersistentPty.UpdateInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PersistentPtyUpdateInput {
    /// Wire field: `attachmentID`.
    #[serde(default, rename = "attachmentID", skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: PersistentPtyUpdateInputSize,
}

/// Wire schema: `Plugin.Features`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginFeatures {
    /// Wire field: `rpc`.
    #[serde(default, rename = "rpc", skip_serializing_if = "Option::is_none")]
    pub rpc: Option<bool>,
    /// Wire field: `server`.
    #[serde(default, rename = "server", skip_serializing_if = "Option::is_none")]
    pub server: Option<bool>,
    /// Wire field: `tui`.
    #[serde(default, rename = "tui", skip_serializing_if = "Option::is_none")]
    pub tui: Option<bool>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginSourceVariant1Type {
    #[default]
    #[serde(rename = "builtin")]
    Builtin,
}

impl PluginSourceVariant1Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginSourceVariant1 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PluginSourceVariant1Type,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginSourceVariant2Type {
    #[default]
    #[serde(rename = "package")]
    Package,
}

impl PluginSourceVariant2Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Package => "package",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginSourceVariant2 {
    /// Wire field: `outdated`.
    #[serde(default, rename = "outdated", skip_serializing_if = "Option::is_none")]
    pub outdated: Option<bool>,
    /// Wire field: `target`.
    #[serde(rename = "target")]
    pub target: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PluginSourceVariant2Type,
    /// Wire field: `updating`.
    #[serde(default, rename = "updating", skip_serializing_if = "Option::is_none")]
    pub updating: Option<bool>,
    /// Wire field: `version`.
    #[serde(default, rename = "version", skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginSourceVariant3Type {
    #[default]
    #[serde(rename = "local")]
    Local,
}

impl PluginSourceVariant3Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginSourceVariant3 {
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PluginSourceVariant3Type,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginSourceVariant4Type {
    #[default]
    #[serde(rename = "sdk")]
    Sdk,
}

impl PluginSourceVariant4Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sdk => "sdk",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginSourceVariant4 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PluginSourceVariant4Type,
}

/// Wire schema: `Plugin.Source`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Plugin.Source`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PluginSource {
    Builtin(PluginSourceVariant1),
    Package(PluginSourceVariant2),
    Local(PluginSourceVariant3),
    Sdk(PluginSourceVariant4),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginStateVariant1Status {
    #[default]
    #[serde(rename = "active")]
    Active,
}

impl PluginStateVariant1Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginStateVariant1 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: PluginStateVariant1Status,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PluginStateVariant2Status {
    #[default]
    #[serde(rename = "failed")]
    Failed,
}

impl PluginStateVariant2Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginStateVariant2 {
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: String,
    /// Wire field: `ref`.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: PluginStateVariant2Status,
}

/// Wire schema: `Plugin.State`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Plugin.State`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PluginState {
    Active(PluginStateVariant1),
    Failed(PluginStateVariant2),
}

/// Wire schema: `Plugin.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    /// Wire field: `features`.
    #[serde(rename = "features")]
    pub features: PluginFeatures,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `source`.
    #[serde(rename = "source")]
    pub source: PluginSource,
    /// Wire field: `state`.
    #[serde(rename = "state")]
    pub state: PluginState,
}

/// Wire schema: `Project.Commands`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectCommands {
    /// Startup script to run when creating a new workspace (worktree)
    #[serde(default, rename = "start", skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
}

/// Wire schema: `Project.Icon`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectIcon {
    /// Wire field: `color`.
    #[serde(default, rename = "color", skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Wire field: `override`.
    #[serde(default, rename = "override", skip_serializing_if = "Option::is_none")]
    pub override_: Option<String>,
    /// Wire field: `url`.
    #[serde(default, rename = "url", skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Wire schema: `Project.Time`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectTime {
    /// Wire field: `active`.
    #[serde(rename = "active")]
    pub active: i64,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: i64,
    /// Wire field: `updated`.
    #[serde(rename = "updated")]
    pub updated: i64,
}

/// Wire schema: `Project.Vcs`.
pub type ProjectVcs = String;

/// Wire schema: `Project`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    /// Wire field: `canonical`.
    #[serde(rename = "canonical")]
    pub canonical: String,
    /// Wire field: `commands`.
    #[serde(default, rename = "commands", skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
    /// Wire field: `icon`.
    #[serde(default, rename = "icon", skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `sandboxes`.
    #[serde(rename = "sandboxes")]
    pub sandboxes: Vec<String>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: ProjectTime,
    /// Wire field: `vcs`.
    #[serde(default, rename = "vcs", skip_serializing_if = "Option::is_none")]
    pub vcs: Option<ProjectVcs>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ProjectNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "ProjectNotFoundError")]
    ProjectNotFoundError,
}

impl ProjectNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProjectNotFoundError => "ProjectNotFoundError",
        }
    }
}

/// Wire schema: `ProjectNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ProjectNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
}

/// Wire schema: `Prompt.Mention`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptMention {
    /// Wire field: `end`.
    #[serde(rename = "end")]
    pub end: f64,
    /// Wire field: `start`.
    #[serde(rename = "start")]
    pub start: f64,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Wire schema: `Prompt.AgentAttachment`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptAgentAttachment {
    /// Wire field: `mention`.
    #[serde(default, rename = "mention", skip_serializing_if = "Option::is_none")]
    pub mention: Option<PromptMention>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
}

/// Wire schema: `Prompt.Base64`.
pub type PromptBase64 = String;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PromptFileSourceVariant1Type {
    #[default]
    #[serde(rename = "inline")]
    Inline,
}

impl PromptFileSourceVariant1Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inline => "inline",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptFileSourceVariant1 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PromptFileSourceVariant1Type,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PromptFileSourceVariant2Type {
    #[default]
    #[serde(rename = "uri")]
    Uri,
}

impl PromptFileSourceVariant2Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Uri => "uri",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptFileSourceVariant2 {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: PromptFileSourceVariant2Type,
    /// Wire field: `uri`.
    #[serde(rename = "uri")]
    pub uri: String,
}

/// Wire schema: `Prompt.FileSource`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Prompt.FileSource`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PromptFileSource {
    Inline(PromptFileSourceVariant1),
    Uri(PromptFileSourceVariant2),
}

/// Wire schema: `Prompt.FileAttachment`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptFileAttachment {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: PromptBase64,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `mention`.
    #[serde(default, rename = "mention", skip_serializing_if = "Option::is_none")]
    pub mention: Option<PromptMention>,
    /// Wire field: `mime`.
    #[serde(rename = "mime")]
    pub mime: String,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `source`.
    #[serde(rename = "source")]
    pub source: PromptFileSource,
}

/// Wire schema: `Prompt.SkillAttachment`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptSkillAttachment {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `mention`.
    #[serde(default, rename = "mention", skip_serializing_if = "Option::is_none")]
    pub mention: Option<PromptMention>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `text`.
    #[serde(default, rename = "text", skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Wire schema: `PromptInput.FileAttachment`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptInputFileAttachment {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `mention`.
    #[serde(default, rename = "mention", skip_serializing_if = "Option::is_none")]
    pub mention: Option<PromptMention>,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `uri`.
    #[serde(rename = "uri")]
    pub uri: String,
}

/// Wire schema: `PromptInput.SkillAttachment`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptInputSkillAttachment {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `mention`.
    #[serde(default, rename = "mention", skip_serializing_if = "Option::is_none")]
    pub mention: Option<PromptMention>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProviderInfoActivation {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}

impl ProviderInfoActivation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

/// Wire schema: `Provider.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderInfo {
    /// Wire field: `activation`.
    #[serde(rename = "activation")]
    pub activation: ProviderInfoActivation,
    /// Wire field: `body`.
    #[serde(default, rename = "body", skip_serializing_if = "Option::is_none")]
    pub body: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `canonical`.
    #[serde(default, rename = "canonical", skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    /// Wire field: `headers`.
    #[serde(default, rename = "headers", skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `integrationID`.
    #[serde(default, rename = "integrationID", skip_serializing_if = "Option::is_none")]
    pub integration_id: Option<String>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `package`.
    #[serde(rename = "package")]
    pub package: String,
    /// Wire field: `settings`.
    #[serde(default, rename = "settings", skip_serializing_if = "Option::is_none")]
    pub settings: Option<ProviderSettings>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ProviderNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "ProviderNotFoundError")]
    ProviderNotFoundError,
}

impl ProviderNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProviderNotFoundError => "ProviderNotFoundError",
        }
    }
}

/// Wire schema: `ProviderNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ProviderNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PtyStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "exited")]
    Exited,
}

impl PtyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
        }
    }
}

/// Wire schema: `Pty`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pty {
    /// Wire field: `args`.
    #[serde(rename = "args")]
    pub args: Vec<String>,
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `cwd`.
    #[serde(rename = "cwd")]
    pub cwd: String,
    /// Wire field: `exitCode`.
    #[serde(default, rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `pid`.
    #[serde(rename = "pid")]
    pub pid: i64,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: PtyStatus,
    /// Wire field: `title`.
    #[serde(rename = "title")]
    pub title: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PtyNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "PtyNotFoundError")]
    PtyNotFoundError,
}

impl PtyNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PtyNotFoundError => "PtyNotFoundError",
        }
    }
}

/// Wire schema: `PtyNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: PtyNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `ptyID`.
    #[serde(rename = "ptyID")]
    pub pty_id: String,
}

/// Wire schema: `PtyTicket.ConnectToken`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyTicketConnectToken {
    /// Wire field: `expires_in`.
    #[serde(rename = "expires_in")]
    pub expires_in: i64,
    /// Wire field: `ticket`.
    #[serde(rename = "ticket")]
    pub ticket: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ReferenceGitSourceType {
    #[default]
    #[serde(rename = "git")]
    Git,
}

impl ReferenceGitSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Git => "git",
        }
    }
}

/// Wire schema: `Reference.GitSource`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReferenceGitSource {
    /// Wire field: `branch`.
    #[serde(default, rename = "branch", skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Wire field: `repository`.
    #[serde(rename = "repository")]
    pub repository: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ReferenceGitSourceType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ReferenceLocalSourceType {
    #[default]
    #[serde(rename = "local")]
    Local,
}

impl ReferenceLocalSourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
        }
    }
}

/// Wire schema: `Reference.LocalSource`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReferenceLocalSource {
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ReferenceLocalSourceType,
}

/// Wire schema: `Reference.Source`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Reference.Source`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ReferenceSource {
    LocalSource(ReferenceLocalSource),
    GitSource(ReferenceGitSource),
}

/// Wire schema: `Reference.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReferenceInfo {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `hidden`.
    #[serde(default, rename = "hidden", skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
    /// Wire field: `source`.
    #[serde(rename = "source")]
    pub source: ReferenceSource,
}

/// Wire schema: `Rpc.Input`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpcInput {
    /// Wire field: `input`.
    #[serde(default, rename = "input", skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// Wire schema: `Rpc.Output`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpcOutput {
    /// Wire field: `output`.
    #[serde(default, rename = "output", skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum RpcErrorEncodedTag {
    #[default]
    #[serde(rename = "RpcError")]
    RpcError,
}

impl RpcErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RpcError => "RpcError",
        }
    }
}

/// Wire schema: `RpcErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpcErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: RpcErrorEncodedTag,
    /// Wire field: `data`.
    #[serde(default, rename = "data", skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum RpcInternalErrorEncodedTag {
    #[default]
    #[serde(rename = "RpcInternalError")]
    RpcInternalError,
}

impl RpcInternalErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RpcInternalError => "RpcInternalError",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RpcInternalErrorEncodedType {
    #[serde(rename = "rpc.internal")]
    RpcInternal,
    #[serde(rename = "rpc.invalid_output")]
    RpcInvalidOutput,
}

impl RpcInternalErrorEncodedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RpcInternal => "rpc.internal",
            Self::RpcInvalidOutput => "rpc.invalid_output",
        }
    }
}

/// Wire schema: `RpcInternalErrorEncoded`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RpcInternalErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: RpcInternalErrorEncodedTag,
    /// Wire field: `data`.
    #[serde(default, rename = "data", skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: RpcInternalErrorEncodedType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerInfoCapabilities {
    /// Wire field: `persistentPty`.
    #[serde(default, rename = "persistentPty", skip_serializing_if = "Option::is_none")]
    pub persistent_pty: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerInfoPaths {
    /// Wire field: `tmp`.
    #[serde(rename = "tmp")]
    pub tmp: String,
}

/// Wire schema: `ServerInfo`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerInfo {
    /// Wire field: `capabilities`.
    #[serde(default, rename = "capabilities", skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<ServerInfoCapabilities>,
    /// Wire field: `paths`.
    #[serde(rename = "paths")]
    pub paths: ServerInfoPaths,
    /// Wire field: `pid`.
    #[serde(rename = "pid")]
    pub pid: i64,
    /// Wire field: `urls`.
    #[serde(rename = "urls")]
    pub urls: Vec<String>,
    /// Wire field: `version`.
    #[serde(rename = "version")]
    pub version: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ServiceUnavailableErrorEncodedTag {
    #[default]
    #[serde(rename = "ServiceUnavailableError")]
    ServiceUnavailableError,
}

impl ServiceUnavailableErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ServiceUnavailableError => "ServiceUnavailableError",
        }
    }
}

/// Wire schema: `ServiceUnavailableErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServiceUnavailableErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ServiceUnavailableErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `service`.
    #[serde(default, rename = "service", skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionForkBoundaryVariant1Type {
    #[default]
    #[serde(rename = "before")]
    Before,
}

impl SessionForkBoundaryVariant1Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Before => "before",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionForkBoundaryVariant1 {
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionForkBoundaryVariant1Type,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionForkBoundaryVariant2Type {
    #[default]
    #[serde(rename = "through")]
    Through,
}

impl SessionForkBoundaryVariant2Type {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Through => "through",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionForkBoundaryVariant2 {
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionForkBoundaryVariant2Type,
}

/// Wire schema: `Session.ForkBoundary`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Session.ForkBoundary`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionForkBoundary {
    Before(SessionForkBoundaryVariant1),
    Through(SessionForkBoundaryVariant2),
}

/// Wire schema: `Session.Inbox.Delivery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionInboxDelivery {
    #[serde(rename = "steer")]
    Steer,
    #[serde(rename = "queue")]
    Queue,
}

impl SessionInboxDelivery {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steer => "steer",
            Self::Queue => "queue",
        }
    }
}

/// Wire schema: `Session.Inbox.CompactionPayload`.
/// Untagged union; serde matches variants in declared order (wire schema `Session.Inbox.CompactionPayload`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionInboxCompactionPayload {
    Map(std::collections::BTreeMap<String, serde_json::Value>),
    Array(Vec<serde_json::Value>),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxCompactionTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionInboxCompactionType {
    #[default]
    #[serde(rename = "compaction")]
    Compaction,
}

impl SessionInboxCompactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
        }
    }
}

/// Wire schema: `Session.Inbox.Compaction`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxCompaction {
    /// Wire field: `delivery`.
    #[serde(rename = "delivery")]
    pub delivery: SessionInboxDelivery,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `payload`.
    #[serde(rename = "payload")]
    pub payload: SessionInboxCompactionPayload,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionInboxCompactionTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionInboxCompactionType,
}

/// Wire schema: `Session.Inbox.UserPayload`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxUserPayload {
    /// Wire field: `agents`.
    #[serde(default, rename = "agents", skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<PromptAgentAttachment>>,
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<PromptFileAttachment>>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `skills`.
    #[serde(default, rename = "skills", skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<PromptSkillAttachment>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxUserTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionInboxUserType {
    #[default]
    #[serde(rename = "user")]
    User,
}

impl SessionInboxUserType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
        }
    }
}

/// Wire schema: `Session.Inbox.User`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxUser {
    /// Wire field: `delivery`.
    #[serde(rename = "delivery")]
    pub delivery: SessionInboxDelivery,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `payload`.
    #[serde(rename = "payload")]
    pub payload: SessionInboxUserPayload,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionInboxUserTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionInboxUserType,
}

/// Wire schema: `Session.Inbox.SyntheticPayload`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxSyntheticPayload {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxSyntheticTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionInboxSyntheticType {
    #[default]
    #[serde(rename = "synthetic")]
    Synthetic,
}

impl SessionInboxSyntheticType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
        }
    }
}

/// Wire schema: `Session.Inbox.Synthetic`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxSynthetic {
    /// Wire field: `delivery`.
    #[serde(rename = "delivery")]
    pub delivery: SessionInboxDelivery,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `payload`.
    #[serde(rename = "payload")]
    pub payload: SessionInboxSyntheticPayload,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionInboxSyntheticTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionInboxSyntheticType,
}

/// Wire schema: `Session.Inbox.MovePayload`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxMovePayload {
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
    /// Wire field: `subpath`.
    #[serde(default, rename = "subpath", skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxMoveTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionInboxMoveType {
    #[default]
    #[serde(rename = "move")]
    Move,
}

impl SessionInboxMoveType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Move => "move",
        }
    }
}

/// Wire schema: `Session.Inbox.Move`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxMove {
    /// Wire field: `delivery`.
    #[serde(rename = "delivery")]
    pub delivery: SessionInboxDelivery,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `payload`.
    #[serde(rename = "payload")]
    pub payload: SessionInboxMovePayload,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionInboxMoveTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionInboxMoveType,
}

/// Wire schema: `Session.Inbox.Info`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Session.Inbox.Info`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionInboxInfo {
    User(SessionInboxUser),
    Synthetic(SessionInboxSynthetic),
    Compaction(SessionInboxCompaction),
    Move(SessionInboxMove),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInfoFork {
    /// Wire field: `boundary`.
    #[serde(rename = "boundary")]
    pub boundary: SessionForkBoundary,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// Wire schema: `Session.Metadata`.
pub type SessionMetadata = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionInfoOutcome {
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "interrupted")]
    Interrupted,
}

impl SessionInfoOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Wire schema: `Session.Revert`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionRevert {
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileDiffInfo>>,
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
    /// Wire field: `partID`.
    #[serde(default, rename = "partID", skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    /// Wire field: `snapshot`.
    #[serde(default, rename = "snapshot", skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInfoTime {
    /// Wire field: `archived`.
    #[serde(default, rename = "archived", skip_serializing_if = "Option::is_none")]
    pub archived: Option<f64>,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
    /// Wire field: `idle`.
    #[serde(default, rename = "idle", skip_serializing_if = "Option::is_none")]
    pub idle: Option<f64>,
    /// Wire field: `updated`.
    #[serde(rename = "updated")]
    pub updated: f64,
    /// Wire field: `viewed`.
    #[serde(default, rename = "viewed", skip_serializing_if = "Option::is_none")]
    pub viewed: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenUsageInfoCache {
    /// Wire field: `read`.
    #[serde(rename = "read")]
    pub read: f64,
    /// Wire field: `write`.
    #[serde(rename = "write")]
    pub write: f64,
}

/// Wire schema: `TokenUsage.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TokenUsageInfo {
    /// Wire field: `cache`.
    #[serde(rename = "cache")]
    pub cache: TokenUsageInfoCache,
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: f64,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: f64,
    /// Wire field: `reasoning`.
    #[serde(rename = "reasoning")]
    pub reasoning: f64,
}

/// Wire schema: `Session.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    /// Wire field: `agent`.
    #[serde(default, rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wire field: `cost`.
    #[serde(rename = "cost")]
    pub cost: MoneyUsd,
    /// Wire field: `fork`.
    #[serde(default, rename = "fork", skip_serializing_if = "Option::is_none")]
    pub fork: Option<SessionInfoFork>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Wire field: `outcome`.
    #[serde(default, rename = "outcome", skip_serializing_if = "Option::is_none")]
    pub outcome: Option<SessionInfoOutcome>,
    /// Wire field: `parentID`.
    #[serde(default, rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Wire field: `permissions`.
    #[serde(default, rename = "permissions", skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionRuleset>,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
    /// Wire field: `revert`.
    #[serde(default, rename = "revert", skip_serializing_if = "Option::is_none")]
    pub revert: Option<SessionRevert>,
    /// Wire field: `subpath`.
    #[serde(default, rename = "subpath", skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionInfoTime,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `tokens`.
    #[serde(rename = "tokens")]
    pub tokens: TokenUsageInfo,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAgentSelectedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageAgentSelectedType {
    #[default]
    #[serde(rename = "agent-switched")]
    AgentSwitched,
}

impl SessionMessageAgentSelectedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentSwitched => "agent-switched",
        }
    }
}

/// Wire schema: `Session.Message.AgentSelected`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAgentSelected {
    /// Wire field: `agent`.
    #[serde(rename = "agent")]
    pub agent: String,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `previous`.
    #[serde(default, rename = "previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageAgentSelectedTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageAgentSelectedType,
}

/// Wire schema: `Session.Message.ProviderState`.
pub type SessionMessageProviderState = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageAssistantTextType {
    #[default]
    #[serde(rename = "text")]
    Text,
}

impl SessionMessageAssistantTextType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
        }
    }
}

/// Wire schema: `Session.Message.Assistant.Text`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantText {
    /// Wire field: `state`.
    #[serde(default, rename = "state", skip_serializing_if = "Option::is_none")]
    pub state: Option<SessionMessageProviderState>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageAssistantTextType,
}

/// Wire schema: `Session.Message.ProviderState_1`.
pub type SessionMessageProviderState1 = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantReasoningTime {
    /// Wire field: `completed`.
    #[serde(default, rename = "completed", skip_serializing_if = "Option::is_none")]
    pub completed: Option<f64>,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageAssistantReasoningType {
    #[default]
    #[serde(rename = "reasoning")]
    Reasoning,
}

impl SessionMessageAssistantReasoningType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
        }
    }
}

/// Wire schema: `Session.Message.Assistant.Reasoning`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantReasoning {
    /// Wire field: `state`.
    #[serde(default, rename = "state", skip_serializing_if = "Option::is_none")]
    pub state: Option<SessionMessageProviderState1>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `time`.
    #[serde(default, rename = "time", skip_serializing_if = "Option::is_none")]
    pub time: Option<SessionMessageAssistantReasoningTime>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageAssistantReasoningType,
}

/// Wire schema: `Session.Message.ProviderState_3`.
pub type SessionMessageProviderState3 = std::collections::BTreeMap<String, serde_json::Value>;

/// Wire schema: `Session.Message.ProviderState_2`.
pub type SessionMessageProviderState2 = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageToolStateStreamingStatus {
    #[default]
    #[serde(rename = "streaming")]
    Streaming,
}

impl SessionMessageToolStateStreamingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
        }
    }
}

/// Wire schema: `Session.Message.ToolState.Streaming`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageToolStateStreaming {
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageToolStateStreamingStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageToolStateRunningStatus {
    #[default]
    #[serde(rename = "running")]
    Running,
}

impl SessionMessageToolStateRunningStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
        }
    }
}

/// Wire schema: `Session.Message.ToolState.Running`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageToolStateRunning {
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `metadata`.
    #[serde(rename = "metadata")]
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageToolStateRunningStatus,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ToolTextContentType {
    #[default]
    #[serde(rename = "text")]
    Text,
}

impl ToolTextContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
        }
    }
}

/// Wire schema: `Tool.TextContent`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolTextContent {
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ToolTextContentType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ToolFileContentType {
    #[default]
    #[serde(rename = "file")]
    File,
}

impl ToolFileContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
        }
    }
}

/// Wire schema: `Tool.FileContent`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolFileContent {
    /// Wire field: `mime`.
    #[serde(rename = "mime")]
    pub mime: String,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: ToolFileContentType,
    /// Wire field: `uri`.
    #[serde(rename = "uri")]
    pub uri: String,
}

/// Wire schema: `Tool.Content`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Tool.Content`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ToolContent {
    TextContent(ToolTextContent),
    FileContent(ToolFileContent),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageToolStateCompletedStatus {
    #[default]
    #[serde(rename = "completed")]
    Completed,
}

impl SessionMessageToolStateCompletedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
        }
    }
}

/// Wire schema: `Session.Message.ToolState.Completed`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageToolStateCompleted {
    /// Wire field: `content`.
    #[serde(rename = "content")]
    pub content: Vec<ToolContent>,
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageToolStateCompletedStatus,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStructuredErrorResponse {
    /// Wire field: `body`.
    #[serde(rename = "body")]
    pub body: String,
}

/// Wire schema: `Session.StructuredError`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStructuredError {
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `response`.
    #[serde(default, rename = "response", skip_serializing_if = "Option::is_none")]
    pub response: Option<SessionStructuredErrorResponse>,
    /// Wire field: `status`.
    #[serde(default, rename = "status", skip_serializing_if = "Option::is_none")]
    pub status: Option<i64>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageToolStateErrorStatus {
    #[default]
    #[serde(rename = "error")]
    Error,
}

impl SessionMessageToolStateErrorStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
        }
    }
}

/// Wire schema: `Session.Message.ToolState.Error`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageToolStateError {
    /// Wire field: `content`.
    #[serde(default, rename = "content", skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<ToolContent>>,
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: SessionStructuredError,
    /// Wire field: `input`.
    #[serde(rename = "input")]
    pub input: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageToolStateErrorStatus,
}

/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `SessionMessageAssistantToolState`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionMessageAssistantToolState {
    Streaming(SessionMessageToolStateStreaming),
    Running(SessionMessageToolStateRunning),
    Completed(SessionMessageToolStateCompleted),
    Error(SessionMessageToolStateError),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantToolTime {
    /// Wire field: `completed`.
    #[serde(default, rename = "completed", skip_serializing_if = "Option::is_none")]
    pub completed: Option<f64>,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
    /// Wire field: `ran`.
    #[serde(default, rename = "ran", skip_serializing_if = "Option::is_none")]
    pub ran: Option<f64>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageAssistantToolType {
    #[default]
    #[serde(rename = "tool")]
    Tool,
}

impl SessionMessageAssistantToolType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tool => "tool",
        }
    }
}

/// Wire schema: `Session.Message.Assistant.Tool`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantTool {
    /// Wire field: `executed`.
    #[serde(default, rename = "executed", skip_serializing_if = "Option::is_none")]
    pub executed: Option<bool>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `providerResultState`.
    #[serde(default, rename = "providerResultState", skip_serializing_if = "Option::is_none")]
    pub provider_result_state: Option<SessionMessageProviderState3>,
    /// Wire field: `providerState`.
    #[serde(default, rename = "providerState", skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<SessionMessageProviderState2>,
    /// Wire field: `state`.
    #[serde(rename = "state")]
    pub state: SessionMessageAssistantToolState,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageAssistantToolTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageAssistantToolType,
}

/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `SessionMessageAssistantContentItem`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionMessageAssistantContentItem {
    Text(SessionMessageAssistantText),
    Reasoning(SessionMessageAssistantReasoning),
    Tool(SessionMessageAssistantTool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageAssistantFinish {
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "length")]
    Length,
    #[serde(rename = "tool-calls")]
    ToolCalls,
    #[serde(rename = "content-filter")]
    ContentFilter,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "unknown")]
    Unknown,
}

impl SessionMessageAssistantFinish {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ToolCalls => "tool-calls",
            Self::ContentFilter => "content-filter",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

/// Wire schema: `Session.Message.ProviderState_4`.
pub type SessionMessageProviderState4 = std::collections::BTreeMap<String, serde_json::Value>;

/// Wire schema: `Session.Message.Assistant.Retry`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantRetry {
    /// Wire field: `at`.
    #[serde(rename = "at")]
    pub at: f64,
    /// Wire field: `attempt`.
    #[serde(rename = "attempt")]
    pub attempt: i64,
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: SessionStructuredError,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantSnapshot {
    /// Wire field: `end`.
    #[serde(default, rename = "end", skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Wire field: `start`.
    #[serde(default, rename = "start", skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistantTime {
    /// Wire field: `completed`.
    #[serde(default, rename = "completed", skip_serializing_if = "Option::is_none")]
    pub completed: Option<f64>,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
    /// Wire field: `streamed`.
    #[serde(default, rename = "streamed", skip_serializing_if = "Option::is_none")]
    pub streamed: Option<f64>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageAssistantType {
    #[default]
    #[serde(rename = "assistant")]
    Assistant,
}

impl SessionMessageAssistantType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Assistant => "assistant",
        }
    }
}

/// Wire schema: `Session.Message.Assistant`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageAssistant {
    /// Wire field: `agent`.
    #[serde(rename = "agent")]
    pub agent: String,
    /// Wire field: `content`.
    #[serde(rename = "content")]
    pub content: Vec<SessionMessageAssistantContentItem>,
    /// Wire field: `cost`.
    #[serde(default, rename = "cost", skip_serializing_if = "Option::is_none")]
    pub cost: Option<MoneyUsd>,
    /// Wire field: `error`.
    #[serde(default, rename = "error", skip_serializing_if = "Option::is_none")]
    pub error: Option<SessionStructuredError>,
    /// Wire field: `finish`.
    #[serde(default, rename = "finish", skip_serializing_if = "Option::is_none")]
    pub finish: Option<SessionMessageAssistantFinish>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: ModelRef,
    /// Wire field: `providerState`.
    #[serde(default, rename = "providerState", skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<SessionMessageProviderState4>,
    /// Wire field: `rawFinish`.
    #[serde(default, rename = "rawFinish", skip_serializing_if = "Option::is_none")]
    pub raw_finish: Option<String>,
    /// Wire field: `retry`.
    #[serde(default, rename = "retry", skip_serializing_if = "Option::is_none")]
    pub retry: Option<SessionMessageAssistantRetry>,
    /// Wire field: `snapshot`.
    #[serde(default, rename = "snapshot", skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SessionMessageAssistantSnapshot>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageAssistantTime,
    /// Wire field: `tokens`.
    #[serde(default, rename = "tokens", skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsageInfo>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageAssistantType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageCompactionRunningReason {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

impl SessionMessageCompactionRunningReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionRunningStatus {
    #[default]
    #[serde(rename = "running")]
    Running,
}

impl SessionMessageCompactionRunningStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionRunningTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionRunningType {
    #[default]
    #[serde(rename = "compaction")]
    Compaction,
}

impl SessionMessageCompactionRunningType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
        }
    }
}

/// Wire schema: `Session.Message.Compaction.Running`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionRunning {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `reason`.
    #[serde(rename = "reason")]
    pub reason: SessionMessageCompactionRunningReason,
    /// Wire field: `recent`.
    #[serde(rename = "recent")]
    pub recent: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageCompactionRunningStatus,
    /// Wire field: `summary`.
    #[serde(rename = "summary")]
    pub summary: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageCompactionRunningTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageCompactionRunningType,
}

/// Wire schema: `Session.ProviderContext.Provenance`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionProviderContextProvenance {
    /// Wire field: `endpoint`.
    #[serde(rename = "endpoint")]
    pub endpoint: String,
    /// Wire field: `modelID`.
    #[serde(rename = "modelID")]
    pub model_id: String,
    /// Wire field: `protocol`.
    #[serde(rename = "protocol")]
    pub protocol: String,
    /// Wire field: `provider`.
    #[serde(rename = "provider")]
    pub provider: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `route`.
    #[serde(rename = "route")]
    pub route: String,
}

/// Wire schema: `Session.ProviderContext`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionProviderContext {
    /// Wire field: `messages`.
    #[serde(rename = "messages")]
    pub messages: serde_json::Value,
    /// Wire field: `provenance`.
    #[serde(rename = "provenance")]
    pub provenance: SessionProviderContextProvenance,
    /// Wire field: `version`.
    #[serde(rename = "version")]
    pub version: f64,
}

/// Wire schema: `Session.Message.ProviderState_5`.
pub type SessionMessageProviderState5 = std::collections::BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageCompactionCompletedReason {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

impl SessionMessageCompactionCompletedReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionCompletedStatus {
    #[default]
    #[serde(rename = "completed")]
    Completed,
}

impl SessionMessageCompactionCompletedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionCompletedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionCompletedType {
    #[default]
    #[serde(rename = "compaction")]
    Compaction,
}

impl SessionMessageCompactionCompletedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
        }
    }
}

/// Wire schema: `Session.Message.Compaction.Completed`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionCompleted {
    /// Wire field: `cost`.
    #[serde(default, rename = "cost", skip_serializing_if = "Option::is_none")]
    pub cost: Option<MoneyUsd>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Wire field: `providerContext`.
    #[serde(default, rename = "providerContext", skip_serializing_if = "Option::is_none")]
    pub provider_context: Option<SessionProviderContext>,
    /// Wire field: `providerState`.
    #[serde(default, rename = "providerState", skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<SessionMessageProviderState5>,
    /// Wire field: `reason`.
    #[serde(rename = "reason")]
    pub reason: SessionMessageCompactionCompletedReason,
    /// Wire field: `recent`.
    #[serde(rename = "recent")]
    pub recent: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageCompactionCompletedStatus,
    /// Wire field: `summary`.
    #[serde(rename = "summary")]
    pub summary: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageCompactionCompletedTime,
    /// Wire field: `tokens`.
    #[serde(default, rename = "tokens", skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsageInfo>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageCompactionCompletedType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageCompactionFailedReason {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "manual")]
    Manual,
}

impl SessionMessageCompactionFailedReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionFailedStatus {
    #[default]
    #[serde(rename = "failed")]
    Failed,
}

impl SessionMessageCompactionFailedStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionFailedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageCompactionFailedType {
    #[default]
    #[serde(rename = "compaction")]
    Compaction,
}

impl SessionMessageCompactionFailedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compaction => "compaction",
        }
    }
}

/// Wire schema: `Session.Message.Compaction.Failed`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageCompactionFailed {
    /// Wire field: `cost`.
    #[serde(default, rename = "cost", skip_serializing_if = "Option::is_none")]
    pub cost: Option<MoneyUsd>,
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: SessionStructuredError,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `reason`.
    #[serde(rename = "reason")]
    pub reason: SessionMessageCompactionFailedReason,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageCompactionFailedStatus,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageCompactionFailedTime,
    /// Wire field: `tokens`.
    #[serde(default, rename = "tokens", skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsageInfo>,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageCompactionFailedType,
}

/// Wire schema: `Session.Message.Compaction`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `Session.Message.Compaction`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionMessageCompaction {
    Running(SessionMessageCompactionRunning),
    Completed(SessionMessageCompactionCompleted),
    Failed(SessionMessageCompactionFailed),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageIdleOutcome {
    #[serde(rename = "succeeded")]
    Succeeded,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "interrupted")]
    Interrupted,
}

impl SessionMessageIdleOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageIdleTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageIdleType {
    #[default]
    #[serde(rename = "idle")]
    Idle,
}

impl SessionMessageIdleType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
        }
    }
}

/// Wire schema: `Session.Message.Idle`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageIdle {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `outcome`.
    #[serde(rename = "outcome")]
    pub outcome: SessionMessageIdleOutcome,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageIdleTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageIdleType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageModelSelectedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageModelSelectedType {
    #[default]
    #[serde(rename = "model-switched")]
    ModelSwitched,
}

impl SessionMessageModelSelectedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ModelSwitched => "model-switched",
        }
    }
}

/// Wire schema: `Session.Message.ModelSelected`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageModelSelected {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: ModelRef,
    /// Wire field: `previous`.
    #[serde(default, rename = "previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<ModelRef>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageModelSelectedTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageModelSelectedType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageLocationSwitchedPrevious {
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
    /// Wire field: `projectID`.
    #[serde(default, rename = "projectID", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Wire field: `subpath`.
    #[serde(default, rename = "subpath", skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageLocationSwitchedTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageLocationSwitchedType {
    #[default]
    #[serde(rename = "location-switched")]
    LocationSwitched,
}

impl SessionMessageLocationSwitchedType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocationSwitched => "location-switched",
        }
    }
}

/// Wire schema: `Session.Message.LocationSwitched`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageLocationSwitched {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `previous`.
    #[serde(default, rename = "previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<SessionMessageLocationSwitchedPrevious>,
    /// Wire field: `projectID`.
    #[serde(default, rename = "projectID", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Wire field: `subpath`.
    #[serde(default, rename = "subpath", skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageLocationSwitchedTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageLocationSwitchedType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageUserTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageUserType {
    #[default]
    #[serde(rename = "user")]
    User,
}

impl SessionMessageUserType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
        }
    }
}

/// Wire schema: `Session.Message.User`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageUser {
    /// Wire field: `agents`.
    #[serde(default, rename = "agents", skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<PromptAgentAttachment>>,
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<PromptFileAttachment>>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `skills`.
    #[serde(default, rename = "skills", skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<PromptSkillAttachment>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageUserTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageUserType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSyntheticTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageSyntheticType {
    #[default]
    #[serde(rename = "synthetic")]
    Synthetic,
}

impl SessionMessageSyntheticType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
        }
    }
}

/// Wire schema: `Session.Message.Synthetic`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSynthetic {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageSyntheticTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageSyntheticType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSystemTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageSystemType {
    #[default]
    #[serde(rename = "system")]
    System,
}

impl SessionMessageSystemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
        }
    }
}

/// Wire schema: `Session.Message.System`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSystem {
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageSystemTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageSystemType,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSkillTime {
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageSkillType {
    #[default]
    #[serde(rename = "skill")]
    Skill,
}

impl SessionMessageSkillType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
        }
    }
}

/// Wire schema: `Session.Message.Skill`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageSkill {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `skill`.
    #[serde(rename = "skill")]
    pub skill: String,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageSkillTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageSkillType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageShellExitVariant2 {
    #[serde(rename = "Infinity")]
    Infinity,
    #[serde(rename = "-Infinity")]
    Infinity2,
    #[serde(rename = "NaN")]
    NaN,
}

impl SessionMessageShellExitVariant2 {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Infinity => "Infinity",
            Self::Infinity2 => "-Infinity",
            Self::NaN => "NaN",
        }
    }
}

/// Untagged union; serde matches variants in declared order (wire schema `SessionMessageShellExit`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionMessageShellExit {
    Number(f64),
    SessionMessageShellExitVariant2(SessionMessageShellExitVariant2),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageShellOutput {
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: i64,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: String,
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: i64,
    /// Wire field: `truncated`.
    #[serde(rename = "truncated")]
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageShellStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "exited")]
    Exited,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "killed")]
    Killed,
}

impl SessionMessageShellStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Timeout => "timeout",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageShellTime {
    /// Wire field: `completed`.
    #[serde(default, rename = "completed", skip_serializing_if = "Option::is_none")]
    pub completed: Option<f64>,
    /// Wire field: `created`.
    #[serde(rename = "created")]
    pub created: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionMessageShellType {
    #[default]
    #[serde(rename = "shell")]
    Shell,
}

impl SessionMessageShellType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Shell => "shell",
        }
    }
}

/// Wire schema: `Session.Message.Shell`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessageShell {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `exit`.
    #[serde(default, rename = "exit", skip_serializing_if = "Option::is_none")]
    pub exit: Option<SessionMessageShellExit>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `output`.
    #[serde(default, rename = "output", skip_serializing_if = "Option::is_none")]
    pub output: Option<SessionMessageShellOutput>,
    /// Wire field: `shellID`.
    #[serde(rename = "shellID")]
    pub shell_id: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: SessionMessageShellStatus,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: SessionMessageShellTime,
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionMessageShellType,
}

/// Wire schema: `Session.Message.Info`.
/// Untagged union; serde matches variants in declared order (wire schema `Session.Message.Info`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionMessageInfo {
    AgentSelected(SessionMessageAgentSelected),
    ModelSelected(SessionMessageModelSelected),
    LocationSwitched(SessionMessageLocationSwitched),
    User(SessionMessageUser),
    Synthetic(SessionMessageSynthetic),
    System(SessionMessageSystem),
    Skill(SessionMessageSkill),
    Shell(SessionMessageShell),
    Assistant(SessionMessageAssistant),
    Compaction(SessionMessageCompaction),
    Idle(SessionMessageIdle),
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionActiveType {
    #[default]
    #[serde(rename = "running")]
    Running,
}

impl SessionActiveType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
        }
    }
}

/// Wire schema: `SessionActive`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionActive {
    /// Wire field: `type`.
    #[serde(rename = "type")]
    pub type_: SessionActiveType,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionBusyErrorEncodedTag {
    #[default]
    #[serde(rename = "SessionBusyError")]
    SessionBusyError,
}

impl SessionBusyErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionBusyError => "SessionBusyError",
        }
    }
}

/// Wire schema: `SessionBusyErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionBusyErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: SessionBusyErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionGenerateResponseData {
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Wire schema: `SessionGenerateResponse`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionGenerateResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: SessionGenerateResponseData,
}

/// Wire schema: `SessionInterruptResponse`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInterruptResponse {
    /// Whether an active execution owned by this OpenCode process was interrupted.
    #[serde(rename = "interrupted")]
    pub interrupted: bool,
}

/// Wire schema: `SessionLogItemEncoded`.
pub type SessionLogItemEncoded = String;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessagesResponseCursor {
    /// Wire field: `next`.
    #[serde(default, rename = "next", skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Wire field: `previous`.
    #[serde(default, rename = "previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

/// Wire schema: `SessionMessagesResponse`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMessagesResponse {
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: SessionMessagesResponseCursor,
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<SessionMessageInfo>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "SessionNotFoundError")]
    SessionNotFoundError,
}

impl SessionNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionNotFoundError => "SessionNotFoundError",
        }
    }
}

/// Wire schema: `SessionNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: SessionNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `sessionID`.
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// Wire schema: `SessionStats.Activity`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsActivity {
    /// Wire field: `date`.
    #[serde(rename = "date")]
    pub date: String,
    /// Wire field: `steps`.
    #[serde(rename = "steps")]
    pub steps: i64,
}

/// Wire schema: `SessionStats.ModelUsage`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsModelUsage {
    /// Wire field: `cost`.
    #[serde(rename = "cost")]
    pub cost: MoneyUsd,
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: ModelRef,
    /// Wire field: `steps`.
    #[serde(rename = "steps")]
    pub steps: i64,
    /// Wire field: `tokens`.
    #[serde(rename = "tokens")]
    pub tokens: TokenUsageInfo,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsInfoRange {
    /// Wire field: `from`.
    #[serde(rename = "from")]
    pub from: f64,
    /// Wire field: `to`.
    #[serde(rename = "to")]
    pub to: f64,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionStatsToolsVariant1Mode {
    #[default]
    #[serde(rename = "none")]
    None,
}

impl SessionStatsToolsVariant1Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsToolsVariant1 {
    /// Wire field: `mode`.
    #[serde(rename = "mode")]
    pub mode: SessionStatsToolsVariant1Mode,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionStatsToolsVariant2Mode {
    #[default]
    #[serde(rename = "summary")]
    Summary,
}

impl SessionStatsToolsVariant2Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Summary => "summary",
        }
    }
}

/// Wire schema: `SessionStats.ToolTotals`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsToolTotals {
    /// Wire field: `calls`.
    #[serde(rename = "calls")]
    pub calls: i64,
    /// Wire field: `failed`.
    #[serde(rename = "failed")]
    pub failed: i64,
    /// Wire field: `succeeded`.
    #[serde(rename = "succeeded")]
    pub succeeded: i64,
    /// Wire field: `unfinished`.
    #[serde(rename = "unfinished")]
    pub unfinished: i64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsToolsVariant2 {
    /// Wire field: `mode`.
    #[serde(rename = "mode")]
    pub mode: SessionStatsToolsVariant2Mode,
    /// Wire field: `totals`.
    #[serde(rename = "totals")]
    pub totals: SessionStatsToolTotals,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SessionStatsToolsVariant3Mode {
    #[default]
    #[serde(rename = "detail")]
    Detail,
}

impl SessionStatsToolsVariant3Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Detail => "detail",
        }
    }
}

/// Wire schema: `SessionStats.ToolUsage`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsToolUsage {
    /// Wire field: `calls`.
    #[serde(rename = "calls")]
    pub calls: i64,
    /// Wire field: `durationP50`.
    #[serde(default, rename = "durationP50", skip_serializing_if = "Option::is_none")]
    pub duration_p50: Option<f64>,
    /// Wire field: `failed`.
    #[serde(rename = "failed")]
    pub failed: i64,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `succeeded`.
    #[serde(rename = "succeeded")]
    pub succeeded: i64,
    /// Wire field: `unfinished`.
    #[serde(rename = "unfinished")]
    pub unfinished: i64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsToolsVariant3 {
    /// Wire field: `mode`.
    #[serde(rename = "mode")]
    pub mode: SessionStatsToolsVariant3Mode,
    /// Wire field: `totals`.
    #[serde(rename = "totals")]
    pub totals: SessionStatsToolTotals,
    /// Wire field: `usage`.
    #[serde(rename = "usage")]
    pub usage: Vec<SessionStatsToolUsage>,
}

/// Wire schema: `SessionStats.Tools`.
/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `SessionStats.Tools`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum SessionStatsTools {
    None(SessionStatsToolsVariant1),
    Summary(SessionStatsToolsVariant2),
    Detail(SessionStatsToolsVariant3),
}

/// Wire schema: `SessionStats.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionStatsInfo {
    /// Wire field: `activeDays`.
    #[serde(rename = "activeDays")]
    pub active_days: i64,
    /// Wire field: `activity`.
    #[serde(rename = "activity")]
    pub activity: Vec<SessionStatsActivity>,
    /// Wire field: `cost`.
    #[serde(rename = "cost")]
    pub cost: MoneyUsd,
    /// Wire field: `models`.
    #[serde(rename = "models")]
    pub models: Vec<SessionStatsModelUsage>,
    /// Wire field: `prompts`.
    #[serde(rename = "prompts")]
    pub prompts: i64,
    /// Wire field: `range`.
    #[serde(rename = "range")]
    pub range: SessionStatsInfoRange,
    /// Wire field: `sessions`.
    #[serde(rename = "sessions")]
    pub sessions: i64,
    /// Wire field: `steps`.
    #[serde(rename = "steps")]
    pub steps: i64,
    /// Wire field: `streak`.
    #[serde(rename = "streak")]
    pub streak: i64,
    /// Wire field: `subagents`.
    #[serde(rename = "subagents")]
    pub subagents: i64,
    /// Wire field: `tokens`.
    #[serde(rename = "tokens")]
    pub tokens: TokenUsageInfo,
    /// Wire field: `tools`.
    #[serde(rename = "tools")]
    pub tools: SessionStatsTools,
}

/// Wire schema: `SessionTransfer.Data`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionTransferData {
    /// Wire field: `info`.
    #[serde(rename = "info")]
    pub info: SessionInfo,
    /// Wire field: `messages`.
    #[serde(rename = "messages")]
    pub messages: Vec<SessionMessageInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionsResponseCursor {
    /// Wire field: `next`.
    #[serde(default, rename = "next", skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Wire field: `previous`.
    #[serde(default, rename = "previous", skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

/// Wire schema: `SessionsResponse`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionsResponse {
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: SessionsResponseCursor,
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ShellInfoStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "exited")]
    Exited,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "killed")]
    Killed,
}

impl ShellInfoStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Timeout => "timeout",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellInfoTime {
    /// Wire field: `completed`.
    #[serde(default, rename = "completed", skip_serializing_if = "Option::is_none")]
    pub completed: Option<f64>,
    /// Start time in milliseconds since the Unix epoch
    #[serde(rename = "started")]
    pub started: f64,
}

/// Wire schema: `Shell.Info`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellInfo {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `cwd`.
    #[serde(rename = "cwd")]
    pub cwd: String,
    /// Wire field: `exit`.
    #[serde(default, rename = "exit", skip_serializing_if = "Option::is_none")]
    pub exit: Option<f64>,
    /// Wire field: `file`.
    #[serde(rename = "file")]
    pub file: String,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `metadata`.
    #[serde(rename = "metadata")]
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// Wire field: `pid`.
    #[serde(default, rename = "pid", skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    /// Wire field: `shell`.
    #[serde(rename = "shell")]
    pub shell: String,
    /// Wire field: `signal`.
    #[serde(default, rename = "signal", skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: ShellInfoStatus,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: ShellInfoTime,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ShellNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "ShellNotFoundError")]
    ShellNotFoundError,
}

impl ShellNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ShellNotFoundError => "ShellNotFoundError",
        }
    }
}

/// Wire schema: `ShellNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: ShellNotFoundErrorEncodedTag,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

/// Wire schema: `Skill.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkillInfo {
    /// Wire field: `autoinvoke`.
    #[serde(default, rename = "autoinvoke", skip_serializing_if = "Option::is_none")]
    pub autoinvoke: Option<bool>,
    /// Wire field: `content`.
    #[serde(rename = "content")]
    pub content: String,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `path`.
    #[serde(rename = "path")]
    pub path: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SkillNotFoundErrorEncodedTag {
    #[default]
    #[serde(rename = "SkillNotFoundError")]
    SkillNotFoundError,
}

impl SkillNotFoundErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SkillNotFoundError => "SkillNotFoundError",
        }
    }
}

/// Wire schema: `SkillNotFoundErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkillNotFoundErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: SkillNotFoundErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `skill`.
    #[serde(rename = "skill")]
    pub skill: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum UnauthorizedErrorEncodedTag {
    #[default]
    #[serde(rename = "UnauthorizedError")]
    UnauthorizedError,
}

impl UnauthorizedErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnauthorizedError => "UnauthorizedError",
        }
    }
}

/// Wire schema: `UnauthorizedErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnauthorizedErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: UnauthorizedErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum UnknownErrorEncodedTag {
    #[default]
    #[serde(rename = "UnknownError")]
    UnknownError,
}

impl UnknownErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnknownError => "UnknownError",
        }
    }
}

/// Wire schema: `UnknownErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnknownErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: UnknownErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `ref`.
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
}

/// Wire schema: `V2EventEncoded`.
pub type V2EventEncoded = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VcsBaseSource {
    #[serde(rename = "reflog")]
    Reflog,
    #[serde(rename = "default")]
    Default,
}

impl VcsBaseSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reflog => "reflog",
            Self::Default => "default",
        }
    }
}

/// Wire schema: `Vcs.Base`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsBase {
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `ref`.
    #[serde(rename = "ref")]
    pub ref_: String,
    /// Wire field: `source`.
    #[serde(rename = "source")]
    pub source: VcsBaseSource,
}

/// Wire schema: `Vcs.Branch`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsBranch {
    /// Wire field: `current`.
    #[serde(default, rename = "current", skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Wire field: `default`.
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// Wire schema: `Vcs.BranchList`.
pub type VcsBranchList = Vec<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VcsFileStatusStatus {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "modified")]
    Modified,
}

impl VcsFileStatusStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Modified => "modified",
        }
    }
}

/// Wire schema: `Vcs.FileStatus`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsFileStatus {
    /// Wire field: `additions`.
    #[serde(rename = "additions")]
    pub additions: i64,
    /// Wire field: `deletions`.
    #[serde(rename = "deletions")]
    pub deletions: i64,
    /// Wire field: `file`.
    #[serde(rename = "file")]
    pub file: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: VcsFileStatusStatus,
}

/// Wire schema: `Vcs.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsInfo {
    /// Wire field: `branch`.
    #[serde(rename = "branch")]
    pub branch: VcsBranch,
    /// Wire field: `provider`.
    #[serde(default, rename = "provider", skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// Wire schema: `Vcs.Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VcsMode {
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "branch")]
    Branch,
    #[serde(rename = "committed")]
    Committed,
}

impl VcsMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Branch => "branch",
            Self::Committed => "committed",
        }
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum VcsInitNotSupportedErrorEncodedTag {
    #[default]
    #[serde(rename = "VcsInitNotSupportedError")]
    VcsInitNotSupportedError,
}

impl VcsInitNotSupportedErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VcsInitNotSupportedError => "VcsInitNotSupportedError",
        }
    }
}

/// Wire schema: `VcsInitNotSupportedErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsInitNotSupportedErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: VcsInitNotSupportedErrorEncodedTag,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
}

/// Wire schema: `WebSearch.Provider`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchProvider {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchResultTime {
    /// Wire field: `published`.
    #[serde(default, rename = "published", skip_serializing_if = "Option::is_none")]
    pub published: Option<f64>,
}

/// Wire schema: `WebSearch.Result`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchResult {
    /// Wire field: `content`.
    #[serde(default, rename = "content", skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Wire field: `time`.
    #[serde(rename = "time")]
    pub time: WebSearchResultTime,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Wire field: `url`.
    #[serde(rename = "url")]
    pub url: String,
}

/// Wire schema: `WebSearch.ResponseEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchResponseEncoded {
    /// Wire field: `providerID`.
    #[serde(rename = "providerID")]
    pub provider_id: String,
    /// Wire field: `results`.
    #[serde(rename = "results")]
    pub results: Vec<WebSearchResult>,
}

/// Wire schema: `Worktree.CreateInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeCreateInput {
    /// Wire field: `branch`.
    #[serde(default, rename = "branch", skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Wire field: `directory`.
    #[serde(default, rename = "directory", skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    /// Wire field: `from`.
    #[serde(default, rename = "from", skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
}

/// Wire schema: `Worktree.Directory`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeDirectory {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
    /// Wire field: `strategy`.
    #[serde(default, rename = "strategy", skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

/// Wire schema: `Worktree.Info`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeInfo {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
}

/// Wire schema: `Worktree.List`.
pub type WorktreeList = Vec<WorktreeDirectory>;

/// Wire schema: `Worktree.RemoveInput`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeRemoveInput {
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
    /// Wire field: `force`.
    #[serde(rename = "force")]
    pub force: bool,
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum WorktreeErrorEncodedTag {
    #[default]
    #[serde(rename = "WorktreeError")]
    WorktreeError,
}

impl WorktreeErrorEncodedTag {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorktreeError => "WorktreeError",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeErrorEncodedData {
    /// Wire field: `forceRequired`.
    #[serde(default, rename = "forceRequired", skip_serializing_if = "Option::is_none")]
    pub force_required: Option<bool>,
    /// Wire field: `message`.
    #[serde(rename = "message")]
    pub message: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum WorktreeErrorEncodedName {
    #[default]
    #[serde(rename = "WorktreeError")]
    WorktreeError,
}

impl WorktreeErrorEncodedName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WorktreeError => "WorktreeError",
        }
    }
}

/// Wire schema: `WorktreeErrorEncoded`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeErrorEncoded {
    /// Wire field: `_tag`.
    #[serde(rename = "_tag")]
    pub tag: WorktreeErrorEncodedTag,
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: WorktreeErrorEncodedData,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: WorktreeErrorEncodedName,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `agent.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<AgentInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `agent.get`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: AgentInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `command.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<CommandInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `credential.update`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CredentialUpdateBody {
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DebugLocationEvictQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalFsWriteQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `path` (query).
    pub path: String,
}

/// Response envelope for `experimental.fs.write`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalFsWriteResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: FileSystemWrite,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

/// Request body for `experimental.generate.text`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalGenerateTextBody {
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Wire field: `prompt`.
    #[serde(rename = "prompt")]
    pub prompt: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalIntegrationWellknownAddQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `experimental.integration.wellknown.add`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalIntegrationWellknownAddBody {
    /// Wire field: `url`.
    #[serde(rename = "url")]
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalMcpRemoveQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalMcpAddQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Discriminated union (each variant carries its tag field);
/// serde matches variants in declared order (wire schema `ExperimentalMcpAddBodyConfig`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ExperimentalMcpAddBodyConfig {
    LocalConfigEncoded(McpLocalConfigEncoded),
    RemoteConfigEncoded(McpRemoteConfigEncoded),
}

/// Request body for `experimental.mcp.add`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalMcpAddBody {
    /// Wire field: `config`.
    #[serde(rename = "config")]
    pub config: ExperimentalMcpAddBodyConfig,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalMcpConnectQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalMcpDisconnectQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExperimentalMigrationV1StatusResponseVariant1Status {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "completed")]
    Completed,
}

impl ExperimentalMigrationV1StatusResponseVariant1Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalMigrationV1StatusResponseVariant1 {
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: ExperimentalMigrationV1StatusResponseVariant1Status,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalMigrationV1StatusResponseVariant2Progress {
    /// Wire field: `denominator`.
    #[serde(default, rename = "denominator", skip_serializing_if = "Option::is_none")]
    pub denominator: Option<i64>,
    /// Wire field: `label`.
    #[serde(rename = "label")]
    pub label: String,
    /// Wire field: `numerator`.
    #[serde(default, rename = "numerator", skip_serializing_if = "Option::is_none")]
    pub numerator: Option<i64>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ExperimentalMigrationV1StatusResponseVariant2Status {
    #[default]
    #[serde(rename = "running")]
    Running,
}

impl ExperimentalMigrationV1StatusResponseVariant2Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalMigrationV1StatusResponseVariant2 {
    /// Wire field: `progress`.
    #[serde(rename = "progress")]
    pub progress: ExperimentalMigrationV1StatusResponseVariant2Progress,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: ExperimentalMigrationV1StatusResponseVariant2Status,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ExperimentalMigrationV1StatusResponseVariant3Status {
    #[default]
    #[serde(rename = "error")]
    Error,
}

impl ExperimentalMigrationV1StatusResponseVariant3Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalMigrationV1StatusResponseVariant3 {
    /// Wire field: `error`.
    #[serde(rename = "error")]
    pub error: String,
    /// Wire field: `status`.
    #[serde(rename = "status")]
    pub status: ExperimentalMigrationV1StatusResponseVariant3Status,
}

/// Untagged union; serde matches variants in declared order (wire schema `ExperimentalMigrationV1StatusResponse`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum ExperimentalMigrationV1StatusResponse {
    ExperimentalMigrationV1StatusResponseVariant1(ExperimentalMigrationV1StatusResponseVariant1),
    ExperimentalMigrationV1StatusResponseVariant2(ExperimentalMigrationV1StatusResponseVariant2),
    ExperimentalMigrationV1StatusResponseVariant3(ExperimentalMigrationV1StatusResponseVariant3),
}

/// Response envelope for `server.experimental.persistentPty.handoff`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerExperimentalPersistentPtyHandoffResponse {
    /// Wire field: `handoff`.
    #[serde(rename = "handoff")]
    pub handoff: Option<PersistentPtyHandoff>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersistentPtyConnectQuery {
    /// Wire param: `attachment_id` (query).
    pub attachment_id: Option<String>,
    /// Wire param: `cursor` (query).
    pub cursor: Option<String>,
    /// Wire param: `input_protocol` (query).
    pub input_protocol: Option<String>,
    /// Wire param: `role` (query).
    pub role: Option<String>,
    /// Wire param: `takeover` (query).
    pub takeover: Option<String>,
    /// Wire param: `ticket` (query).
    pub ticket: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerExperimentalPersistentPtyConnectTokenQuery {
    /// Wire param: `x-opencode-ticket` (header).
    pub x_opencode_ticket: Option<String>,
}

/// Request body for `experimental.session.import`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalSessionImportBody {
    /// Wire field: `info`.
    #[serde(rename = "info")]
    pub info: SessionInfo,
    /// Wire field: `location`.
    #[serde(default, rename = "location", skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationPublicRef>,
    /// Wire field: `messages`.
    #[serde(rename = "messages")]
    pub messages: Vec<SessionMessageInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExperimentalSessionStatsQueryTools {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "summary")]
    Summary,
    #[serde(rename = "detail")]
    Detail,
}

impl ExperimentalSessionStatsQueryTools {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Summary => "summary",
            Self::Detail => "detail",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalSessionStatsQuery {
    /// Wire param: `from` (query).
    pub from: Option<String>,
    /// Wire param: `project` (query).
    pub project: Option<String>,
    /// Wire param: `timezone` (query).
    pub timezone: Option<String>,
    /// Wire param: `to` (query).
    pub to: Option<String>,
    /// Wire param: `tools` (query).
    pub tools: Option<ExperimentalSessionStatsQueryTools>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExperimentalSessionExportQuerySanitize {
    #[serde(rename = "true")]
    True,
    #[serde(rename = "false")]
    False,
}

impl ExperimentalSessionExportQuerySanitize {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExperimentalSessionExportQuery {
    /// Wire param: `sanitize` (query).
    pub sanitize: Option<ExperimentalSessionExportQuerySanitize>,
}

/// Request body for `experimental.session.instructions.entry.put`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalSessionInstructionsEntryPutBody {
    /// Wire field: `value`.
    #[serde(rename = "value")]
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionLogQueryFollow {
    #[serde(rename = "true")]
    True,
    #[serde(rename = "false")]
    False,
}

impl SessionLogQueryFollow {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionLogQuery {
    /// Wire param: `after` (query).
    pub after: Option<String>,
    /// Wire param: `follow` (query).
    pub follow: Option<SessionLogQueryFollow>,
}

/// Request body for `experimental.session.skill`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExperimentalSessionSkillBody {
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
    /// Wire field: `resume`.
    #[serde(default, rename = "resume", skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerExperimentalPersistentPtyReadQuery {
    /// Wire param: `lines` (query).
    pub lines: Option<PersistentPtyReadLinesEncoded>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `form.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<FormInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FsFindQueryType {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "directory")]
    Directory,
}

impl FsFindQueryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsFindQuery {
    /// Wire param: `limit` (query).
    pub limit: Option<String>,
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `query` (query).
    pub query: String,
    /// Wire param: `type` (query).
    pub type_: Option<FsFindQueryType>,
}

/// Response envelope for `fs.find`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FsFindResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<FileSystemEntry>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `path` (query).
    pub path: Option<String>,
}

/// Response envelope for `fs.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FsListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<FileSystemEntry>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FsReadQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `integration.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<IntegrationInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `integration.get`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: IntegrationInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationCommandConnectQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `integration.command.connect`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandConnectBody {
    /// Wire field: `label`.
    #[serde(default, rename = "label", skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Wire field: `methodID`.
    #[serde(rename = "methodID")]
    pub method_id: String,
}

/// Response envelope for `integration.command.connect`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandConnectResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: IntegrationCommandAttempt,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationCommandCancelQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationCommandStatusQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `integration.command.status`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationCommandStatusResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: IntegrationCommandAttemptStatus,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationConnectKeyQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `integration.connect.key`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationConnectKeyBody {
    /// Wire field: `answer`.
    #[serde(default, rename = "answer", skip_serializing_if = "Option::is_none")]
    pub answer: Option<FormAnswer>,
    /// Wire field: `key`.
    #[serde(rename = "key")]
    pub key: String,
    /// Wire field: `label`.
    #[serde(default, rename = "label", skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationOauthConnectQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `integration.oauth.connect`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationOauthConnectBody {
    /// Wire field: `answer`.
    #[serde(default, rename = "answer", skip_serializing_if = "Option::is_none")]
    pub answer: Option<FormAnswer>,
    /// Wire field: `label`.
    #[serde(default, rename = "label", skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Wire field: `methodID`.
    #[serde(rename = "methodID")]
    pub method_id: String,
}

/// Response envelope for `integration.oauth.connect`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationOauthConnectResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: IntegrationAttemptEncoded,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationOauthCancelQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationOauthStatusQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `integration.oauth.status`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationOauthStatusResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: IntegrationAttemptStatus,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntegrationOauthCompleteQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `integration.oauth.complete`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IntegrationOauthCompleteBody {
    /// Wire field: `code`.
    #[serde(default, rename = "code", skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocationGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `mcp.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<McpServer>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpResourceCatalogQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `mcp.resource.catalog`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpResourceCatalogResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: McpResourceCatalog,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `model.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<ModelInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelDefaultQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `model.default`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelDefaultResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Option<ModelInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionRequestListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `permission.request.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PermissionRequestListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<PermissionRequest>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PermissionSavedListQuery {
    /// Wire param: `projectID` (query).
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `plugin.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<PluginInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginCheckQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `plugin.check`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginCheckBody {
    /// Wire field: `target`.
    #[serde(default, rename = "target", skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// Response envelope for `plugin.check`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginCheckResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<PluginInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginUpdateQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `plugin.update`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginUpdateBody {
    /// Wire field: `targets`.
    #[serde(rename = "targets")]
    pub targets: Vec<String>,
}

/// Request body for `project.update`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectUpdateBody {
    /// Wire field: `canonical`.
    #[serde(default, rename = "canonical", skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    /// Wire field: `commands`.
    #[serde(default, rename = "commands", skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
    /// Wire field: `icon`.
    #[serde(default, rename = "icon", skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    /// Wire field: `name`.
    #[serde(default, rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `provider.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<ProviderInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `provider.get`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: ProviderInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `pty.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<Pty>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyCreateQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `pty.create`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyCreateBody {
    /// Wire field: `args`.
    #[serde(default, rename = "args", skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Wire field: `command`.
    #[serde(default, rename = "command", skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Wire field: `cwd`.
    #[serde(default, rename = "cwd", skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Wire field: `env`.
    #[serde(default, rename = "env", skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Response envelope for `pty.create`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyCreateResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Pty,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyRemoveQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `pty.get`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Pty,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyUpdateQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyUpdateBodySize {
    /// Wire field: `cols`.
    #[serde(rename = "cols")]
    pub cols: i64,
    /// Wire field: `rows`.
    #[serde(rename = "rows")]
    pub rows: i64,
}

/// Request body for `pty.update`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyUpdateBody {
    /// Wire field: `size`.
    #[serde(default, rename = "size", skip_serializing_if = "Option::is_none")]
    pub size: Option<PtyUpdateBodySize>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Response envelope for `pty.update`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyUpdateResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Pty,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyConnectQuery {
    /// Wire param: `cursor` (query).
    pub cursor: Option<String>,
    /// Wire param: `location[directory]` (query).
    pub location_directory: Option<String>,
    /// Wire param: `ticket` (query).
    pub ticket: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PtyConnectTokenQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `x-opencode-ticket` (header).
    pub x_opencode_ticket: Option<String>,
}

/// Response envelope for `pty.connect.token`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PtyConnectTokenResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: PtyTicketConnectToken,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReferenceListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `reference.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReferenceListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<ReferenceInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RpcCallQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionListQueryOrder {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

impl SessionListQueryOrder {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionListQuery {
    /// Wire param: `cursor` (query).
    pub cursor: Option<String>,
    /// Wire param: `directory` (query).
    pub directory: Option<String>,
    /// Wire param: `limit` (query).
    pub limit: Option<String>,
    /// Wire param: `order` (query).
    pub order: Option<SessionListQueryOrder>,
    /// Wire param: `parentID` (query).
    pub parent_id: Option<String>,
    /// Wire param: `project` (query).
    pub project: Option<String>,
    /// Wire param: `search` (query).
    pub search: Option<String>,
    /// Wire param: `subpath` (query).
    pub subpath: Option<String>,
}

/// Request body for `session.create`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionCreateBody {
    /// Wire field: `agent`.
    #[serde(default, rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `location`.
    #[serde(default, rename = "location", skip_serializing_if = "Option::is_none")]
    pub location: Option<LocationPublicRef>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
    /// Wire field: `model`.
    #[serde(default, rename = "model", skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
    /// Wire field: `parentID`.
    #[serde(default, rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Wire field: `permissions`.
    #[serde(default, rename = "permissions", skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionRuleset>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Request body for `session.update`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionUpdateBody {
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
    /// Wire field: `permissions`.
    #[serde(default, rename = "permissions", skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionRuleset>,
    /// Wire field: `title`.
    #[serde(default, rename = "title", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Request body for `session.switchAgent`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionSwitchAgentBody {
    /// Wire field: `agent`.
    #[serde(rename = "agent")]
    pub agent: String,
}

/// Request body for `session.command`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionCommandBody {
    /// Wire field: `agents`.
    #[serde(default, rename = "agents", skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<PromptAgentAttachment>>,
    /// Wire field: `delivery`.
    #[serde(default, rename = "delivery", skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SessionInboxDelivery>,
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<PromptInputFileAttachment>>,
    /// Wire field: `name`.
    #[serde(rename = "name")]
    pub name: String,
    /// Wire field: `skills`.
    #[serde(default, rename = "skills", skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<PromptInputSkillAttachment>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Request body for `session.compact`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionCompactBody {
    /// Wire field: `delivery`.
    #[serde(default, rename = "delivery", skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SessionInboxDelivery>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionDiffQuery {
    /// Wire param: `context` (query).
    pub context: Option<String>,
    /// Wire param: `from` (query).
    pub from: Option<String>,
    /// Wire param: `to` (query).
    pub to: Option<String>,
}

/// Request body for `session.environment`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionEnvironmentBody {
    /// Wire field: `variables`.
    #[serde(rename = "variables")]
    pub variables: std::collections::BTreeMap<String, String>,
}

/// Request body for `session.fork`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionForkBody {
    /// Wire field: `before`.
    #[serde(default, rename = "before", skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionFormCancelQuery {
    /// Wire param: `message` (query).
    pub message: Option<String>,
}

/// Request body for `session.generate`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionGenerateBody {
    /// Wire field: `prompt`.
    #[serde(rename = "prompt")]
    pub prompt: String,
}

/// Request body for `session.inbox.update`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInboxUpdateBody {
    /// Wire field: `delivery`.
    #[serde(rename = "delivery")]
    pub delivery: SessionInboxDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionInterruptQueryResume {
    #[serde(rename = "true")]
    True,
    #[serde(rename = "false")]
    False,
}

impl SessionInterruptQueryResume {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionInterruptQuery {
    /// Wire param: `resume` (query).
    pub resume: Option<SessionInterruptQueryResume>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageListQueryOrder {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

impl SessionMessageListQueryOrder {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SessionMessageListQueryType {
    #[serde(rename = "agent-switched")]
    AgentSwitched,
    #[serde(rename = "model-switched")]
    ModelSwitched,
    #[serde(rename = "location-switched")]
    LocationSwitched,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "synthetic")]
    Synthetic,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "skill")]
    Skill,
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "compaction")]
    Compaction,
}

impl SessionMessageListQueryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AgentSwitched => "agent-switched",
            Self::ModelSwitched => "model-switched",
            Self::LocationSwitched => "location-switched",
            Self::User => "user",
            Self::Synthetic => "synthetic",
            Self::System => "system",
            Self::Skill => "skill",
            Self::Shell => "shell",
            Self::Assistant => "assistant",
            Self::Compaction => "compaction",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMessageListQuery {
    /// Wire param: `cursor` (query).
    pub cursor: Option<String>,
    /// Wire param: `limit` (query).
    pub limit: Option<String>,
    /// Wire param: `order` (query).
    pub order: Option<SessionMessageListQueryOrder>,
    /// Wire param: `type` (query).
    pub type_: Option<SessionMessageListQueryType>,
}

/// Request body for `session.switchModel`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionSwitchModelBody {
    /// Wire field: `model`.
    #[serde(rename = "model")]
    pub model: ModelRef,
}

/// Request body for `session.move`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionMoveBody {
    /// Wire field: `delivery`.
    #[serde(default, rename = "delivery", skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SessionInboxDelivery>,
    /// Wire field: `directory`.
    #[serde(rename = "directory")]
    pub directory: String,
}

/// Request body for `session.permission.create`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionPermissionCreateBody {
    /// Wire field: `action`.
    #[serde(rename = "action")]
    pub action: String,
    /// Wire field: `agent`.
    #[serde(default, rename = "agent", skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `resources`.
    #[serde(rename = "resources")]
    pub resources: Vec<String>,
    /// Wire field: `save`.
    #[serde(default, rename = "save", skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    /// Wire field: `source`.
    #[serde(default, rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<PermissionSource>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionPermissionCreateResponseData {
    /// Wire field: `effect`.
    #[serde(rename = "effect")]
    pub effect: PermissionEffect,
    /// Wire field: `id`.
    #[serde(rename = "id")]
    pub id: String,
}

/// Request body for `session.permission.reply`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionPermissionReplyBody {
    /// Wire field: `decision`.
    #[serde(rename = "decision")]
    pub decision: PermissionReply,
    /// Wire field: `message`.
    #[serde(default, rename = "message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Request body for `session.prompt`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionPromptBody {
    /// Wire field: `agents`.
    #[serde(default, rename = "agents", skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<PromptAgentAttachment>>,
    /// Wire field: `delivery`.
    #[serde(default, rename = "delivery", skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SessionInboxDelivery>,
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<PromptInputFileAttachment>>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `resume`.
    #[serde(default, rename = "resume", skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    /// Wire field: `skills`.
    #[serde(default, rename = "skills", skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<PromptInputSkillAttachment>>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Request body for `session.revert.stage`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionRevertStageBody {
    /// Wire field: `files`.
    #[serde(default, rename = "files", skip_serializing_if = "Option::is_none")]
    pub files: Option<bool>,
    /// Wire field: `messageID`.
    #[serde(rename = "messageID")]
    pub message_id: String,
}

/// Request body for `session.shell`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionShellBody {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Request body for `session.synthetic`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionSyntheticBody {
    /// Wire field: `delivery`.
    #[serde(default, rename = "delivery", skip_serializing_if = "Option::is_none")]
    pub delivery: Option<SessionInboxDelivery>,
    /// Wire field: `description`.
    #[serde(default, rename = "description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Wire field: `id`.
    #[serde(default, rename = "id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `resume`.
    #[serde(default, rename = "resume", skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    /// Wire field: `text`.
    #[serde(rename = "text")]
    pub text: String,
}

/// Request body for `session.view`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionViewBody {
    /// Wire field: `idle`.
    #[serde(rename = "idle")]
    pub idle: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `shell.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<ShellInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellCreateQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `shell.create`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellCreateBody {
    /// Wire field: `command`.
    #[serde(rename = "command")]
    pub command: String,
    /// Wire field: `cwd`.
    #[serde(default, rename = "cwd", skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Wire field: `metadata`.
    #[serde(default, rename = "metadata", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    /// Wire field: `timeout`.
    #[serde(default, rename = "timeout", skip_serializing_if = "Option::is_none")]
    pub timeout: Option<i64>,
}

/// Response envelope for `shell.create`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellCreateResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: ShellInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellRemoveQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `shell.get`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: ShellInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellOutputQuery {
    /// Wire param: `cursor` (query).
    pub cursor: Option<String>,
    /// Wire param: `limit` (query).
    pub limit: Option<String>,
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellOutputResponseData {
    /// Wire field: `cursor`.
    #[serde(rename = "cursor")]
    pub cursor: i64,
    /// Wire field: `output`.
    #[serde(rename = "output")]
    pub output: String,
    /// Wire field: `size`.
    #[serde(rename = "size")]
    pub size: i64,
    /// Wire field: `truncated`.
    #[serde(rename = "truncated")]
    pub truncated: bool,
}

/// Response envelope for `shell.output`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShellOutputResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: ShellOutputResponseData,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkillListQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `skill.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkillListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<SkillInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcsGetQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `vcs.get`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsGetResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: VcsInfo,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcsBaseQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `vcs.base`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsBaseResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Option<VcsBase>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcsBranchListQuery {
    /// Wire param: `limit` (query).
    pub limit: Option<String>,
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `search` (query).
    pub search: Option<String>,
}

/// Response envelope for `vcs.branch.list`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsBranchListResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: VcsBranchList,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VcsDiffQuery {
    /// Wire param: `base` (query).
    pub base: Option<String>,
    /// Wire param: `context` (query).
    pub context: Option<String>,
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `mode` (query).
    pub mode: VcsMode,
}

/// Response envelope for `vcs.diff`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsDiffResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<FileDiffInfo>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcsInitQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
    /// Wire param: `provider` (query).
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcsStatusQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `vcs.status`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VcsStatusResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<VcsFileStatus>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebsearchQueryQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Request body for `websearch.query`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebsearchQueryBody {
    /// Wire field: `providerID`.
    #[serde(default, rename = "providerID", skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Wire field: `query`.
    #[serde(rename = "query")]
    pub query: String,
}

/// Response envelope for `websearch.query`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebsearchQueryResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: WebSearchResponseEncoded,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebsearchProvidersQuery {
    /// Wire param: `location` (query).
    pub location: Option<QueryLocation>,
}

/// Response envelope for `websearch.providers`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebsearchProvidersResponse {
    /// Wire field: `data`.
    #[serde(rename = "data")]
    pub data: Vec<WebSearchProvider>,
    /// Wire field: `location`.
    #[serde(rename = "location")]
    pub location: LocationPublicRef,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorktreeListQuery {
    /// Wire param: `projectID` (query).
    pub project_id: String,
}

/// Request body for `worktree.refresh`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorktreeRefreshBody {
    /// Wire field: `projectID`.
    #[serde(rename = "projectID")]
    pub project_id: String,
}
