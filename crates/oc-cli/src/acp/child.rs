//! Child-session (subagent) shaping (port of `acp/child.ts`): the
//! `opencode/session/child_update` extension notification, `_meta`
//! tags, namespaced toolCallIds, and title prefixes.

use agent_client_protocol::schema::v1::{SessionUpdate, ToolCallUpdate};
use serde_json::{json, Value};

/// `UpdateMethod` (child.ts:3).
pub const UPDATE_METHOD: &str = "opencode/session/child_update";

/// A tracked child session (child.ts:5-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSession {
    pub id: String,
    pub parent_id: String,
    pub depth: usize,
    pub title: Option<String>,
}

/// A child status event (child.ts:12-17).
#[derive(Debug, Clone, PartialEq)]
pub enum ChildEvent {
    Update { update: Box<SessionUpdate> },
    Status { status: &'static str, error: Option<SessionError> },
}

/// `SessionError.Error` — the two fields the update carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError {
    pub r#type: String,
    pub message: String,
}

/// `update` (child.ts:28-37): the extension notification params.
#[must_use]
pub fn update(root_session_id: &str, child: &ChildSession, event: &ChildEvent) -> Value {
    let mut value = json!({
        "rootSessionId": root_session_id,
        "childSessionId": child.id,
        "parentSessionId": child.parent_id,
        "depth": child.depth,
    });
    if let Some(title) = &child.title {
        value["title"] = json!(title);
    }
    match event {
        ChildEvent::Update { update } => {
            value["type"] = json!("update");
            value["update"] = serde_json::to_value(update.as_ref()).unwrap_or_else(|_| json!({}));
        }
        ChildEvent::Status { status, error } => {
            value["type"] = json!("status");
            value["status"] = json!(status);
            if let Some(error) = error {
                value["error"] = json!({ "type": error.r#type, "message": error.message });
            }
        }
    }
    value
}

/// `meta` (child.ts:49-58): the `_meta` tag for projected updates.
#[must_use]
pub fn meta(child: &ChildSession) -> Value {
    let mut value = json!({
        "id": child.id,
        "parentID": child.parent_id,
        "depth": child.depth,
    });
    if let Some(title) = &child.title {
        value["title"] = json!(title);
    }
    json!({ "opencode/child-session": value })
}

/// `toolCallID` (child.ts:60-62): child tool calls are namespaced by
/// their session id.
#[must_use]
pub fn tool_call_id(child: Option<&ChildSession>, id: &str) -> String {
    match child {
        Some(child) => format!("{}:{}", child.id, id),
        None => id.to_owned(),
    }
}

/// `prefixTitle` (child.ts:64-66).
#[must_use]
pub fn prefix_title(child: Option<&ChildSession>, value: &str) -> String {
    match child.and_then(|child| child.title.as_deref()) {
        Some(title) => format!("{title}: {value}"),
        None => value.to_owned(),
    }
}

/// `project` (child.ts:39-47): tag a session update with the child's
/// `_meta` and namespace its tool call ids and titles.
#[must_use]
pub fn project(mut update: SessionUpdate, child: &ChildSession) -> SessionUpdate {
    match &mut update {
        SessionUpdate::ToolCall(call) => {
            call.tool_call_id = tool_call_id(Some(child), call.tool_call_id.0.as_ref()).into();
            if !call.title.is_empty() {
                call.title = prefix_title(Some(child), &call.title);
            }
        }
        SessionUpdate::ToolCallUpdate(tool_call_update) => {
            project_update_fields(tool_call_update, child);
        }
        _ => {}
    }
    update
}

fn project_update_fields(tool_call_update: &mut ToolCallUpdate, child: &ChildSession) {
    tool_call_update.tool_call_id =
        tool_call_id(Some(child), tool_call_update.tool_call_id.0.as_ref()).into();
    if let Some(title) = tool_call_update.fields.title.take() {
        tool_call_update.fields.title = Some(prefix_title(Some(child), &title));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, TextContent};

    fn child() -> ChildSession {
        ChildSession {
            id: "ses_child".to_owned(),
            parent_id: "ses_root".to_owned(),
            depth: 1,
            title: Some("Researcher".to_owned()),
        }
    }

    #[test]
    fn update_params_carry_the_hierarchy() {
        let value =
            update("ses_root", &child(), &ChildEvent::Status { status: "running", error: None });
        assert_eq!(value["rootSessionId"], "ses_root");
        assert_eq!(value["childSessionId"], "ses_child");
        assert_eq!(value["parentSessionId"], "ses_root");
        assert_eq!(value["depth"], 1);
        assert_eq!(value["title"], "Researcher");
        assert_eq!(value["type"], "status");
        assert_eq!(value["status"], "running");
    }

    #[test]
    fn tool_call_ids_are_namespaced_and_titles_prefixed() {
        let chunk = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hi"),
        )));
        let _ = chunk; // shape check only
        let call = agent_client_protocol::schema::v1::ToolCall::new("tc_9", "Working");
        let projected = project(SessionUpdate::ToolCall(call), &child());
        let SessionUpdate::ToolCall(call) = projected else { panic!("tool call") };
        assert_eq!(
            call.tool_call_id,
            agent_client_protocol::schema::v1::ToolCallId::from("ses_child:tc_9")
        );
        assert_eq!(call.title, "Researcher: Working");
    }

    #[test]
    fn meta_tags_the_child_session() {
        assert_eq!(meta(&child())["opencode/child-session"]["id"], json!("ses_child"));
    }
}
