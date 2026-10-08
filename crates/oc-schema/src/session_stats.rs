//! Port of `session-stats.ts`.

use serde::{Deserialize, Serialize};

use crate::common::{NonNegativeInt, Num, TokenUsage, Usd};
use crate::model;

/// `SessionStats.Activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Activity {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub steps: NonNegativeInt,
}

/// `SessionStats.ModelUsage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelUsage {
    pub model: model::Ref,
    pub steps: NonNegativeInt,
    pub tokens: TokenUsage,
    pub cost: Usd,
}

/// `SessionStats.ToolUsage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolUsage {
    pub name: String,
    pub calls: NonNegativeInt,
    pub succeeded: NonNegativeInt,
    pub failed: NonNegativeInt,
    pub unfinished: NonNegativeInt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_p50: Option<Num>,
}

/// `SessionStats.ToolTotals`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolTotals {
    pub calls: NonNegativeInt,
    pub succeeded: NonNegativeInt,
    pub failed: NonNegativeInt,
    pub unfinished: NonNegativeInt,
}

/// `SessionStats.Tools` — tagged by `mode`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Tools {
    /// Tools omitted.
    None,
    /// Aggregate totals only.
    Summary { totals: ToolTotals },
    /// Totals plus per-tool usage.
    Detail { totals: ToolTotals, usage: Vec<ToolUsage> },
}

/// `SessionStats.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub range: Range,
    pub sessions: NonNegativeInt,
    pub subagents: NonNegativeInt,
    pub prompts: NonNegativeInt,
    pub steps: NonNegativeInt,
    pub tokens: TokenUsage,
    pub cost: Usd,
    pub tools: Tools,
    pub active_days: NonNegativeInt,
    pub streak: NonNegativeInt,
    pub activity: Vec<Activity>,
    pub models: Vec<ModelUsage>,
}

/// Stats window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Range {
    /// Inclusive start (epoch millis).
    pub from: u64,
    /// Inclusive end (epoch millis).
    pub to: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_wire() {
        let value = serde_json::json!({
            "mode": "summary",
            "totals": {"calls": 10, "succeeded": 8, "failed": 1, "unfinished": 1}
        });
        let parsed: Tools = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Tools::Summary { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn info_minimal_wire() {
        let info = Info {
            range: Range { from: 1, to: 2 },
            sessions: 3,
            subagents: 0,
            prompts: 3,
            steps: 7,
            tokens: TokenUsage::default(),
            cost: Usd::zero(),
            tools: Tools::None,
            active_days: 2,
            streak: 1,
            activity: vec![],
            models: vec![],
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["tools"]["mode"], "none");
        assert_eq!(json["activeDays"], 2);
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }
}
