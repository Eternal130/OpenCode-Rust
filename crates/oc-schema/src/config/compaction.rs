//! `config/compaction.ts` — `Config.Compaction`.

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;

/// `Config.Compaction.Keep`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Keep {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<NonNegativeInt>,
}

/// `Config.Compaction`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Compaction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<Keep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buffer: Option<NonNegativeInt>,
}
