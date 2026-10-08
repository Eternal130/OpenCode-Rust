//! Port of `skill.ts`.

use serde::{Deserialize, Serialize};

use crate::common::AbsolutePath;
use crate::id::{SkillId, SkillName};

/// `Skill.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Skill {
    pub id: SkillId,
    pub name: SkillName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "autoinvoke", default, skip_serializing_if = "Option::is_none")]
    pub auto_invoke: Option<bool>,
    pub path: AbsolutePath,
    pub content: String,
}

/// `Skill.Source` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkillSource {
    /// Loaded from a directory of skill files.
    Directory {
        /// Absolute path.
        path: AbsolutePath,
    },
    /// Loaded from a URL.
    Url { url: String },
    /// Embedded skill document.
    Embedded { skill: Skill },
}

impl SkillSource {
    /// `Skill.Source.key` — stable identity key for dedup.
    pub fn key(&self) -> String {
        match self {
            SkillSource::Directory { path } => format!("directory:{}", path.as_str()),
            SkillSource::Url { url } => format!("url:{url}"),
            SkillSource::Embedded { skill } => format!("embedded:{}", skill.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_wire() {
        let skill = Skill {
            id: SkillId::parse("review"),
            name: SkillName::parse("Review"),
            description: Some("Review code".into()),
            auto_invoke: None,
            path: AbsolutePath::parse("/skills/review/SKILL.md"),
            content: "instructions".into(),
        };
        let json = serde_json::to_value(&skill).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "review",
                "name": "Review",
                "description": "Review code",
                "path": "/skills/review/SKILL.md",
                "content": "instructions"
            })
        );
    }

    #[test]
    fn source_wire_and_key() {
        let value = serde_json::json!({"type": "url", "url": "https://example.com/skill.md"});
        let parsed: SkillSource = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.key(), "url:https://example.com/skill.md");
    }
}
