//! Port of `agent.ts`.

use serde::{Deserialize, Serialize};

use crate::common::PositiveInt;
use crate::id::{AgentId, AgentName};
use crate::model;
use crate::permission::{Effect, Rule, Ruleset};
use crate::provider;

/// `Agent.Info.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Subagent,
    Primary,
    All,
}

/// `Agent.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: AgentId,
    pub name: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<model::Ref>,
    pub request: provider::Request,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub mode: Mode,
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<PositiveInt>,
    pub permissions: Ruleset,
}

impl Info {
    /// `Agent.Info.default` — the built-in permission baseline v2 grants every
    /// synthetic default agent.
    pub fn default_for(id: AgentId) -> Self {
        Self {
            name: AgentName(id.0.clone()),
            id,
            model: None,
            request: provider::Request::default(),
            system: None,
            description: None,
            mode: Mode::Primary,
            hidden: false,
            color: None,
            steps: None,
            permissions: vec![
                Rule { action: "*".into(), resource: "*".into(), effect: Effect::Allow },
                Rule {
                    action: "external_directory".into(),
                    resource: "*".into(),
                    effect: Effect::Ask,
                },
                Rule { action: "read".into(), resource: "*.env".into(), effect: Effect::Ask },
                Rule { action: "read".into(), resource: "*.env.*".into(), effect: Effect::Ask },
                Rule {
                    action: "read".into(),
                    resource: "*.env.example".into(),
                    effect: Effect::Allow,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_default_wire() {
        let info = Info::default_for(AgentId::parse("build"));
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "build",
                "name": "build",
                "request": {"settings": {}, "headers": {}, "body": {}},
                "mode": "primary",
                "hidden": false,
                "permissions": [
                    {"action": "*", "resource": "*", "effect": "allow"},
                    {"action": "external_directory", "resource": "*", "effect": "ask"},
                    {"action": "read", "resource": "*.env", "effect": "ask"},
                    {"action": "read", "resource": "*.env.*", "effect": "ask"},
                    {"action": "read", "resource": "*.env.example", "effect": "allow"}
                ]
            })
        );
    }
}
