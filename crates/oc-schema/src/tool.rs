//! Port of `tool.ts` — only the wire-relevant pieces: `Tool.CallID` lives in
//! [`crate::id`], and `Content` is the tagged `text`/`file` union used by tool
//! results and assistant tool parts.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::Record;

/// `Tool.Content` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Content {
    /// Plain text content.
    Text {
        /// The text.
        text: String,
    },
    /// File content referenced by URI.
    File {
        /// Data or remote URI.
        uri: String,
        /// MIME type.
        mime: String,
        /// Display name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// `Tool.Error` — the wire shape used inside error payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_wire() {
        let text = Content::Text { text: "hello".into() };
        assert_eq!(
            serde_json::to_value(&text).unwrap(),
            serde_json::json!({"type": "text", "text": "hello"})
        );
        let file =
            Content::File { uri: "file:///tmp/a.png".into(), mime: "image/png".into(), name: None };
        assert_eq!(
            serde_json::to_value(&file).unwrap(),
            serde_json::json!({"type": "file", "uri": "file:///tmp/a.png", "mime": "image/png"})
        );
        let back: Content = serde_json::from_value(serde_json::json!({
            "type": "file", "uri": "u", "mime": "m", "name": "n"
        }))
        .unwrap();
        assert_eq!(
            back,
            Content::File { uri: "u".into(), mime: "m".into(), name: Some("n".into()) }
        );
    }
}
