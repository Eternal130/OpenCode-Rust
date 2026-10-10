//! The event → `session/update` fold (port of `acp/translate.ts`): one
//! pure function turns the opencode SSE event stream of a turn into
//! ACP session updates, permission/form asks and a terminal result.

use std::collections::{HashMap, HashSet};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, PromptResponse, SessionUpdate, StopReason, TextContent,
};
use serde_json::{json, Map, Value};

use crate::acp::child::{self, ChildEvent, ChildSession, SessionError as ChildError};
use crate::acp::compaction::{self, Started};
use crate::acp::error::AcpError;
use crate::acp::tool::{self};

/// `RetryMeta` (translate.ts:12).
pub const RETRY_META: &str = "opencode/retry";

/// `TurnStart` (translate.ts:14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnStart {
    pub kind: TurnStartKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStartKind {
    Input,
    Compaction,
}

/// `Terminal` (translate.ts:16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    Succeeded,
    Failed,
    Interrupted,
}

/// `TurnContext` (translate.ts:18-24).
#[derive(Debug, Clone)]
pub struct TurnContext {
    pub session_id: String,
    pub cwd: String,
    pub start: TurnStart,
    pub child_updates: bool,
    pub compaction: bool,
}

/// One tracked tool call (translate.ts:26-32).
#[derive(Debug, Clone)]
pub struct Tool {
    pub session_id: String,
    pub id: String,
    pub name: String,
    pub input: Map<String, Value>,
    pub metadata: Map<String, Value>,
}

/// `RetryStatus` (translate.ts:34-38).
#[derive(Debug, Clone, PartialEq)]
pub struct RetryStatus {
    pub attempt: i64,
    pub next_retry_at: String,
    pub error: Map<String, Value>,
}

/// Token usage accumulator (`TokenUsage.Info`, recordStep translate.ts:426-442).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TokenUsage {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl TokenUsage {
    fn from_value(value: &Value) -> Self {
        Self {
            input: number(value, "input"),
            output: number(value, "output"),
            reasoning: number(value, "reasoning"),
            cache_read: value.pointer("/cache/read").and_then(Value::as_f64).unwrap_or(0.0),
            cache_write: value.pointer("/cache/write").and_then(Value::as_f64).unwrap_or(0.0),
        }
    }

    /// Turn token total (input+output+reasoning+cache). Token counts
    /// are whole numbers on the wire; the truncation mirrors JS's
    /// `Number → BigInt` cut at the usage update (turn.ts:323).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn total(&self) -> u64 {
        (self.input + self.output + self.reasoning + self.cache_read + self.cache_write) as u64
    }
}

/// `TurnState` (translate.ts:40-52).
#[derive(Debug, Clone, Default)]
pub struct TurnState {
    pub started: bool,
    pub tools: HashMap<String, Tool>,
    pub retries: HashMap<String, RetryStatus>,
    pub compactions: HashMap<String, Started>,
    pub children: HashMap<String, ChildSession>,
    pub open_children: HashSet<String>,
    pub asks: HashSet<String>,
    pub finish: Option<String>,
    pub usage: Option<(TokenUsage, TokenUsage)>,
    pub step_error: Option<Map<String, Value>>,
    pub execution_error: Option<Map<String, Value>>,
}

/// `Output` (translate.ts:58-68).
#[derive(Debug, Clone)]
pub enum Output {
    SessionUpdate(SessionUpdate),
    ChildUpdate(Value),
    PermissionAsk {
        /// The raw `permission.asked` data.
        data: Value,
        /// The tracked tool call the ask belongs to, if any.
        tool: Option<Tool>,
        child: Option<ChildSession>,
    },
    FormAsk {
        /// The raw `form.created` data (`{form: {...}}`).
        data: Value,
        child: Option<ChildSession>,
    },
    AskSettled {
        id: String,
    },
}

/// `Folded` (translate.ts:70-74).
#[derive(Debug, Clone, Default)]
pub struct Folded {
    pub outputs: Vec<Output>,
    pub terminal: Option<Terminal>,
    /// The next turn state (identity when the fold produced none).
    state: Option<TurnState>,
}

/// One bus event off the SSE feed (the wire envelope's `type` + `data`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub type_: String,
    pub data: Value,
}

impl Event {
    #[must_use]
    pub fn new(type_: &str, data: Value) -> Self {
        Self { type_: type_.to_owned(), data }
    }

    fn session_id(&self) -> Option<&str> {
        // sessionIDFromEvent (translate.ts:450-454).
        if let Some(id) = self.data.get("sessionID").and_then(Value::as_str) {
            return Some(id);
        }
        if self.type_ == "form.created" {
            return self.data.pointer("/form/sessionID").and_then(Value::as_str);
        }
        None
    }
}

