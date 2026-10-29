//! Consumer-side typed bus events (RE-06): the client mirror of the
//! server's event publicness manifest (`oc-server/src/event_manifest.rs`,
//! itself the port of v2 `packages/schema/src/event-manifest.ts`).
//!
//! The server EMIT side is typed and classified against the manifest
//! (G-12/RV-P7); this module is the missing consumer-side half. A
//! `/api/event` frame decodes into a [`BusEventFrame`] whose `type`
//! string is classified ONCE into the typed [`EventKind`] enum — one
//! variant per public wire event type (the discriminants mirror the
//! oc-schema event unions: `events.rs`, `session_event.rs`,
//! `misc_events.rs`, `persistent_pty.rs`, plus the manifest's
//! `SERVICE_FRAMES` such as `reference.updated`) — so drift between the
//! wire surface and this enum fails CI instead of silently probing
//! strings:
//!
//! * envelope-shape drift (missing/non-string `type`) is a LOUD decode
//!   error, never a silent `None`;
//! * unknown-but-well-formed types pass through as
//!   [`EventKind::Unknown`] (forward compatibility — the server may add
//!   types before this crate catches up);
//! * `rpc.*` guest frames collapse into [`EventKind::Rpc`] (the
//!   manifest's wildcard segment);
//! * the `oc-server` drift-guard test
//!   (`event_manifest.rs::manifest_client_event_kinds_match_public_surface`)
//!   pins `KNOWN_EVENT_TYPES` == `server.connected` ∪ registry types ∪
//!   service frames, so a type added or renamed on the server side
//!   fails that test until this enum is updated.
//!
//! Payload decoding stays schema-driven and loud: [`BusEventFrame::data`]
//! decodes `data` into any `DeserializeOwned` (e.g. the oc-schema
//! payload structs like `oc_schema::session_event::TextDelta`),
//! erroring on shape mismatch instead of probing `Value` strings.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::ClientError;
use crate::shared_events::SharedEvent;

/// Every public `/api/event` wire type (sorted). Single source for the
/// drift guard: mirrors `oc-server`'s manifest classification set.
pub const KNOWN_EVENT_TYPES: &[&str] = &[
    "agent.updated",
    "command.updated",
    "config.updated",
    "credential.switched",
    "credential.updated",
    "filesystem.changed",
    "form.cancelled",
    "form.created",
    "form.replied",
    "global.disposed",
    "installation.update-available",
    "installation.updated",
    "integration.updated",
    "location.shutdown",
    "log.synced",
    "lsp.updated",
    "mcp.prompts.changed",
    "mcp.resources.changed",
    "mcp.status.changed",
    "mcp.tools.changed",
    "message.part.delta",
    "message.part.removed",
    "message.part.updated",
    "message.removed",
    "message.updated",
    "model.updated",
    "models-dev.refreshed",
    "permission.asked",
    "permission.replied",
    "persistent-pty.added",
    "persistent-pty.removed",
    "plugin.updated",
    "project.updated",
    "provider.updated",
    "pty.created",
    "pty.deleted",
    "pty.exited",
    "pty.updated",
    "reference.updated",
    "server.connected",
    "session.agent.selected",
    "session.compacted",
    "session.compaction.delta",
    "session.compaction.ended",
    "session.compaction.failed",
    "session.compaction.started",
    "session.created",
    "session.deleted",
    "session.diff",
    "session.error",
    "session.execution.failed",
    "session.execution.interrupted",
    "session.execution.started",
    "session.execution.succeeded",
    "session.forked",
    "session.idle",
    "session.inbox.cancelled",
    "session.inbox.delivered",
    "session.inbox.delivery.changed",
    "session.inbox.enqueued",
    "session.instructions.updated",
    "session.message.content.updated",
    "session.metadata.updated",
    "session.model.selected",
    "session.moved",
    "session.permissions",
    "session.reasoning.delta",
    "session.reasoning.ended",
    "session.reasoning.started",
    "session.renamed",
    "session.retry.scheduled",
    "session.revert.cleared",
    "session.revert.committed",
    "session.revert.staged",
    "session.shell.ended",
    "session.shell.started",
    "session.skill.activated",
    "session.status",
    "session.step.ended",
    "session.step.failed",
    "session.step.started",
    "session.step.streamed",
    "session.synthetic",
    "session.text.delta",
    "session.text.ended",
    "session.text.started",
    "session.tool.called",
    "session.tool.failed",
    "session.tool.input.delta",
    "session.tool.input.ended",
    "session.tool.input.started",
    "session.tool.progress",
    "session.tool.success",
    "session.updated",
    "session.usage.recorded",
    "session.usage.updated",
    "session.viewed",
    "shell.created",
    "shell.deleted",
    "shell.exited",
    "skill.updated",
    "tui.command.execute",
    "tui.prompt.append",
    "tui.session.select",
    "tui.toast.show",
    "vcs.branch.updated",
    "websearch.updated",
    "workspace.failed",
    "workspace.ready",
    "workspace.status",
    "worktree.failed",
    "worktree.ready",
    "worktree.resolved",
    "worktree.updated",
];

