//! `config/experimental.ts` — `Config.Experimental`.

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;
use crate::config::policy::Policy;

/// `Config.Experimental`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Experimental {
    /// Enable the experimental portable shell permission scanner (default false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable_shell_scanner: Option<bool>,
    /// Maximum subagent nesting depth (default 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_depth: Option<NonNegativeInt>,
    /// Ordered policies controlling access to configured resources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policies: Option<Vec<Policy>>,
}
