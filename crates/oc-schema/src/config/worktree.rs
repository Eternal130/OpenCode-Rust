//! `config/worktree.ts` — `Config.Worktree`.

use serde::{Deserialize, Serialize};

/// `Config.Worktree`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Worktree {
    /// Parent directory for new worktrees, relative to the project's primary
    /// checkout when not absolute.
    pub directory: String,
}
