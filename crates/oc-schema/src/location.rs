//! Port of `location.ts` and `project.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::{AbsolutePath, NonNegativeInt};
use crate::id::{ProjectId, WorkspaceId};

/// `Location.Ref` — `{ directory, workspaceID? }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ref {
    pub directory: AbsolutePath,
    #[serde(rename = "workspaceID", default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
}

/// `Location.PublicRef` — `Ref` without `workspaceID`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PublicRef {
    pub directory: AbsolutePath,
}

/// `Project.Current` — the identity fields of the project a location maps to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectCurrent {
    pub id: ProjectId,
    pub directory: AbsolutePath,
    pub canonical: AbsolutePath,
}

/// `Location.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub directory: AbsolutePath,
    #[serde(rename = "workspaceID", default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub project: ProjectCurrent,
}

/// `Location.PublicInfo` — `Info` without `workspaceID`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PublicInfo {
    pub directory: AbsolutePath,
    pub project: ProjectCurrent,
}

/// `Project.Vcs` — vcs identifier (`^[a-z][a-z0-9._-]*$`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct ProjectVcs(pub String);

/// `Project.Icon`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectIcon {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "override")]
    pub override_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// `Project.Commands`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectCommands {
    /// Startup script run when creating a new workspace (worktree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
}

/// `Project.Time` — epoch milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectTime {
    pub created: NonNegativeInt,
    pub updated: NonNegativeInt,
    pub active: NonNegativeInt,
}

/// `Project` (`Project.Info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    pub id: ProjectId,
    pub canonical: AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs: Option<ProjectVcs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
    pub time: ProjectTime,
    pub sandboxes: Vec<String>,
}

/// `Project.UpdateInput`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectUpdateInput {
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<AbsolutePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<ProjectIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<ProjectCommands>,
}

/// Extra keys accepted on `Project` payloads (v2 struct is closed; kept for
/// forward-compat during the port).
pub type ProjectExtra = BTreeMap<String, serde_json::Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_wire() {
        let value = serde_json::json!({
            "directory": "/repo",
            "workspaceID": "wrk_abc"
        });
        let parsed: Ref = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(parsed.directory.as_str(), "/repo");
        assert_eq!(parsed.workspace_id.as_ref().unwrap().as_str(), "wrk_abc");
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn public_ref_omits_workspace() {
        let parsed: PublicRef =
            serde_json::from_value(serde_json::json!({"directory": "/repo"})).unwrap();
        assert_eq!(
            serde_json::to_value(&parsed).unwrap(),
            serde_json::json!({"directory": "/repo"})
        );
    }

    #[test]
    fn project_info_wire() {
        let project = Project {
            id: ProjectId::global(),
            canonical: AbsolutePath::parse("/repo"),
            vcs: None,
            name: Some("repo".into()),
            icon: None,
            commands: None,
            time: ProjectTime { created: 1, updated: 2, active: 3 },
            sandboxes: vec![],
        };
        let json = serde_json::to_value(&project).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "global",
                "canonical": "/repo",
                "name": "repo",
                "time": {"created": 1, "updated": 2, "active": 3},
                "sandboxes": []
            })
        );
    }
}
