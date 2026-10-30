//! `config/experimental.ts` — `Config.Experimental`.

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;
use crate::config::policy::Policy;

/// `Config.Experimental.FileRetention` — file-retention sweep tuning
/// (spilled tool output + shell captures). Absent fields keep the
/// production defaults (hourly sweep, 7-day retention).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileRetention {
    /// Cleanup sweep interval in milliseconds (default 3_600_000 — hourly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_interval: Option<NonNegativeInt>,
}

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
    /// File-retention sweep tuning (`file_retention`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_retention: Option<FileRetention>,
}
