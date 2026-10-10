//! Prompt content conversion (port of `acp/content.ts`): ACP content
//! blocks ↔ opencode prompt parts, and replay parts → content chunks.

use agent_client_protocol::schema::v1::{
    Annotations, ContentBlock, ContentChunk, EmbeddedResource, ImageContent,
    ResourceLink as AcpResourceLink, Role, TextContent,
};
use base64::Engine as _;
use serde_json::json;

/// `PromptPart` (content.ts:6-9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptPart {
    Text { text: String, synthetic: bool, ignored: bool },
    File { url: String, filename: Option<String>, mime: String },
}

/// `promptContentToParts` (content.ts:12-14).
#[must_use]
pub fn prompt_content_to_parts(content: &[ContentBlock]) -> Vec<PromptPart> {
    content.iter().flat_map(content_block_to_parts).collect()
}

/// `contentBlockToParts` (content.ts:16-67).
fn content_block_to_parts(block: &ContentBlock) -> Vec<PromptPart> {
    match block {
        ContentBlock::Text(text) => {
            let audience = text
                .annotations
                .as_ref()
                .and_then(|annotations| annotations.audience.as_deref())
                .unwrap_or_default();
            // audience?.length === 1 && audience[0] === "assistant"/"user"
            // (content.ts:19-27).
            if audience.len() == 1 && audience[0] == Role::Assistant {
                return vec![PromptPart::Text {
                    text: text.text.clone(),
                    synthetic: true,
                    ignored: false,
                }];
            }
            if audience.len() == 1 && audience[0] == Role::User {
                return vec![PromptPart::Text {
                    text: text.text.clone(),
                    synthetic: false,
                    ignored: true,
                }];
            }
            vec![PromptPart::Text { text: text.text.clone(), synthetic: false, ignored: false }]
        }
        ContentBlock::Image(image) => {
            let url = if image.data.is_empty() {
                image.uri.clone().unwrap_or_default()
            } else {
                format!("data:{};base64,{}", image.mime_type, image.data)
            };
            if url.is_empty() {
                return Vec::new();
            }
            let filename = filename_from_uri(image.uri.as_deref()).or(Some("image".to_owned()));
            if url.starts_with("data:") {
                return vec![PromptPart::File { url, filename, mime: image.mime_type.clone() }];
            }
            vec![resource_link_to_part(
                &url,
                filename.filter(|name| name != "image"),
                Mime::Named(image.mime_type.clone()),
            )]
        }
        ContentBlock::ResourceLink(link) => vec![resource_link_to_part(
            &link.uri,
            Some(link.name.clone()).filter(|name| !name.is_empty()),
            link.mime_type.clone().map_or(Mime::Plain, Mime::Named),
        )],
        ContentBlock::Resource(resource) => match &resource.resource {
            agent_client_protocol::schema::v1::EmbeddedResourceResource::TextResourceContents(
                contents,
            ) => {
                // file: URIs become file references; everything else is
                // inlined as text (content.ts:37-51).
                if let Some(path) = file_url_to_path(&contents.uri) {
                    let line = contents
                        .uri
                        .rsplit('#')
                        .next()
                        .and_then(|fragment| fragment.strip_prefix('L'))
                        .and_then(|digits| digits.parse::<u32>().ok());
                    let reference = match line {
                        Some(line) => format!("[{path}:{line}]"),
                        None => format!("[{path}]"),
                    };
                    vec![PromptPart::Text {
                        text: format!("{reference}\n{}", contents.text),
                        synthetic: false,
                        ignored: false,
                    }]
                } else {
                    vec![PromptPart::Text {
                        text: format!("[{}]\n{}", contents.uri, contents.text),
                        synthetic: false,
                        ignored: false,
                    }]
                }
            }
            agent_client_protocol::schema::v1::EmbeddedResourceResource::BlobResourceContents(
                contents,
            ) => {
                let Some(mime) = contents.mime_type.clone() else {
                    return Vec::new();
                };
                let url = if contents.uri.starts_with("data:") {
                    contents.uri.clone()
                } else {
                    format!("data:{mime};base64,{}", contents.blob)
                };
                vec![PromptPart::File {
                    url,
                    filename: filename_from_uri(Some(&contents.uri)).or(Some("file".to_owned())),
                    mime,
                }]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `linkReference` (content.ts:160-162).
#[must_use]
pub fn link_reference(name: Option<&str>, uri: &str) -> PromptPart {
    PromptPart::Text {
        text: match name {
            Some(name) => format!("[{name}]({uri})"),
            None => uri.to_owned(),
        },
        synthetic: false,
        ignored: false,
    }
}

enum Mime {
    Named(String),
    Plain,
}

fn resource_link_to_part(uri: &str, name: Option<String>, mime: Mime) -> PromptPart {
    let mime = match mime {
        Mime::Named(mime) => mime,
        Mime::Plain => "text/plain".to_owned(),
    };
    if uri.starts_with("file://") {
        let filename = match name {
            Some(name) if !name.is_empty() => Some(name),
            _ => filename_from_uri(Some(uri))
                .filter(|name| name != "file://")
                .or(Some("file".to_owned())),
        };
        return PromptPart::File { url: uri.to_owned(), filename, mime };
    }
    if uri.starts_with("zed://") {
        if let Some(path) = uri
            .split_once("?path=")
            .map(|(_, path)| path.split('&').next().unwrap_or_default().to_owned())
            .filter(|path| !path.is_empty())
        {
            let filename = match name {
                Some(name) if !name.is_empty() => Some(name),
                _ => path
                    .rsplit('/')
                    .next()
                    .map(str::to_owned)
                    .filter(|name| !name.is_empty())
                    .or(Some("file".to_owned())),
            };
            return PromptPart::File { url: format!("file://{path}"), filename, mime };
        }
    }
    // linkReference (content.ts:160-162).
    match name.as_deref() {
        Some(name) if !name.is_empty() => link_reference(Some(name), uri),
        _ => link_reference(None, uri),
    }
}

/// One replayed message part (`ReplayPart`, content.ts:10). The
/// reasoning variant rides the replay caller
/// (`acp/replay.rs`), which folds reasoning inline.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ReplayPart {
    Prompt(PromptPart),
    Reasoning { text: String },
}

/// `partsToContentChunks` (content.ts:69-136). The message id rides the
/// enclosing session update (replay.ts:43-51), so it is threaded in.
#[must_use]
pub fn parts_to_content_chunks(message_id: &str, parts: &[ReplayPart]) -> Vec<ContentChunk> {
    parts
        .iter()
        .flat_map(|part| match part {
            ReplayPart::Reasoning { text } => {
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())))]
                }
            }
            ReplayPart::Prompt(part) => prompt_part_chunks(message_id, part),
        })
        .collect()
}

