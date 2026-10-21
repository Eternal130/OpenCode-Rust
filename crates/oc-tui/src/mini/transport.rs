//! The session transport — port of
//! `packages/tui/src/mini/stream-v2.transport.ts` (the SSE→
//! presentation seam): subscribes `/api/event`, feeds permission/
//! form events into the footer machine, folds message-part events
//! into transcript [`Commit`]s, and submits prompts (`session.prompt`).
//!
//! The event vocabulary mirrors `oc-cli`'s noninteractive loop (the
//! same generated client + the same wire shapes).

#![allow(clippy::match_same_arms)] // the event arms mirror the transport switch
use oc_client::types::{PermissionReply, SessionPermissionReplyBody, SessionPromptBody};
use oc_client::Client;
use serde_json::Value;

use super::scrollback::{Commit, CommitKind, Phase};
use crate::components::footer::permission::PermissionRequest;
use crate::components::footer::{FormField, FormRequest};

/// A permission ask decoded from `permission.asked`.
fn decode_permission(data: &Value) -> PermissionRequest {
    let text = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    PermissionRequest {
        id: text("id"),
        session_id: text("sessionID"),
        action: text("action"),
        ..PermissionRequest::default()
    }
}

/// A form ask decoded from `form.created` (`data.form`).
fn decode_form(form: &Value) -> FormRequest {
    let text = |key: &str| form.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let fields = form
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .map(|field| {
                    FormField::text(
                        field.get("key").and_then(Value::as_str).unwrap_or_default(),
                        field
                            .get("title")
                            .or_else(|| field.get("label"))
                            .and_then(Value::as_str)
                            .unwrap_or("Field"),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    FormRequest { id: text("id"), session_id: text("sessionID"), title: text("title"), fields }
}

/// How one event folded (`createSessionTransport`'s event switch).
#[derive(Debug, Clone, PartialEq)]
pub enum Fold {
    /// Nothing to present (foreign session / heartbeat / ignored kind).
    Ignored,
    /// A permission ask for the footer.
    Permission(PermissionRequest),
    /// `permission.replied` — drop the ask.
    PermissionReplied(String),
    /// A form ask for the footer.
    Form(FormRequest),
    /// `form.replied` / `form.cancelled`.
    FormRemoved(String),
    /// Transcript rows to append.
    Commit(Commit),
    /// The running turn finished (`session.execution.*`).
    Idle,
}

/// The per-message text accumulators (`StreamState`: rendered text /
/// reasoning / tool titles by part).
#[derive(Default)]
pub struct TransportState {
    session_id: String,
    texts: std::collections::HashMap<String, String>,
    reasons: std::collections::HashMap<String, String>,
    tools: std::collections::HashMap<String, String>,
    running: bool,
    /// A terminal execution event arrived (`waitForIdle`'s signal).
    terminated: bool,
}

fn text_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn data_field(value: &Value) -> Value {
    value.get("data").cloned().unwrap_or(Value::Null)
}

impl TransportState {
    /// A transport bound to one session (events for other sessions
    /// fold to `Ignored`).
    #[must_use]
    pub fn new(session_id: &str) -> Self {
        Self { session_id: session_id.to_owned(), ..Self::default() }
    }

    /// Fold one SSE event (the `session.*` / `permission.*` /
    /// `form.*` switch; `stream-v2.transport.ts`).
    pub fn fold(&mut self, event: &Value) -> Fold {
        let event_type = text_field(event, "type");
        let data = data_field(event);
        match event_type.as_str() {
            "permission.asked" => return Fold::Permission(decode_permission(&data)),
            "permission.replied" => return Fold::PermissionReplied(text_field(&data, "requestID")),
            "form.created" => return Fold::Form(decode_form(data.get("form").unwrap_or(&data))),
            "form.replied" | "form.cancelled" => {
                return Fold::FormRemoved(text_field(&data, "formID"))
            }
            _ => {}
        }
        let session_id = text_field(&data, "sessionID");
        if !session_id.is_empty() && self.session_id != session_id {
            return Fold::Ignored;
        }
        self.fold_session(&event_type, &data)
    }

    /// The `session.*` half of the event switch.
    fn fold_session(&mut self, event_type: &str, data: &Value) -> Fold {
        match event_type {
            "session.text.started" => self.text_started(data),
            "session.text.delta" => self.text_delta(data),
            "session.text.ended" => self.text_ended(data),
            "session.reasoning.started" => {
                self.reasons.insert(part_key(data), String::new());
                Fold::Ignored
            }
            "session.reasoning.delta" => self.reasoning_delta(data),
            "session.reasoning.ended" => self.reasoning_ended(data),
            "session.tool.input.started" | "session.tool.called" => self.tool_started(data),
            "session.tool.input.delta" | "session.tool.progress" => Fold::Ignored,
            "session.tool.success" | "session.tool.failed" => self.tool_finished(event_type, data),
            "session.step.started" | "session.step.ended" | "session.step.failed" => Fold::Ignored,
            "session.execution.started" | "session.execution.resumed" => {
                self.running = true;
                self.terminated = false;
                Fold::Commit(Commit {
                    kind: CommitKind::System,
                    text: if event_type == "session.execution.resumed" {
                        "resumed".to_owned()
                    } else {
                        String::new()
                    },
                    phase: Phase::Progress,
                    ..Commit::default()
                })
            }
            "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted" => self.execution_ended(event_type, data),
            "session.inbox.delivered" => Fold::Ignored,
            _ => Fold::Ignored,
        }
    }

    fn text_started(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        self.texts.insert(key.clone(), String::new());
        Fold::Commit(Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Start,
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn text_delta(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let accumulated = self.texts.entry(key.clone()).or_default();
        accumulated.push_str(&text_field(data, "delta"));
        Fold::Commit(Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            text: accumulated.clone(),
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn text_ended(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let text = self.texts.remove(&key).unwrap_or_default();
        Fold::Commit(Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Final,
            text,
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn reasoning_delta(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let accumulated = self.reasons.entry(key.clone()).or_default();
        accumulated.push_str(&text_field(data, "delta"));
        Fold::Commit(Commit {
            kind: CommitKind::Reasoning,
            phase: Phase::Progress,
            text: accumulated.clone(),
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn reasoning_ended(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let text = self.reasons.remove(&key).unwrap_or_default();
        Fold::Commit(Commit {
            kind: CommitKind::Reasoning,
            phase: Phase::Final,
            text,
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn tool_started(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let caption = tool_caption(data);
        self.tools.insert(key.clone(), caption.clone());
        Fold::Commit(Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            text: caption,
            tool: Some(text_field(data, "tool")),
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn tool_finished(&mut self, event_type: &str, data: &Value) -> Fold {
        let key = part_key(data);
        let caption = self.tools.remove(&key).unwrap_or_else(|| tool_caption(data));
        let failed = event_type == "session.tool.failed";
        Fold::Commit(Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            text: caption,
            tool: Some(text_field(data, "tool")),
            tool_state: Some(if failed { "error" } else { "completed" }),
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn execution_ended(&mut self, event_type: &str, data: &Value) -> Fold {
        self.running = false;
        self.terminated = true;
        if event_type == "session.execution.failed" {
            let error = data.get("error").cloned().unwrap_or(Value::Null);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .map_or_else(|| "execution failed".to_owned(), str::to_owned);
            return Fold::Commit(Commit::error(&message));
        }
        if event_type == "session.execution.interrupted" {
            return Fold::Commit(Commit {
                kind: CommitKind::System,
                text: "interrupted".to_owned(),
                phase: Phase::Final,
                ..Commit::default()
            });
        }
        Fold::Idle
    }

    /// Is a turn mid-flight (the footer phase)?
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Did a terminal execution event arrive (`waitForIdle`)?
    #[must_use]
    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    /// A new turn begins (`runPromptTurn`): clear the terminal marker.
    pub fn begin_turn(&mut self) {
        self.terminated = false;
    }
}

/// The transport's session scope (events for other sessions fold to
/// `Ignored`).
pub struct Transport {
    pub session_id: String,
    pub state: TransportState,
}

impl Transport {
    #[must_use]
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_owned(),
            state: TransportState { session_id: session_id.to_owned(), ..Default::default() },
        }
    }
}

fn part_key(data: &Value) -> String {
    text_field(data, "partID")
}

fn message_id(data: &Value) -> Option<String> {
    let id = text_field(data, "messageID");
    (!id.is_empty()).then_some(id)
}

/// The mono tool caption (`tool.ts` caption subset): the human tool
/// name + a short input summary.
fn tool_caption(data: &Value) -> String {
    let tool = text_field(data, "tool");
    let input = data.get("input").cloned().unwrap_or(Value::Null);
    let detail = match tool.as_str() {
        "bash" => input.get("command").and_then(Value::as_str).map(first_line).unwrap_or_default(),
        "read" | "write" | "edit" | "multiedit" | "patch" => input
            .get("file")
            .or_else(|| input.get("filePath"))
            .and_then(Value::as_str)
            .map(|file| {
                std::path::Path::new(file)
                    .file_name()
                    .map_or_else(|| file.to_owned(), |name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default(),
        "grep" | "glob" | "list" => input
            .get("pattern")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("path"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default(),
        "task" | "subagent" => input
            .get("description")
            .or_else(|| input.get("prompt"))
            .and_then(Value::as_str)
            .map(first_line)
            .unwrap_or_default(),
        _ => input
            .get("title")
            .or_else(|| input.get("description"))
            .and_then(Value::as_str)
            .map(first_line)
            .unwrap_or_default(),
    };
    if detail.is_empty() {
        title_case(&tool)
    } else {
        format!("{} · {}", title_case(&tool), detail)
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Send one prompt turn (`runPromptTurn` → `session.prompt` with
/// `delivery` per the queue's admission, `stream-v2.transport.ts`).
pub async fn send_prompt(
    client: &Client,
    session_id: &str,
    message_id: &str,
    text: &str,
    delivery: Option<&str>,
) -> Result<(), String> {
    let body = SessionPromptBody {
        id: Some(message_id.to_owned()),
        text: text.to_owned(),
        delivery: delivery.map(|delivery| match delivery {
            "steer" => oc_client::types::SessionInboxDelivery::Steer,
            _ => oc_client::types::SessionInboxDelivery::Queue,
        }),
        ..SessionPromptBody::default()
    };
    client.session().prompt(session_id, &body).await.map(|_| ()).map_err(|error| error.to_string())
}

/// Reply to a permission ask (`onPermissionReply`,
/// `runtime.lifecycle.ts:267-274`).
pub async fn reply_permission(
    client: &Client,
    session_id: &str,
    request_id: &str,
    decision: &str,
    message: Option<String>,
) -> Result<(), String> {
    let body = SessionPermissionReplyBody {
        decision: match decision {
            "once" | "allow" => PermissionReply::Once,
            "always" => PermissionReply::Always,
            _ => PermissionReply::Reject,
        },
        message,
    };
    client
        .session()
        .permission_reply(session_id, request_id, &body)
        .await
        .map_err(|error| error.to_string())
}

/// Interrupt the active turn (`onInterrupt`, `runtime.ts:371-385`).
pub async fn interrupt(client: &Client, session_id: &str, resume: bool) -> Result<(), String> {
    client
        .session()
        .interrupt(
            session_id,
            &oc_client::types::SessionInterruptQuery {
                resume: Some(if resume {
                    oc_client::types::SessionInterruptQueryResume::True
                } else {
                    oc_client::types::SessionInterruptQueryResume::False
                }),
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Compact the session (`onCompact`, `runtime.ts:926-928`).
pub async fn compact(client: &Client, session_id: &str) -> Result<(), String> {
    client
        .session()
        .compact(session_id, &oc_client::types::SessionCompactBody::default())
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

// -- TUI-17: the subagent tracker (stream-v2.subagent.ts:16-19) ------
//
// Discovers child sessions of the parent from the live stream:
//   * `session.created` whose `parentID` is the parent (source 4 —
//     live events by parentID);
//   * the parent's subagent tool calls whose input metadata carries
//     a `sessionID` (source 1 — the projected tool output);
// and tracks one footer tab + detail transcript per child, reduced
// through the SAME per-session fold the parent uses. The session-list
// and active-map hydration sources (2/3) belong to reconnect
// hydration — the live port boots once and never reconnects.

/// `FooterSubagentTab["status"]` (mini/types.ts:302-310).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SubagentStatus {
    #[default]
    Running,
    Completed,
    Cancelled,
    Error,
}

/// One tracked child (`FooterSubagentTab` + its `FooterSubagentDetail`
/// commits — the detail transcript, newest 80).
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentChild {
    pub session_id: String,
    /// `Locale.titlecase(agent)` — "Subagent" until a title lands.
    pub label: String,
    /// `session.renamed`/`session.updated` titles.
    pub title: String,
    /// The task tool's description input.
    pub description: String,
    pub status: SubagentStatus,
    pub commits: Vec<Commit>,
}

/// `FALLBACK_LABEL` (stream-v2.subagent.ts).
const SUBAGENT_FALLBACK_LABEL: &str = "Subagent";

/// `FAMILY_LIST_LIMIT` (stream-v2.subagent.ts): the tracked-children
/// bound.
const FAMILY_LIST_LIMIT: usize = 50;

/// The detail transcript's row cap (the child's `frames` bound —
/// `CHILD_FRAME_LIMIT`, stream-v2.subagent.ts:15): the newest 80
/// commits survive.
pub const CHILD_COMMIT_LIMIT: usize = 80;

/// What one tracker fold produced (the redraw signals).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubagentFold {
    /// A child's tab/detail changed (the footer repaints).
    pub changed: bool,
    /// A NEW child was discovered (the statusline count changed).
    pub admitted: bool,
}

/// The per-child state bundle: the fold engine beside its tab.
struct ChildSlot {
    child: SubagentChild,
    state: TransportState,
}

/// The subagent tracker (`createSubagentTracker`): folds EVERY event
/// (parent AND foreign sessions) — parent events may carry child
/// metadata, foreign events may be known children.
pub struct SubagentTracker {
    parent: String,
    slots: Vec<ChildSlot>,
}

impl SubagentTracker {
    /// A tracker bound to one parent session.
    #[must_use]
    pub fn new(parent: &str) -> Self {
        Self { parent: parent.to_owned(), slots: Vec::new() }
    }

    /// The tracked children (discovery order — the footer's tab
    /// order).
    #[must_use]
    pub fn children(&self) -> Vec<SubagentChild> {
        self.slots.iter().map(|slot| slot.child.clone()).collect()
    }

    /// The running children count (the statusline's `N subagents`).
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.child.status == SubagentStatus::Running).count()
    }

    /// Fold one SSE event through the tracker.
    pub fn fold(&mut self, event: &Value) -> SubagentFold {
        let event_type = text_field(event, "type");
        let data = data_field(event);
        let session_id = text_field(&data, "sessionID");
        let mut fold = SubagentFold::default();

        // Source 4 — `session.created` with our parent: admit.
        if event_type == "session.created" && text_field(&data, "parentID") == self.parent {
            let id = session_id.clone();
            if self.admit(&id, text_field(&data, "title"), String::new()) {
                fold.admitted = true;
                fold.changed = true;
            }
            return fold;
        }
        // Source 1 — the parent's subagent tool calls carrying the
        // child's sessionID in the input metadata.
        if session_id == self.parent
            && matches!(
                event_type.as_str(),
                "session.tool.input.started" | "session.tool.called" | "session.tool.success"
            )
        {
            let child_id = data
                .pointer("/input/metadata/sessionID")
                .or_else(|| data.pointer("/metadata/sessionID"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !child_id.is_empty() && child_id != self.parent {
                let description = data
                    .pointer("/input/description")
                    .or_else(|| data.pointer("/input/prompt"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if self.admit(&child_id, String::new(), description) {
                    fold.admitted = true;
                    fold.changed = true;
                }
            }
        }

        // Known child events: the child's own transport fold + the
        // status transitions.
        if session_id.is_empty() || session_id == self.parent {
            return fold;
        }
        let Some(index) = self.slots.iter().position(|slot| slot.child.session_id == session_id)
        else {
            return fold;
        };
        match event_type.as_str() {
            "session.execution.started" | "session.execution.resumed" => {
                self.slots[index].child.status = SubagentStatus::Running;
                fold.changed = true;
            }
            "session.execution.succeeded" => {
                self.slots[index].child.status = SubagentStatus::Completed;
                fold.changed = true;
            }
            "session.execution.failed" => {
                self.slots[index].child.status = SubagentStatus::Error;
                fold.changed = true;
            }
            "session.execution.interrupted" => {
                self.slots[index].child.status = SubagentStatus::Cancelled;
                fold.changed = true;
            }
            "session.updated" | "session.renamed" => {
                let title = text_field(&data, "title");
                if !title.is_empty() && self.slots[index].child.title != title {
                    self.slots[index].child.title = title;
                    fold.changed = true;
                }
            }
            _ => {}
        }
        // The transcript fold (the same switch as the parent).
        let slot = &mut self.slots[index];
        match slot.state.fold(event) {
            Fold::Commit(commit) => {
                slot.child.commits.push(commit);
                let excess = slot.child.commits.len().saturating_sub(CHILD_COMMIT_LIMIT);
                if excess > 0 {
                    slot.child.commits.drain(0..excess);
                }
                fold.changed = true;
            }
            Fold::Ignored | Fold::Idle => {}
            Fold::Permission(_)
            | Fold::PermissionReplied(_)
            | Fold::Form(_)
            | Fold::FormRemoved(_) => {}
        }
        fold
    }

    /// `admitChild`: insert-or-refresh one child; `false` when the
    /// list is full or nothing changed.
    fn admit(&mut self, session_id: &str, title: String, description: String) -> bool {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.child.session_id == session_id) {
            let mut changed = false;
            if !title.is_empty() && slot.child.title != title {
                slot.child.title = title;
                changed = true;
            }
            if !description.is_empty() && slot.child.description != description {
                slot.child.description = description;
                changed = true;
            }
            return changed;
        }
        if self.slots.len() >= FAMILY_LIST_LIMIT {
            return false;
        }
        self.slots.push(ChildSlot {
            child: SubagentChild {
                session_id: session_id.to_owned(),
                label: SUBAGENT_FALLBACK_LABEL.to_owned(),
                title,
                description,
                status: SubagentStatus::Running,
                commits: Vec::new(),
            },
            state: TransportState::new(session_id),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(kind: &str, data: &Value) -> Value {
        json!({ "type": kind, "data": data })
    }

    #[test]
    fn text_deltas_accumulate_into_one_part_group() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let started = state.fold(&event(
            "session.text.started",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1" }),
        ));
        assert!(matches!(started, Fold::Commit(ref commit) if commit.phase == Phase::Start));
        let delta = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "delta": "Hello " }),
        ));
        match delta {
            Fold::Commit(commit) => assert_eq!(commit.text, "Hello "),
            other => panic!("delta folded: {other:?}"),
        }
        let delta = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "delta": "world" }),
        ));
        match delta {
            Fold::Commit(commit) => assert_eq!(commit.text, "Hello world"),
            other => panic!("delta folded: {other:?}"),
        }
        let ended = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1" }),
        ));
        match ended {
            Fold::Commit(commit) => assert_eq!(commit.phase, Phase::Final),
            other => panic!("ended folded: {other:?}"),
        }
        assert!(!state.is_running());
    }

    #[test]
    fn foreign_sessions_fold_to_ignored() {
        let mut state = TransportState { session_id: "mine".into(), ..Default::default() };
        let folded = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "other", "messageID": "m", "partID": "p", "delta": "x" }),
        ));
        assert_eq!(folded, Fold::Ignored);
    }

    #[test]
    fn permission_and_form_events_feed_the_footer() {
        let mut state = TransportState::default();
        let folded = state.fold(&event(
            "permission.asked",
            &json!({ "id": "prm_1", "sessionID": "s1", "action": "bash" }),
        ));
        match folded {
            Fold::Permission(request) => {
                assert_eq!(request.id, "prm_1");
                assert_eq!(request.action, "bash");
            }
            other => panic!("asked folded: {other:?}"),
        }
        let folded = state.fold(&event("permission.replied", &json!({ "requestID": "prm_1" })));
        assert_eq!(folded, Fold::PermissionReplied("prm_1".into()));
        let folded = state.fold(&event(
            "form.created",
            &json!({ "form": { "id": "frm_1", "sessionID": "s1", "title": "Question",
                     "fields": [{ "key": "q", "title": "What?" }] } }),
        ));
        match folded {
            Fold::Form(request) => {
                assert_eq!(request.id, "frm_1");
                assert_eq!(request.fields.len(), 1);
            }
            other => panic!("form folded: {other:?}"),
        }
    }

    #[test]
    fn tool_captions_carry_input_details() {
        let caption =
            tool_caption(&json!({ "tool": "bash", "input": { "command": "ls -la\nwc -l" } }));
        assert_eq!(caption, "Bash · ls -la");
        let caption = tool_caption(&json!({ "tool": "read", "input": { "file": "src/main.rs" } }));
        assert_eq!(caption, "Read · main.rs");
        let caption = tool_caption(&json!({ "tool": "grep", "input": { "pattern": "mini" } }));
        assert_eq!(caption, "Grep · mini");
        let caption = tool_caption(&json!({ "tool": "webfetch" }));
        assert_eq!(caption, "Webfetch");
    }

    #[test]
    fn execution_events_drive_the_running_phase() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        state.fold(&event("session.execution.started", &json!({ "sessionID": "s1" })));
        assert!(state.is_running());
        let folded = state.fold(&event(
            "session.execution.failed",
            &json!({ "sessionID": "s1", "error": { "message": "boom" } }),
        ));
        match folded {
            Fold::Commit(commit) => {
                assert_eq!(commit.kind, CommitKind::Error);
                assert_eq!(commit.text, "boom");
            }
            other => panic!("failed folded: {other:?}"),
        }
        assert!(!state.is_running());
    }
}
