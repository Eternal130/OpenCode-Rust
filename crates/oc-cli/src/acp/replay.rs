//! History replay (port of `acp/replay.ts`): `session/load` streams the
//! stored message tree as `session/update` notifications.

use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, SessionUpdate, TextContent};
use serde_json::Value;

use crate::acp::capabilities::Capabilities;
use crate::acp::compaction;
use crate::acp::content::{parts_to_content_chunks, ReplayPart};
use crate::acp::tool;
use crate::acp::translate::reasoning_message_id;

/// `history` (replay.ts:14-37): page through `message.list` (ascending,
/// 200 per page) and emit each message's updates.
pub async fn history(
    client: &oc_client::Client,
    conn: &crate::acp::Conn,
    session_id: &str,
    cwd: &str,
    capabilities: Capabilities,
) -> Result<(), crate::acp::error::AcpError> {
    let mut cursor: Option<String> = None;
    loop {
        let query = oc_client::types::SessionMessageListQuery {
            cursor: cursor.clone(),
            limit: Some("200".to_owned()),
            order: Some(oc_client::types::SessionMessageListQueryOrder::Asc),
            type_: None,
        };
        let page = client
            .message()
            .list(session_id, &query)
            .await
            .map_err(|error| crate::acp::error::classify(&error))?;
        for message in &page.data {
            let message = serde_json::to_value(message).unwrap_or(Value::Null);
            for update in updates(&message, cwd, capabilities) {
                conn.send_notification(
                    agent_client_protocol::schema::v1::SessionNotification::new(
                        session_id.to_owned(),
                        update,
                    ),
                )
                .map_err(|error| crate::acp::error::AcpError::ServiceFailure {
                    safe_message: format!("session update failed: {error}"),
                    service: Some("session".to_owned()),
                    error_name: None,
                })?;
            }
        }
        cursor.clone_from(&page.cursor.next);
        if cursor.is_none() {
            return Ok(());
        }
    }
}