/// `fold` (translate.ts:86-123).
#[must_use]
pub fn fold(state: &TurnState, event: &Event, ctx: &TurnContext) -> Folded {
    if event.type_ == "session.created" {
        return child_created(state, event, ctx);
    }

    let session_id = event.session_id();
    let child = session_id.and_then(|id| state.children.get(id)).cloned();

    if event.type_ == "permission.asked"
        && (event.data.get("sessionID").and_then(Value::as_str) == Some(ctx.session_id.as_str())
            || child.is_some())
    {
        let mut next = state.clone();
        next.asks.insert(string(&event.data, "id"));
        let tool = event
            .data
            .pointer("/source/id")
            .and_then(Value::as_str)
            .and_then(|tool_id| {
                state
                    .tools
                    .get(&tool_key(event.data.get("sessionID").and_then(Value::as_str)?, tool_id))
            })
            .cloned();
        return Folded {
            outputs: vec![Output::PermissionAsk { data: event.data.clone(), tool, child }],
            terminal: None,
            ..Folded::default()
        }
        .with_state(next);
    }
    if event.type_ == "form.created"
        && (event.data.pointer("/form/sessionID").and_then(Value::as_str)
            == Some(ctx.session_id.as_str())
            || child.is_some())
    {
        let mut next = state.clone();
        if let Some(form_id) = string_opt(event.data.pointer("/form/id")) {
            next.asks.insert(form_id);
        }
        return Folded {
            outputs: vec![Output::FormAsk { data: event.data.clone(), child }],
            terminal: None,
            ..Folded::default()
        }
        .with_state(next);
    }
    let settled_id = match event.type_.as_str() {
        "permission.replied" => string_opt(event.data.get("requestID")),
        "form.replied" | "form.cancelled" => string_opt(event.data.get("id")),
        _ => None,
    };
    if let Some(settled_id) = settled_id {
        if state.asks.contains(&settled_id) {
            let mut next = state.clone();
            next.asks.remove(&settled_id);
            return Folded {
                outputs: vec![Output::AskSettled { id: settled_id }],
                terminal: None,
                ..Folded::default()
            }
            .with_state(next);
        }
    }
    let Some(session_id) = session_id else {
        return Folded { outputs: Vec::new(), terminal: None, ..Folded::default() }
            .with_state(state.clone());
    };
    if session_id != ctx.session_id && child.is_none() {
        return Folded { outputs: Vec::new(), terminal: None, ..Folded::default() }
            .with_state(state.clone());
    }
    if event.type_ == "session.inbox.delivered" && string(&event.data, "inboxID") == ctx.start.id {
        let mut next = state.clone();
        next.started = true;
        return Folded::default().with_state(next);
    }
    if !state.started {
        return Folded::default().with_state(state.clone());
    }
    match child {
        Some(child) => child_event(state, event, ctx, &child),
        None => root_event(state, event, ctx),
    }
}

impl Folded {
    /// Attach the next state (fold plumbing, also used by the turn
    /// consumer's background mode).
    pub fn with_state(mut self, state: TurnState) -> Self {
        self.state = Some(state);
        self
    }

    /// The next turn state (default when the fold produced none).
    #[must_use]
    pub fn state(&self) -> TurnState {
        self.state.clone().unwrap_or_default()
    }
}

/// `fromTrackedChild` (translate.ts:125-128): does this event belong to
/// a child this state tracks?
#[must_use]
pub fn from_tracked_child(state: &TurnState, event: &Event) -> bool {
    let session_id = if event.type_ == "session.created" {
        event.data.get("parentID").and_then(Value::as_str)
    } else {
        event.data.get("sessionID").and_then(Value::as_str)
    };
    session_id.is_some_and(|id| state.children.contains_key(id))
}

/// `failure` (translate.ts:130-141).
#[must_use]
pub fn failure(state: &TurnState) -> Option<AcpError> {
    let error = state.step_error.as_ref().or(state.execution_error.as_ref())?;
    let error_type = string_in(error, "type");
    if error_type == "provider.auth" {
        return Some(AcpError::AuthRequired);
    }
    if error_type != "aborted" && error_type != "provider.content-filter" {
        return Some(AcpError::ServiceFailure {
            safe_message: {
                let message = string_in(error, "message");
                if message.is_empty() {
                    "OpenCode prompt failed".to_owned()
                } else {
                    message
                }
            },
            service: Some("session".to_owned()),
            error_name: Some(error_type),
        });
    }
    None
}

