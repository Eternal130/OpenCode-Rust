//! `config/mcp.ts` — `Config.MCP` (re-export of the Mcp wire shapes).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `Mcp.TimeoutConfig`.
pub use crate::mcp::TimeoutConfig;

/// `Mcp.LocalConfig`.
pub use crate::mcp::LocalConfig;

/// `Mcp.RemoteConfig`.
pub use crate::mcp::RemoteConfig;

/// `Mcp.ServerConfig`.
pub use crate::mcp::ServerConfig;

/// `Mcp.Protocol`.
pub use crate::mcp::Protocol;

/// `Config.MCP`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mcp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub servers: Option<BTreeMap<String, ServerConfig>>,
}
