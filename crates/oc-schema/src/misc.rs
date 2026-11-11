//! Ports of `vcs.ts`, `worktree.ts`, `command.ts`, `websearch.ts`,
//! `plugin.ts`, `question.rs` (from `question.ts`), `workspace.ts`.

use serde::{Deserialize, Serialize};

use crate::common::{AbsolutePath, NonNegativeInt};
use crate::id::{PluginId, ProjectId};

/// `Vcs.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub branch: VcsBranch,
}

/// `Vcs.Branch`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsBranch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// `Vcs.BranchList`.
pub type BranchList = Vec<String>;

/// `Vcs.Base`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsBase {
    pub name: String,
    #[serde(rename = "ref")]
    pub ref_: String,
    pub source: VcsBaseSource,
}

/// `Vcs.Base.source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VcsBaseSource {
    Reflog,
    Default,
}

/// `Vcs.Mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum VcsMode {
    Working,
    Branch,
    Committed,
}

/// `Vcs.FileStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct VcsFileStatus {
    pub file: String,
    pub additions: NonNegativeInt,
    pub deletions: NonNegativeInt,
    pub status: crate::session_support::FileDiffStatus,
}

/// `Worktree.CreateInput`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeCreateInput {
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AbsolutePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<AbsolutePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `Worktree.RemoveInput`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeRemoveInput {
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    pub directory: AbsolutePath,
    pub force: bool,
}

/// `Worktree.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeInfo {
    pub directory: AbsolutePath,
}

/// `Worktree.Directory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeDirectory {
    pub directory: AbsolutePath,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
}

/// `Worktree.ListEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeListEntry {
    pub directory: AbsolutePath,
    #[serde(rename = "type")]
    pub entry_type: WorktreeEntryType,
}

/// `Worktree.ListEntry.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorktreeEntryType {
    Root,
    Worktree,
}

/// `Worktree.List`.
pub type WorktreeList = Vec<WorktreeDirectory>;

/// Input to [`worktree_adopt`] — the session-side facts (a subset of
/// `Session.Info`: `projectID`, `directory`, and `workspaceID?`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAdoptSession {
    pub project_id: ProjectId,
    /// Session directory (may use backslashes on Windows-style paths).
    pub directory: String,
    pub workspace_id: Option<String>,
}

/// Input to [`worktree_adopt`] — the `worktree.resolved` event data
/// (`Worktree.Resolved`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAdoptEvent {
    pub project_id: ProjectId,
    pub directory: String,
    pub previous: ProjectId,
    pub adopted: Option<Vec<ProjectId>>,
}

/// The [`worktree_adopt`] result: the session's new ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeAdopted {
    pub project_id: ProjectId,
    /// `None` when the session sits at the worktree root itself.
    pub subpath: Option<String>,
}

/// `Worktree.adopt` (worktree.ts:73) — the pure client-store projection:
/// given a session and a `worktree.resolved` event, decides whether the
/// session's ownership follows the directory's new project identity, and
/// with which subpath. G-87: ported 1:1 from the ref (consumed there by
/// `packages/client/src/solid/data.ts` on every `worktree.resolved`
/// frame; the DB-side twin is `oc-core`'s session projector
/// `project_worktree`). Returns `None` when the session is not adopted.
#[must_use]
pub fn worktree_adopt(
    session: &WorktreeAdoptSession,
    event: &WorktreeAdoptEvent,
) -> Option<WorktreeAdopted> {
    if session.workspace_id.is_some() {
        return None;
    }
    let explicitly_adopted =
        event.adopted.as_ref().is_some_and(|ids| ids.contains(&session.project_id));
    if session.project_id != event.previous
        && !session.project_id.is_global()
        && !explicitly_adopted
    {
        return None;
    }
    if session.project_id == event.project_id {
        return None;
    }
    let directory = normalize_slashes(&session.directory);
    let root = normalize_slashes(&event.directory);
    // Windows drive-letter roots compare case-insensitively (`/c:/...`).
    let windows = root.len() >= 2
        && root.starts_with('/')
        && root.as_bytes()[1].is_ascii_alphabetic()
        && root[2..].starts_with(':');
    let (key, parent) = if windows {
        (directory.to_lowercase(), root.to_lowercase())
    } else {
        (directory.clone(), root.clone())
    };
    if key != parent && !key.starts_with(&format!("{parent}/")) {
        return None;
    }
    let subpath = (key != parent).then(|| directory[root.len() + 1..].to_string());
    Some(WorktreeAdopted { project_id: event.project_id.clone(), subpath })
}

