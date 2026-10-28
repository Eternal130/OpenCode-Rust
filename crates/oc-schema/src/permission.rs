//! Port of `permission.ts` (plus `permission-saved.ts` at the bottom).

use serde::{Deserialize, Serialize};

use crate::common::{Record, RelativePath};
use crate::id::{PermissionId, PermissionSavedId, ProjectId, SessionId};

/// `Permission.Effect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Allow,
    Deny,
    Ask,
}

/// `Permission.Rule` — `{ action, resource, effect }`. The last matching rule
/// in a ruleset wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Rule {
    pub action: String,
    pub resource: String,
    pub effect: Effect,
}

/// `Permission.Ruleset` — ordered rules.
pub type Ruleset = Vec<Rule>;

/// `Permission.Source` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Source {
    /// Originating tool call.
    Tool {
        #[serde(rename = "messageID")]
        message_id: String,
        /// Tool call id.
        id: String,
    },
}

/// `Permission.Request`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Request {
    pub id: PermissionId,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub action: String,
    pub resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `Permission.Reply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Reply {
    Once,
    Always,
    Reject,
}

/// `PermissionSaved.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionSaved {
    pub id: PermissionSavedId,
    // ref wire key is `projectID` (permission-saved.ts:16) — without this
    // rename a direct serde serialization would emit snake_case.
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    pub action: String,
    pub resource: String,
    pub time: PermissionSavedTime,
}

/// Time fields of [`PermissionSaved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionSavedTime {
    pub created: u64,
    pub updated: u64,
}

// `RelativePath` re-exported for ruleset helpers that resolve resources.
#[allow(dead_code)]
type _KeepRelativePath = RelativePath;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_wire() {
        let rule = Rule { action: "read".into(), resource: "*.env".into(), effect: Effect::Ask };
        assert_eq!(
            serde_json::to_value(&rule).unwrap(),
            serde_json::json!({"action": "read", "resource": "*.env", "effect": "ask"})
        );
    }

    #[test]
    fn source_wire() {
        let value = serde_json::json!({
            "type": "tool", "messageID": "msg_1", "id": "call_1"
        });
        let parsed: Source = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    /// RW-T1: direct serde of `PermissionSaved` emits the ref wire key
    /// `projectID` (permission-saved.ts:16), not snake_case.
    #[test]
    fn permission_saved_wire_camel_case() {
        let saved = PermissionSaved {
            id: PermissionSavedId("psv_1".into()),
            project_id: ProjectId("prj_1".into()),
            action: "bash".into(),
            resource: "*".into(),
            time: PermissionSavedTime { created: 1, updated: 2 },
        };
        let json = serde_json::to_value(&saved).unwrap();
        assert!(json.get("projectID").is_some(), "wire key must be projectID: {json}");
        assert!(json.get("project_id").is_none(), "snake_case must not leak: {json}");
        let back: PermissionSaved = serde_json::from_value(json).unwrap();
        assert_eq!(back, saved);
    }
}
