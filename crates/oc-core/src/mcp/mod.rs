//! MCP lane — port of `packages/core/src/mcp/**` + `tool/mcp.ts`
//! (core-runner-map §5).
//!
//! - [`client`]: the rmcp-based connection (timeouts, transient retry,
//!   session-expiry detection, `toCallToolResult`).
//! - [`elicitation`]: JSON-Schema → form-field conversion, URL legacy
//!   mode, the `"global"` sentinel session, completion settlement.
//! - [`registry`]: the server registry (statuses, startup latch, per-server
//!   and per-URL serialization, watch callbacks, reconcile, OAuth
//!   integration identity, credential seam) and the tool-registry bridge.

pub mod client;
pub mod elicitation;
pub mod registry;
#[cfg(test)]
mod tests;

pub use client::{
    CallToolContent, CallToolResult, ConnectError, ConnectFailure, ConnectInput, NeedsAuthError,
    RequestError, SessionExpiredError, DEFAULT_CATALOG_TIMEOUT, DEFAULT_EXECUTION_TIMEOUT,
    DEFAULT_STARTUP_TIMEOUT, SESSION_EXPIRED_MESSAGE, TERMINATE_TIMEOUT,
};
pub use elicitation::{
    ClientElicitation, ElicitationAction, ElicitationCreate, ElicitationHandler, ElicitationParams,
    ElicitationResult, ElicitationValue, FormCancelled, FormDismissed, FormService,
    RegistryElicitation, GLOBAL_ELICITATION_SESSION_ID, URL_ELICITATION_FIELD_KEY,
};
pub use registry::{
    integration_suffix, merge_catalogs, namespace, tool_name, unavailable, Connector,
    CredentialStore, DatabaseCredentials, McpError, McpRegistry, McpToolBridge, McpToolExecutor,
    NoCredentials, NotFoundError, Prompt, ServerInfo, ServerInstructions, Tool, ToolCallError,
    ToolResult, PROMPTS_CHANGED, TOOL_REFRESH_DEBOUNCE,
};