/// The ref's `normalize` (worktree.ts:91-101): backslashes → forward
/// slashes, then a segment fold dropping empty/`.` segments and
/// cancelling `..` against the accumulated tail.
fn normalize_slashes(value: &str) -> String {
    let mut result = String::new();
    for segment in value.replace('\\', "/").split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            if let Some(pos) = result.rfind('/') {
                result.truncate(pos);
            } else {
                result.clear();
            }
            continue;
        }
        result.push('/');
        result.push_str(segment);
    }
    result
}

/// `Worktree.OperationError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeOperationError {
    pub message: String,
    // ref wire key is `forceRequired` (worktree.ts:49). Currently dead code
    // (no emitter) but kept 1:1 with the ref wire shape.
    #[serde(rename = "forceRequired", default, skip_serializing_if = "Option::is_none")]
    pub force_required: Option<bool>,
}

/// `Command.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CommandInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `WebSearch.Provider`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchProvider {
    pub id: crate::id::WebSearchId,
    pub name: String,
}

/// `WebSearch.Result`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchResult {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub time: WebSearchResultTime,
}

/// `WebSearch.Result.time`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchResultTime {
    /// Publication time in milliseconds since the Unix epoch. The ref
    /// types it `Schema.Finite` (websearch.ts:29) — any finite NUMBER,
    /// not an unsigned integer (G-380④); the generated openapi/client
    /// face already says `type: number` (types.gen.rs `published: f64`).
    /// JSON cannot encode NaN/Inf, so `f64` here is finite by
    /// construction on decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<f64>,
}

/// `WebSearch.Response`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchResponse {
    #[serde(rename = "providerID")]
    pub provider_id: crate::id::WebSearchId,
    pub results: Vec<WebSearchResult>,
}

/// `WebSearch.Input`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchInput {
    pub query: String,
    #[serde(rename = "providerID", default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<crate::id::WebSearchId>,
}

/// `Plugin.Source` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    /// Compiled into the binary.
    Builtin,
    /// Installed package.
    Package {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outdated: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updating: Option<bool>,
    },
    /// Local directory or file.
    Local { path: String },
    /// Loaded via the SDK.
    Sdk,
}

/// `Plugin.Features`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginFeatures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tui: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rpc: Option<bool>,
}

/// `Plugin.State` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum PluginState {
    Active,
    Failed {
        error: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_: Option<String>,
    },
}

/// `Plugin.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PluginInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<PluginId>,
    pub source: PluginSource,
    pub features: PluginFeatures,
    pub state: PluginState,
}

/// `Question.Prompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuestionPrompt {
    /// Complete question.
    pub question: String,
    /// Very short label (max 30 chars).
    pub header: String,
    /// Available choices.
    pub options: Vec<QuestionOption>,
    /// Allow selecting multiple choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
}

/// `Question.Prompt` option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuestionOption {
    /// Display text (1-5 words, concise).
    pub label: String,
    /// Explanation of choice.
    pub description: String,
}

/// `Question.Answer`.
pub type QuestionAnswer = Vec<String>;

