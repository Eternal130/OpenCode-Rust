//! `config/command.ts` — `Config.Command` (slash command definitions; the
//! markdown frontmatter shape lives in core).

use serde::{Deserialize, Serialize};

use crate::config::model::ModelSelection;

/// `Config.Command`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Command {
    /// The command template (may contain `{key}` substitutions).
    pub template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<bool>,
    /// Deprecated alias for subagent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtask: Option<bool>,
}
