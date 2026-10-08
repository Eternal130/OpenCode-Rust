//! Port of `session.ts` (`Session.Info`, `Session.ListAnchor`).

use serde::{Deserialize, Serialize};

use crate::common::{RelativePath, TokenUsage, Usd};
use crate::id::{AgentId, ProjectId, SessionId};
use crate::location;
use crate::model;
use crate::permission::Ruleset;
use crate::session_support::{ForkBoundary, Revert, SessionMetadata};

/// `Session.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: SessionId,
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork: Option<Fork>,
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<model::Ref>,
    pub cost: Usd,
    pub tokens: TokenUsage,
    /// Outcome of the last completed execution; absent until a terminal
    /// transition records `time.idle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    pub time: Time,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub location: location::Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<RelativePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
    /// Evaluated after the agent's rules; the last matching rule wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Ruleset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<Revert>,
}

/// Fork provenance of [`Info`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Fork {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub boundary: ForkBoundary,
}

/// `Session.Info.time` — epoch milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Time {
    pub created: u64,
    pub updated: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<u64>,
}

/// `Session.Info.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Succeeded,
    Failed,
    Interrupted,
}

/// `SessionTransfer.Data` — full session payload for export/import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TransferData {
    pub info: Info,
    pub messages: Vec<crate::session_message::Info>,
}

/// `Session.ListAnchor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListAnchor {
    pub id: SessionId,
    pub time: u64,
    pub direction: Direction,
}

/// `Session.ListAnchor.direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Previous,
    Next,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_wire() {
        let info = Info {
            id: SessionId::parse("ses_1").unwrap(),
            parent_id: None,
            fork: None,
            project_id: ProjectId::global(),
            agent: None,
            model: None,
            cost: Usd::zero(),
            tokens: TokenUsage::default(),
            outcome: None,
            time: Time { created: 1, updated: 2, idle: None, viewed: None, archived: None },
            title: Some("Test".into()),
            location: location::Ref {
                directory: crate::common::AbsolutePath::parse("/repo"),
                workspace_id: None,
            },
            subpath: None,
            metadata: None,
            permissions: None,
            revert: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["id"], "ses_1");
        assert_eq!(json["projectID"], "global");
        assert_eq!(json["title"], "Test");
        assert_eq!(json["cost"], 0);
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }
}
