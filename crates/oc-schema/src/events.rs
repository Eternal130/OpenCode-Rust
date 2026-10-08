//! Ports of the small event modules: `server-event.ts`,
//! `session-status-event.ts`, `session-compaction-event.ts`,
//! `location-event.ts`, `mcp-event.ts`, `vcs-event.ts`, `worktree-event.ts`,
//! `workspace-event.ts`, `installation-event.ts`, `lsp-event.ts`, and the
//! `event-log.ts` synced marker. Each uses the shared envelope from
//! [`crate::event`]; the unions are internally tagged by `type`.

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;
use crate::event::EphemeralEnvelope;
use crate::id::{SessionId, WorkspaceId};

#[cfg(test)]
use crate::id::EventId;

/// `server.connected` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServerConnected {}

/// `global.disposed` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GlobalDisposed {}

/// `ServerEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ServerEvent {
    /// The server finished startup.
    #[serde(rename = "server.connected")]
    Connected(EphemeralEnvelope<ServerConnected>),
    /// The server is shutting down.
    #[serde(rename = "global.disposed")]
    Disposed(EphemeralEnvelope<GlobalDisposed>),
}

/// `SessionStatus.Info` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StatusInfo {
    /// Session has no active execution.
    Idle,
    /// Retry scheduled after a failure.
    Retry {
        attempt: NonNegativeInt,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<RetryAction>,
        /// Epoch milliseconds of the next attempt.
        next: NonNegativeInt,
    },
    /// Session is executing.
    Busy,
}

/// Retry remediation hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetryAction {
    pub reason: String,
    pub provider: String,
    pub title: String,
    pub message: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

/// `session.status` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub status: StatusInfo,
}

/// `session.idle` data (deprecated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionIdle {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `SessionStatusEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum SessionStatusEvent {
    /// Status transition.
    #[serde(rename = "session.status")]
    Status(EphemeralEnvelope<StatusUpdated>),
    /// Deprecated idle marker.
    #[serde(rename = "session.idle")]
    Idle(EphemeralEnvelope<SessionIdle>),
}

/// `session.compacted` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionCompacted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `SessionCompactionEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum SessionCompactionEvent {
    /// Compaction completed and was projected.
    #[serde(rename = "session.compacted")]
    Compacted(EphemeralEnvelope<SessionCompacted>),
}

/// `location.shutdown` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LocationShutdown {}

/// `LocationEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum LocationEvent {
    /// The location's cached services were shut down; revalidate reads.
    #[serde(rename = "location.shutdown")]
    Shutdown(EphemeralEnvelope<LocationShutdown>),
}

/// `mcp.tools.changed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpToolsChanged {
    pub server: String,
}

/// `mcp.resources.changed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpResourcesChanged {
    pub server: String,
}

/// `mcp.status.changed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct McpStatusChanged {
    pub server: String,
}

/// `McpEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum McpEvent {
    /// Tool list changed.
    #[serde(rename = "mcp.tools.changed")]
    ToolsChanged(EphemeralEnvelope<McpToolsChanged>),
    /// Resource list changed.
    #[serde(rename = "mcp.resources.changed")]
    ResourcesChanged(EphemeralEnvelope<McpResourcesChanged>),
    /// Connection status settled.
    #[serde(rename = "mcp.status.changed")]
    StatusChanged(EphemeralEnvelope<McpStatusChanged>),
}

/// `vcs.branch.updated` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsBranchUpdated {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// `VcsEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum VcsEvent {
    /// Branch changed.
    #[serde(rename = "vcs.branch.updated")]
    BranchUpdated(EphemeralEnvelope<VcsBranchUpdated>),
}

/// `worktree.ready` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeReady {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// `worktree.failed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeFailed {
    pub message: String,
}

/// `WorktreeEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WorktreeEvent {
    /// Worktree created and usable.
    #[serde(rename = "worktree.ready")]
    Ready(EphemeralEnvelope<WorktreeReady>),
    /// Worktree creation failed.
    #[serde(rename = "worktree.failed")]
    Failed(EphemeralEnvelope<WorktreeFailed>),
}

/// `workspace.ready` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceReady {
    pub name: String,
}

/// `workspace.failed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceFailed {
    pub message: String,
}

/// `workspace.status` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceStatusData {
    #[serde(rename = "workspaceID")]
    pub workspace_id: WorkspaceId,
    pub status: WorkspaceConnectionStatus,
}

/// `WorkspaceEvent.ConnectionStatus.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
    Error,
}