/// `response` (translate.ts:143-160). Token usage needs the SDK's
/// `unstable_end_turn_token_usage` field (not enabled in this build), so
/// turn usage rides the `usage_update` session notification instead
/// (turn.ts:321-343).
#[must_use]
pub fn response(state: &TurnState, session_id: &str, terminal: Terminal) -> PromptResponse {
    let error = state
        .step_error
        .as_ref()
        .or(state.execution_error.as_ref())
        .map(|error| string_in(error, "type"));
    let stop_reason = resolve_stop_reason(terminal, state.finish.as_deref(), error.as_deref());
    let mut response = PromptResponse::new(stop_reason);
    // Interruption clears the projected retry, so a retry pending at
    // interrupt is reported here (translate.ts:157-159).
    if let Some(retry) = state.retries.get(session_id) {
        let mut meta = serde_json::Map::new();
        meta.insert(
            RETRY_META.to_owned(),
            json!({
                "attempt": retry.attempt,
                "nextRetryAt": retry.next_retry_at,
                "error": Value::Object(retry.error.clone()),
            }),
        );
        response.meta = Some(meta);
    }
    response
}

/// `abandon` (translate.ts:163-187): fail every tracked tool and the
/// root compaction.
#[must_use]
pub fn abandon(state: &TurnState, ctx: &TurnContext) -> (TurnState, Vec<Output>) {
    let mut outputs = Vec::new();
    for tracked in state.tools.values() {
        outputs.extend(route(
            ctx,
            state.children.get(&tracked.session_id),
            SessionUpdate::ToolCallUpdate(tool::error_tool_update(
                &tracked.id,
                &tracked.name,
                &tracked.input,
                None,
                Some(&tracked.metadata),
                "Cancelled",
                &ctx.cwd,
                None,
            )),
        ));
    }
    if let Some(compaction_started) = state.compactions.get(&ctx.session_id) {
        outputs.extend(route(
            ctx,
            None,
            compaction::abandon(
                compaction_started,
                compaction::uses_standard_updates(ctx.compaction, ctx.child_updates, false),
            ),
        ));
    }
    let mut next = state.clone();
    next.tools.clear();
    next.compactions.remove(&ctx.session_id);
    (next, outputs)
}

/// `reasoningMessageID` (translate.ts:189-191).
#[must_use]
pub fn reasoning_message_id(message_id: &str, ordinal: u64) -> String {
    format!("{message_id}:reasoning:{ordinal}")
}

/// `childCreated` (translate.ts:193-207).
fn child_created(state: &TurnState, event: &Event, ctx: &TurnContext) -> Folded {
    let Some(parent_id) = event.data.get("parentID").and_then(Value::as_str) else {
        return Folded::default().with_state(state.clone());
    };
    let parent =
        if parent_id == ctx.session_id { None } else { state.children.get(parent_id).cloned() };
    if parent.is_none() && parent_id != ctx.session_id {
        return Folded::default().with_state(state.clone());
    }
    let child = ChildSession {
        id: string(&event.data, "sessionID"),
        parent_id: parent_id.to_owned(),
        depth: parent.as_ref().map_or(1, |parent| parent.depth + 1),
        title: optional(&event.data, "title"),
    };
    let mut next = state.clone();
    next.children.insert(child.id.clone(), child.clone());
    next.open_children.insert(child.id.clone());
    Folded {
        outputs: child_status(ctx, &child, &ChildEvent::Status { status: "created", error: None }),
        terminal: None,
        ..Folded::default()
    }
    .with_state(next)
}

/// `rootEvent` (translate.ts:209-228).
fn root_event(state: &TurnState, event: &Event, ctx: &TurnContext) -> Folded {
    match event.type_.as_str() {
        "session.step.started" => {
            let mut next = state.clone();
            next.step_error = None;
            session_event(&next, event, ctx, None)
        }
        "session.step.ended" => {
            let mut next = state.clone();
            next.finish = optional(&event.data, "finish");
            next.usage = Some(record_step(state.usage, event.data.get("tokens")));
            Folded::default().with_state(next)
        }
        "session.step.failed" => {
            let mut next = state.clone();
            if let Some(tokens) = event.data.get("tokens").filter(|tokens| !tokens.is_null()) {
                next.usage = Some(record_step(state.usage, Some(tokens)));
            }
            next.step_error = error_object(&event.data);
            Folded::default().with_state(next)
        }
        "session.execution.succeeded" => {
            Folded { terminal: Some(Terminal::Succeeded), ..Folded::default() }
                .with_state(state.clone())
        }
        "session.execution.interrupted" => {
            Folded { terminal: Some(Terminal::Interrupted), ..Folded::default() }
                .with_state(state.clone())
        }
        "session.execution.failed" => {
            let mut next = state.clone();
            next.execution_error = error_object(&event.data);
            Folded { terminal: Some(Terminal::Failed), ..Folded::default() }.with_state(next)
        }
        _ => session_event(state, event, ctx, None),
    }
}