/// `updates` (replay.ts:39-125).
#[must_use]
pub fn updates(message: &Value, cwd: &str, capabilities: Capabilities) -> Vec<SessionUpdate> {
    let id = message.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    match message.get("type").and_then(Value::as_str).unwrap_or_default() {
        "user" => {
            let text = message.get("text").and_then(Value::as_str).unwrap_or_default();
            let mut result = vec![SessionUpdate::UserMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                    .message_id(agent_client_protocol::schema::v1::MessageId::new(id.clone())),
            )];
            let files: Vec<ReplayPart> = message
                .get("files")
                .and_then(Value::as_array)
                .map(|files| {
                    files
                        .iter()
                        .map(|file| {
                            let mime = file.get("mime").and_then(Value::as_str).unwrap_or_default();
                            let url = match file.pointer("/source/type").and_then(Value::as_str) {
                                Some("uri") => file
                                    .pointer("/source/uri")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_owned(),
                                _ => format!(
                                    "data:{mime};base64,{}",
                                    file.get("data").and_then(Value::as_str).unwrap_or_default()
                                ),
                            };
                            ReplayPart::Prompt(crate::acp::content::PromptPart::File {
                                url,
                                filename: file
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                                mime: mime.to_owned(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            for chunk in parts_to_content_chunks(&id, &files) {
                result.push(SessionUpdate::UserMessageChunk(chunk));
            }
            result
        }
        "compaction" => {
            compaction::replay(message, capabilities.compaction).into_iter().collect::<Vec<_>>()
        }
        "assistant" => assistant_updates(message, &id, cwd),
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_lines)] // the 1:1 port of replay.ts:59-124
fn assistant_updates(message: &Value, id: &str, cwd: &str) -> Vec<SessionUpdate> {
    // Live reasoning ordinals count only reasoning parts, not the mixed
    // content array (replay.ts:57-58).
    let content = message.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let reasoning_index: Vec<usize> = content
        .iter()
        .enumerate()
        .filter(|(_, part)| part.get("type").and_then(Value::as_str) == Some("reasoning"))
        .map(|(index, _)| index)
        .collect();
    let mut result = Vec::new();
    for (index, part) in content.iter().enumerate() {
        match part.get("type").and_then(Value::as_str).unwrap_or_default() {
            "text" => {
                let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                if !text.is_empty() {
                    result.push(SessionUpdate::AgentMessageChunk(
                        ContentChunk::new(ContentBlock::Text(TextContent::new(text))).message_id(
                            agent_client_protocol::schema::v1::MessageId::new(id.to_owned()),
                        ),
                    ));
                }
            }
            "reasoning" => {
                let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                if !text.is_empty() {
                    let ordinal = reasoning_index
                        .iter()
                        .position(|reasoning| *reasoning == index)
                        .unwrap_or(0);
                    result.push(SessionUpdate::AgentThoughtChunk(
                        ContentChunk::new(ContentBlock::Text(TextContent::new(text))).message_id(
                            agent_client_protocol::schema::v1::MessageId::new(
                                reasoning_message_id(id, ordinal as u64),
                            ),
                        ),
                    ));
                }
            }
            "tool" => {
                let part_id = part.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = part.get("name").and_then(Value::as_str).unwrap_or("tool");
                let state_input = part
                    .pointer("/state/input")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let streaming =
                    part.pointer("/state/status").and_then(Value::as_str) == Some("streaming");
                let call_input =
                    if streaming { serde_json::Map::new() } else { state_input.clone() };
                let call = tool::pending_tool_call(part_id, name, &call_input, None, cwd, None);
                result.push(SessionUpdate::ToolCall(call));
                match part.pointer("/state/status").and_then(Value::as_str).unwrap_or_default() {
                    "completed" => {
                        let content = part
                            .pointer("/state/content")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        result.push(SessionUpdate::ToolCallUpdate(tool::completed_tool_update(
                            part_id,
                            name,
                            &state_input,
                            &content,
                            part.pointer("/state/metadata").and_then(Value::as_object),
                            cwd,
                        )));
                    }
                    "running" => {
                        result.push(SessionUpdate::ToolCallUpdate(tool::running_tool_update(
                            part_id,
                            name,
                            &state_input,
                            cwd,
                            None,
                        )));
                    }
                    "error" => {
                        let content = part
                            .pointer("/state/content")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        result.push(SessionUpdate::ToolCallUpdate(tool::error_tool_update(
                            part_id,
                            name,
                            &state_input,
                            Some(&content),
                            part.pointer("/state/metadata").and_then(Value::as_object),
                            part.pointer("/state/error/message")
                                .and_then(Value::as_str)
                                .unwrap_or("tool failed"),
                            cwd,
                            None,
                        )));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_messages_replay_text_and_files() {
        let message = json!({
            "id": "msg_1",
            "type": "user",
            "text": "hello",
            "files": [
                { "name": "a.txt", "mime": "text/plain", "source": { "type": "uri", "uri": "file:///w/a.txt" } },
            ],
        });
        let updates = updates(&message, "/w", Capabilities::default());
        assert_eq!(updates.len(), 2);
        assert!(matches!(updates[0], SessionUpdate::UserMessageChunk(_)));
        assert!(matches!(updates[1], SessionUpdate::UserMessageChunk(_)));
    }

    #[test]
    fn assistant_messages_replay_text_reasoning_and_tools() {
        let message = json!({
            "id": "msg_2",
            "type": "assistant",
            "content": [
                { "type": "reasoning", "text": "thinking" },
                { "type": "text", "text": "answer" },
                { "type": "tool", "id": "tsk_1", "name": "read",
                  "state": { "status": "completed", "input": { "path": "a" },
                             "content": [{ "type": "text", "text": "data" }] } },
            ],
        });
        let updates = updates(&message, "/w", Capabilities::default());
        assert_eq!(updates.len(), 4);
        assert!(matches!(updates[0], SessionUpdate::AgentThoughtChunk(_)));
        assert!(matches!(updates[1], SessionUpdate::AgentMessageChunk(_)));
        assert!(matches!(updates[2], SessionUpdate::ToolCall(_)));
        assert!(matches!(updates[3], SessionUpdate::ToolCallUpdate(_)));
        let SessionUpdate::AgentThoughtChunk(chunk) = &updates[0] else {
            panic!("thought chunk");
        };
        assert_eq!(chunk.message_id.as_ref().map(|id| id.0.as_ref()), Some("msg_2:reasoning:0"));
    }

    #[test]
    fn streaming_tools_replay_as_pending_only() {
        let message = json!({
            "id": "msg_3",
            "type": "assistant",
            "content": [
                { "type": "tool", "id": "tsk_1", "name": "shell",
                  "state": { "status": "streaming" } },
            ],
        });
        let updates = updates(&message, "/w", Capabilities::default());
        assert_eq!(updates.len(), 1);
        assert!(matches!(updates[0], SessionUpdate::ToolCall(_)));
    }

    #[test]
    fn compaction_messages_replay_per_capability() {
        let message = json!({
            "id": "msg_4", "type": "compaction", "status": "completed",
            "reason": "manual", "summary": "the summary",
        });
        assert_eq!(
            updates(&message, "/w", Capabilities { compaction: true, ..Capabilities::default() })
                .len(),
            1
        );
        assert_eq!(
            updates(&message, "/w", Capabilities::default()).len(),
            1,
            "the _meta marker still replays"
        );
        let running =
            json!({ "id": "msg_5", "type": "compaction", "status": "running", "reason": "auto" });
        assert_eq!(updates(&running, "/w", Capabilities::default()).len(), 0);
    }
}
