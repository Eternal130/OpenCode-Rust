//! Branded string identifiers, ported from the v2 schema packages
//! (`session-id.ts`, `session-message.ts`, `event.ts`, `shell.ts`, `form.ts`,
//! `permission.ts`, `workspace-id.ts`, `permission-saved.ts`, `credential.ts`,
//! `integration.ts`, `tool.ts`, plus the un-prefixed brands from `agent.ts`,
//! `model.ts`, `provider.ts`, `skill.ts`, `project-id.ts`, `snapshot.ts`).
//!
//! Wire format: transparent JSON strings. Prefix-checked ids expose
//! [`parse`](SessionId::parse) plus a generating constructor matching v2's
//! identifier scheme (see [`crate::identifier`]).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::identifier;

/// Error returned when a prefixed id fails validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind} `{value}`: expected prefix `{prefix}`")]
pub struct InvalidId {
    pub kind: &'static str,
    pub prefix: &'static str,
    pub value: String,
}

macro_rules! branded_id {
    (
        $(#[$meta:meta])*
        $name:ident, $kind:literal, check $check:literal, generate $gen:literal, $generator:ident
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
        )]
        #[serde(transparent)]
        #[schemars(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Validates `value` against the v2 prefix rule.
            pub fn parse(value: impl Into<String>) -> Result<Self, InvalidId> {
                let value = value.into();
                if value.starts_with($check) {
                    Ok(Self(value))
                } else {
                    Err(InvalidId { kind: $kind, prefix: $check, value })
                }
            }

            /// Generates a fresh id using the v2 identifier scheme.
            pub fn new() -> Self {
                Self(format!($gen, identifier::$generator()))
            }

            /// Borrow the inner string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
    (
        $(#[$meta:meta])*
        $name:ident, $kind:literal
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
        )]
        #[serde(transparent)]
        #[schemars(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Wraps an existing value without validation (v2 brands these
            /// strings but imposes no format check).
            pub fn parse(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the inner string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

branded_id! {
    /// `Session.ID` — `ses_…`, generated descending so newer sessions sort first.
    SessionId, "Session.ID", check "ses_", generate "ses_{}", descending
}
branded_id! {
    /// `Session.Message.ID` — `msg_…`, ascending.
    MessageId, "Session.Message.ID", check "msg_", generate "msg_{}", ascending
}
branded_id! {
    /// `Event.ID` — `evt_…`, ascending.
    EventId, "Event.ID", check "evt_", generate "evt_{}", ascending
}
branded_id! {
    /// `Workspace.ID` — `wrk_…` (v2 checks the `wrk` prefix), ascending.
    WorkspaceId, "Workspace.ID", check "wrk", generate "wrk_{}", ascending
}
branded_id! {
    /// `Permission.ID` — `per_…`, ascending.
    PermissionId, "Permission.ID", check "per_", generate "per_{}", ascending
}
branded_id! {
    /// `Shell.ID` — `sh_…`, ascending.
    ShellId, "Shell.ID", check "sh_", generate "sh_{}", ascending
}
branded_id! {
    /// `Form.ID` — `frm_…`, ascending.
    FormId, "Form.ID", check "frm_", generate "frm_{}", ascending
}
branded_id! {
    /// `PermissionSaved.ID` — `psv_…`, ascending.
    PermissionSavedId, "PermissionSaved.ID", check "psv_", generate "psv_{}", ascending
}
branded_id! {
    /// `Credential.ID` — `cred_…`, ascending.
    CredentialId, "Credential.ID", check "cred_", generate "cred_{}", ascending
}
branded_id! {
    /// `Integration.AttemptID` — `con_…`, ascending.
    IntegrationAttemptId, "Integration.AttemptID", check "con_", generate "con_{}", ascending
}

branded_id! {
    /// `Project.ID` — arbitrary string; the synthetic global project is `"global"`.
    ProjectId, "Project.ID"
}
branded_id! {
    /// `Agent.ID`.
    AgentId, "Agent.ID"
}
branded_id! {
    /// `Agent.Name`.
    AgentName, "Agent.Name"
}
branded_id! {
    /// `Model.ID`.
    ModelId, "Model.ID"
}
branded_id! {
    /// `Model.VariantID`.
    ModelVariantId, "Model.VariantID"
}
branded_id! {
    /// `Provider.ID`.
    ProviderId, "Provider.ID"
}
branded_id! {
    /// `Skill.ID`.
    SkillId, "Skill.ID"
}
branded_id! {
    /// `Skill.Name`.
    SkillName, "Skill.Name"
}
branded_id! {
    /// `Integration.ID`.
    IntegrationId, "Integration.ID"
}
branded_id! {
    /// `Integration.MethodID`.
    IntegrationMethodId, "Integration.MethodID"
}
branded_id! {
    /// `Snapshot.ID`.
    SnapshotId, "Snapshot.ID"
}
branded_id! {
    /// `Plugin.ID`.
    PluginId, "Plugin.ID"
}
branded_id! {
    /// `Tool.CallID` — provider-assigned tool call identifier.
    CallId, "Tool.CallID"
}
branded_id! {
    /// `WebSearch.ID`.
    WebSearchId, "WebSearch.ID"
}

impl ProjectId {
    /// The synthetic project every location belongs to before detection.
    pub fn global() -> Self {
        Self("global".to_string())
    }
}

impl ProviderId {
    /// The opencode provider.
    pub fn opencode() -> Self {
        Self("opencode".to_string())
    }
    /// The anthropic provider.
    pub fn anthropic() -> Self {
        Self("anthropic".to_string())
    }
    /// The openai provider.
    pub fn openai() -> Self {
        Self("openai".to_string())
    }
    /// The google provider.
    pub fn google() -> Self {
        Self("google".to_string())
    }
}

impl MessageId {
    /// `Session.Message.ID.fromEvent`: rebrands an `evt_…` id as `msg_…`.
    pub fn from_event(event_id: &EventId) -> Self {
        Self(event_id.as_str().replacen("evt_", "msg_", 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_prefix() {
        assert!(SessionId::parse("ses_abc").is_ok());
        assert_eq!(
            SessionId::parse("msg_abc").unwrap_err(),
            InvalidId { kind: "Session.ID", prefix: "ses_", value: "msg_abc".to_string() }
        );
    }

    #[test]
    fn generates_with_prefix() {
        assert!(SessionId::new().as_str().starts_with("ses_"));
        assert!(MessageId::new().as_str().starts_with("msg_"));
        assert!(EventId::new().as_str().starts_with("evt_"));
        assert!(WorkspaceId::new().as_str().starts_with("wrk_"));
        assert!(ShellId::new().as_str().starts_with("sh_"));
        assert!(FormId::new().as_str().starts_with("frm_"));
        assert!(PermissionSavedId::new().as_str().starts_with("psv_"));
        assert!(CredentialId::new().as_str().starts_with("cred_"));
        assert!(IntegrationAttemptId::new().as_str().starts_with("con_"));
    }

    #[test]
    fn message_id_from_event() {
        let event = EventId::parse("evt_timeline_0001").unwrap();
        assert_eq!(MessageId::from_event(&event).as_str(), "msg_timeline_0001");
    }

    #[test]
    fn serde_transparent() {
        let id = SessionId::parse("ses_abc").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"ses_abc\"");
        let back: SessionId = serde_json::from_str("\"ses_abc\"").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn global_project() {
        assert_eq!(ProjectId::global().as_str(), "global");
    }
}
