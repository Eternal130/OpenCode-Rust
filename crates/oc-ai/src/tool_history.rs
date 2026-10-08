//! Tool history normalization (port of `src/tool-history.ts`): fill missing
//! tool results before the next step, align result identity with its call,
//! replace empty outputs, and hold system updates until pending calls are
//! answered.

use std::collections::BTreeMap;

use crate::schema::{ContentPart, Message, MessageRole, ToolContentItems, ToolResultValue};

pub const EMPTY_TOOL_OUTPUT: &str = "(no tool output)";
pub const MISSING_TOOL_RESULT: &str = "Tool result missing";

/// A tool-call part plus its optional namespace, as tracked while pending.
#[derive(Debug, Clone)]
pub struct PendingCall {
    pub id: String,
    pub name: String,
    pub namespace: Option<String>,
}

impl PendingCall {
    /// Extract identity from a tool-call content part.
    #[must_use]
    pub fn from_part(part: &ContentPart) -> Option<Self> {
        match part {
            ContentPart::ToolCall { id, name, namespace, .. } => {
                Some(Self { id: id.clone(), name: name.clone(), namespace: namespace.clone() })
            }
            _ => None,
        }
    }
}

/// Convenience view over a tool-call content part.
pub type ToolCallPart = ContentPart;

/// Normalize a message list (port of `normalizeToolHistory`).
#[must_use]
pub fn normalize_tool_history(messages: &[Message]) -> Vec<Message> {
    let mut normalized: Vec<Message> = Vec::new();
    let mut pending: BTreeMap<String, PendingCall> = BTreeMap::new();
    // System updates cannot sit between a tool call and its results, so they
    // wait until every pending call is answered.
    let mut held: Vec<Message> = Vec::new();

    let release_held = |normalized: &mut Vec<Message>,
                        held: &mut Vec<Message>,
                        pending: &BTreeMap<String, PendingCall>| {
        if pending.is_empty() {
            normalized.append(held);
        }
    };
    let append_missing_results = |normalized: &mut Vec<Message>,
                                  pending: &mut BTreeMap<String, PendingCall>,
                                  held: &mut Vec<Message>| {
        if !pending.is_empty() {
            let calls = pending.values().cloned().collect::<Vec<_>>();
            normalized.push(missing_tool_results(&calls));
        }
        pending.clear();
        release_held(normalized, held, pending);
    };

    for message in messages {
        match message.role {
            MessageRole::User | MessageRole::Assistant => {
                append_missing_results(&mut normalized, &mut pending, &mut held);
            }
            MessageRole::Tool => {
                if let Some(tool) = normalize_tool_message(message, &mut pending) {
                    normalized.push(tool);
                }
                release_held(&mut normalized, &mut held, &pending);
                continue;
            }
            MessageRole::System if !pending.is_empty() => {
                held.push(message.clone());
                continue;
            }
            MessageRole::System => {}
        }

        normalized.push(message.clone());
        if message.role == MessageRole::Assistant {
            for part in &message.content {
                if let ContentPart::ToolCall {
                    id, name, namespace, provider_executed: None, ..
                } = part
                {
                    pending.insert(
                        id.clone(),
                        PendingCall {
                            id: id.clone(),
                            name: name.clone(),
                            namespace: namespace.clone(),
                        },
                    );
                }
            }
        }
    }
    append_missing_results(&mut normalized, &mut pending, &mut held);
    normalized
}

fn missing_tool_results(calls: &[PendingCall]) -> Message {
    Message::new(
        MessageRole::Tool,
        calls
            .iter()
            .map(|call| {
                tool_result_part(
                    call.id.clone(),
                    call.name.clone(),
                    call.namespace.clone(),
                    ToolResultValue::Error { value: MISSING_TOOL_RESULT.into() },
                )
            })
            .collect(),
    )
}