/// The typed classification of one bus event's `type` discriminant —
/// the consumer-side mirror of the server's event manifest. Dispatch on
/// the variant instead of probing strings; [`EventKind::Unknown`] keeps
/// unknown-but-well-formed types flowing (forward compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    ServerConnected,
    GlobalDisposed,
    SessionStatus,
    SessionIdle,
    SessionCompacted,
    LocationShutdown,
    McpToolsChanged,
    McpPromptsChanged,
    McpResourcesChanged,
    McpStatusChanged,
    VcsBranchUpdated,
    WorktreeReady,
    WorktreeFailed,
    WorktreeUpdated,
    WorktreeResolved,
    WorkspaceReady,
    WorkspaceFailed,
    WorkspaceStatus,
    InstallationUpdated,
    InstallationUpdateAvailable,
    LspUpdated,
    AgentUpdated,
    ModelUpdated,
    ProviderUpdated,
    SkillUpdated,
    CommandUpdated,
    ConfigUpdated,
    IntegrationUpdated,
    CredentialUpdated,
    CredentialSwitched,
    WebsearchUpdated,
    ModelsDevRefreshed,
    PluginUpdated,
    ProjectUpdated,
    ReferenceUpdated,
    PermissionAsked,
    PermissionReplied,
    ShellCreated,
    ShellExited,
    ShellDeleted,
    FormCreated,
    FormReplied,
    FormCancelled,
    FilesystemChanged,
    PtyCreated,
    PtyUpdated,
    PtyExited,
    PtyDeleted,
    PersistentPtyAdded,
    PersistentPtyRemoved,
    LogSynced,
    SessionCreated,
    SessionUpdated,
    SessionDiff,
    SessionError,
    MessageUpdated,
    MessageRemoved,
    MessagePartUpdated,
    MessagePartRemoved,
    MessagePartDelta,
    SessionAgentSelected,
    SessionModelSelected,
    SessionMoved,
    SessionRenamed,
    SessionMetadataUpdated,
    SessionPermissions,
    SessionViewed,
    SessionUsageUpdated,
    SessionUsageRecorded,
    SessionDeleted,
    SessionForked,
    SessionInboxDelivered,
    SessionInboxEnqueued,
    SessionInboxCancelled,
    SessionInboxDeliveryChanged,
    SessionExecutionStarted,
    SessionExecutionSucceeded,
    SessionExecutionFailed,
    SessionExecutionInterrupted,
    SessionInstructionsUpdated,
    SessionSynthetic,
    SessionSkillActivated,
    SessionShellStarted,
    SessionShellEnded,
    SessionStepStarted,
    SessionStepStreamed,
    SessionStepEnded,
    SessionStepFailed,
    SessionTextStarted,
    SessionTextDelta,
    SessionTextEnded,
    SessionReasoningStarted,
    SessionReasoningDelta,
    SessionReasoningEnded,
    SessionToolInputStarted,
    SessionToolInputDelta,
    SessionToolInputEnded,
    SessionToolCalled,
    SessionToolProgress,
    SessionToolSuccess,
    SessionToolFailed,
    SessionRetryScheduled,
    SessionCompactionStarted,
    SessionCompactionDelta,
    SessionCompactionEnded,
    SessionCompactionFailed,
    SessionRevertStaged,
    SessionRevertCleared,
    SessionRevertCommitted,
    SessionMessageContentUpdated,
    TuiPromptAppend,
    TuiCommandExecute,
    TuiToastShow,
    TuiSessionSelect,
    /// A `rpc.<id>.<method>` guest frame (the manifest's wildcard
    /// segment — the concrete method lives in the raw `type` string).
    Rpc,
    /// A well-formed type this client does not know (server newer than
    /// client; pass through untouched).
    Unknown,
}