/// `childEvent` (translate.ts:230-243).
fn child_event(
    state: &TurnState,
    event: &Event,
    ctx: &TurnContext,
    child: &ChildSession,
) -> Folded {
    match event.type_.as_str() {
        "session.execution.started" => Folded {
            outputs: child_status(
                ctx,
                child,
                &ChildEvent::Status { status: "running", error: None },
            ),
            ..Folded::default()
        }
        .with_state(state.clone()),
        "session.execution.succeeded" => {
            child_ended(state, ctx, child, &ChildEvent::Status { status: "completed", error: None })
        }
        "session.execution.interrupted" => child_ended(
            state,
            ctx,
            child,
            &ChildEvent::Status { status: "interrupted", error: None },
        ),
        "session.execution.failed" => child_ended(
            state,
            ctx,
            child,
            &ChildEvent::Status {
                status: "failed",
                error: event.data.get("error").map(|error| ChildError {
                    r#type: string(error, "type"),
                    message: string(error, "message"),
                }),
            },
        ),
        _ => session_event(state, event, ctx, Some(child)),
    }
}

fn child_ended(
    state: &TurnState,
    ctx: &TurnContext,
    child: &ChildSession,
    status: &ChildEvent,
) -> Folded {
    let mut next = state.clone();
    next.open_children.remove(&child.id);
    Folded { outputs: child_status(ctx, child, status), ..Folded::default() }.with_state(next)
}

