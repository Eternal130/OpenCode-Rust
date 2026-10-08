//! `config/tool-output.ts` — `Config.ToolOutput`.

use serde::{Deserialize, Serialize};

use crate::common::PositiveInt;

/// `Config.ToolOutput`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ToolOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<PositiveInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<PositiveInt>,
}