impl EventKind {
    /// Classifies a wire `type` string. Known strings map to their
    /// variant, `rpc.`-prefixed strings to [`EventKind::Rpc`], anything
    /// else to [`EventKind::Unknown`].
    #[must_use]
    pub fn parse(type_: &str) -> Self {
        if let Some(kind) = Self::from_known(type_) {
            return kind;
        }
        if type_.len() > 4 && type_.starts_with("rpc.") {
            return Self::Rpc;
        }
        Self::Unknown
    }

    /// The classification table — exactly [`KNOWN_EVENT_TYPES`], one
    /// arm each (the drift guard pins the two stay in lockstep).
    // One flat arm per wire type: mirrors the manifest inventory.
    #[allow(clippy::too_many_lines)]
    fn from_known(type_: &str) -> Option<Self> {
        Some(match type_ {
            "agent.updated" => Self::AgentUpdated,
            "command.updated" => Self::CommandUpdated,
            "config.updated" => Self::ConfigUpdated,
            "credential.switched" => Self::CredentialSwitched,
            "credential.updated" => Self::CredentialUpdated,
            "form.cancelled" => Self::FormCancelled,
            "form.created" => Self::FormCreated,
            "form.replied" => Self::FormReplied,
            "filesystem.changed" => Self::FilesystemChanged,
            "global.disposed" => Self::GlobalDisposed,
            "installation.update-available" => Self::InstallationUpdateAvailable,
            "installation.updated" => Self::InstallationUpdated,
            "integration.updated" => Self::IntegrationUpdated,
            "location.shutdown" => Self::LocationShutdown,
            "log.synced" => Self::LogSynced,
            "lsp.updated" => Self::LspUpdated,
            "mcp.prompts.changed" => Self::McpPromptsChanged,
            "mcp.resources.changed" => Self::McpResourcesChanged,
            "mcp.status.changed" => Self::McpStatusChanged,
            "mcp.tools.changed" => Self::McpToolsChanged,
            "message.part.delta" => Self::MessagePartDelta,
            "message.part.removed" => Self::MessagePartRemoved,
            "message.part.updated" => Self::MessagePartUpdated,
            "message.removed" => Self::MessageRemoved,
            "message.updated" => Self::MessageUpdated,
            "model.updated" => Self::ModelUpdated,
            "models-dev.refreshed" => Self::ModelsDevRefreshed,
            "permission.asked" => Self::PermissionAsked,
            "permission.replied" => Self::PermissionReplied,
            "persistent-pty.added" => Self::PersistentPtyAdded,
            "persistent-pty.removed" => Self::PersistentPtyRemoved,
            "plugin.updated" => Self::PluginUpdated,
            "project.updated" => Self::ProjectUpdated,
            "provider.updated" => Self::ProviderUpdated,
            "pty.created" => Self::PtyCreated,
            "pty.deleted" => Self::PtyDeleted,
            "pty.exited" => Self::PtyExited,
            "pty.updated" => Self::PtyUpdated,
            "reference.updated" => Self::ReferenceUpdated,
            "server.connected" => Self::ServerConnected,
            "session.agent.selected" => Self::SessionAgentSelected,
            "session.compacted" => Self::SessionCompacted,
            "session.compaction.delta" => Self::SessionCompactionDelta,
            "session.compaction.ended" => Self::SessionCompactionEnded,
            "session.compaction.failed" => Self::SessionCompactionFailed,
            "session.compaction.started" => Self::SessionCompactionStarted,
            "session.created" => Self::SessionCreated,
            "session.deleted" => Self::SessionDeleted,
            "session.diff" => Self::SessionDiff,
            "session.error" => Self::SessionError,
            "session.execution.failed" => Self::SessionExecutionFailed,
            "session.execution.interrupted" => Self::SessionExecutionInterrupted,
            "session.execution.started" => Self::SessionExecutionStarted,
            "session.execution.succeeded" => Self::SessionExecutionSucceeded,
            "session.forked" => Self::SessionForked,
            "session.idle" => Self::SessionIdle,
            "session.inbox.cancelled" => Self::SessionInboxCancelled,
            "session.inbox.delivery.changed" => Self::SessionInboxDeliveryChanged,
            "session.inbox.delivered" => Self::SessionInboxDelivered,
            "session.inbox.enqueued" => Self::SessionInboxEnqueued,
            "session.instructions.updated" => Self::SessionInstructionsUpdated,
            "session.message.content.updated" => Self::SessionMessageContentUpdated,
            "session.metadata.updated" => Self::SessionMetadataUpdated,
            "session.model.selected" => Self::SessionModelSelected,
            "session.moved" => Self::SessionMoved,
            "session.permissions" => Self::SessionPermissions,
            "session.reasoning.delta" => Self::SessionReasoningDelta,
            "session.reasoning.ended" => Self::SessionReasoningEnded,
            "session.reasoning.started" => Self::SessionReasoningStarted,
            "session.renamed" => Self::SessionRenamed,
            "session.retry.scheduled" => Self::SessionRetryScheduled,
            "session.revert.cleared" => Self::SessionRevertCleared,
            "session.revert.committed" => Self::SessionRevertCommitted,
            "session.revert.staged" => Self::SessionRevertStaged,
            "session.shell.ended" => Self::SessionShellEnded,
            "session.shell.started" => Self::SessionShellStarted,
            "session.skill.activated" => Self::SessionSkillActivated,
            "session.status" => Self::SessionStatus,
            "session.step.ended" => Self::SessionStepEnded,
            "session.step.failed" => Self::SessionStepFailed,
            "session.step.started" => Self::SessionStepStarted,
            "session.step.streamed" => Self::SessionStepStreamed,
            "session.synthetic" => Self::SessionSynthetic,
            "session.text.delta" => Self::SessionTextDelta,
            "session.text.ended" => Self::SessionTextEnded,
            "session.text.started" => Self::SessionTextStarted,
            "session.tool.called" => Self::SessionToolCalled,
            "session.tool.failed" => Self::SessionToolFailed,
            "session.tool.input.delta" => Self::SessionToolInputDelta,
            "session.tool.input.ended" => Self::SessionToolInputEnded,
            "session.tool.input.started" => Self::SessionToolInputStarted,
            "session.tool.progress" => Self::SessionToolProgress,
            "session.tool.success" => Self::SessionToolSuccess,
            "session.usage.recorded" => Self::SessionUsageRecorded,
            "session.usage.updated" => Self::SessionUsageUpdated,
            "session.updated" => Self::SessionUpdated,
            "session.viewed" => Self::SessionViewed,
            "shell.created" => Self::ShellCreated,
            "shell.deleted" => Self::ShellDeleted,
            "shell.exited" => Self::ShellExited,
            "skill.updated" => Self::SkillUpdated,
            "tui.command.execute" => Self::TuiCommandExecute,
            "tui.prompt.append" => Self::TuiPromptAppend,
            "tui.session.select" => Self::TuiSessionSelect,
            "tui.toast.show" => Self::TuiToastShow,
            "vcs.branch.updated" => Self::VcsBranchUpdated,
            "websearch.updated" => Self::WebsearchUpdated,
            "worktree.failed" => Self::WorktreeFailed,
            "worktree.ready" => Self::WorktreeReady,
            "worktree.resolved" => Self::WorktreeResolved,
            "worktree.updated" => Self::WorktreeUpdated,
            "workspace.failed" => Self::WorkspaceFailed,
            "workspace.ready" => Self::WorkspaceReady,
            "workspace.status" => Self::WorkspaceStatus,
            _ => return None,
        })
    }