/// `sessionEvent` (translate.ts:245-402). The mechanical event-by-event
/// switch mirrors the reference 1:1.
#[allow(clippy::too_many_lines)]
fn session_event(
    state: &TurnState,
    event: &Event,
    ctx: &TurnContext,
    child: Option<&ChildSession>,
) -> Folded {
    let session_id = child.map_or_else(|| ctx.session_id.clone(), |child| child.id.clone());
    let send = |update: SessionUpdate| route(ctx, child, update);
    match event.type_.as_str() {
        "session.step.started" => {
            if !state.retries.contains_key(&session_id) {
                return Folded::default().with_state(state.clone());
            }
            let mut next = state.clone();
            next.retries.remove(&session_id);
            let mut meta = serde_json::Map::new();
            meta.insert(RETRY_META.to_owned(), Value::Null);
            let update = info_update_with_meta(meta);
            Folded { outputs: send(update), ..Folded::default() }.with_state(next)
        }
        "session.retry.scheduled" => {
            let retry = RetryStatus {
                attempt: event.data.get("attempt").and_then(Value::as_i64).unwrap_or_default(),
                next_retry_at: string(&event.data, "at"),
                error: event
                    .data
                    .get("error")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default(),
            };
            let mut meta = serde_json::Map::new();
            meta.insert(
                RETRY_META.to_owned(),
                json!({
                    "attempt": retry.attempt,
                    "nextRetryAt": retry.next_retry_at,
                    "error": Value::Object(retry.error.clone()),
                }),
            );
            let mut next = state.clone();
            next.retries.insert(session_id.clone(), retry);
            Folded { outputs: send(info_update_with_meta(meta)), ..Folded::default() }
                .with_state(next)
        }
        "session.compaction.started" | "session.compaction.ended" | "session.compaction.failed" => {
            let mut next = state.clone();
            let updates = compaction::apply(
                &event.type_,
                &session_id,
                &event.data,
                &string(&event.data, "id"),
                &mut next.compactions,
                compaction::uses_standard_updates(
                    ctx.compaction,
                    ctx.child_updates,
                    child.is_some(),
                ),
            );
            Folded { outputs: updates.into_iter().flat_map(send).collect(), ..Folded::default() }
                .with_state(next)
        }
        "session.compaction.delta" => {
            let update = compaction::chunk(
                state.compactions.get(&session_id),
                &string_or(event.data.get("text"), ""),
                compaction::uses_standard_updates(
                    ctx.compaction,
                    ctx.child_updates,
                    child.is_some(),
                ),
            );
            let outputs = update.map_or_else(Vec::new, send);
            Folded { outputs, ..Folded::default() }.with_state(state.clone())
        }
        "session.text.delta" => Folded {
            outputs: send(SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(string(
                    &event.data,
                    "delta",
                ))))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(
                    string(&event.data, "assistantMessageID"),
                )),
            )),
            ..Folded::default()
        }
        .with_state(state.clone()),
        "session.reasoning.delta" => Folded {
            outputs: send(SessionUpdate::AgentThoughtChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(string(
                    &event.data,
                    "delta",
                ))))
                .message_id(agent_client_protocol::schema::v1::MessageId::new(
                    reasoning_message_id(
                        &string(&event.data, "assistantMessageID"),
                        event.data.get("ordinal").and_then(Value::as_u64).unwrap_or_default(),
                    ),
                )),
            )),
            ..Folded::default()
        }
        .with_state(state.clone()),
        "session.tool.input.started" => {
            let tool = Tool {
                session_id: session_id.clone(),
                id: string(&event.data, "id"),
                name: string(&event.data, "name"),
                input: Map::new(),
                metadata: Map::new(),
            };
            let mut next = state.clone();
            next.tools.insert(tool_key(&tool.session_id, &tool.id), tool.clone());
            let update = pending_update(ctx, child, &tool, None);
            Folded { outputs: send(SessionUpdate::ToolCall(update)), ..Folded::default() }
                .with_state(next)
        }
        "session.tool.called" => {
            let key = tool_key(&session_id, &string(&event.data, "id"));
            let mut tool = state.tools.get(&key).cloned().unwrap_or_else(|| Tool {
                session_id: session_id.clone(),
                id: string(&event.data, "id"),
                name: "tool".to_owned(),
                input: Map::new(),
                metadata: Map::new(),
            });
            tool.input =
                event.data.get("input").and_then(Value::as_object).cloned().unwrap_or_default();
            let update =
                tool::running_tool_update(&tool.id, &tool.name, &tool.input, &ctx.cwd, None);
            let mut next = state.clone();
            next.tools.insert(key, tool);
            Folded { outputs: send(SessionUpdate::ToolCallUpdate(update)), ..Folded::default() }
                .with_state(next)
        }
        "session.tool.progress" => {
            let key = tool_key(&session_id, &string(&event.data, "id"));
            let Some(current) = state.tools.get(&key).cloned() else {
                return Folded::default().with_state(state.clone());
            };
            let mut tool = current.clone();
            tool.metadata =
                event.data.get("metadata").and_then(Value::as_object).cloned().unwrap_or_default();
            let update =
                tool::running_tool_update(&tool.id, &tool.name, &tool.input, &ctx.cwd, None);
            let mut next = state.clone();
            next.tools.insert(key, tool);
            Folded { outputs: send(SessionUpdate::ToolCallUpdate(update)), ..Folded::default() }
                .with_state(next)
        }
        "session.tool.success" => {
            let key = tool_key(&session_id, &string(&event.data, "id"));
            let tool = state.tools.get(&key).cloned().unwrap_or_else(|| Tool {
                session_id: session_id.clone(),
                id: string(&event.data, "id"),
                name: "tool".to_owned(),
                input: Map::new(),
                metadata: Map::new(),
            });
            let content =
                event.data.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
            let update = tool::completed_tool_update(
                &tool.id,
                &tool.name,
                &tool.input,
                &content,
                event.data.get("metadata").and_then(Value::as_object),
                &ctx.cwd,
            );
            let mut next = state.clone();
            next.tools.remove(&key);
            Folded { outputs: send(SessionUpdate::ToolCallUpdate(update)), ..Folded::default() }
                .with_state(next)
        }
        "session.tool.failed" => {
            let key = tool_key(&session_id, &string(&event.data, "id"));
            let fallback = Tool {
                session_id: session_id.clone(),
                id: string(&event.data, "id"),
                name: "tool".to_owned(),
                input: Map::new(),
                metadata: Map::new(),
            };
            let tool = state.tools.get(&key).cloned().unwrap_or(fallback);
            let content =
                event.data.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
            let update = tool::error_tool_update(
                &tool.id,
                &tool.name,
                &tool.input,
                Some(&content),
                event.data.get("metadata").and_then(Value::as_object).or(Some(&tool.metadata)),
                &string_or(event.data.pointer("/error/message"), "tool failed"),
                &ctx.cwd,
                None,
            );
            let mut next = state.clone();
            next.tools.remove(&key);
            Folded { outputs: send(SessionUpdate::ToolCallUpdate(update)), ..Folded::default() }
                .with_state(next)
        }
        _ => Folded::default().with_state(state.clone()),
    }
}

fn pending_update(
    ctx: &TurnContext,
    child: Option<&ChildSession>,
    tool: &Tool,
    title: Option<&str>,
) -> agent_client_protocol::schema::v1::ToolCall {
    let id = child::tool_call_id(child, &tool.id);
    tool::pending_tool_call(&id, &tool.name, &tool.input, title, &ctx.cwd, None)
}

/// `route` (translate.ts:408-413).
fn route(ctx: &TurnContext, child: Option<&ChildSession>, update: SessionUpdate) -> Vec<Output> {
    let Some(child) = child else {
        return vec![Output::SessionUpdate(update)];
    };
    let projected = child::project(update, child);
    if ctx.child_updates {
        return child_status(ctx, child, &ChildEvent::Update { update: Box::new(projected) });
    }
    vec![Output::SessionUpdate(projected)]
}

/// `childStatus` (translate.ts:415-418).
fn child_status(ctx: &TurnContext, child: &ChildSession, event: &ChildEvent) -> Vec<Output> {
    if !ctx.child_updates {
        return Vec::new();
    }
    vec![Output::ChildUpdate(child::update(&ctx.session_id, child, event))]
}