fn prompt_part_chunks(message_id: &str, part: &PromptPart) -> Vec<ContentChunk> {
    match part {
        PromptPart::Text { text, synthetic, ignored } => {
            if text.is_empty() {
                return Vec::new();
            }
            let mut content = TextContent::new(text.clone());
            if *synthetic {
                content = content.annotations(Annotations::new().audience(vec![Role::Assistant]));
            } else if *ignored {
                content = content.annotations(Annotations::new().audience(vec![Role::User]));
            }
            vec![ContentChunk::new(ContentBlock::Text(content))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(message_id))]
        }
        PromptPart::File { url, filename, mime } => {
            let name = filename.clone().unwrap_or_else(|| "file".to_owned());
            if url.starts_with("file://") {
                return vec![ContentChunk::new(ContentBlock::ResourceLink(
                    AcpResourceLink::new(name, url.clone()).mime_type(mime.clone()),
                ))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(message_id))];
            }
            if !url.starts_with("data:") {
                return Vec::new();
            }
            // ^data:([^;]+);base64,(.*)$ (content.ts:100).
            let Some(rest) = url.strip_prefix("data:") else { return Vec::new() };
            let Some((mime_part, data)) = rest.split_once(";base64,") else {
                return Vec::new();
            };
            if mime_part.is_empty() || data.is_empty() {
                return Vec::new();
            }
            let file_url = format!("file:///{name}");
            if mime_part.starts_with("image/") {
                return vec![ContentChunk::new(ContentBlock::Image(
                    ImageContent::new(data, mime_part).uri(file_url),
                ))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(message_id))];
            }
            let resource = if mime_part.starts_with("text/") || mime_part == "application/json" {
                let text = String::from_utf8(
                    base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default(),
                )
                .unwrap_or_default();
                json!({ "uri": file_url, "mimeType": mime_part, "text": text })
            } else {
                json!({ "uri": file_url, "mimeType": mime_part, "blob": data })
            };
            let Ok(resource) =
                serde_json::from_value::<EmbeddedResource>(json!({ "resource": resource }))
            else {
                return Vec::new();
            };
            vec![ContentChunk::new(ContentBlock::Resource(resource))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(message_id))]
        }
    }
}

