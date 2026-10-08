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

// TODO(port): Worktree.adopt() — session/project reattribution projection
// helper from worktree.ts (pure function over session + worktree.resolved
// event data); belongs in oc-core once the projector lands.

/// `Worktree.OperationError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorktreeOperationError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchResult {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub time: WebSearchResultTime,
}

/// `WebSearch.Result.time`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WebSearchResultTime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<NonNegativeInt>,
}

/// `WebSearch.Response`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
}
