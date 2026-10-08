//! Port of `mcp.ts` (wire-relevant config + status shapes).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::PositiveInt;
use crate::id::IntegrationId;

/// `Mcp.TimeoutConfig`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutConfig {
    /// Maximum time in milliseconds to establish and initialize the server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<PositiveInt>,
    /// Maximum time in milliseconds for discovery requests (tools/list…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<PositiveInt>,
    /// Maximum time in milliseconds for tool and prompt execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<PositiveInt>,
}

/// `Mcp.Protocol` negotiation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Protocol {
    /// Initialize handshake, revisions up to 2025-11-25 (default).
    Legacy,
    /// Probe for 2026-07-28, fall back to legacy.
    Auto,
    /// Require the 2026-07-28 revision.
    #[serde(rename = "2026-07-28")]
    Dated,
}

/// `Mcp.LocalConfig` — the `local` variant of [`ServerConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LocalConfig {
    pub command: Vec<String>,
    /// Working directory; relative paths resolve from the workspace directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Expose this server's tools through Code Mode. Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codemode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
}

/// `Mcp.OAuthConfig`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OAuthConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_server_metadata_url: Option<String>,
}

/// `Mcp.RemoteConfig` — the `remote` variant of [`ServerConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RemoteConfig {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    /// Expose this server's tools through Code Mode. Defaults to true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codemode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Protocol>,
}

/// `oauth` value: config object or `false` to disable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum OAuthValue {
    Config(OAuthConfig),
    Disabled(bool),
}

/// `Mcp.ServerConfig` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerConfig {
    /// Local stdio server.
    Local(Box<LocalConfig>),
    /// Remote HTTP server.
    Remote(Box<RemoteConfig>),
}

/// `Mcp.Status` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// Connected and initialized.
    Connected,
    /// Starting up.
    Pending,
    /// Disabled by configuration.
    Disabled,
    /// Failed to connect.
    Failed { error: String },
    /// Needs (re)authentication.
    #[serde(rename = "needs_auth")]
    NeedsAuth { error: String },
}

/// `Mcp.Server`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Server {
    pub name: String,
    pub status: Status,
    /// Set for remote servers registered as OAuth integrations.
    #[serde(rename = "integrationID", default, skip_serializing_if = "Option::is_none")]
    pub integration_id: Option<IntegrationId>,
}

/// `Mcp.Resource`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Resource {
    pub server: String,
    pub name: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// `Mcp.ResourceTemplate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResourceTemplate {
    pub server: String,
    pub name: String,
    pub uri_template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// `Mcp.ResourceCatalog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResourceCatalog {
    pub resources: Vec<Resource>,
    pub templates: Vec<ResourceTemplate>,
}

/// `Mcp.ResourceContentPart` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ResourceContentPart {
    /// Text content.
    Text {
        uri: String,
        text: String,
        #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
    /// Base64 blob content.
    Blob {
        uri: String,
        blob: String,
        #[serde(rename = "mimeType", default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
    },
}

/// `Mcp.ResourceContent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResourceContent {
    pub server: String,
    pub uri: String,
    pub contents: Vec<ResourceContentPart>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_local_wire() {
        let value = serde_json::json!({
            "type": "local",
            "command": ["npx", "-y", "@modelcontextprotocol/server-everything"]
        });
        let parsed: ServerConfig = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, ServerConfig::Local(_)));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn status_wire() {
        let value = serde_json::json!({"status": "needs_auth", "error": "expired"});
        let parsed: Status = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        assert_eq!(
            serde_json::to_value(Status::Connected).unwrap(),
            serde_json::json!({"status": "connected"})
        );
    }
}