/// `WorkspaceEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WorkspaceEvent {
    /// Workspace is ready.
    #[serde(rename = "workspace.ready")]
    Ready(EphemeralEnvelope<WorkspaceReady>),
    /// Workspace failed.
    #[serde(rename = "workspace.failed")]
    Failed(EphemeralEnvelope<WorkspaceFailed>),
    /// Workspace connection status.
    #[serde(rename = "workspace.status")]
    Status(EphemeralEnvelope<WorkspaceStatusData>),
}

/// `installation.updated` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstallationUpdated {
    pub version: String,
}

/// `installation.update-available` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstallationUpdateAvailable {
    pub version: String,
}

/// `InstallationEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum InstallationEvent {
    /// Install finished updating.
    #[serde(rename = "installation.updated")]
    Updated(EphemeralEnvelope<InstallationUpdated>),
    /// An update is available.
    #[serde(rename = "installation.update-available")]
    UpdateAvailable(EphemeralEnvelope<InstallationUpdateAvailable>),
}

/// `lsp.updated` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LspUpdated {}

/// `LspEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum LspEvent {
    /// LSP registry changed.
    #[serde(rename = "lsp.updated")]
    Updated(EphemeralEnvelope<LspUpdated>),
}

/// Serde helper for the constant `"log.synced"` type field.
#[allow(clippy::trivially_copy_pass_by_ref)]
mod log_synced_type {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("log.synced")
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "log.synced" {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected literal \"log.synced\""))
        }
    }
}

/// `tui.prompt.append` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TuiPromptAppend {
    pub text: String,
}

/// `tui.command.execute` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TuiCommandExecute {
    /// A known command name or any plugin-registered string.
    pub command: String,
}

/// `tui.toast.show` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TuiToastShow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub message: String,
    pub variant: ToastVariant,
    /// Duration in milliseconds (default 5000).
    #[serde(default = "default_toast_duration")]
    pub duration: crate::common::PositiveInt,
}

/// Default toast duration.
fn default_toast_duration() -> crate::common::PositiveInt {
    5000
}

/// `tui.toast.show` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ToastVariant {
    Info,
    Success,
    Warning,
    Error,
}

/// `tui.session.select` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TuiSessionSelect {
    /// Session ID to navigate to.
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `TuiEvent` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum TuiEvent {
    /// Append text to the prompt buffer.
    #[serde(rename = "tui.prompt.append")]
    PromptAppend(EphemeralEnvelope<TuiPromptAppend>),
    /// Execute a TUI command.
    #[serde(rename = "tui.command.execute")]
    CommandExecute(EphemeralEnvelope<TuiCommandExecute>),
    /// Show a toast.
    #[serde(rename = "tui.toast.show")]
    ToastShow(EphemeralEnvelope<TuiToastShow>),
    /// Navigate to a session.
    #[serde(rename = "tui.session.select")]
    SessionSelect(EphemeralEnvelope<TuiSessionSelect>),
}

/// `EventLog.Synced` — replay-to-live boundary marker. Not an envelope event:
/// it is framed as `{type, aggregateID, seq?}` directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LogSynced {
    /// Always `"log.synced"`.
    #[serde(rename = "type", with = "log_synced_type")]
    #[schemars(with = "String")]
    pub log_type: (),
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope<D>(data: D) -> EphemeralEnvelope<D> {
        EphemeralEnvelope {
            id: EventId::parse("evt_9").unwrap(),
            created: 42,
            metadata: None,
            location: None,
            data,
        }
    }

    #[test]
    fn server_event_wire() {
        let event = ServerEvent::Connected(envelope(ServerConnected {}));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "evt_9",
                "created": 42,
                "type": "server.connected",
                "data": {}
            })
        );
        let back: ServerEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn status_event_wire() {
        let event = SessionStatusEvent::Status(envelope(StatusUpdated {
            session_id: SessionId::parse("ses_1").unwrap(),
            status: StatusInfo::Retry {
                attempt: 1,
                message: "overloaded".into(),
                action: None,
                next: 1_700_000_001_000,
            },
        }));
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "session.status");
        assert_eq!(json["data"]["sessionID"], "ses_1");
        assert_eq!(json["data"]["status"]["type"], "retry");
        assert_eq!(json["data"]["status"]["next"], 1_700_000_001_000u64);
        let back: SessionStatusEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn log_synced_wire() {
        let marker = LogSynced { log_type: (), aggregate_id: "ses_test".into(), seq: Some(1) };
        assert_eq!(
            serde_json::to_value(&marker).unwrap(),
            serde_json::json!({"type": "log.synced", "aggregateID": "ses_test", "seq": 1})
        );
        let marker = LogSynced { log_type: (), aggregate_id: "ses_test".into(), seq: None };
        assert_eq!(
            serde_json::to_value(&marker).unwrap(),
            serde_json::json!({"type": "log.synced", "aggregateID": "ses_test"})
        );
    }
}
