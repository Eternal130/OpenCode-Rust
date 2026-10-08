//! `config/watcher.ts` — `Config.Watcher`.

use serde::{Deserialize, Serialize};

/// `Config.Watcher`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Watcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore: Option<Vec<String>>,
}
