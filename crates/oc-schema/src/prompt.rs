//! Port of `prompt.ts`.

use serde::{Deserialize, Serialize};

use crate::common::Num;

use crate::id::{SkillId, SkillName};
use crate::skill;

/// `Prompt.Mention` — source span of an @-mention inside prompt text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Mention {
    pub start: Num,
    pub end: Num,
    pub text: String,
}

/// `Prompt.FileSource` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FileSource {
    /// Inline base64 data.
    Inline,
    /// Referenced by URI.
    Uri { uri: String },
}

/// `Prompt.FileAttachment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileAttachment {
    /// Base64 payload (`^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$`).
    pub data: String,
    pub mime: String,
    pub source: FileSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<Mention>,
}

/// `PromptInput.FileAttachment` (prompt-input.ts) — the *wire* form
/// the client POSTs (`session.prompt`'s `files` array): a data/file
/// URI the server materializes into a resolved [`FileAttachment`]
/// (core/session/prompt.ts `materializeAttachment`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InputFileAttachment {
    /// `data:` (inline base64) or `file://` URI.
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<Mention>,
}

/// `Prompt.AgentAttachment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentAttachment {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<Mention>,
}

/// `Prompt.SkillAttachment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillAttachment {
    pub id: SkillId,
    pub name: SkillName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<Mention>,
}

/// `Prompt`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Prompt {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<AgentAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<SkillAttachment>>,
}

/// Keep the `skill` module import for doc-link parity with v2.
#[allow(dead_code)]
type _Skill = skill::Skill;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_source_wire() {
        let value = serde_json::json!({"type": "uri", "uri": "file:///tmp/x.png"});
        let parsed: FileSource = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        assert_eq!(
            serde_json::to_value(FileSource::Inline).unwrap(),
            serde_json::json!({"type": "inline"})
        );
    }

    #[test]
    fn input_file_attachment_wire() {
        // The PromptInput shape the TUI POSTs (uri form, unlike the
        // resolved FileAttachment's data/mime/source).
        let value = serde_json::json!({
            "uri": "data:image/png;base64,AAECAw==",
            "name": "screenshot.png",
            "mention": {"start": 0.0, "end": 9.0, "text": "[Image 1]"}
        });
        let parsed: InputFileAttachment = serde_json::from_value(value.clone()).unwrap();
        // Round-trip: `Num` serializes compact integers (9, not 9.0),
        // so compare via re-parse instead of raw JSON equality.
        let round: InputFileAttachment =
            serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
        assert_eq!(round, parsed);
        assert_eq!(parsed.name.as_deref(), Some("screenshot.png"));
        assert_eq!(parsed.mention.as_ref().map(|m| m.text.as_str()), Some("[Image 1]"));
        // `uri` is the only required field.
        let minimal: InputFileAttachment =
            serde_json::from_value(serde_json::json!({"uri": "file:///tmp/x.pdf"})).unwrap();
        assert_eq!(minimal.name, None);
    }

    #[test]
    fn prompt_wire() {
        let prompt = Prompt { text: "hello".into(), ..Default::default() };
        assert_eq!(serde_json::to_value(&prompt).unwrap(), serde_json::json!({"text": "hello"}));
    }
}
