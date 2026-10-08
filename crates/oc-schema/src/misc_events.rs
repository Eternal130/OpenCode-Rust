//! Remaining domain events ported from their per-file modules:
//! `agent.ts`, `model.ts`, `provider.ts`, `skill.ts`, `command.ts`,
//! `config.ts`, `integration.ts`, `credential.ts`, `permission.ts`,
//! `shell.ts`, `plugin.ts`, `project.ts`, `worktree.ts`, `websearch.ts`,
//! `models-dev.ts`. All share the envelope from [`crate::event`].

use serde::{Deserialize, Serialize};

use crate::common::{NonNegativeInt, Record};
use crate::event::{DurableEnvelope, DurableInfo, EphemeralEnvelope};
use crate::form;
use crate::id::{
    CredentialId, EventId, IntegrationId, PermissionId, ProjectId, SessionId, ShellId,
};
use crate::permission::Reply;
use crate::shell;

/// Data for events whose payload is empty (`{}`).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Empty {}

/// `agent.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// Agent registry changed.
    #[serde(rename = "agent.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `model.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ModelEvent {
    /// Model catalog changed.
    #[serde(rename = "model.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `provider.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ProviderEvent {
    /// Provider registry changed.
    #[serde(rename = "provider.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `skill.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum SkillEvent {
    /// Skill registry changed.
    #[serde(rename = "skill.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `command.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum CommandEvent {
    /// Command registry changed.
    #[serde(rename = "command.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `config.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ConfigEvent {
    /// Config documents changed.
    #[serde(rename = "config.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `integration.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum IntegrationEvent {
    /// Integration registry changed.
    #[serde(rename = "integration.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `credential.updated` / `credential.switched` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum CredentialEvent {
    /// Credential store changed.
    #[serde(rename = "credential.updated")]
    Updated(EphemeralEnvelope<Empty>),
    /// Active credential changed; `credentialID` is null when deselected.
    #[serde(rename = "credential.switched")]
    Switched(EphemeralEnvelope<CredentialSwitched>),
}

/// `credential.switched` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CredentialSwitched {
    #[serde(rename = "integrationID")]
    pub integration_id: IntegrationId,
    #[serde(rename = "credentialID")]
    pub credential_id: Option<CredentialId>,
}

/// `websearch.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WebSearchEvent {
    /// Provider status changed.
    #[serde(rename = "websearch.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `models-dev.refreshed` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ModelsDevEvent {
    /// Catalog refreshed.
    #[serde(rename = "models-dev.refreshed")]
    Refreshed(EphemeralEnvelope<Empty>),
}

/// `plugin.updated` (ephemeral, empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum PluginEvent {
    /// Plugin registry changed.
    #[serde(rename = "plugin.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

/// `project.updated` data (the full `Project` fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectUpdated {
    pub id: ProjectId,
    pub canonical: crate::common::AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs: Option<crate::location::ProjectVcs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<crate::location::ProjectIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<crate::location::ProjectCommands>,
    pub time: crate::location::ProjectTime,
    pub sandboxes: Vec<String>,
}

/// `project.updated` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ProjectEvent {
    /// Project registry changed.
    #[serde(rename = "project.updated")]
    Updated(EphemeralEnvelope<ProjectUpdated>),
}

/// `permission.asked` data — `Permission.Request` fields.
pub use crate::permission::Request as PermissionAsked;

/// `permission.replied` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionReplied {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "requestID")]
    pub request_id: PermissionId,
    pub reply: Reply,
}

/// `permission.asked` / `permission.replied` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum PermissionEvent {
    /// A permission prompt was raised.
    #[serde(rename = "permission.asked")]
    Asked(EphemeralEnvelope<PermissionAsked>),
    /// The prompt was answered.
    #[serde(rename = "permission.replied")]
    Replied(EphemeralEnvelope<PermissionReplied>),
}

/// `shell.created` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellCreated {
    pub info: shell::Info,
}

/// `shell.exited` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellExited {
    pub id: ShellId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<crate::common::Num>,
    pub status: shell::Status,
}

/// `shell.deleted` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellDeleted {
    pub id: ShellId,
}