fn tool_result_part(
    id: String,
    name: String,
    namespace: Option<String>,
    result: ToolResultValue,
) -> ContentPart {
    ContentPart::ToolResult {
        id,
        name,
        namespace,
        result,
        provider_executed: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

fn normalize_tool_message(
    message: &Message,
    pending: &mut BTreeMap<String, PendingCall>,
) -> Option<Message> {
    let mut content = Vec::with_capacity(message.content.len());
    for part in &message.content {
        let ContentPart::ToolResult { id, provider_executed, .. } = part else {
            content.push(part.clone());
            continue;
        };
        if *provider_executed == Some(true) {
            content.push(part.clone());
            continue;
        }
        let call = pending.remove(id);
        content.push(normalize_tool_result(part, call.as_ref()));
    }
    if content.is_empty() {
        return None;
    }
    Some(Message { content, ..message.clone() })
}

fn normalize_tool_result(part: &ContentPart, call: Option<&PendingCall>) -> ContentPart {
    let ContentPart::ToolResult {
        id,
        name,
        namespace,
        result,
        provider_executed,
        cache,
        metadata,
        provider_metadata,
    } = part
    else {
        return part.clone();
    };
    // A matching call is the complete tool identity (port of the rename step).
    let (name, namespace) = match call {
        Some(call) if name != &call.name || namespace != &call.namespace => {
            (call.name.clone(), call.namespace.clone())
        }
        _ => (name.clone(), namespace.clone()),
    };
    let result = match result {
        ToolResultValue::Text { value } if value.as_str() == Some("") => {
            ToolResultValue::Text { value: EMPTY_TOOL_OUTPUT.into() }
        }
        ToolResultValue::Error { value } if value.as_str() == Some("") => {
            ToolResultValue::Error { value: EMPTY_TOOL_OUTPUT.into() }
        }
        ToolResultValue::Content { value } => {
            // Drop empty text items; an all-empty list becomes a text marker.
            let filtered: Vec<serde_json::Value> = value
                .0
                .iter()
                .filter(|item| {
                    !(item.get("type").and_then(|t| t.as_str()) == Some("text")
                        && item.get("text").and_then(|t| t.as_str()) == Some(""))
                })
                .cloned()
                .collect();
            if filtered.is_empty() {
                ToolResultValue::Text { value: EMPTY_TOOL_OUTPUT.into() }
            } else if filtered.len() == value.0.len() {
                ToolResultValue::Content { value: ToolContentItems(value.0.clone()) }
            } else {
                ToolResultValue::Content { value: ToolContentItems(filtered) }
            }
        }
        other => other.clone(),
    };
    ContentPart::ToolResult {
        id: id.clone(),
        name,
        namespace,
        result,
        provider_executed: *provider_executed,
        cache: cache.clone(),
        metadata: metadata.clone(),
        provider_metadata: provider_metadata.clone(),
    }
}

/// The tool-result part shape used by this module's tests and callers.
pub type ToolResultPart = ContentPart;

#[allow(dead_code)]
fn unused(_: &ToolCallPart, _: &ToolResultPart) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call(id: &str) -> ContentPart {
        ContentPart::ToolCall {
            id: id.into(),
            name: id.into(),
            namespace: None,
            input: json!({}),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }
    }

    fn tool_result(id: &str, value: serde_json::Value, result_type: &str) -> Message {
        Message::tool(ContentPart::ToolResult {
            id: id.into(),
            name: id.into(),
            namespace: None,
            result: ToolResultValue::make(value, result_type),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        })
    }

    fn roles(messages: &[Message]) -> Vec<&'static str> {
        messages
            .iter()
            .map(|m| match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            })
            .collect()
    }

    #[test]
    fn fills_missing_local_results_before_the_next_step() {
        let normalized = normalize_tool_history(&[
            Message::assistant(vec![tool_call("first"), tool_call("second")]),
            tool_result("first", "done".into(), "text"),
            Message::user("Continue."),
            Message::assistant(vec![tool_call("trailing")]),
        ]);
        assert_eq!(roles(&normalized), ["assistant", "tool", "tool", "user", "assistant", "tool"]);
        match &normalized[1].content[0] {
            ContentPart::ToolResult { id, name, .. } => {
                assert_eq!((id.as_str(), name.as_str()), ("first", "first"));
            }
            other => panic!("expected tool-result, got {}", other.kind()),
        }
        // the mis-named result is aligned to its call; the missing one is filled
        match &normalized[2].content[0] {
            ContentPart::ToolResult { id, result, .. } => {
                assert_eq!(id, "second");
                assert_eq!(result, &ToolResultValue::Error { value: MISSING_TOOL_RESULT.into() });
            }
            other => panic!("expected tool-result, got {}", other.kind()),
        }
        match &normalized[5].content[0] {
            ContentPart::ToolResult { id, result, .. } => {
                assert_eq!(id, "trailing");
                assert_eq!(result, &ToolResultValue::Error { value: MISSING_TOOL_RESULT.into() });
            }
            other => panic!("expected tool-result, got {}", other.kind()),
        }
    }

    #[test]
    fn uses_a_matching_call_as_the_complete_tool_identity() {
        let normalized = normalize_tool_history(&[
            Message::assistant(vec![ContentPart::ToolCall {
                id: "call_1".into(),
                name: "lookup".into(),
                namespace: None,
                input: json!({}),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }]),
            Message::tool(ContentPart::ToolResult {
                id: "call_1".into(),
                name: "wrong".into(),
                namespace: Some("stale".into()),
                result: ToolResultValue::Text { value: "done".into() },
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }),
        ]);
        match &normalized[1].content[0] {
            ContentPart::ToolResult { name, namespace, .. } => {
                assert_eq!(name, "lookup");
                assert!(namespace.is_none());
            }
            other => panic!("expected tool-result, got {}", other.kind()),
        }
    }

    #[test]
    fn normalizes_empty_results_without_changing_whitespace() {
        let media =
            json!({"type": "file", "uri": "data:image/png;base64,AQID", "mime": "image/png"});
        let normalized = normalize_tool_history(&[
            Message::assistant(vec![
                tool_call("text"),
                tool_call("content"),
                tool_call("error"),
                tool_call("mixed"),
                tool_call("whitespace"),
            ]),
            tool_result("text", "".into(), "text"),
            tool_result("content", json!([]), "content"),
            tool_result("error", "".into(), "error"),
            tool_result("mixed", json!([{"type": "text", "text": ""}, media.clone()]), "content"),
            tool_result("whitespace", "   ".into(), "text"),
        ]);
        let expect = [
            ToolResultValue::Text { value: EMPTY_TOOL_OUTPUT.into() },
            ToolResultValue::Text { value: EMPTY_TOOL_OUTPUT.into() },
            ToolResultValue::Error { value: EMPTY_TOOL_OUTPUT.into() },
            ToolResultValue::Content { value: ToolContentItems(vec![media]) },
            ToolResultValue::Text { value: "   ".into() },
        ];
        for (index, expected) in expect.iter().enumerate() {
            match &normalized[index + 1].content[0] {
                ContentPart::ToolResult { result, .. } => {
                    assert_eq!(result, expected, "index {index}");
                }
                other => panic!("expected tool-result, got {}", other.kind()),
            }
        }
    }

    #[test]
    fn leaves_provider_executed_history_unchanged() {
        let hosted_call = ContentPart::ToolCall {
            id: "hosted".into(),
            name: "web_search".into(),
            namespace: None,
            input: json!({}),
            provider_executed: Some(true),
            cache: None,
            metadata: None,
            provider_metadata: None,
        };
        let hosted_result = ContentPart::ToolResult {
            id: "hosted".into(),
            name: "web_search".into(),
            namespace: None,
            result: ToolResultValue::Text { value: "".into() },
            provider_executed: Some(true),
            cache: None,
            metadata: None,
            provider_metadata: None,
        };
        let hosted = Message::assistant(vec![hosted_call, hosted_result]);
        let orphan = tool_result("orphan", "ignored".into(), "text");
        let normalized = normalize_tool_history(&[orphan.clone(), hosted.clone()]);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0], orphan);
        assert_eq!(normalized[1].content.len(), 2);
    }

    #[test]
    fn moves_system_updates_after_the_results_of_pending_calls() {
        let calls = Message::assistant(vec![tool_call("first"), tool_call("second")]);
        let first = tool_result("first", "one".into(), "text");
        let second = tool_result("second", "two".into(), "text");
        let update = Message::system("First update.");
        let later = Message::system("Second update.");
        let user = Message::user("Continue.");

        let normalized = normalize_tool_history(&[
            calls.clone(),
            first.clone(),
            update.clone(),
            later.clone(),
            second.clone(),
            user.clone(),
        ]);
        assert_eq!(normalized, vec![calls, first, second, update.clone(), later, user.clone()]);

        let normalized = normalize_tool_history(&[
            Message::assistant(vec![tool_call("first")]),
            update.clone(),
            user.clone(),
        ]);
        assert_eq!(roles(&normalized), ["assistant", "tool", "system", "user"]);
    }

    #[test]
    fn moves_effort_updates_after_the_results_of_pending_calls() {
        let call = Message::assistant(vec![tool_call("first")]);
        let result = tool_result("first", "one".into(), "text");
        let effort = Message::effort(Some("low".into()), Some("high".into()));
        assert_eq!(
            normalize_tool_history(&[call.clone(), effort.clone(), result.clone()]),
            vec![call, result, effort]
        );
    }

    #[test]
    fn keeps_system_updates_in_place_when_no_call_is_pending() {
        let call = Message::assistant(vec![tool_call("first")]);
        let result = tool_result("first", "one".into(), "text");
        let update = Message::system("Update.");
        let input = vec![
            Message::user("Start."),
            update.clone(),
            call.clone(),
            result.clone(),
            update.clone(),
            Message::user("Continue."),
        ];
        let normalized = normalize_tool_history(&input);
        // first update untouched; second lands after the filled result
        assert_eq!(normalized[1], update);
        assert_eq!(roles(&normalized), ["user", "system", "assistant", "tool", "system", "user"]);
        assert_eq!(normalized[4], update);
    }
}