    /// The canonical wire string for a KNOWN kind (`None` for
    /// [`EventKind::Rpc`] and [`EventKind::Unknown`] — those carry the
    /// raw type string alongside).
    // One flat arm per wire type: mirrors the manifest inventory.
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn as_str(&self) -> Option<&'static str> {
        Some(match self {
            Self::AgentUpdated => "agent.updated",
            Self::ModelUpdated => "model.updated",
            Self::CommandUpdated => "command.updated",
            Self::ConfigUpdated => "config.updated",
            Self::CredentialSwitched => "credential.switched",
            Self::CredentialUpdated => "credential.updated",
            Self::FormCancelled => "form.cancelled",
            Self::FormCreated => "form.created",
            Self::FormReplied => "form.replied",
            Self::FilesystemChanged => "filesystem.changed",
            Self::GlobalDisposed => "global.disposed",
            Self::InstallationUpdateAvailable => "installation.update-available",
            Self::InstallationUpdated => "installation.updated",
            Self::IntegrationUpdated => "integration.updated",
            Self::LocationShutdown => "location.shutdown",
            Self::LogSynced => "log.synced",
            Self::LspUpdated => "lsp.updated",
            Self::McpPromptsChanged => "mcp.prompts.changed",
            Self::McpResourcesChanged => "mcp.resources.changed",
            Self::McpStatusChanged => "mcp.status.changed",
            Self::McpToolsChanged => "mcp.tools.changed",
            Self::MessagePartDelta => "message.part.delta",
            Self::MessagePartRemoved => "message.part.removed",
            Self::MessagePartUpdated => "message.part.updated",
            Self::MessageRemoved => "message.removed",
            Self::MessageUpdated => "message.updated",
            Self::ModelsDevRefreshed => "models-dev.refreshed",
            Self::PermissionAsked => "permission.asked",
            Self::PermissionReplied => "permission.replied",
            Self::PersistentPtyAdded => "persistent-pty.added",
            Self::PersistentPtyRemoved => "persistent-pty.removed",
            Self::PluginUpdated => "plugin.updated",
            Self::ProjectUpdated => "project.updated",
            Self::ProviderUpdated => "provider.updated",
            Self::PtyCreated => "pty.created",
            Self::PtyDeleted => "pty.deleted",
            Self::PtyExited => "pty.exited",
            Self::PtyUpdated => "pty.updated",
            Self::ReferenceUpdated => "reference.updated",
            Self::ServerConnected => "server.connected",
            Self::SessionAgentSelected => "session.agent.selected",
            Self::SessionCompacted => "session.compacted",
            Self::SessionCompactionDelta => "session.compaction.delta",
            Self::SessionCompactionEnded => "session.compaction.ended",
            Self::SessionCompactionFailed => "session.compaction.failed",
            Self::SessionCompactionStarted => "session.compaction.started",
            Self::SessionCreated => "session.created",
            Self::SessionDeleted => "session.deleted",
            Self::SessionDiff => "session.diff",
            Self::SessionError => "session.error",
            Self::SessionExecutionFailed => "session.execution.failed",
            Self::SessionExecutionInterrupted => "session.execution.interrupted",
            Self::SessionExecutionStarted => "session.execution.started",
            Self::SessionExecutionSucceeded => "session.execution.succeeded",
            Self::SessionForked => "session.forked",
            Self::SessionIdle => "session.idle",
            Self::SessionInboxCancelled => "session.inbox.cancelled",
            Self::SessionInboxDeliveryChanged => "session.inbox.delivery.changed",
            Self::SessionInboxDelivered => "session.inbox.delivered",
            Self::SessionInboxEnqueued => "session.inbox.enqueued",
            Self::SessionInstructionsUpdated => "session.instructions.updated",
            Self::SessionMessageContentUpdated => "session.message.content.updated",
            Self::SessionMetadataUpdated => "session.metadata.updated",
            Self::SessionModelSelected => "session.model.selected",
            Self::SessionMoved => "session.moved",
            Self::SessionPermissions => "session.permissions",
            Self::SessionReasoningDelta => "session.reasoning.delta",
            Self::SessionReasoningEnded => "session.reasoning.ended",
            Self::SessionReasoningStarted => "session.reasoning.started",
            Self::SessionRenamed => "session.renamed",
            Self::SessionRetryScheduled => "session.retry.scheduled",
            Self::SessionRevertCleared => "session.revert.cleared",
            Self::SessionRevertCommitted => "session.revert.committed",
            Self::SessionRevertStaged => "session.revert.staged",
            Self::SessionShellEnded => "session.shell.ended",
            Self::SessionShellStarted => "session.shell.started",
            Self::SessionSkillActivated => "session.skill.activated",
            Self::SessionStatus => "session.status",
            Self::SessionStepEnded => "session.step.ended",
            Self::SessionStepFailed => "session.step.failed",
            Self::SessionStepStarted => "session.step.started",
            Self::SessionStepStreamed => "session.step.streamed",
            Self::SessionSynthetic => "session.synthetic",
            Self::SessionTextDelta => "session.text.delta",
            Self::SessionTextEnded => "session.text.ended",
            Self::SessionTextStarted => "session.text.started",
            Self::SessionToolCalled => "session.tool.called",
            Self::SessionToolFailed => "session.tool.failed",
            Self::SessionToolInputDelta => "session.tool.input.delta",
            Self::SessionToolInputEnded => "session.tool.input.ended",
            Self::SessionToolInputStarted => "session.tool.input.started",
            Self::SessionToolProgress => "session.tool.progress",
            Self::SessionToolSuccess => "session.tool.success",
            Self::SessionUsageRecorded => "session.usage.recorded",
            Self::SessionUsageUpdated => "session.usage.updated",
            Self::SessionUpdated => "session.updated",
            Self::SessionViewed => "session.viewed",
            Self::ShellCreated => "shell.created",
            Self::ShellDeleted => "shell.deleted",
            Self::ShellExited => "shell.exited",
            Self::SkillUpdated => "skill.updated",
            Self::TuiCommandExecute => "tui.command.execute",
            Self::TuiPromptAppend => "tui.prompt.append",
            Self::TuiSessionSelect => "tui.session.select",
            Self::TuiToastShow => "tui.toast.show",
            Self::VcsBranchUpdated => "vcs.branch.updated",
            Self::WebsearchUpdated => "websearch.updated",
            Self::WorktreeFailed => "worktree.failed",
            Self::WorktreeReady => "worktree.ready",
            Self::WorktreeResolved => "worktree.resolved",
            Self::WorktreeUpdated => "worktree.updated",
            Self::WorkspaceFailed => "workspace.failed",
            Self::WorkspaceReady => "workspace.ready",
            Self::WorkspaceStatus => "workspace.status",
            Self::Rpc | Self::Unknown => return None,
        })
    }
}