/// `recordStep` (translate.ts:426-442).
fn record_step(
    usage: Option<(TokenUsage, TokenUsage)>,
    tokens: Option<&Value>,
) -> (TokenUsage, TokenUsage) {
    let Some(tokens) = tokens else { return usage.unwrap_or_default() };
    let tokens = TokenUsage::from_value(tokens);
    match usage {
        Some((turn, _)) => (
            TokenUsage {
                input: turn.input + tokens.input,
                output: turn.output + tokens.output,
                reasoning: turn.reasoning + tokens.reasoning,
                cache_read: turn.cache_read + tokens.cache_read,
                cache_write: turn.cache_write + tokens.cache_write,
            },
            tokens,
        ),
        None => (tokens, tokens),
    }
}

/// `resolveStopReason` (translate.ts:460-469).
fn resolve_stop_reason(
    terminal: Terminal,
    finish: Option<&str>,
    error: Option<&str>,
) -> StopReason {
    if terminal == Terminal::Interrupted || error == Some("aborted") {
        return StopReason::Cancelled;
    }
    if finish == Some("length") {
        return StopReason::MaxTokens;
    }
    if finish == Some("content-filter") || error == Some("provider.content-filter") {
        return StopReason::Refusal;
    }
    StopReason::EndTurn
}

fn tool_key(session_id: &str, id: &str) -> String {
    format!("{session_id}:{id}")
}

fn info_update_with_meta(meta: serde_json::Map<String, Value>) -> SessionUpdate {
    SessionUpdate::SessionInfoUpdate(
        agent_client_protocol::schema::v1::SessionInfoUpdate::new().meta(meta),
    )
}

fn error_object(data: &Value) -> Option<Map<String, Value>> {
    data.get("error").and_then(Value::as_object).cloned()
}

fn string(data: &Value, field: &str) -> String {
    string_or(data.get(field), "")
}

