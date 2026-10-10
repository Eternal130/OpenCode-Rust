//! Prompt preparation (port of `acp/prompt.ts`): ACP content blocks →
//! the synthetic/visible split, files, slash commands, and the start id.

use agent_client_protocol::schema::v1::ContentBlock;
use serde_json::{json, Value};

use crate::acp::catalog::{Catalog, BUILTIN_COMPACT};
use crate::acp::content::{link_reference, prompt_content_to_parts, PromptPart};
use crate::acp::error::AcpError;
use crate::acp::translate::{TurnStart, TurnStartKind};

/// `Prepared` (prompt.ts:12-19).
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub start: TurnStart,
    pub text: String,
    pub files: Vec<Value>,
    pub synthetic: Vec<String>,
    pub slash: Option<(String, String)>,
    pub command: Option<crate::acp::catalog::Command>,
}

/// `prepare` (prompt.ts:21-43).
pub fn prepare(catalog: &Catalog, content: &[ContentBlock]) -> Result<Prepared, AcpError> {
    if content.iter().any(|block| {
        matches!(block, ContentBlock::Image(image) if image.data.is_empty() && image.uri.is_none())
    }) {
        return Err(AcpError::InvalidRequest {
            message: "image content has no data or uri".to_owned(),
            field: Some("prompt".to_owned()),
        });
    }
    let parts = prompt_content_to_parts(content)
        .into_iter()
        .map(reference_unreadable_file)
        .collect::<Vec<_>>();
    let visible: Vec<&PromptPart> = parts
        .iter()
        .filter(|part| match part {
            PromptPart::Text { synthetic, ignored, .. } => !synthetic && !ignored,
            PromptPart::File { .. } => true,
        })
        .collect();
    let text = visible
        .iter()
        .filter_map(|part| match part {
            PromptPart::Text { text, .. } => Some(text.clone()),
            PromptPart::File { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let files: Vec<Value> = visible
        .iter()
        .filter_map(|part| match part {
            PromptPart::File { url, filename, .. } => Some(json!({ "uri": url, "name": filename })),
            PromptPart::Text { .. } => None,
        })
        .collect();
    let synthetic: Vec<String> = parts
        .iter()
        .filter_map(|part| match part {
            PromptPart::Text { text, synthetic: true, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let slash = detect_slash_command(&text);
    let start = if slash.as_ref().is_some_and(|(name, _)| name == BUILTIN_COMPACT) {
        TurnStart { kind: TurnStartKind::Compaction, id: new_message_id() }
    } else {
        TurnStart { kind: TurnStartKind::Input, id: new_message_id() }
    };
    let command = slash.as_ref().and_then(|(name, _)| {
        catalog.commands.iter().find(|command| &command.name == name).cloned()
    });
    Ok(Prepared { start, text, files, synthetic, slash, command })
}

/// `SessionMessage.ID.create()` — the server's `msg_…` id shape.
fn new_message_id() -> String {
    oc_schema::MessageId::new().as_str().to_owned()
}

/// `referenceUnreadableFile` (prompt.ts:45-51): an unreadable `file://`
/// attachment degrades to a text reference.
fn reference_unreadable_file(part: PromptPart) -> PromptPart {
    let PromptPart::File { url, filename, .. } = &part else { return part };
    if !url.starts_with("file://") {
        return part;
    }
    let path = url.strip_prefix("file://").unwrap_or(url);
    let readable = std::fs::metadata(path).is_ok_and(|meta| !meta.is_dir())
        && std::fs::File::open(path).is_ok();
    if readable {
        part
    } else {
        link_reference(filename.as_deref(), url)
    }
}

/// `detectSlashCommand` (prompt.ts:53-59).
fn detect_slash_command(text: &str) -> Option<(String, String)> {
    let value = text.trim();
    if !value.starts_with('/') {
        return None;
    }
    let mut words = value[1..].split_whitespace();
    let name = words.next()?.to_owned();
    if name.is_empty() {
        return None;
    }
    let args = words.collect::<Vec<_>>().join(" ").trim().to_owned();
    Some((name, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{ImageContent, TextContent};

    fn catalog() -> Catalog {
        Catalog {
            models: vec![],
            default_model: crate::acp::catalog::ModelRef::default(),
            modes: vec![],
            default_mode_id: "build".to_owned(),
            commands: vec![crate::acp::catalog::Command {
                name: "clear".to_owned(),
                description: None,
            }],
        }
    }

    fn text(value: &str) -> ContentBlock {
        ContentBlock::Text(TextContent::new(value))
    }

    #[test]
    fn plain_prompts_carry_text_and_a_fresh_message_id() {
        let prepared = prepare(&catalog(), &[text("hello")]).unwrap();
        assert_eq!(prepared.text, "hello");
        assert!(prepared.start.id.starts_with("msg_"));
        assert_eq!(prepared.start.kind, TurnStartKind::Input);
        assert_eq!(prepared.files.len(), 0, "no attachments");
        assert_eq!(prepared.synthetic.len(), 0, "no synthetic parts");
    }

    #[test]
    fn images_without_payload_are_rejected() {
        let error = prepare(&catalog(), &[ContentBlock::Image(ImageContent::new("", "image/png"))])
            .unwrap_err();
        assert!(
            matches!(error, AcpError::InvalidRequest { field: Some(field), .. } if field == "prompt")
        );
    }

    #[test]
    fn slash_commands_split_name_and_args() {
        let prepared = prepare(&catalog(), &[text("/clear all the things")]).unwrap();
        assert_eq!(prepared.slash, Some(("clear".to_owned(), "all the things".to_owned())));
        assert!(prepared.command.is_some());
        let prepared = prepare(&catalog(), &[text("/compact now")]).unwrap();
        assert_eq!(prepared.start.kind, TurnStartKind::Compaction);
    }

    #[test]
    fn plain_text_without_slash_is_not_a_command() {
        assert!(detect_slash_command("hello /world").is_none());
        assert!(detect_slash_command("/").is_none());
    }
}