/// One decoded `/api/event` frame: the `{ id, created, type, data }`
/// bus envelope with the `type` discriminant classified into
/// [`EventKind`]. Envelope-shape drift (missing/non-string `type`) is a
/// LOUD [`ClientError::MalformedResponse`]; unknown-but-well-formed
/// types pass through as [`EventKind::Unknown`].
#[derive(Debug, Clone, PartialEq)]
pub struct BusEventFrame {
    /// Envelope `id` (the replay-dedup cursor); `None` when absent
    /// (fixture frames).
    pub id: Option<String>,
    /// Envelope `created` (epoch ms).
    pub created: Option<u64>,
    /// The raw wire `type` string (needed for `Rpc`/`Unknown` kinds and
    /// the store's reducer compat).
    pub type_: String,
    /// The classified discriminant — dispatch on this.
    pub kind: EventKind,
    /// The raw `data` payload; decode through [`BusEventFrame::data`].
    pub data: Value,
}

impl BusEventFrame {
    /// Decodes a double-decoded SSE JSON payload, ERRING loudly on
    /// envelope-shape drift: `type` must exist and be a string (the
    /// one field every consumer dispatches on).
    pub fn decode(value: &Value) -> Result<Self, ClientError> {
        let object = value.as_object().ok_or_else(|| {
            ClientError::MalformedResponse("bus event frame must be a JSON object".to_owned())
        })?;
        let type_ = object.get("type").and_then(Value::as_str).ok_or_else(|| {
            ClientError::MalformedResponse(
                "bus event frame missing string `type` — wire drift".to_owned(),
            )
        })?;
        Ok(Self {
            id: object.get("id").and_then(Value::as_str).map(str::to_owned),
            created: object.get("created").and_then(Value::as_u64),
            kind: EventKind::parse(type_),
            type_: type_.to_owned(),
            data: object.get("data").cloned().unwrap_or(Value::Null),
        })
    }