/// `shell.created` / `shell.exited` / `shell.deleted` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ShellEvent {
    /// Process started.
    #[serde(rename = "shell.created")]
    Created(EphemeralEnvelope<ShellCreated>),
    /// Process reached a terminal status.
    #[serde(rename = "shell.exited")]
    Exited(EphemeralEnvelope<ShellExited>),
    /// Output file dropped.
    #[serde(rename = "shell.deleted")]
    Deleted(EphemeralEnvelope<ShellDeleted>),
}

/// `form.created` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FormCreated {
    pub form: form::Info,
}

/// `form.replied` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FormReplied {
    pub id: crate::id::FormId,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub answer: form::Answer,
}

/// `form.cancelled` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FormCancelled {
    pub id: crate::id::FormId,
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// `form.created` / `form.replied` / `form.cancelled` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum FormEvent {
    /// A form was raised.
    #[serde(rename = "form.created")]
    Created(EphemeralEnvelope<FormCreated>),
    /// The form was answered.
    #[serde(rename = "form.replied")]
    Replied(EphemeralEnvelope<FormReplied>),
    /// The form was cancelled.
    #[serde(rename = "form.cancelled")]
    Cancelled(EphemeralEnvelope<FormCancelled>),
}

/// `worktree.updated` data (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeUpdated {
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
}

/// `worktree.resolved` data (durable v1, aggregate `projectID`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeResolved {
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    pub directory: crate::common::AbsolutePath,
    pub previous: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adopted: Option<Vec<ProjectId>>,
}

/// `worktree.updated` / `worktree.resolved`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum WorktreeRegistryEvent {
    /// Worktree list changed.
    #[serde(rename = "worktree.updated")]
    Updated(EphemeralEnvelope<WorktreeUpdated>),
    /// A worktree directory resolved to a project (durable, aggregate
    /// `projectID`, version 1).
    #[serde(rename = "worktree.resolved")]
    Resolved(DurableEnvelope<WorktreeResolved>),
}

/// Keep unused imports alive for parity docs.
#[allow(dead_code)]
type _NonNegativeInt = NonNegativeInt;
#[allow(dead_code)]
type _Record = Record;
#[allow(dead_code)]
type _EventId = EventId;
#[allow(dead_code)]
type _DurableInfo = DurableInfo;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_exited_wire() {
        let event = ShellEvent::Exited(EphemeralEnvelope {
            id: EventId::parse("evt_1").unwrap(),
            created: 1,
            metadata: None,
            location: None,
            data: ShellExited {
                id: ShellId::parse("sh_1").unwrap(),
                exit: Some(crate::common::Num(0.0)),
                status: shell::Status::Exited,
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "shell.exited");
        assert_eq!(json["data"]["exit"], 0);
        assert_eq!(json["data"]["status"], "exited");
        let back: ShellEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn permission_asked_wire() {
        let event = PermissionEvent::Asked(EphemeralEnvelope {
            id: EventId::parse("evt_2").unwrap(),
            created: 2,
            metadata: None,
            location: None,
            data: PermissionAsked {
                id: PermissionId::parse("per_1").unwrap(),
                session_id: SessionId::parse("ses_1").unwrap(),
                action: "edit".into(),
                resources: vec!["/repo/file.ts".into()],
                save: None,
                metadata: None,
                source: None,
                message: None,
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "permission.asked");
        assert_eq!(json["data"]["sessionID"], "ses_1");
        assert_eq!(json["data"]["resources"][0], "/repo/file.ts");
    }

    #[test]
    fn worktree_resolved_durable_wire() {
        let event = WorktreeRegistryEvent::Resolved(DurableEnvelope {
            id: EventId::parse("evt_3").unwrap(),
            created: 3,
            metadata: None,
            durable: DurableInfo { aggregate_id: "proj_1".into(), seq: 0, version: 1 },
            location: None,
            data: WorktreeResolved {
                project_id: ProjectId::parse("proj_1"),
                directory: crate::common::AbsolutePath::parse("/repo/.worktrees/feature"),
                previous: ProjectId::global(),
                adopted: None,
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "worktree.resolved");
        assert_eq!(json["durable"]["aggregateID"], "proj_1");
        assert_eq!(json["data"]["previous"], "global");
        let back: WorktreeRegistryEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn empty_payload_events_wire() {
        let event = AgentEvent::Updated(EphemeralEnvelope {
            id: EventId::parse("evt_4").unwrap(),
            created: 4,
            metadata: None,
            location: None,
            data: Empty {},
        });
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "id": "evt_4", "created": 4, "type": "agent.updated", "data": {}
            })
        );
    }
}