fn string_opt(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn string_or(value: Option<&Value>, fallback: &str) -> String {
    value.and_then(Value::as_str).unwrap_or(fallback).to_owned()
}

fn optional(data: &Value, field: &str) -> Option<String> {
    data.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn string_in(map: &Map<String, Value>, field: &str) -> String {
    map.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn number(value: &Value, field: &str) -> f64 {
    value.get(field).and_then(Value::as_f64).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TurnContext {
        TurnContext {
            session_id: "ses_root".to_owned(),
            cwd: "/w".to_owned(),
            start: TurnStart { kind: TurnStartKind::Input, id: "msg_start".to_owned() },
            child_updates: false,
            compaction: false,
        }
    }

    fn started(state: &TurnState) -> TurnState {
        let mut state = state.clone();
        state.started = true;
        state
    }

    #[test]
    fn deltas_stream_message_and_thought_chunks() {
        let state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new(
                "session.text.delta",
                json!({ "sessionID": "ses_root", "assistantMessageID": "msg_1", "delta": "Hi" }),
            ),
            &ctx(),
        );
        assert!(matches!(
            folded.outputs[0],
            Output::SessionUpdate(SessionUpdate::AgentMessageChunk(_))
        ));
        let folded = fold(
            &state,
            &Event::new(
                "session.reasoning.delta",
                json!({ "sessionID": "ses_root", "assistantMessageID": "msg_1", "ordinal": 2, "delta": "hm" }),
            ),
            &ctx(),
        );
        let Output::SessionUpdate(SessionUpdate::AgentThoughtChunk(chunk)) = &folded.outputs[0]
        else {
            panic!("thought chunk");
        };
        assert_eq!(chunk.message_id.as_ref().map(|id| id.0.as_ref()), Some("msg_1:reasoning:2"));
    }

    #[test]
    fn events_before_delivery_are_ignored() {
        let state = TurnState::default();
        let folded = fold(
            &state,
            &Event::new(
                "session.text.delta",
                json!({ "sessionID": "ses_root", "assistantMessageID": "msg_1", "delta": "Hi" }),
            ),
            &ctx(),
        );
        assert_eq!(folded.outputs.len(), 0);
        let folded = fold(
            &state,
            &Event::new(
                "session.inbox.delivered",
                json!({ "sessionID": "ses_root", "inboxID": "msg_start" }),
            ),
            &ctx(),
        );
        assert!(folded.state().started);
    }

    #[test]
    fn other_sessions_are_ignored() {
        let state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new(
                "session.text.delta",
                json!({ "sessionID": "ses_other", "assistantMessageID": "msg_1", "delta": "Hi" }),
            ),
            &ctx(),
        );
        assert_eq!(folded.outputs.len(), 0);
    }

    #[test]
    fn tool_lifecycle_streams_pending_running_completed() {
        let state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new(
                "session.tool.input.started",
                json!({ "sessionID": "ses_root", "id": "tsk_1", "name": "shell" }),
            ),
            &ctx(),
        );
        let Output::SessionUpdate(SessionUpdate::ToolCall(call)) = folded.outputs[0].clone() else {
            panic!("tool call");
        };
        assert_eq!(call.title, "shell");
        assert!(folded.state().tools.contains_key("ses_root:tsk_1"));

        let state = folded.state();
        let folded = fold(
            &state,
            &Event::new(
                "session.tool.called",
                json!({ "sessionID": "ses_root", "id": "tsk_1", "input": { "command": "ls" } }),
            ),
            &ctx(),
        );
        let Output::SessionUpdate(SessionUpdate::ToolCallUpdate(update)) = &folded.outputs[0]
        else {
            panic!("tool update");
        };
        assert_eq!(update.fields.title, Some("ls".to_owned()));
        assert_eq!(update.fields.raw_input, Some(json!({ "command": "ls", "cwd": "/w" })));

        let state = folded.state();
        let folded = fold(
            &state,
            &Event::new(
                "session.tool.success",
                json!({
                    "sessionID": "ses_root",
                    "id": "tsk_1",
                    "content": [{ "type": "text", "text": "ok" }]
                }),
            ),
            &ctx(),
        );
        let Output::SessionUpdate(SessionUpdate::ToolCallUpdate(update)) = &folded.outputs[0]
        else {
            panic!("tool update");
        };
        assert_eq!(
            update.fields.status,
            Some(agent_client_protocol::schema::v1::ToolCallStatus::Completed)
        );
        assert_eq!(folded.state().tools.len(), 0);
    }

    #[test]
    fn terminal_events_end_the_turn() {
        let state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new("session.execution.succeeded", json!({ "sessionID": "ses_root" })),
            &ctx(),
        );
        assert_eq!(folded.terminal, Some(Terminal::Succeeded));
        let folded = fold(
            &state,
            &Event::new("session.execution.interrupted", json!({ "sessionID": "ses_root" })),
            &ctx(),
        );
        assert_eq!(folded.terminal, Some(Terminal::Interrupted));
    }

    #[test]
    fn usage_accumulates_over_steps() {
        let mut state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new(
                "session.step.ended",
                json!({ "sessionID": "ses_root", "finish": "stop", "tokens": {
                    "input": 10, "output": 5, "reasoning": 1,
                    "cache": { "read": 2, "write": 3 }
                }}),
            ),
            &ctx(),
        );
        state = folded.state();
        let (turn, last) = state.usage.expect("usage");
        assert_eq!(turn.input, 10.0);
        assert_eq!(last.output, 5.0);
        let folded = fold(
            &state,
            &Event::new(
                "session.step.ended",
                json!({ "sessionID": "ses_root", "finish": "stop", "tokens": {
                    "input": 1, "output": 1, "reasoning": 0,
                    "cache": { "read": 0, "write": 0 }
                }}),
            ),
            &ctx(),
        );
        let (turn, _) = folded.state().usage.expect("usage");
        assert_eq!(turn.input, 11.0);
        assert_eq!(turn.total(), 23);
    }

    #[test]
    fn stop_reasons_follow_v2_mapping() {
        assert_eq!(resolve_stop_reason(Terminal::Interrupted, None, None), StopReason::Cancelled);
        assert_eq!(
            resolve_stop_reason(Terminal::Succeeded, Some("length"), None),
            StopReason::MaxTokens
        );
        assert_eq!(
            resolve_stop_reason(Terminal::Failed, None, Some("provider.content-filter")),
            StopReason::Refusal
        );
        assert_eq!(
            resolve_stop_reason(Terminal::Succeeded, Some("stop"), None),
            StopReason::EndTurn
        );
    }

    #[test]
    fn provider_auth_failures_map_to_auth_required() {
        let state = TurnState {
            step_error: json!({ "type": "provider.auth", "message": "no key" })
                .as_object()
                .cloned(),
            ..TurnState::default()
        };
        assert!(matches!(failure(&state), Some(AcpError::AuthRequired)));
    }

    #[test]
    fn aborted_errors_do_not_fail_the_prompt() {
        let state = TurnState {
            execution_error: json!({ "type": "aborted", "message": "x" }).as_object().cloned(),
            ..TurnState::default()
        };
        assert!(failure(&state).is_none());
    }

    #[test]
    fn permission_asks_carry_the_tracked_tool() {
        let mut state = started(&TurnState::default());
        state.tools.insert(
            "ses_root:tsk_1".to_owned(),
            Tool {
                session_id: "ses_root".to_owned(),
                id: "tsk_1".to_owned(),
                name: "edit".to_owned(),
                input: json!({ "path": "a" }).as_object().cloned().unwrap(),
                metadata: Map::new(),
            },
        );
        let folded = fold(
            &state,
            &Event::new(
                "permission.asked",
                json!({ "id": "per_1", "sessionID": "ses_root", "action": "edit",
                        "resources": ["a"], "source": { "id": "tsk_1" } }),
            ),
            &ctx(),
        );
        let Output::PermissionAsk { tool, .. } = &folded.outputs[0] else {
            panic!("permission ask");
        };
        assert_eq!(tool.as_ref().expect("tool").name, "edit");
        assert!(folded.state().asks.contains("per_1"));

        let settled = fold(
            &folded.state(),
            &Event::new(
                "permission.replied",
                json!({ "sessionID": "ses_root", "requestID": "per_1" }),
            ),
            &ctx(),
        );
        assert!(matches!(settled.outputs[0], Output::AskSettled { .. }));
        assert_eq!(settled.state().asks.len(), 0);
    }

    #[test]
    fn children_route_through_projected_updates() {
        let ctx = TurnContext { child_updates: false, ..ctx() };
        let state = started(&TurnState::default());
        let created = fold(
            &state,
            &Event::new(
                "session.created",
                json!({ "sessionID": "ses_child", "parentID": "ses_root", "title": "Helper" }),
            ),
            &ctx,
        );
        assert!(created.state().children.contains_key("ses_child"));
        assert!(from_tracked_child(
            &created.state(),
            &Event::new("x", json!({ "sessionID": "ses_child" }))
        ));

        let state = created.state();
        let folded = fold(
            &state,
            &Event::new(
                "session.text.delta",
                json!({ "sessionID": "ses_child", "assistantMessageID": "msg_1", "delta": "hi" }),
            ),
            &ctx,
        );
        let Output::SessionUpdate(SessionUpdate::AgentMessageChunk(chunk)) = &folded.outputs[0]
        else {
            panic!("projected chunk");
        };
        // Without child updates the update reads as the parent's own
        // (child.ts:39-47 routes through the projected session update).
        assert_eq!(chunk.message_id.as_ref().map(|id| id.0.as_ref()), Some("msg_1"));

        let ended = fold(
            &folded.state(),
            &Event::new("session.execution.succeeded", json!({ "sessionID": "ses_child" })),
            &ctx,
        );
        assert_eq!(ended.state().open_children.len(), 0);
    }

    #[test]
    fn child_updates_go_to_the_extension_notification() {
        let ctx = TurnContext { child_updates: true, ..ctx() };
        let state = started(&TurnState::default());
        let created = fold(
            &state,
            &Event::new(
                "session.created",
                json!({ "sessionID": "ses_child", "parentID": "ses_root" }),
            ),
            &ctx,
        );
        let Output::ChildUpdate(update) = &created.outputs[0] else {
            panic!("child update");
        };
        assert_eq!(update["type"], "status");
        assert_eq!(update["status"], "created");
        assert_eq!(update["rootSessionId"], "ses_root");
    }

    #[test]
    fn retries_project_through_session_info_meta() {
        let state = started(&TurnState::default());
        let folded = fold(
            &state,
            &Event::new(
                "session.retry.scheduled",
                json!({ "sessionID": "ses_root", "attempt": 1, "at": "2026-01-01T00:00:00.000Z",
                        "error": { "type": "provider.rate", "message": "slow down" } }),
            ),
            &ctx(),
        );
        let Output::SessionUpdate(SessionUpdate::SessionInfoUpdate(info)) = &folded.outputs[0]
        else {
            panic!("info update");
        };
        assert_eq!(info.meta.as_ref().expect("meta")[RETRY_META]["attempt"], serde_json::json!(1));
    }

    #[test]
    fn abandon_fails_tracked_tools_as_cancelled() {
        let mut state = started(&TurnState::default());
        state.tools.insert(
            "ses_root:tsk_1".to_owned(),
            Tool {
                session_id: "ses_root".to_owned(),
                id: "tsk_1".to_owned(),
                name: "read".to_owned(),
                input: json!({ "path": "a" }).as_object().cloned().unwrap(),
                metadata: Map::new(),
            },
        );
        let (next, outputs) = abandon(&state, &ctx());
        assert_eq!(next.tools.len(), 0);
        let Output::SessionUpdate(SessionUpdate::ToolCallUpdate(update)) = &outputs[0] else {
            panic!("tool update");
        };
        assert_eq!(
            update.fields.status,
            Some(agent_client_protocol::schema::v1::ToolCallStatus::Failed)
        );
        // v2 keeps the (empty) tracked metadata record alongside the
        // error (tool.ts:144-147 with metadata: tool.metadata).
        assert_eq!(update.fields.raw_output, Some(json!({ "metadata": {}, "error": "Cancelled" })));
    }
}