    /// Whether this is the `server.connected` replay marker.
    #[must_use]
    pub fn is_server_connected(&self) -> bool {
        self.kind == EventKind::ServerConnected
    }

    /// Loud typed payload decode — the ref's `RpcRuntime.read` /
    /// schema-driven `data` decode: decodes `data` into `D` (e.g. an
    /// `oc_schema` payload struct), surfacing shape drift as
    /// [`ClientError::MalformedResponse`] instead of silent `None`s.
    pub fn data<D: DeserializeOwned>(&self) -> Result<D, ClientError> {
        serde_json::from_value(self.data.clone()).map_err(|error| {
            ClientError::MalformedResponse(format!(
                "event `{}` payload decode failed: {error}",
                self.type_
            ))
        })
    }
}

impl SharedEvent for BusEventFrame {
    fn is_server_connected(&self) -> bool {
        BusEventFrame::is_server_connected(self)
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    /// The classification table, `as_str`, and `KNOWN_EVENT_TYPES` are
    /// three hand-written views of one set — pin their lockstep so an
    /// edit to any one fails here.
    #[test]
    fn classification_table_matches_known_event_types() {
        let mut offenders = Vec::new();
        for pair in KNOWN_EVENT_TYPES.windows(2) {
            if pair[0] >= pair[1] {
                offenders.push(format!("{:?} >= {:?}", pair[0], pair[1]));
            }
        }
        assert!(offenders.is_empty(), "KNOWN_EVENT_TYPES must stay sorted: {offenders:?}");
        for type_ in KNOWN_EVENT_TYPES {
            let kind = EventKind::parse(type_);
            assert_ne!(kind, EventKind::Unknown, "{type_} must classify");
            assert_ne!(kind, EventKind::Rpc, "{type_} is not an rpc frame");
            assert_eq!(kind.as_str(), Some(*type_), "{type_} must round-trip");
        }
        // `as_str` has no arm absent from the table: every known kind
        // round-trips, and parse is total, so the table covers as_str.
        assert_eq!(EventKind::parse("no.such.type"), EventKind::Unknown);
        assert_eq!(EventKind::Unknown.as_str(), None);
        assert_eq!(EventKind::Rpc.as_str(), None);
    }

    /// rpc.* guest frames collapse into the wildcard kind.
    #[test]
    fn rpc_frames_collapse_into_the_wildcard_kind() {
        assert_eq!(EventKind::parse("rpc.custom"), EventKind::Rpc);
        assert_eq!(EventKind::parse("rpc.guest.thing"), EventKind::Rpc);
        // The bare `rpc` prefix-without-suffix is NOT an rpc frame (the
        // manifest classifies `rpc.` + non-empty remainder only).
        assert_eq!(EventKind::parse("rpc."), EventKind::Unknown);
        assert_eq!(EventKind::parse("rpc"), EventKind::Unknown);
    }

    /// Envelope decode: typed kind, tolerant id/created, LOUD errors on
    /// shape drift (RE-06: wire drift surfaces as errors, not None).
    #[test]
    fn frame_decode_classifies_and_errors_loudly() {
        let frame =
            BusEventFrame::decode(&json!({"id": "evt_1", "created": 7, "type": "session.text.delta", "data": {"sessionID": "ses_1"}}))
                .expect("decodes");
        assert_eq!(frame.kind, EventKind::SessionTextDelta);
        assert_eq!(frame.id.as_deref(), Some("evt_1"));
        assert_eq!(frame.created, Some(7));
        assert!(!frame.is_server_connected());

        let connected =
            BusEventFrame::decode(&json!({"type": "server.connected"})).expect("decodes");
        assert_eq!(connected.kind, EventKind::ServerConnected);
        assert!(connected.is_server_connected());
        assert_eq!(connected.id, None);

        // Shape drift is loud.
        assert!(BusEventFrame::decode(&json!({"created": 0})).is_err(), "missing type must error");
        assert!(BusEventFrame::decode(&json!({"type": 42})).is_err(), "non-string type must error");
        assert!(BusEventFrame::decode(&json!("scalar")).is_err(), "non-object frame must error");

        // Unknown-but-well-formed passes through.
        let unknown = BusEventFrame::decode(&json!({"type": "future.thing", "data": {}}))
            .expect("forward-compatible");
        assert_eq!(unknown.kind, EventKind::Unknown);
    }

    /// Typed payload decode through the oc-schema structs errors loudly
    /// on mismatch (schema drift surfaces, never silent None).
    #[test]
    fn typed_payload_decode_is_schema_driven() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Delta {
            #[serde(rename = "sessionID")]
            session_id: String,
        }
        let frame = BusEventFrame::decode(&json!({
            "type": "session.text.delta",
            "data": {"sessionID": "ses_1"}
        }))
        .expect("decodes");
        let delta: Delta = frame.data().expect("payload decodes");
        assert_eq!(delta.session_id, "ses_1");

        // Wrong shape → error naming the event type.
        let bad = BusEventFrame::decode(&json!({
            "type": "session.text.delta",
            "data": {"unexpected": true}
        }))
        .expect("decodes");
        let error = bad.data::<Delta>().expect_err("shape drift must error");
        assert!(error.to_string().contains("session.text.delta"), "{error}");
    }

    /// The oc-schema payload structs decode straight off the frame
    /// (single schema source; consumer mirrors nothing).
    #[test]
    fn oc_schema_payloads_decode_off_the_frame() {
        let frame = BusEventFrame::decode(&json!({
            "id": "evt_2",
            "created": 9,
            "type": "session.renamed",
            "data": {"sessionID": "ses_1", "title": "New title"}
        }))
        .expect("decodes");
        assert_eq!(frame.kind, EventKind::SessionRenamed);
        let payload: oc_schema::session_event::Renamed =
            frame.data().expect("schema payload decodes");
        assert_eq!(payload.title, "New title");
    }
}