/// `Workspace.DestroyResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceDestroyResult {
    /// True when this request transitioned the workspace from existing to
    /// destroyed.
    pub destroyed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcs_info_wire() {
        let info = VcsInfo {
            provider: Some("git".into()),
            branch: VcsBranch { current: Some("main".into()), default: Some("main".into()) },
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "provider": "git",
                "branch": {"current": "main", "default": "main"}
            })
        );
    }

    #[test]
    fn plugin_source_wire() {
        let value = serde_json::json!({
            "type": "package",
            "target": "github:opencode/plugin",
            "version": "1.0.0"
        });
        let parsed: PluginSource = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, PluginSource::Package { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    fn adopt_case(
        session_project: &str,
        session_dir: &str,
        event_previous: &str,
        event_adopted: Option<&[&str]>,
    ) -> Option<super::WorktreeAdopted> {
        super::worktree_adopt(
            &super::WorktreeAdoptSession {
                project_id: ProjectId::parse(session_project),
                directory: session_dir.to_string(),
                workspace_id: None,
            },
            &super::WorktreeAdoptEvent {
                project_id: ProjectId::parse("proj_new"),
                directory: "/repo/.worktrees/feature".to_string(),
                previous: ProjectId::parse(event_previous),
                adopted: event_adopted
                    .map(|ids| ids.iter().map(|id| ProjectId::parse(*id)).collect()),
            },
        )
    }

    #[test]
    fn worktree_adopt_follows_previous_identity() {
        let adopted = adopt_case("proj_old", "/repo/.worktrees/feature/sub", "proj_old", None)
            .expect("previous-identity session under the resolved root is adopted");
        assert_eq!(adopted.project_id, ProjectId::parse("proj_new"));
        assert_eq!(adopted.subpath.as_deref(), Some("sub"));
    }

    #[test]
    fn worktree_adopt_root_session_has_no_subpath() {
        let adopted = adopt_case("proj_old", "/repo/.worktrees/feature", "proj_old", None)
            .expect("root session is adopted");
        assert_eq!(adopted.subpath, None);
    }

    #[test]
    fn worktree_adopt_explicitly_adopted_project_uses_canonical_directory() {
        // Explicitly adopted projects are candidates regardless of
        // `previous`; adoption still requires the session directory to be
        // contained in the resolved root (ref `Worktree.adopt` keep).
        let adopted = adopt_case(
            "proj_marker",
            "/repo/.worktrees/feature/nested",
            "proj_old",
            Some(&["proj_marker"]),
        )
        .expect("explicitly adopted + contained adopts");
        assert_eq!(adopted.project_id, ProjectId::parse("proj_new"));
        assert_eq!(adopted.subpath.as_deref(), Some("nested"));
    }

    #[test]
    fn worktree_adopt_rejects_non_candidates() {
        // Neither previous, global, nor explicitly adopted → not a candidate.
        assert!(adopt_case("proj_other", "/repo/.worktrees/feature", "proj_old", None).is_none());
        // Workspace sessions never re-attribute.
        let workspace = super::worktree_adopt(
            &super::WorktreeAdoptSession {
                project_id: ProjectId::parse("proj_old"),
                directory: "/repo/.worktrees/feature".to_string(),
                workspace_id: Some("ws_1".to_string()),
            },
            &super::WorktreeAdoptEvent {
                project_id: ProjectId::parse("proj_new"),
                directory: "/repo/.worktrees/feature".to_string(),
                previous: ProjectId::parse("proj_old"),
                adopted: None,
            },
        );
        assert!(workspace.is_none());
    }

    #[test]
    fn worktree_adopt_rejects_same_project_and_outside_directory() {
        // Already the resolved project → no move.
        assert!(adopt_case("proj_new", "/repo/.worktrees/feature", "proj_new", None).is_none());
        // Under previous identity but outside the resolved directory → no move.
        assert!(adopt_case("proj_old", "/elsewhere", "proj_old", None).is_none());
        // Global sessions adopt when contained.
        let adopted = adopt_case("global", "/repo/.worktrees/feature/x.ts", "proj_old", None)
            .expect("global session under the root adopts");
        assert_eq!(adopted.subpath.as_deref(), Some("x.ts"));
    }

    #[test]
    fn worktree_adopt_normalizes_backslashes_and_dots() {
        let adopted = adopt_case(
            "proj_old",
            "/repo/.worktrees/feature/./sub/../sub2\\deep",
            "proj_old",
            None,
        )
        .expect("normalized path is contained");
        assert_eq!(adopted.subpath.as_deref(), Some("sub2/deep"));
    }
}