fn file_url_to_path(uri: &str) -> Option<String> {
    let path = uri.strip_prefix("file://")?;
    let path = percent_decode(path.split('#').next().unwrap_or(path));
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `filenameFromUri` (content.ts:164-171).
fn filename_from_uri(uri: Option<&str>) -> Option<String> {
    let uri = uri?;
    if uri.starts_with("data:") {
        return None;
    }
    let path = if let Some(query) =
        uri.strip_prefix("zed://").and_then(|rest| rest.split_once("?path="))
    {
        query.1.split('&').next().unwrap_or_default().to_owned()
    } else if uri.contains("://") {
        uri.split("://").nth(1)?.split(['?', '#']).next()?.to_owned()
    } else {
        uri.to_owned()
    };
    let base = path.rsplit('/').next()?.to_owned();
    (!base.is_empty()).then_some(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> ContentBlock {
        ContentBlock::Text(TextContent::new(value))
    }

    #[test]
    fn assistant_audience_text_is_synthetic() {
        let block = ContentBlock::Text(
            TextContent::new("psst")
                .annotations(Annotations::new().audience(vec![Role::Assistant])),
        );
        assert_eq!(
            prompt_content_to_parts(&[block]),
            vec![PromptPart::Text { text: "psst".to_owned(), synthetic: true, ignored: false }]
        );
    }

    #[test]
    fn user_audience_text_is_ignored() {
        let block = ContentBlock::Text(
            TextContent::new("note").annotations(Annotations::new().audience(vec![Role::User])),
        );
        assert_eq!(
            prompt_content_to_parts(&[block]),
            vec![PromptPart::Text { text: "note".to_owned(), synthetic: false, ignored: true }]
        );
    }

    #[test]
    fn inline_images_become_data_urls() {
        let block = ContentBlock::Image(ImageContent::new("aGk=", "image/png"));
        assert_eq!(
            prompt_content_to_parts(&[block]),
            vec![PromptPart::File {
                url: "data:image/png;base64,aGk=".to_owned(),
                filename: Some("image".to_owned()),
                mime: "image/png".to_owned()
            }]
        );
        let block = ContentBlock::Image(
            ImageContent::new("", "image/png").uri("https://example.com/pic.png"),
        );
        assert_eq!(
            prompt_content_to_parts(&[block]),
            vec![PromptPart::Text {
                text: "[pic.png](https://example.com/pic.png)".to_owned(),
                synthetic: false,
                ignored: false
            }]
        );
    }

    #[test]
    fn file_resources_inline_as_references() {
        let block = serde_json::from_value::<ContentBlock>(json!({
            "type": "resource",
            "resource": { "uri": "file:///w/a.txt#L12", "text": "the content" }
        }))
        .unwrap();
        assert_eq!(
            prompt_content_to_parts(&[block]),
            vec![PromptPart::Text {
                text: "[/w/a.txt:12]\nthe content".to_owned(),
                synthetic: false,
                ignored: false
            }]
        );
    }

    #[test]
    fn file_links_round_trip_through_replay_chunks() {
        let parts = vec![ReplayPart::Prompt(PromptPart::File {
            url: "file:///w/a.txt".to_owned(),
            filename: Some("a.txt".to_owned()),
            mime: "text/plain".to_owned(),
        })];
        let chunks = parts_to_content_chunks("msg_1", &parts);
        assert!(matches!(chunks[0].content, ContentBlock::ResourceLink(_)));
    }

    #[test]
    fn data_urls_become_images_or_resources() {
        let image = parts_to_content_chunks(
            "msg_1",
            &[ReplayPart::Prompt(PromptPart::File {
                url: "data:image/png;base64,aGk=".to_owned(),
                filename: Some("x.png".to_owned()),
                mime: "image/png".to_owned(),
            })],
        );
        assert!(matches!(image[0].content, ContentBlock::Image(_)));
        let json = parts_to_content_chunks(
            "msg_1",
            &[ReplayPart::Prompt(PromptPart::File {
                url: "data:application/json;base64,e30=".to_owned(),
                filename: Some("x.json".to_owned()),
                mime: "application/json".to_owned(),
            })],
        );
        assert!(matches!(json[0].content, ContentBlock::Resource(_)));
        let empty = parts_to_content_chunks(
            "msg_1",
            &[ReplayPart::Prompt(PromptPart::File {
                url: "https://elsewhere".to_owned(),
                filename: None,
                mime: "text/plain".to_owned(),
            })],
        );
        assert_eq!(empty.len(), 0, "non-file URLs are dropped");
    }

    #[test]
    fn plain_text_round_trips() {
        assert_eq!(
            prompt_content_to_parts(&[text("hello")]),
            vec![PromptPart::Text { text: "hello".to_owned(), synthetic: false, ignored: false }]
        );
        let _ = text("x"); // silence unused warnings in certain builds
    }
}
