//! The session transport — port of
//! `packages/tui/src/mini/stream-v2.transport.ts` (the SSE→
//! presentation seam): subscribes `/api/event`, feeds permission/
//! form events into the footer machine, folds message-part events
//! into transcript [`Commit`]s, and submits prompts (`session.prompt`).
//!
//! The event vocabulary mirrors `oc-cli`'s noninteractive loop (the
//! same generated client + the same wire shapes).

#![allow(clippy::match_same_arms)] // the event arms mirror the transport switch
use oc_client::types::{
    PermissionReply, PromptAgentAttachment, PromptInputFileAttachment, PromptInputSkillAttachment,
    SessionInboxUpdateBody, SessionPermissionReplyBody, SessionPromptBody, SessionShellBody,
    SessionSwitchAgentBody, SessionSwitchModelBody,
};
use oc_client::Client;
use serde_json::Value;

use super::fragment::{fragment_ref, FragmentReconciler, FragmentRef, FragmentRestore};
use super::image;
use super::scrollback::{Commit, CommitKind, Phase};
use super::tool_snapshot::{tool_snapshot, ToolSnapshot};
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

/// A form ask decoded from `form.created` (`data.form`) — the
/// `form.list` hydrate rows share the shape (G-272).
pub(super) fn decode_form(form: &Value) -> FormRequest {
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
    /// G-221 — transcript rows from a MULTI-row event (the settled
    /// shell's start+terminal pair, the compaction end's tail+close,
    /// the delivered inbox row's skill+user rows — `write(commits)`
    /// with a list, stream-v2.transport.ts).
    Commits(Vec<Commit>),
    /// The running turn finished (`session.execution.*`).
    Idle,
    /// G-272 — `session.step.ended`'s usage patch (transport.ts:1371-
    /// 1380): the summed tokens with the step's model (the percent's
    /// context limit) and the turn's cost.
    Usage {
        model: Option<(String, String)>,
        tokens: u64,
        cost: Option<f64>,
    },
    /// G-272 — the ROOT session's `session.renamed`
    /// (`input.onSessionTitle`, transport.ts:1036-1039).
    SessionTitle(String),
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
    /// G-68 — the fragment reconciler (`state.fragments`,
    /// stream-v2.fragment.ts): message-part re-projection with the
    /// append/covered/conflict restore modes.
    fragments: FragmentReconciler,
    /// G-68 — the image-commit dedup set (`state.imageIDs`).
    image_ids: std::collections::HashSet<String>,
    /// G-221 — `state.messageIDs` (the rendered-message dedup set —
    /// `renderUser`'s `visible`, the compaction header's once-guard).
    message_ids: std::collections::HashSet<String>,
    /// G-221 — `state.promotedMessages` (inbox rows already delivered).
    promoted: std::collections::HashSet<String>,
    /// G-221 — `state.pending` (the server inbox's user prompts, in
    /// admission order — the footer's `queued.prompts` view).
    pending: Vec<QueuedPrompt>,
    /// G-221 — `state.skillMessages` (the `→ Skill` dedup set).
    skill_messages: std::collections::HashSet<String>,
    /// G-221 — `state.shellCommands` / `shellStarted` / `shellEnded`
    /// (the direct-shell display sets).
    shell_commands: std::collections::HashMap<String, String>,
    shell_started: std::collections::HashSet<String>,
    shell_ended: std::collections::HashSet<String>,
    /// G-221 — `state.activeCompaction` (the running compaction's
    /// message id — the delta/end/failed events key on it).
    active_compaction: Option<String>,
    /// G-221 — the streaming tool-input parts (`state.tools`'
    /// streaming subset): `tool.input.started` admits the part,
    /// `tool.called` / `tool.input.ended` retire it
    /// (stream-v2.transport.ts:1260-1272).
    streaming_tools: std::collections::HashSet<String>,
    /// G-221 — the streaming parts' tool names (`tool.input.ended`
    /// carries no name; the started event's `name` stands in).
    tool_names: std::collections::HashMap<String, String>,
    /// G-272 — `state.stepModel` (transport.ts:1073-1076): the
    /// running step's model (the usage percent's context limit).
    step_model: Option<(String, String)>,
    /// G-272 — `state.errors` (transport.ts:1386-1389): the
    /// assistant messages whose step failure already rendered.
    error_messages: std::collections::HashSet<String>,
}

/// G-221 — `FooterQueuedPrompt` (mini/types.ts:92-98 — the text
/// subset): one server-inbox pending prompt (`pendingPrompt`,
/// transport.ts:196-208).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueuedPrompt {
    /// `messageID` — the inbox row id.
    pub message_id: String,
    /// `prompt.text` (`messagePrompt(item.payload).text`).
    pub text: String,
    /// `delivery` — `queue` | `steer`.
    pub delivery: String,
    /// `skills` — the `→ Skill` rows rendered on delivery.
    pub skills: Vec<QueuedSkill>,
}

/// G-221 — the queued prompt's skill (`{ id, name }`,
/// transport.ts:203-205).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueuedSkill {
    pub id: String,
    pub name: String,
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
        self.fold_session(event, &event_type, &data)
    }

    /// The `session.*` half of the event switch.
    fn fold_session(&mut self, event: &Value, event_type: &str, data: &Value) -> Fold {
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
            "session.tool.input.started" => self.tool_started(data),
            "session.tool.called" => self.tool_called(data),
            "session.tool.input.delta" | "session.tool.progress" => Fold::Ignored,
            // G-221 — the streaming input settles at the event's
            // final text (transport.ts:1260-1272).
            "session.tool.input.ended" => self.tool_input_ended(data),
            "session.tool.success" | "session.tool.failed" => self.tool_finished(event_type, data),
            // G-272 — `state.stepModel` + the usage fold
            // (transport.ts:1073-1081, 1361-1400).
            "session.step.started" => {
                let model = data.get("model").cloned().unwrap_or(Value::Null);
                let provider = text_field(&model, "providerID");
                let id = text_field(&model, "id");
                if !provider.is_empty() && !id.is_empty() {
                    self.step_model = Some((provider, id));
                }
                Fold::Ignored
            }
            "session.step.ended" => self.step_ended(data),
            "session.step.failed" => self.step_failed(data),
            // G-221 — the server inbox pending prompts
            // (transport.ts:1040-1072).
            "session.inbox.enqueued" => self.inbox_enqueued(data),
            "session.inbox.delivered" => self.inbox_delivered(data),
            "session.inbox.delivery.changed" => self.inbox_delivery_changed(data),
            "session.inbox.cancelled" => self.inbox_cancelled(data),
            // G-221 — skill activation (transport.ts:1078-1084).
            "session.skill.activated" => self.skill_activated(event, data),
            // G-221 — server-driven compaction (transport.ts:1086-
            // 1124).
            "session.compaction.started" | "session.compaction.delta"
            | "session.compaction.ended" | "session.compaction.failed" => {
                self.compaction_event(event, event_type, data)
            }
            // G-221 — the direct-shell rows (transport.ts:1125-1164).
            "session.shell.started" | "session.shell.ended" => self.shell_event(event_type, data),
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
            // G-272 — the ROOT session's rename
            // (`input.onSessionTitle?.(event.data.title)`,
            // transport.ts:1036-1039 — the subagent twin lives in the
            // tracker).
            "session.renamed" => Fold::SessionTitle(text_field(data, "title")),
            _ => Fold::Ignored,
        }
    }

    /// `session.step.ended` (transport.ts:1361-1381): the usage patch
    /// — the summed tokens (input+output+reasoning+cache.read+
    /// cache.write) with the step model for the percent's context
    /// limit, and the cost; `None` when both are zero (the ref's
    /// `total > 0 || event.data.cost` gate).
    fn step_ended(&mut self, data: &Value) -> Fold {
        let tokens = data.get("tokens").cloned().unwrap_or(Value::Null);
        let number = |key: &str| -> u64 {
            tokens.get(key).and_then(Value::as_f64).map_or(0.0, f64::round).max(0.0) as u64
        };
        let cache = tokens.get("cache").cloned().unwrap_or(Value::Null);
        let cache_number = |key: &str| -> u64 {
            cache.get(key).and_then(Value::as_f64).map_or(0.0, f64::round).max(0.0) as u64
        };
        let total = number("input")
            + number("output")
            + number("reasoning")
            + cache_number("read")
            + cache_number("write");
        let cost = data.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
        let model = self.step_model.take();
        if total == 0 && cost == 0.0 {
            return Fold::Ignored;
        }
        Fold::Usage { model, tokens: total, cost: (cost != 0.0).then_some(cost) }
    }

    /// `session.step.failed` (transport.ts:1383-1400): the error row —
    /// once per assistant message (`state.errors`), the message of
    /// the event's error (`errorMessage`).
    fn step_failed(&mut self, data: &Value) -> Fold {
        self.step_model = None;
        let message_id = text_field(data, "assistantMessageID");
        if !self.error_messages.insert(message_id) {
            return Fold::Ignored;
        }
        let error = data.get("error").cloned().unwrap_or(Value::Null);
        let message = match &error {
            Value::String(text) => text.clone(),
            value => text_field(value, "message"),
        };
        let message = if message.is_empty() { "step failed".to_owned() } else { message };
        Fold::Commit(Commit::error(&message))
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
        let delta = text_field(data, "delta");
        // G-68 — the reconciler owns the accumulation: a chunk a
        // prior projection already showed is consumed (no commit —
        // the covered-tail rule, stream-v2.fragment.ts:49-56).
        let fragment =
            FragmentRef { message_id: message_id(data).unwrap_or_default(), part_id: key.clone() };
        let Some(update) = self.fragments.delta(&fragment, &delta) else {
            return Fold::Ignored;
        };
        let _ = update;
        let accumulated = self.texts.entry(key.clone()).or_default();
        accumulated.push_str(&delta);
        // G-241 — the commit carries the CHUNK, never the
        // reconciler's accumulated text (`text: event.data.delta`,
        // transport.ts:1180): the mono writer is append-only (no
        // per-part row replacement), so a cumulative commit would
        // re-print the whole prefix on every delta.
        Fold::Commit(Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            text: delta,
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn text_ended(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let event_text = text_field(data, "text");
        let fragment = FragmentRef {
            message_id: message_id(data).unwrap_or_default(),
            part_id: key.clone(),
        };
        // An event without the final text keeps the COMMITTED text —
        // the reconciler's value (a projection's full text included),
        // never the streamed-chunks tail alone (a covered chunk a
        // projection consumed would otherwise drop from the final
        // commit).
        let committed =
            self.fragments.value(&fragment).map(str::to_owned).unwrap_or_default();
        let text = if event_text.is_empty() {
            committed.clone().is_empty().then(|| self.texts.remove(&key)).flatten().unwrap_or(committed)
        } else {
            event_text
        };
        // G-68 — the part closes at the reconciler's `end` (the
        // committed text snaps to the final text verbatim).
        let update = self.fragments.end(&fragment, &text);
        self.texts.remove(&key);
        // G-241 — `ended` replays only the UNSEEN suffix as a
        // progress commit (transport.ts:1197-1207): the writer
        // already printed the streamed chunks, and the Assistant
        // Final body renders empty (entry.body.ts) — a full-text
        // final would either duplicate the prefix or (when the
        // server's final is longer than the stream delivered) drop
        // the tail for good.
        match text.get(update.previous.len()..).filter(|suffix| !suffix.is_empty()) {
            Some(suffix) => Fold::Commit(Commit {
                kind: CommitKind::Assistant,
                phase: Phase::Progress,
                text: suffix.to_owned(),
                message_id: message_id(data),
                part_id: Some(key),
                ..Commit::default()
            }),
            None => Fold::Ignored,
        }
    }

    fn reasoning_delta(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let delta = text_field(data, "delta");
        let fragment =
            FragmentRef { message_id: message_id(data).unwrap_or_default(), part_id: key.clone() };
        let Some(update) = self.fragments.delta(&fragment, &delta) else {
            return Fold::Ignored;
        };
        let accumulated = self.reasons.entry(key.clone()).or_default();
        accumulated.push_str(&delta);
        // G-241 — the FIRST chunk of a part carries the `Thinking: `
        // mark, later chunks are the bare incremental delta
        // (`update.previous ? delta : `Thinking: ${delta}``,
        // transport.ts:1224) — the same append-only rule as
        // `text_delta`.
        let text = if update.previous.is_empty() {
            format!("Thinking: {delta}")
        } else {
            delta
        };
        Fold::Commit(Commit {
            kind: CommitKind::Reasoning,
            phase: Phase::Progress,
            text,
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn reasoning_ended(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let event_text = text_field(data, "text");
        let fragment = FragmentRef {
            message_id: message_id(data).unwrap_or_default(),
            part_id: key.clone(),
        };
        // Same covered-prefix preservation as `text_ended`.
        let committed =
            self.fragments.value(&fragment).map(str::to_owned).unwrap_or_default();
        let text = if event_text.is_empty() {
            committed.clone().is_empty().then(|| self.reasons.remove(&key)).flatten().unwrap_or(committed)
        } else {
            event_text
        };
        let update = self.fragments.end(&fragment, &text);
        self.reasons.remove(&key);
        // G-241 — the unseen suffix (transport.ts:1237-1247): the
        // `Thinking: ` mark rides the FIRST run (no streamed chunk
        // arrived — the mark would otherwise never print).
        match text.get(update.previous.len()..).filter(|suffix| !suffix.is_empty()) {
            Some(suffix) => {
                let body = if update.previous.is_empty() {
                    format!("Thinking: {suffix}")
                } else {
                    suffix.to_owned()
                };
                Fold::Commit(Commit {
                    kind: CommitKind::Reasoning,
                    phase: Phase::Progress,
                    text: body,
                    message_id: message_id(data),
                    part_id: Some(key),
                    ..Commit::default()
                })
            }
            None => Fold::Ignored,
        }
    }

    fn tool_started(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        let caption = tool_caption(data);
        // G-221 — the streaming-part bookkeeping: `started` admits
        // the part (transport.ts:1250-1259's streaming map entry).
        self.streaming_tools.insert(key.clone());
        self.tool_names.insert(key.clone(), tool_name_field(data));
        self.tools.insert(key.clone(), caption.clone());
        Fold::Commit(Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            text: caption,
            tool: Some(tool_name_field(data)),
            message_id: message_id(data),
            part_id: Some(key),
            // G-272 — the part's first caption is the ref's "start"
            // commit (`toolCommit(…, "start", …)`, transport.ts:746).
            started: true,
            ..Commit::default()
        })
    }

    /// G-221 — `session.tool.called` (transport.ts:1273-1288): the
    /// input stream is settled; the part leaves the streaming set
    /// (`tool.input.ended` after `called` is a no-op, as in ref).
    fn tool_called(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        self.streaming_tools.remove(&key);
        self.tool_names.remove(&key);
        self.tool_started(data)
    }

    /// G-221 — `session.tool.input.ended` (transport.ts:1260-1272):
    /// the streaming input settles at the event's final `text` — the
    /// caption row refreshes with the complete input detail (the ref
    /// records the final input for the next `renderTool`; the mono
    /// row model re-derives the caption row here).
    fn tool_input_ended(&mut self, data: &Value) -> Fold {
        let key = part_key(data);
        if !self.streaming_tools.remove(&key) {
            return Fold::Ignored; // not a live streaming part
        }
        let text = text_field(data, "text");
        let Ok(input) = serde_json::from_str::<Value>(&text) else {
            return Fold::Ignored; // a non-JSON final text carries no detail
        };
        let name = self.tool_names.get(&key).cloned().unwrap_or_default();
        if name.is_empty() {
            return Fold::Ignored;
        }
        self.tool_names.remove(&key);
        let caption = tool_caption(&serde_json::json!({ "tool": name, "input": input }));
        self.tools.insert(key.clone(), caption.clone());
        Fold::Commit(Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            text: caption,
            tool: Some(name),
            message_id: message_id(data),
            part_id: Some(key),
            ..Commit::default()
        })
    }

    fn tool_finished(&mut self, event_type: &str, data: &Value) -> Fold {
        let key = part_key(data);
        let caption = self.tools.remove(&key).unwrap_or_else(|| tool_caption(data));
        let failed = event_type == "session.tool.failed";
        let tool = text_field(data, "tool");
        // G-134 — the completed structured snapshot (`toolView(name)
        // .snap`, mini/tool.ts:920-967): Write/Edit/Patch/Task cards
        // (the caption families keep the caption row).
        let snapshot = if failed {
            ToolSnapshot::None
        } else {
            tool_snapshot(
                tool.as_str(),
                data.get("input").unwrap_or(&Value::Null),
                data.get("metadata").unwrap_or(&Value::Null),
                data.get("metadata")
                    .or_else(|| data.get("output"))
                    .unwrap_or(&Value::Null),
            )
        };
        // G-272 — the meta dict for `structuredFallback` (tool.ts:
        // 1333-1335): rule-less completed tools render the pretty
        // JSON input when no structured snapshot exists.
        let meta = (!failed)
            .then(|| {
                data.get("input")
                    .filter(|input| input.as_object().is_some_and(|object| !object.is_empty()))
                    .and_then(|input| serde_json::to_string(input).ok())
            })
            .flatten();
        Fold::Commit(Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            text: caption,
            tool: Some(tool),
            tool_state: Some(if failed { "error" } else { "completed" }),
            message_id: message_id(data),
            part_id: Some(key),
            snapshot,
            meta,
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

    /// G-272 — the hydrate's root-active seed (`state.rootActive =
    /// input.sessionID in active`, transport.ts:988): a session
    /// still running server-side keeps the footer's running phase
    /// after a reconnect (the live `execution.*` folds take over).
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
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

    // -- G-221: the server inbox pending prompts -----------------------
    // (stream-v2.transport.ts:196-208, 582-590, 620-630, 1040-1072) ---

    /// The pending list (`state.pending`) — the footer's
    /// `queued.prompts` view (`syncPending`, transport.ts:582-590).
    #[must_use]
    pub fn pending_prompts(&self) -> &[QueuedPrompt] {
        &self.pending
    }

    /// The hydrate seed (`hydrate`'s `session.inbox.list` walk,
    /// transport.ts:971-977): REPLACE the pending list with the
    /// server's inbox (only user rows; already-delivered rows never
    /// re-queue).
    pub fn seed_pending(&mut self, items: &[Value]) {
        self.pending = items
            .iter()
            .filter(|item| text_field(item, "type") == "user")
            .filter(|item| {
                let id = text_field(item, "id");
                !id.is_empty() && !self.promoted.contains(&id)
            })
            .map(|item| queued_prompt(&text_field(item, "id"), item))
            .collect();
    }

    /// `session.inbox.enqueued` (transport.ts:1040-1049): a user item
    /// merges into the pending list (`mergePending`); an
    /// already-promoted row renders as the user echo instead.
    fn inbox_enqueued(&mut self, data: &Value) -> Fold {
        let inbox_id = text_field(data, "inboxID");
        let item = data.get("item").cloned().unwrap_or(Value::Null);
        if inbox_id.is_empty() || text_field(&item, "type") != "user" {
            return Fold::Ignored;
        }
        let prompt = queued_prompt(&inbox_id, &item);
        if self.promoted.contains(&prompt.message_id) {
            return Fold::Commits(self.render_user_rows(&prompt));
        }
        self.pending.retain(|pending| pending.message_id != prompt.message_id);
        self.pending.push(prompt);
        Fold::Ignored
    }

    /// `session.inbox.delivered` (transport.ts:1050-1059): the queued
    /// row promotes — the user echo renders (skills first) and the
    /// list drops it.
    fn inbox_delivered(&mut self, data: &Value) -> Fold {
        let inbox_id = text_field(data, "inboxID");
        if inbox_id.is_empty() {
            return Fold::Ignored;
        }
        self.promoted.insert(inbox_id.clone());
        let at = self.pending.iter().position(|pending| pending.message_id == inbox_id);
        match at.map(|at| self.pending.remove(at)) {
            Some(prompt) => Fold::Commits(self.render_user_rows(&prompt)),
            None => Fold::Ignored,
        }
    }

    /// `session.inbox.delivery.changed` (transport.ts:1061-1067).
    fn inbox_delivery_changed(&mut self, data: &Value) -> Fold {
        let inbox_id = text_field(data, "inboxID");
        let delivery = text_field(data, "delivery");
        if let Some(pending) = self.pending.iter_mut().find(|pending| pending.message_id == inbox_id)
        {
            pending.delivery = delivery;
        }
        Fold::Ignored
    }

    /// `session.inbox.cancelled` (transport.ts:1068-1072).
    fn inbox_cancelled(&mut self, data: &Value) -> Fold {
        let inbox_id = text_field(data, "inboxID");
        self.pending.retain(|pending| pending.message_id != inbox_id);
        Fold::Ignored
    }

    /// `renderUser` (transport.ts:600-618 — the text subset): the
    /// skill rows, then the user text row — once per message (the
    /// `visible` guard). The delivery fold AND the replay walk share
    /// this seam.
    pub fn render_user_rows(&mut self, prompt: &QueuedPrompt) -> Vec<Commit> {
        let visible = self.message_ids.contains(&prompt.message_id);
        self.message_ids.insert(prompt.message_id.clone());
        let mut commits = Vec::new();
        if !visible {
            for skill in &prompt.skills {
                commits.push(skill_commit(&prompt.message_id, &skill.name, &skill.id));
            }
            if !prompt.text.trim().is_empty() {
                commits.push(Commit {
                    kind: CommitKind::User,
                    text: prompt.text.clone(),
                    phase: Phase::Start,
                    message_id: Some(prompt.message_id.clone()),
                    ..Commit::default()
                });
            }
        }
        commits
    }

    // -- G-221: skill activation ---------------------------------------
    // (stream-v2.transport.ts:399-414, 780-788, 1078-1084) -------------

    /// `session.skill.activated` (transport.ts:1078-1084): the
    /// event-id-derived message row, once per message
    /// (`state.skillMessages`).
    fn skill_activated(&mut self, event: &Value, data: &Value) -> Fold {
        let message_id = message_id_from_event(&text_field(event, "id"));
        if message_id.is_empty() {
            return Fold::Ignored;
        }
        if !self.skill_messages.insert(message_id.clone()) {
            return Fold::Ignored;
        }
        Fold::Commit(skill_commit(&message_id, &text_field(data, "name"), &message_id))
    }

    /// `renderMessage`'s "skill" arm (transport.ts:780-788): the
    /// replayed skill message's row, once per message.
    #[must_use]
    pub fn replay_skill_message(&mut self, message_id: &str, name: &str) -> Vec<Commit> {
        if message_id.is_empty() || !self.skill_messages.insert(message_id.to_owned()) {
            return Vec::new();
        }
        vec![skill_commit(message_id, name, message_id)]
    }

    // -- G-221: server-driven compaction --------------------------------
    // (stream-v2.transport.ts:416-448, 821-841, 1086-1124) -------------

    /// `session.compaction.*` (transport.ts:1086-1124).
    fn compaction_event(&mut self, event: &Value, event_type: &str, data: &Value) -> Fold {
        match event_type {
            // `started` (transport.ts:1086-1093): the header divider;
            // `inputID` — or the event id — names the compaction
            // message.
            "session.compaction.started" => {
                let input_id = text_field(data, "inputID");
                let message_id = if input_id.is_empty() {
                    message_id_from_event(&text_field(event, "id"))
                } else {
                    input_id
                };
                if message_id.is_empty() {
                    return Fold::Ignored;
                }
                self.active_compaction = Some(message_id.clone());
                if self.message_ids.contains(&message_id) {
                    return Fold::Ignored;
                }
                self.message_ids.insert(message_id.clone());
                Fold::Commit(compaction_header(&message_id))
            }
            // `delta` (transport.ts:1094-1100): the summary fragment
            // advances — the delta row (a covered chunk drops).
            "session.compaction.delta" => {
                let Some(message_id) = self.active_compaction.clone() else {
                    return Fold::Ignored;
                };
                let delta = text_field(data, "text");
                if self
                    .fragments
                    .delta(&compaction_fragment(&message_id), &delta)
                    .is_none()
                {
                    return Fold::Ignored;
                }
                Fold::Commit(compaction_summary(&message_id, &delta, Phase::Progress))
            }
            // `ended` (transport.ts:1101-1113): the unseen tail plus
            // the closing final row.
            "session.compaction.ended" => {
                let Some(message_id) = self.active_compaction.clone() else {
                    return Fold::Ignored;
                };
                self.active_compaction = None;
                let text = text_field(data, "text");
                let update = self.fragments.end(&compaction_fragment(&message_id), &text);
                let mut commits = Vec::new();
                if let Some(suffix) = text.get(update.previous.len()..).filter(|suffix| !suffix.is_empty())
                {
                    commits.push(compaction_summary(&message_id, suffix, Phase::Progress));
                }
                commits.push(compaction_summary(&message_id, "", Phase::Final));
                Fold::Commits(commits)
            }
            // `failed` (transport.ts:1114-1124): aborted closes
            // silently; anything else renders the error row.
            _ => {
                let Some(message_id) = self.active_compaction.clone() else {
                    return Fold::Ignored;
                };
                self.active_compaction = None;
                let error = data.get("error").cloned().unwrap_or(Value::Null);
                if text_field(&error, "type") == "aborted" {
                    return Fold::Commits(vec![compaction_summary(&message_id, "", Phase::Final)]);
                }
                Fold::Commits(vec![
                    compaction_summary(&message_id, "", Phase::Final),
                    compaction_error(&message_id, &text_field(&error, "message")),
                ])
            }
        }
    }

    /// `renderMessage`'s "compaction" arm (transport.ts:821-841): the
    /// replayed compaction message — the header divider, the summary
    /// fragment (projected), the closing row on completion; a failed
    /// compaction renders header + error (aborted stays silent); a
    /// running one re-arms `activeCompaction` for the live tail.
    #[must_use]
    pub fn replay_compaction_message(&mut self, message: &Value) -> Vec<Commit> {
        let message_id = text_field(message, "id");
        if message_id.is_empty() {
            return Vec::new();
        }
        let status = text_field(message, "status");
        let visible = self.message_ids.contains(&message_id);
        self.message_ids.insert(message_id.clone());
        if status == "running" {
            self.active_compaction = Some(message_id.clone());
        } else if self.active_compaction.as_deref() == Some(message_id.as_str()) {
            self.active_compaction = None;
        }
        if visible {
            return Vec::new();
        }
        if status == "failed" {
            let error = message.get("error").cloned().unwrap_or(Value::Null);
            if text_field(&error, "type") == "aborted" {
                return Vec::new();
            }
            return vec![
                compaction_header(&message_id),
                compaction_error(&message_id, &text_field(&error, "message")),
            ];
        }
        let summary = text_field(message, "summary");
        self.fragments.project(&compaction_fragment(&message_id), &summary, true);
        let mut commits = vec![compaction_header(&message_id)];
        if !summary.is_empty() {
            commits.push(compaction_summary(&message_id, &summary, Phase::Progress));
        }
        if status == "completed" {
            commits.push(compaction_summary(&message_id, "", Phase::Final));
        }
        commits
    }

    // -- G-221: the direct-shell rows ----------------------------------
    // (stream-v2.transport.ts:339-373, 790-819, 1125-1164) -------------

    /// `session.shell.started` / `session.shell.ended`
    /// (transport.ts:1125-1164): the `$ command` start row (once per
    /// shell) and, on `ended`, the settled output rows.
    fn shell_event(&mut self, event_type: &str, data: &Value) -> Fold {
        let shell = data.get("shell").cloned().unwrap_or(Value::Null);
        let id = text_field(&shell, "id");
        if id.is_empty() {
            return Fold::Ignored;
        }
        let command = text_field(&shell, "command");
        if !command.is_empty() {
            self.shell_commands.insert(id.clone(), command.clone());
        }
        let mut commits = Vec::new();
        if event_type == "session.shell.started" {
            if self.shell_started.insert(id.clone()) {
                commits.push(shell_commit(&id, &command, "running shell", Phase::Start, Some("running")));
            }
        } else {
            // A late `ended` (the start event was missed) still opens
            // the start row when the command is known
            // (transport.ts:1149-1155).
            if !self.shell_started.contains(&id) {
                self.shell_started.insert(id.clone());
                if !command.is_empty() {
                    commits.push(shell_commit(&id, &command, "running shell", Phase::Start, Some("running")));
                }
            }
            if self.shell_ended.insert(id.clone()) {
                let command = self.shell_commands.get(&id).cloned().unwrap_or(command);
                commits.extend(shell_terminal(
                    &id,
                    &command,
                    &shell,
                    data.get("output").unwrap_or(&Value::Null),
                ));
            }
        }
        if commits.is_empty() {
            Fold::Ignored
        } else {
            Fold::Commits(commits)
        }
    }

    /// `renderMessage`'s "shell" arm (transport.ts:790-819): the
    /// replayed shell message — a settled shell renders start +
    /// terminal rows; a running one only the start row (the live
    /// `shell.ended` event completes it).
    #[must_use]
    pub fn replay_shell_message(&mut self, message: &Value) -> Vec<Commit> {
        let shell_id = text_field(message, "shellID");
        if shell_id.is_empty() {
            return Vec::new();
        }
        let command = text_field(message, "command");
        if !command.is_empty() {
            self.shell_commands.insert(shell_id.clone(), command.clone());
        }
        let completed = message
            .pointer("/time/completed")
            .is_some_and(|value| !value.is_null());
        let mut commits = Vec::new();
        if self.shell_started.insert(shell_id.clone()) {
            commits.push(shell_commit(&shell_id, &command, "running shell", Phase::Start, Some("running")));
        }
        if completed {
            // A settled replayed shell marks both displays so live
            // redelivery stays silent (transport.ts:795-803).
            if self.shell_ended.insert(shell_id.clone()) {
                if let Some(output) = message.get("output").filter(|value| !value.is_null()) {
                    let command = self.shell_commands.get(&shell_id).cloned().unwrap_or(command);
                    commits.extend(shell_terminal(&shell_id, &command, message, output));
                }
            }
        }
        commits
    }

    // -- G-68: the fragment + image seams (stream-v2.fragment.ts /
    // stream-v2.image.ts) ---------------------------------------------

    /// `renderMessage`'s text/reasoning arms (transport.ts:851-881):
    /// re-project a message part's FULL text. `visible` records the
    /// projection (an invisible hydrate keeps the committed text).
    #[must_use]
    pub fn project_part(
        &mut self,
        message_id: &str,
        kind: &str,
        ordinal: usize,
        text: &str,
        visible: bool,
    ) -> super::fragment::FragmentUpdate {
        self.fragments.project(&fragment_ref(message_id, kind, ordinal), text, visible)
    }

    /// The part's committed text (`state.fragments.value`) — the
    /// close-of-part assertion seam.
    #[must_use]
    pub fn part_value(&self, message_id: &str, part_id: &str) -> Option<String> {
        self.fragments
            .value(&FragmentRef {
                message_id: message_id.to_owned(),
                part_id: part_id.to_owned(),
            })
            .map(str::to_owned)
    }

    /// The reconnect replay's row re-admission (transport.ts:1697-
    /// 1710): classify a LOCAL row's text against the server's part
    /// text — `Append` replays only the suffix, `Covered` drops the
    /// row, `Conflict` keeps it verbatim.
    pub fn restore_part(&mut self, message_id: &str, part_id: &str, text: &str) -> FragmentRestore {
        self.fragments.restore(
            &FragmentRef { message_id: message_id.to_owned(), part_id: part_id.to_owned() },
            text,
        )
    }

    /// `renderUser`'s image arm (transport.ts:609): the user
    /// message's image files commit once each (payload-deduped,
    /// `(messageID, partID)`-deduped across replays).
    #[must_use]
    pub fn commit_user_images(&mut self, message_id: &str, files: &[Value]) -> Vec<Commit> {
        let commits = image::user_image_commits(message_id, files);
        image::fresh_images(&mut self.image_ids, commits)
    }

    /// `renderTool`'s image arm (transport.ts:758): the tool part's
    /// `data:image/*` display content commits once per part.
    #[must_use]
    pub fn commit_tool_images(
        &mut self,
        part_id: &str,
        part_state: &Value,
        message_id: &str,
    ) -> Vec<Commit> {
        let commits = image::tool_image_commits(part_id, part_state, message_id);
        image::fresh_images(&mut self.image_ids, commits)
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
    let key = text_field(data, "partID");
    if !key.is_empty() {
        return key;
    }
    // G-221 — the real wire (`session.tool.*` carries `id`, not the
    // part id of the normalized stub shape).
    text_field(data, "id")
}

fn message_id(data: &Value) -> Option<String> {
    let id = text_field(data, "messageID");
    let id = if id.is_empty() { text_field(data, "assistantMessageID") } else { id };
    (!id.is_empty()).then_some(id)
}

/// The tool name field (`tool` on the normalized stub shape, `name`
/// on the real wire — `session.tool.input.started` carries `name`).
fn tool_name_field(data: &Value) -> String {
    let tool = text_field(data, "tool");
    if !tool.is_empty() {
        return tool;
    }
    text_field(data, "name")
}

/// `messageIDFromEvent` (transport.ts:375-381): the SSE event id and
/// its message id share the `evt_` ⇄ `msg_` prefix swap.
fn message_id_from_event(event_id: &str) -> String {
    event_id
        .strip_prefix("evt_")
        .map_or_else(|| event_id.to_owned(), |rest| format!("msg_{rest}"))
}

/// The mono tool caption (`tool.ts` caption subset): the human tool
/// name + a short input summary.
fn tool_caption(data: &Value) -> String {
    let tool = tool_name_field(data);
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

// -- G-221: the block constructors --------------------------------------

/// `pendingPrompt` (transport.ts:196-208): one inbox row → the
/// queued-prompt view (`messageID`, the payload's text and skills,
/// the delivery).
fn queued_prompt(message_id: &str, item: &Value) -> QueuedPrompt {
    let payload = item.get("payload").unwrap_or(item);
    let skills = payload
        .get("skills")
        .and_then(Value::as_array)
        .map(|skills| {
            skills
                .iter()
                .map(|skill| QueuedSkill {
                    id: text_field(skill, "id"),
                    name: text_field(skill, "name"),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    QueuedPrompt {
        message_id: message_id.to_owned(),
        text: text_field(payload, "text"),
        delivery: text_field(item, "delivery"),
        skills,
    }
}

/// `skillCommit` (transport.ts:399-408): the `→ Skill "name"` row
/// (kind system, partID `skill:{skillID}`; the `tool: "skill"` marker
/// carries the runtime's `showTools()` gate — the ref gates at the
/// write site, transport.ts:612,1083).
fn skill_commit(message_id: &str, name: &str, skill_id: &str) -> Commit {
    Commit {
        kind: CommitKind::System,
        tool: Some("skill".to_owned()),
        text: format!("→ Skill \"{name}\""),
        phase: Phase::Start,
        message_id: Some(message_id.to_owned()),
        part_id: Some(format!("skill:{skill_id}")),
        ..Commit::default()
    }
}

/// `compactionCommit` (transport.ts:416-426): the `Compaction` divider
/// header (`compaction: true` — the `─── text ───` row).
fn compaction_header(message_id: &str) -> Commit {
    Commit {
        kind: CommitKind::System,
        text: "Compaction".to_owned(),
        phase: Phase::Start,
        compaction: true,
        message_id: Some(message_id.to_owned()),
        part_id: Some("compaction:header".to_owned()),
        ..Commit::default()
    }
}

/// `compactionSummary` (transport.ts:428-437): the summary fragment's
/// row (partID `compaction:summary`).
fn compaction_summary(message_id: &str, text: &str, phase: Phase) -> Commit {
    Commit {
        kind: CommitKind::Assistant,
        text: text.to_owned(),
        phase,
        message_id: Some(message_id.to_owned()),
        part_id: Some("compaction:summary".to_owned()),
        ..Commit::default()
    }
}

/// `compactionError` (transport.ts:439-448): the failed compaction's
/// error row.
fn compaction_error(message_id: &str, text: &str) -> Commit {
    Commit {
        kind: CommitKind::Error,
        text: text.to_owned(),
        phase: Phase::Start,
        message_id: Some(message_id.to_owned()),
        part_id: Some("compaction:error".to_owned()),
        ..Commit::default()
    }
}

/// The compaction summary's fragment ref (`compaction:summary`).
fn compaction_fragment(message_id: &str) -> FragmentRef {
    FragmentRef { message_id: message_id.to_owned(), part_id: "compaction:summary".to_owned() }
}

/// `shellCommit` (transport.ts:339-352): one direct-shell commit —
/// partID `shell:{id}`, tool "shell", the command on the side (the
/// render branch's `$ command` key).
fn shell_commit(
    id: &str,
    command: &str,
    text: &str,
    phase: Phase,
    tool_state: Option<&'static str>,
) -> Commit {
    Commit {
        kind: CommitKind::Tool,
        tool: Some("shell".to_owned()),
        shell: Some(command.to_owned()),
        part_id: Some(format!("shell:{id}")),
        text: text.to_owned(),
        phase,
        tool_state,
        ..Commit::default()
    }
}

/// `shellTerminal` (transport.ts:354-373): the settled shell's rows —
/// the merged output (`[output truncated]` when the page is
/// incomplete) and, on failure, the exit-code error line.
fn shell_terminal(id: &str, command: &str, shell: &Value, output: &Value) -> Vec<Commit> {
    let text = shell_output_text(output);
    let status = text_field(shell, "status");
    let exit = shell.get("exit").filter(|value| !value.is_null());
    let error = if status == "exited" && exit.and_then(Value::as_f64) == Some(0.0) {
        None
    } else if status == "exited" {
        Some(format!(
            "Shell exited with code {}",
            exit.map(exit_label).unwrap_or_else(|| "unknown".to_owned())
        ))
    } else {
        Some(format!("Shell {status}"))
    };
    match error {
        None => vec![shell_commit(id, command, &text, Phase::Progress, Some("completed"))],
        Some(error) => {
            let mut commits = Vec::new();
            if !text.is_empty() {
                commits.push(shell_commit(id, command, &text, Phase::Progress, Some("running")));
            }
            commits.push(shell_commit(id, command, &error, Phase::Final, Some("error")));
            commits
        }
    }
}

/// The merged output text with the truncation join
/// (`shellTerminal`'s `text`, transport.ts:360-361): an incomplete
/// page (`truncated` or `cursor < size`) closes the output with the
/// `[output truncated]` marker on its own row.
fn shell_output_text(output: &Value) -> String {
    let text = text_field(output, "output");
    let cursor = output.get("cursor").and_then(Value::as_u64).unwrap_or(0);
    let size = output.get("size").and_then(Value::as_u64).unwrap_or(0);
    let truncated = output.get("truncated").and_then(Value::as_bool).unwrap_or(false);
    if !truncated && cursor >= size {
        return text;
    }
    let mut joined = text;
    if !joined.is_empty() && !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined.push_str("[output truncated]");
    joined
}

/// `Shell exited with code ${shell.exit}` (transport.ts:366): the
/// exit renders without a fractional tail (`0`, not `0.0`).
fn exit_label(exit: &Value) -> String {
    match exit.as_f64() {
        Some(number) if number.fract() == 0.0 => format!("{}", number as i64),
        Some(number) => format!("{number}"),
        None => exit.to_string(),
    }
}

/// G-272 — `prepareInitialFile` (stream-v2.transport.ts:243-252): a
/// text/plain initial file becomes an INLINE text block wrapped as
/// `<file name="…">…</file>` (data: URLs decode base64 first; local
/// paths read from disk through the host seam); every other mime
/// stays a wire file attachment. Returns `(inline_text, wire_file)`.
fn prepare_initial_file(
    url: &str,
    filename: &str,
    mime: Option<&str>,
) -> (Option<String>, Option<PromptInputFileAttachment>) {
    if mime != Some("text/plain") {
        return (
            None,
            Some(PromptInputFileAttachment {
                uri: url.to_owned(),
                name: Some(filename.to_owned()),
                ..PromptInputFileAttachment::default()
            }),
        );
    }
    let content = if let Some(encoded) = url.strip_prefix("data:") {
        // `data:{mime};base64,{data}` — the payload after the first
        // comma (transport.ts:249-250).
        let payload = encoded.rsplit_once(',').map_or(encoded, |(_, tail)| tail);
        oc_util::encode::base64_decode_string(payload).ok()
    } else {
        std::fs::read_to_string(url).ok()
    };
    match content {
        Some(content) => (
            Some(format!("<file name=\"{filename}\">\n{content}\n</file>")),
            None,
        ),
        None => (
            None,
            Some(PromptInputFileAttachment {
                uri: url.to_owned(),
                name: Some(filename.to_owned()),
                ..PromptInputFileAttachment::default()
            }),
        ),
    }
}

/// G-272 — `promptFiles` (transport.ts:263-282): the prompt's FILE
/// parts (the `@`-mentioned attachments) map to wire file
/// attachments; agent/skill parts map to the `agents`/`skills`
/// arrays (transport.ts:284-296).
fn prompt_attachments(parts: &[super::queue::RunPromptPart]) -> PromptAttachments {
    let mut out = PromptAttachments::default();
    for part in parts {
        match part {
            super::queue::RunPromptPart::File { url, filename, mime: _, description } => {
                out.files.push(PromptInputFileAttachment {
                    uri: url.clone(),
                    name: filename.clone(),
                    description: description.clone(),
                    ..PromptInputFileAttachment::default()
                });
            }
            super::queue::RunPromptPart::Agent { name } => out.agents.push(PromptAgentAttachment {
                name: name.clone(),
                ..PromptAgentAttachment::default()
            }),
            super::queue::RunPromptPart::Skill { id } => {
                out.skills.push(PromptInputSkillAttachment { id: id.clone(), ..Default::default() })
            }
        }
    }
    out
}

/// G-272 — `prepareAttachments` (transport.ts:284-318): the initial
/// `input.files` (text/plain inlined + wrapped, others attached)
/// MERGED with the prompt's file parts; the inline text blocks join
/// the prompt text with `\n\n` separators (`[next.prompt.text,
/// ...attachments.text].join("\n\n")`, transport.ts:1743).
#[must_use]
pub fn prepare_prompt_body(prompt: &super::queue::RunPrompt) -> SessionPromptBody {
    let mut attachments = prompt_attachments(&prompt.parts);
    let mut inline = Vec::new();
    for part in &prompt.parts {
        if let super::queue::RunPromptPart::File { url, filename, mime, .. } = part {
            let filename = filename.clone().unwrap_or_else(|| url.clone());
            let (text, file) = prepare_initial_file(url, &filename, mime.as_deref());
            if let Some(text) = text {
                inline.push(text);
            }
            if let Some(file) = file {
                attachments.files.push(file);
            }
        }
    }
    let mut text = prompt.text.clone();
    if !inline.is_empty() {
        text = std::iter::once(prompt.text.clone())
            .chain(inline)
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    SessionPromptBody {
        id: None,
        text,
        delivery: prompt.delivery.map(|delivery| match delivery {
            "steer" => oc_client::types::SessionInboxDelivery::Steer,
            _ => oc_client::types::SessionInboxDelivery::Queue,
        }),
        files: (!attachments.files.is_empty()).then_some(attachments.files),
        agents: (!attachments.agents.is_empty()).then_some(attachments.agents),
        skills: (!attachments.skills.is_empty()).then_some(attachments.skills),
        ..SessionPromptBody::default()
    }
}

/// The prepared attachment arrays (`prepareAttachments`'s two lists).
#[derive(Default)]
struct PromptAttachments {
    files: Vec<PromptInputFileAttachment>,
    agents: Vec<PromptAgentAttachment>,
    skills: Vec<PromptInputSkillAttachment>,
}

/// Send one prompt turn (`runPromptTurn` → `session.prompt` with
/// `delivery` per the queue's admission, `stream-v2.transport.ts`).
/// G-272 — the prompt's structured parts ride the same POST
/// (`prepareAttachments` → `files`/`agents`/`skills`).
pub async fn send_prompt(
    client: &Client,
    session_id: &str,
    message_id: &str,
    text: &str,
    delivery: Option<&str>,
) -> Result<(), String> {
    send_prompt_parts(
        client,
        session_id,
        message_id,
        &super::queue::RunPrompt {
            text: text.to_owned(),
            delivery: delivery.map(|delivery| match delivery {
                "steer" => "steer",
                _ => "queue",
            }),
            ..super::queue::RunPrompt::default()
        },
    )
    .await
}

/// G-272 — the full prompt POST over the structured prompt
/// (`admitPrompt`, stream-v2.transport.ts:1735-1752): the message id
/// rides the body, the attachments join per
/// [`prepare_prompt_body`].
pub async fn send_prompt_parts(
    client: &Client,
    session_id: &str,
    message_id: &str,
    prompt: &super::queue::RunPrompt,
) -> Result<(), String> {
    let body = SessionPromptBody { id: Some(message_id.to_owned()), ..prepare_prompt_body(prompt) };
    client.session().prompt(session_id, &body).await.map(|_| ()).map_err(|error| error.to_string())
}

/// G-272 — `runShellTurn`'s send half (stream-v2.transport.ts:
/// 1561-1592): the `!` passthrough POSTs `session.shell` with a
/// client-minted message id; the SSE shell events render the rows
/// (`session.shell.started`/`ended` folds).
pub async fn send_shell(
    client: &Client,
    session_id: &str,
    message_id: &str,
    command: &str,
) -> Result<(), String> {
    let body = SessionShellBody {
        command: command.to_owned(),
        id: Some(message_id.to_owned()),
    };
    client.session().shell(session_id, &body).await.map_err(|error| error.to_string())
}

/// G-272 — the queued-prompt action (`onQueuedPromptAction`,
/// runtime.ts:394-402): `cancel` DELETEs the inbox row; `steer`/
/// `queue` PATCH its delivery.
pub async fn inbox_prompt_action(
    client: &Client,
    session_id: &str,
    inbox_id: &str,
    action: &str,
) -> Result<(), String> {
    if action == "cancel" {
        client
            .session()
            .inbox()
            .cancel(session_id, inbox_id)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let delivery = if action == "steer" {
        oc_client::types::SessionInboxDelivery::Steer
    } else {
        oc_client::types::SessionInboxDelivery::Queue
    };
    client
        .session()
        .inbox()
        .update(session_id, inbox_id, &SessionInboxUpdateBody { delivery })
        .await
        .map_err(|error| error.to_string())
}

/// G-272 — the per-turn agent switch (`client.session.switchAgent`,
/// stream-v2.transport.ts:1799/1816:1834).
pub async fn switch_agent(client: &Client, session_id: &str, agent: &str) -> Result<(), String> {
    client
        .session()
        .switch_agent(session_id, &SessionSwitchAgentBody { agent: agent.to_owned() })
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// G-272 — the per-turn model switch (`client.session.switchModel`,
/// stream-v2.transport.ts:1804/1839: `selected` resolves through the
/// model preference seam).
pub async fn switch_model(
    client: &Client,
    session_id: &str,
    provider_id: &str,
    model_id: &str,
) -> Result<(), String> {
    client
        .session()
        .switch_model(
            session_id,
            &SessionSwitchModelBody {
                model: oc_client::types::ModelRef {
                    id: model_id.to_owned(),
                    provider_id: provider_id.to_owned(),
                    variant: None,
                },
            },
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// G-272 — `onBackground` (runtime.ts:386-392): detach blocking
/// subagent tools into the background (`session.background`).
pub async fn session_background(client: &Client, session_id: &str) -> Result<(), String> {
    client
        .session()
        .background(session_id)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
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
// hydration — G-181④: the runtime's stream pump now reconnects (250
// ms cadence, fresh client swap) and replays the hydrate on every
// reconnection; the session-list hydration source rides the same
// replay seam as the transcript.

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
            // G-221 — a multi-row event (shell/compaction/inbox) folds
            // row-for-row into the child detail transcript.
            Fold::Commits(commits) => {
                for commit in commits {
                    slot.child.commits.push(commit);
                    let excess = slot.child.commits.len().saturating_sub(CHILD_COMMIT_LIMIT);
                    if excess > 0 {
                        slot.child.commits.drain(0..excess);
                    }
                }
                fold.changed = true;
            }
            Fold::Ignored | Fold::Idle => {}
            Fold::Permission(_)
            | Fold::PermissionReplied(_)
            | Fold::Form(_)
            | Fold::FormRemoved(_)
            // G-272 — the parent-owned folds (usage, the root
            // session's title) never reach a child transcript.
            | Fold::Usage { .. }
            | Fold::SessionTitle(_) => {}
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

    /// G-272 — `prepareAttachments` (stream-v2.transport.ts:284-318):
    /// a text/plain initial file inlines as the `<file name>` block
    /// (data: URLs decode base64), other mimes stay wire files, and
    /// the prompt's own parts ride `files`/`agents`/`skills`.
    #[test]
    fn prompt_parts_prepare_the_wire_body() {
        let prompt = super::super::queue::RunPrompt {
            text: "summarize these".to_owned(),
            parts: vec![
                super::super::queue::RunPromptPart::File {
                    url: "data:text/plain;base64,aGVsbG8=".to_owned(),
                    filename: Some("notes.txt".to_owned()),
                    mime: Some("text/plain".to_owned()),
                    description: None,
                },
                super::super::queue::RunPromptPart::File {
                    url: "chart.png".to_owned(),
                    filename: Some("chart.png".to_owned()),
                    mime: Some("image/png".to_owned()),
                    description: None,
                },
                super::super::queue::RunPromptPart::Agent { name: "build".to_owned() },
                super::super::queue::RunPromptPart::Skill { id: "refactor".to_owned() },
            ],
            ..Default::default()
        };
        let body = prepare_prompt_body(&prompt);
        assert_eq!(
            body.text,
            "summarize these\n\n<file name=\"notes.txt\">\nhello\n</file>",
            "inline text blocks join with blank-line separators"
        );
        assert_eq!(body.files.as_ref().map(Vec::len), Some(1), "only the non-text file attaches");
        assert_eq!(body.files.as_ref().unwrap()[0].uri, "chart.png");
        assert_eq!(body.agents.as_ref().map(Vec::len), Some(1));
        assert_eq!(body.skills.as_ref().map(Vec::len), Some(1));
        assert!(body.delivery.is_none(), "no admission delivery on a direct run");
    }

    /// G-272 — a text/plain file that CANNOT be read (missing local
    /// path) falls back to the wire attachment instead of dropping.
    #[test]
    fn unreadable_text_files_fall_back_to_attachments() {
        let prompt = super::super::queue::RunPrompt {
            text: "hi".to_owned(),
            parts: vec![super::super::queue::RunPromptPart::File {
                url: "/nonexistent/definitely-missing.txt".to_owned(),
                filename: Some("gone.txt".to_owned()),
                mime: Some("text/plain".to_owned()),
                description: None,
            }],
            ..Default::default()
        };
        let body = prepare_prompt_body(&prompt);
        assert_eq!(body.text, "hi");
        assert_eq!(body.files.as_ref().map(Vec::len), Some(1));
    }

    /// G-272 — the admission delivery survives the parts merge
    /// (`send.command`'s delivery arm, transport.ts:1743-1752).
    #[test]
    fn admitted_delivery_maps_to_the_wire_enum() {
        let prompt = super::super::queue::RunPrompt {
            text: "queued work".to_owned(),
            delivery: Some("steer"),
            ..Default::default()
        };
        let body = prepare_prompt_body(&prompt);
        assert_eq!(body.delivery, Some(oc_client::types::SessionInboxDelivery::Steer));
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
            // G-241 — every delta commit carries its CHUNK (the
            // writer is append-only; a cumulative commit would
            // re-print the prefix per delta, transport.ts:1180).
            Fold::Commit(commit) => assert_eq!(commit.text, "Hello "),
            other => panic!("delta folded: {other:?}"),
        }
        let delta = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "delta": "world" }),
        ));
        match delta {
            Fold::Commit(commit) => assert_eq!(commit.text, "world"),
            other => panic!("delta folded: {other:?}"),
        }
        // An ended WITHOUT the final text keeps the committed text —
        // nothing new to replay (transport.ts:1197's length check).
        let ended = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1" }),
        ));
        assert_eq!(ended, Fold::Ignored);
        assert!(!state.is_running());
    }

    /// G-241 — the multi-delta shape against the ref: two chunks
    /// commit incrementally, and `text.ended` with a final LONGER
    /// than the stream replays only the unseen suffix.
    #[test]
    fn text_ended_replays_the_unseen_suffix() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        state.fold(&event(
            "session.text.started",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1" }),
        ));
        state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "delta": "Hello " }),
        ));
        state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "delta": "world" }),
        ));
        // The server's final extends the streamed accumulation: only
        // the tail lands (transport.ts:1197-1207).
        let ended = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "text": "Hello world and more." }),
        ));
        match ended {
            Fold::Commit(commit) => {
                assert_eq!(commit.text, " and more.");
                assert_eq!(commit.phase, Phase::Progress, "the suffix is a progress row");
            }
            other => panic!("ended folded: {other:?}"),
        }
        // A SECOND ended never replays again (the reconciler closed).
        let again = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "p1", "text": "Hello world and more." }),
        ));
        assert_eq!(again, Fold::Ignored);
    }

    /// G-241 — the reasoning twin: the first chunk carries the
    /// `Thinking: ` mark, later chunks are bare, and the ended suffix
    /// keeps the mark when no chunk ever arrived.
    #[test]
    fn reasoning_stream_is_incremental_with_the_thinking_mark() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        state.fold(&event(
            "session.reasoning.started",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "r1" }),
        ));
        let first = state.fold(&event(
            "session.reasoning.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "r1", "delta": "deep" }),
        ));
        match first {
            Fold::Commit(commit) => assert_eq!(commit.text, "Thinking: deep"),
            other => panic!("delta folded: {other:?}"),
        }
        let second = state.fold(&event(
            "session.reasoning.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "r1", "delta": " thought" }),
        ));
        match second {
            Fold::Commit(commit) => assert_eq!(commit.text, " thought"),
            other => panic!("delta folded: {other:?}"),
        }
        let ended = state.fold(&event(
            "session.reasoning.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "r1", "text": "deep thought done" }),
        ));
        match ended {
            Fold::Commit(commit) => assert_eq!(commit.text, " done"),
            other => panic!("ended folded: {other:?}"),
        }
        // No chunk ever arrived: the ended carries the whole text
        // under the mark (transport.ts:1242).
        let mut quiet = TransportState { session_id: "s1".into(), ..Default::default() };
        let ended = quiet.fold(&event(
            "session.reasoning.ended",
            &json!({ "sessionID": "s1", "messageID": "m2", "partID": "r2", "text": "all at once" }),
        ));
        match ended {
            Fold::Commit(commit) => assert_eq!(commit.text, "Thinking: all at once"),
            other => panic!("ended folded: {other:?}"),
        }
    }

    /// G-272 — the ROOT session's rename (`input.onSessionTitle`,
    /// transport.ts:1036-1039).
    #[test]
    fn session_renamed_folds_the_root_title() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let folded = state.fold(&event(
            "session.renamed",
            &json!({ "sessionID": "s1", "title": "A fresh title" }),
        ));
        assert_eq!(folded, Fold::SessionTitle("A fresh title".to_owned()));
        // A FOREIGN session's rename never folds.
        let foreign = state.fold(&event(
            "session.renamed",
            &json!({ "sessionID": "other", "title": "Not mine" }),
        ));
        assert_eq!(foreign, Fold::Ignored);
    }

    /// G-272 — the step family (transport.ts:1073-1081, 1361-1400):
    /// `step.started` records the model, `step.ended` folds the usage
    /// patch (summed tokens + cost), and `step.failed` renders the
    /// error row ONCE per assistant message.
    #[test]
    fn step_events_fold_usage_and_failure_rows() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let started = state.fold(&event(
            "session.step.started",
            &json!({ "sessionID": "s1", "model": { "providerID": "p", "id": "m" } }),
        ));
        assert_eq!(started, Fold::Ignored);
        let ended = state.fold(&event(
            "session.step.ended",
            &json!({ "sessionID": "s1",
                     "tokens": { "input": 12000, "output": 300, "reasoning": 50,
                                 "cache": { "read": 20, "write": 5 } },
                     "cost": 0.5 }),
        ));
        assert_eq!(
            ended,
            Fold::Usage {
                model: Some(("p".to_owned(), "m".to_owned())),
                tokens: 12375,
                cost: Some(0.5),
            }
        );
        // Zero tokens and no cost fold nothing.
        let empty = state.fold(&event(
            "session.step.ended",
            &json!({ "sessionID": "s1", "tokens": { "input": 0, "output": 0, "reasoning": 0,
                                                     "cache": { "read": 0, "write": 0 } } }),
        ));
        assert_eq!(empty, Fold::Ignored);
        // A failed step renders its error row once per message.
        let failed = state.fold(&event(
            "session.step.failed",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_f",
                     "error": { "type": "Error", "message": "boom" } }),
        ));
        match failed {
            Fold::Commit(commit) => {
                assert_eq!(commit.kind, CommitKind::Error);
                assert_eq!(commit.text, "boom");
            }
            other => panic!("failed folded: {other:?}"),
        }
        let again = state.fold(&event(
            "session.step.failed",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_f",
                     "error": { "type": "Error", "message": "boom" } }),
        ));
        assert_eq!(again, Fold::Ignored, "the second failure for the same message never re-renders");
        // A STRING error payload keeps its text.
        let string_error = state.fold(&event(
            "session.step.failed",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_g", "error": "plain text" }),
        ));
        match string_error {
            Fold::Commit(commit) => assert_eq!(commit.text, "plain text"),
            other => panic!("failed folded: {other:?}"),
        }
    }

    /// G-68 — the reconciler at the transport seam: a delta a prior
    /// PROJECTION already showed is consumed (Ignored — never a
    /// double-append), and `text.ended`'s server text wins.
    #[test]
    fn projected_parts_consume_covered_deltas() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        // Hydrate rendered the part in full.
        let update = state.project_part("m1", "text", 0, "one two three", true);
        assert_eq!(update.previous, "");
        // The stream re-delivers mid-projection chunks: consumed.
        let covered = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "text:0", "delta": "two " }),
        ));
        assert_eq!(covered, Fold::Ignored, "a covered chunk never commits");
        // ... past the projection the delta appends (its CHUNK —
        // G-241, the accumulated text lives in the reconciler).
        let fresh = state.fold(&event(
            "session.text.delta",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "text:0", "delta": " four" }),
        ));
        match fresh {
            Fold::Commit(commit) => assert_eq!(commit.text, " four"),
            other => panic!("delta folded: {other:?}"),
        }
        // The part closes at the server's final text — a SHORTER
        // final replays nothing (G-241's suffix rule).
        let ended = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "text:0", "text": "FINAL" }),
        ));
        assert_eq!(ended, Fold::Ignored);
        assert_eq!(state.part_value("m1", "text:0"), Some("FINAL".to_owned()));
    }

    /// G-68 — the restore modes over the transport seam: `append`
    /// replays only the suffix, `covered` drops, `conflict` keeps.
    #[test]
    fn restore_classifies_local_rows() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        state.project_part("m1", "text", 0, "abc", true);
        assert_eq!(
            state.restore_part("m1", "text:0", "abcdef"),
            FragmentRestore::Append { suffix: "def".to_owned() }
        );
        assert_eq!(state.restore_part("m1", "text:0", "abc"), FragmentRestore::Covered);
        assert_eq!(state.restore_part("m1", "text:0", "zzz"), FragmentRestore::Conflict);
    }

    /// G-68 — the image commits (stream-v2.image.ts + the dedup's
    /// identity, attachment.ts:24-74): an inline image WITH mention
    /// text dedups on metadata([mime,name,description,mention]) +
    /// payload; different mention text keeps the row; the mime filter
    /// runs AFTER the dedup (survivors number `[Image N]`); user
    /// commits are kind User, tool commits kind Tool; REPLAYS dedup
    /// by `(messageID, partID)`.
    #[test]
    fn image_parts_commit_and_dedupe() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let files = vec![
            // Same metadata + payload as #2 → deduped.
            json!({ "mime": "image/png", "data": "AAEC", "name": "shot.png",
                    "mention": { "text": "[Image 1]" } }),
            json!({ "mime": "image/png", "data": "AAEC", "name": "shot.png",
                    "mention": { "text": "[Image 1]" } }),
            // Same payload, DIFFERENT mention → a distinct row.
            json!({ "mime": "image/png", "data": "AAEC", "name": "dup.png",
                    "mention": { "text": "[Image 2]" } }),
            // No mention text → never deduped (the identity is None).
            json!({ "mime": "image/png", "data": "QUJD" }),
            json!({ "mime": "text/plain", "data": "aGk=" }), // not an image
        ];
        let commits = state.commit_user_images("msg_u", &files);
        assert_eq!(commits.len(), 3, "metadata+payload dedup, mime filter after");
        assert_eq!(commits[0].kind, CommitKind::User);
        assert_eq!(commits[0].text, "shot.png", "the name label");
        assert_eq!(commits[0].image.as_deref(), Some("data:image/png;base64,AAEC"));
        assert_eq!(commits[0].part_id.as_deref(), Some("image:0"));
        assert_eq!(commits[0].phase, Phase::Final);
        assert_eq!(commits[1].text, "dup.png");
        // The mention-less survivor gets the numbered label (index 2).
        assert_eq!(commits[2].text, "[Image 3]");
        // The replay: the SAME files commit nothing.
        assert!(state.commit_user_images("msg_u", &files).is_empty(), "replays dedupe");

        // The tool arm: data:image/* URIs only, kind Tool.
        let tool_state = json!({ "content": [
            { "type": "file", "mime": "image/png", "uri": "data:image/png;base64,AAEC", "name": "out.png" },
            { "type": "file", "mime": "text/plain", "uri": "data:text/plain;base64,aGk=" },
            { "type": "file", "mime": "image/png", "uri": "file:///tmp/no.txt" },
        ]});
        let tool_commits = state.commit_tool_images("prt_1", &tool_state, "msg_a");
        assert_eq!(tool_commits.len(), 1);
        assert_eq!(tool_commits[0].kind, CommitKind::Tool, "the ref's kind: tool");
        assert_eq!(tool_commits[0].text, "out.png");
        assert_eq!(tool_commits[0].part_id.as_deref(), Some("prt_1:image:0"));
        assert!(state.commit_tool_images("prt_1", &tool_state, "msg_a").is_empty(), "replays dedupe");
    }

    /// G-68 (fix #5) — an `ended` WITHOUT the final text preserves the
    /// committed prefix: a projection's full text (whose covered
    /// chunks the stream consumed) stays the part's committed value.
    /// G-241 — the display side: the projection already printed that
    /// text, so the ended commits NOTHING new (no suffix ⇒ Ignored).
    #[test]
    fn ended_without_text_preserves_the_projected_prefix() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        state.project_part("m1", "text", 0, "one two three", true);
        // The stream re-delivers covered chunks (consumed)...
        assert_eq!(
            state.fold(&event(
                "session.text.delta",
                &json!({ "sessionID": "s1", "messageID": "m1", "partID": "text:0", "delta": "one two " }),
            )),
            Fold::Ignored
        );
        // ...and closes with NO final text: the part's committed
        // value keeps the FULL projected text (never just the
        // streamed chunks), and the writer re-prints nothing.
        let ended = state.fold(&event(
            "session.text.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "text:0" }),
        ));
        assert_eq!(ended, Fold::Ignored, "an ended equal to the committed text replays nothing");
        assert_eq!(state.part_value("m1", "text:0"), Some("one two three".to_owned()));
        // The reasoning arm keeps the same guarantee.
        state.project_part("m1", "reasoning", 0, "think", true);
        let ended = state.fold(&event(
            "session.reasoning.ended",
            &json!({ "sessionID": "s1", "messageID": "m1", "partID": "reasoning:0" }),
        ));
        assert_eq!(ended, Fold::Ignored);
        assert_eq!(state.part_value("m1", "reasoning:0"), Some("think".to_owned()));
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

    // -- G-221 ---------------------------------------------------------

    /// G-221 block 1 — the server inbox pending prompts
    /// (transport.ts:1040-1072): enqueued user rows queue (the
    /// footer's `queued.prompts` view), `delivery.changed` updates,
    /// `cancelled` drops, `delivered` promotes (the user echo + skill
    /// rows render and the list empties), and the hydrate seed
    /// (`seed_pending`) replaces the list (delivered rows never
    /// re-queue).
    #[test]
    fn inbox_events_drive_the_queued_prompts_view() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let enqueue = |id: &str, text: &str, delivery: &str| {
            event(
                "session.inbox.enqueued",
                &json!({
                    "sessionID": "s1", "inboxID": id,
                    "item": {
                        "type": "user",
                        "payload": { "text": text, "skills": [ { "id": "skl_a", "name": "deploy" } ] },
                        "delivery": delivery,
                    },
                }),
            )
        };
        assert_eq!(state.fold(&enqueue("msg_q1", "first question", "queue")), Fold::Ignored);
        assert_eq!(state.pending_prompts().len(), 1);
        assert_eq!(state.pending_prompts()[0].message_id, "msg_q1");
        assert_eq!(state.pending_prompts()[0].text, "first question");
        assert_eq!(state.pending_prompts()[0].delivery, "queue");
        assert_eq!(state.pending_prompts()[0].skills, vec![QueuedSkill { id: "skl_a".into(), name: "deploy".into() }]);
        // A non-user item never queues.
        assert_eq!(
            state.fold(&event(
                "session.inbox.enqueued",
                &json!({ "sessionID": "s1", "inboxID": "msg_x", "item": { "type": "synthetic", "payload": { "text": "note" }, "delivery": "queue" } }),
            )),
            Fold::Ignored
        );
        assert_eq!(state.pending_prompts().len(), 1);
        // `delivery.changed` updates the row in place.
        assert_eq!(
            state.fold(&event(
                "session.inbox.delivery.changed",
                &json!({ "sessionID": "s1", "inboxID": "msg_q1", "delivery": "steer" }),
            )),
            Fold::Ignored
        );
        assert_eq!(state.pending_prompts()[0].delivery, "steer");
        // `delivered` promotes: the skill row + the user echo render,
        // the list empties.
        let delivered = state.fold(&event(
            "session.inbox.delivered",
            &json!({ "sessionID": "s1", "inboxID": "msg_q1" }),
        ));
        match delivered {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 2, "the skill row + the user echo");
                assert_eq!(commits[0].text, "→ Skill \"deploy\"");
                assert_eq!(commits[0].part_id.as_deref(), Some("skill:skl_a"));
                assert_eq!(commits[1].kind, CommitKind::User);
                assert_eq!(commits[1].text, "first question");
                assert_eq!(commits[1].message_id.as_deref(), Some("msg_q1"));
            }
            other => panic!("delivered folded: {other:?}"),
        }
        assert!(state.pending_prompts().is_empty());
        // A re-enqueue of the promoted row renders the echo again —
        // but the once-guard (messageIDs) keeps it to one row.
        match state.fold(&enqueue("msg_q1", "first question", "queue")) {
            Fold::Commits(commits) => assert!(commits.is_empty(), "already visible"),
            other => panic!("re-enqueue folded: {other:?}"),
        }
        // `cancelled` drops a queued row.
        state.fold(&enqueue("msg_q2", "second", "queue"));
        assert_eq!(
            state.fold(&event(
                "session.inbox.cancelled",
                &json!({ "sessionID": "s1", "inboxID": "msg_q2" }),
            )),
            Fold::Ignored
        );
        assert!(state.pending_prompts().is_empty());
        // The hydrate seed replaces the list; a promoted row never
        // re-queues (transport.ts:971-977).
        state.seed_pending(&[
            json!({ "id": "msg_q1", "sessionID": "s1", "type": "user", "payload": { "text": "first" }, "delivery": "queue", "time": { "created": 1 } }),
            json!({ "id": "msg_q3", "sessionID": "s1", "type": "user", "payload": { "text": "third" }, "delivery": "queue", "time": { "created": 2 } }),
            json!({ "id": "msg_s", "sessionID": "s1", "type": "synthetic", "payload": { "text": "note" }, "delivery": "queue", "time": { "created": 3 } }),
        ]);
        let ids: Vec<&str> = state.pending_prompts().iter().map(|p| p.message_id.as_str()).collect();
        assert_eq!(ids, vec!["msg_q3"], "the promoted row is skipped, non-user rows dropped");
    }

    /// G-221 block 2 — the direct-shell rows (transport.ts:1125-1164):
    /// `started` renders the `$ command` start row once, `ended`
    /// renders the merged output (completed) or the output + exit-code
    /// error pair, the truncation marker closes an incomplete page,
    /// and a late `ended` (no `started`) opens the start row itself.
    #[test]
    fn shell_events_render_start_output_and_error_rows() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let started = state.fold(&event(
            "session.shell.started",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_1", "command": "echo hi", "status": "running" } }),
        ));
        match started {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 1);
                assert_eq!(commits[0].shell.as_deref(), Some("echo hi"));
                assert_eq!(commits[0].part_id.as_deref(), Some("shell:shl_1"));
                assert_eq!(commits[0].phase, Phase::Start);
                assert_eq!(commits[0].tool_state, Some("running"));
                assert_eq!(super::super::scrollback::entry_body(&commits[0]), vec!["$ echo hi"]);
            }
            other => panic!("started folded: {other:?}"),
        }
        // A duplicate started is silent.
        assert_eq!(
            state.fold(&event(
                "session.shell.started",
                &json!({ "sessionID": "s1", "shell": { "id": "shl_1", "command": "echo hi", "status": "running" } }),
            )),
            Fold::Ignored
        );
        // Exit 0: the output row (progress, completed).
        let ended = state.fold(&event(
            "session.shell.ended",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_1", "command": "echo hi", "status": "exited", "exit": 0 },
                     "output": { "output": "hi\n", "cursor": 3, "size": 3, "truncated": false } }),
        ));
        match ended {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 1);
                assert_eq!(commits[0].phase, Phase::Progress);
                assert_eq!(commits[0].tool_state, Some("completed"));
                assert_eq!(commits[0].text, "hi\n");
            }
            other => panic!("ended folded: {other:?}"),
        }
        // Exit 2: the output row (running) + the error final.
        state.fold(&event(
            "session.shell.started",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_2", "command": "false", "status": "running" } }),
        ));
        let failed = state.fold(&event(
            "session.shell.ended",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_2", "command": "false", "status": "exited", "exit": 2 },
                     "output": { "output": "", "cursor": 0, "size": 0, "truncated": false } }),
        ));
        match failed {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 1, "an empty output skips the progress row");
                assert_eq!(commits[0].text, "Shell exited with code 2");
                assert_eq!(commits[0].phase, Phase::Final);
                assert_eq!(commits[0].tool_state, Some("error"));
            }
            other => panic!("failed folded: {other:?}"),
        }
        // A killed shell with output: both rows; a timeout without
        // exit renders `Shell {status}`.
        state.fold(&event(
            "session.shell.started",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_3", "command": "sleep 9", "status": "running" } }),
        ));
        let killed = state.fold(&event(
            "session.shell.ended",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_3", "command": "sleep 9", "status": "timeout" },
                     "output": { "output": "partial", "cursor": 3, "size": 3, "truncated": false } }),
        ));
        match killed {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 2);
                assert_eq!(commits[0].text, "partial");
                assert_eq!(commits[1].text, "Shell timeout");
            }
            other => panic!("killed folded: {other:?}"),
        }
        // An incomplete page closes with the truncation marker
        // (transport.ts:360-361); a LATE ended (no started event)
        // opens the start row itself (transport.ts:1149-1155).
        let late = state.fold(&event(
            "session.shell.ended",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_4", "command": "cat big.log", "status": "exited", "exit": 0 },
                     "output": { "output": "tail", "cursor": 4, "size": 1048576, "truncated": false } }),
        ));
        match late {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 2, "the late start row + the terminal rows");
                assert_eq!(commits[0].phase, Phase::Start, "the late start row");
                assert_eq!(
                    commits[1].text, "tail\n[output truncated]",
                    "the marker joins the incomplete page"
                );
            }
            other => panic!("late folded: {other:?}"),
        }
    }

    /// G-221 block 3 — skill activation (transport.ts:1078-1084): the
    /// event-id-derived message row `→ Skill "name"` renders once;
    /// replays of the same message stay silent.
    #[test]
    fn skill_activated_renders_the_arrow_row_once() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let activated = json!({ "type": "session.skill.activated", "id": "evt_sk1", "data": { "sessionID": "s1", "id": "skl_1", "name": "deploy", "text": "…" } });
        match state.fold(&activated) {
            Fold::Commit(commit) => {
                assert_eq!(commit.text, "→ Skill \"deploy\"");
                assert_eq!(commit.kind, CommitKind::System);
                assert_eq!(commit.tool.as_deref(), Some("skill"));
                assert_eq!(commit.message_id.as_deref(), Some("msg_sk1"), "evt_ → msg_");
                assert_eq!(commit.part_id.as_deref(), Some("skill:msg_sk1"));
            }
            other => panic!("activated folded: {other:?}"),
        }
        assert_eq!(state.fold(&activated), Fold::Ignored, "one row per message");
        // The replay arm shares the dedup.
        assert!(state.replay_skill_message("msg_sk1", "deploy").is_empty());
        assert_eq!(
            state.replay_skill_message("msg_sk2", "review").len(),
            1,
            "a different message renders"
        );
    }

    /// G-221 block 4 — server-driven compaction (transport.ts:1086-
    /// 1124): started renders the `Compaction` divider header, delta
    /// streams the summary fragment, ended closes with the unseen
    /// tail + the final row, failed renders the error (aborted stays
    /// silent), and events without an active compaction fold to
    /// nothing.
    #[test]
    fn compaction_events_render_header_summary_and_error() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        // Without an active compaction everything folds to nothing.
        assert_eq!(
            state.fold(&event("session.compaction.delta", &json!({ "sessionID": "s1", "text": "x" }))),
            Fold::Ignored
        );
        let started = state.fold(&event(
            "session.compaction.started",
            &json!({ "sessionID": "s1", "reason": "auto", "recent": "r", "inputID": "msg_cmp" }),
        ));
        match started {
            Fold::Commit(commit) => {
                assert!(commit.compaction, "the divider row");
                assert_eq!(commit.text, "Compaction");
                assert_eq!(commit.part_id.as_deref(), Some("compaction:header"));
            }
            other => panic!("started folded: {other:?}"),
        }
        let delta = state.fold(&event(
            "session.compaction.delta",
            &json!({ "sessionID": "s1", "text": "Summarizing " }),
        ));
        match delta {
            Fold::Commit(commit) => {
                assert_eq!(commit.kind, CommitKind::Assistant);
                assert_eq!(commit.text, "Summarizing ");
                assert_eq!(commit.part_id.as_deref(), Some("compaction:summary"));
            }
            other => panic!("delta folded: {other:?}"),
        }
        // A duplicate chunk without a prior projection still appends
        // (the reconciler's covered-consume needs a projected tail —
        // `fragments.delta`, stream-v2.fragment.ts:31-40).
        let delta = state.fold(&event(
            "session.compaction.delta",
            &json!({ "sessionID": "s1", "text": "everything" }),
        ));
        assert!(matches!(delta, Fold::Commit(_)), "an uncovered chunk appends");
        let ended = state.fold(&event(
            "session.compaction.ended",
            &json!({ "sessionID": "s1", "reason": "auto", "text": "Summarizing everything.", "recent": "r" }),
        ));
        match ended {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 2, "the unseen tail + the closing row");
                assert_eq!(commits[0].text, ".", "only the suffix past the committed prefix");
                assert_eq!(commits[0].phase, Phase::Progress);
                assert_eq!(commits[1].phase, Phase::Final);
            }
            other => panic!("ended folded: {other:?}"),
        }
        // A failed compaction: aborted closes silently, anything else
        // errors (the event id names the compaction when `inputID` is
        // absent — transport.ts:1086-1093).
        state.fold(&json!({ "type": "session.compaction.started", "id": "evt_c2", "data": { "sessionID": "s1", "reason": "manual", "recent": "r" } }));
        let failed = state.fold(&event(
            "session.compaction.failed",
            &json!({ "sessionID": "s1", "reason": "manual", "error": { "type": "api", "message": "summarizer down" } }),
        ));
        match failed {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 2);
                assert_eq!(commits[1].kind, CommitKind::Error);
                assert_eq!(commits[1].text, "summarizer down");
                assert_eq!(commits[1].part_id.as_deref(), Some("compaction:error"));
                assert_eq!(commits[1].message_id.as_deref(), Some("msg_c2"), "evt_ → msg_");
            }
            other => panic!("failed folded: {other:?}"),
        }
        state.fold(&json!({ "type": "session.compaction.started", "id": "evt_c3", "data": { "sessionID": "s1", "reason": "manual", "recent": "r" } }));
        let aborted = state.fold(&event(
            "session.compaction.failed",
            &json!({ "sessionID": "s1", "reason": "manual", "error": { "type": "aborted", "message": "interrupted" } }),
        ));
        match aborted {
            Fold::Commits(commits) => {
                assert_eq!(commits.len(), 1);
                assert_eq!(commits[0].kind, CommitKind::Assistant, "no error row on abort");
            }
            other => panic!("aborted folded: {other:?}"),
        }
    }

    /// G-221 block 4 (replay) — the compaction message walk
    /// (transport.ts:821-841): a completed message renders header +
    /// summary + close, a failed one header + error (aborted silent),
    /// and a re-replay stays silent (the once-guard).
    #[test]
    fn compaction_replay_renders_once() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let completed = json!({
            "id": "msg_c1", "type": "compaction", "status": "completed", "reason": "auto",
            "summary": "Earlier turns distilled.", "recent": "recent",
        });
        let commits = state.replay_compaction_message(&completed);
        assert_eq!(commits.len(), 3, "header + summary + close");
        assert!(commits[0].compaction);
        assert_eq!(commits[1].text, "Earlier turns distilled.");
        assert_eq!(commits[2].phase, Phase::Final);
        assert!(state.replay_compaction_message(&completed).is_empty(), "once per message");
        let failed = json!({
            "id": "msg_c2", "type": "compaction", "status": "failed", "reason": "auto",
            "error": { "type": "api", "message": "quota" },
        });
        let commits = state.replay_compaction_message(&failed);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[1].text, "quota");
        let aborted = json!({
            "id": "msg_c3", "type": "compaction", "status": "failed", "reason": "auto",
            "error": { "type": "aborted", "message": "interrupted" },
        });
        assert!(state.replay_compaction_message(&aborted).is_empty());
    }

    /// G-221 block 2 (replay) — the shell message walk
    /// (transport.ts:790-819): a settled shell renders the start +
    /// terminal rows, a running one only the start row (the live
    /// `shell.ended` completes it), and replays stay silent.
    #[test]
    fn shell_replay_renders_settled_and_running() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        let settled = json!({
            "id": "msg_sh1", "type": "shell", "shellID": "shl_r1", "command": "echo replayed",
            "status": "exited", "exit": 0,
            "output": { "output": "replayed\n", "cursor": 9, "size": 9, "truncated": false },
            "time": { "created": 1, "completed": 2 },
        });
        let commits = state.replay_shell_message(&settled);
        assert_eq!(commits.len(), 2, "start + terminal");
        assert_eq!(commits[0].phase, Phase::Start);
        assert_eq!(commits[1].tool_state, Some("completed"));
        assert!(state.replay_shell_message(&settled).is_empty(), "replays dedup");
        let running = json!({
            "id": "msg_sh2", "type": "shell", "shellID": "shl_r2", "command": "tail -f log",
            "status": "running", "time": { "created": 3 },
        });
        let commits = state.replay_shell_message(&running);
        assert_eq!(commits.len(), 1, "only the start row while running");
        // The live ended event completes it (the settled display).
        let ended = state.fold(&event(
            "session.shell.ended",
            &json!({ "sessionID": "s1", "shell": { "id": "shl_r2", "command": "tail -f log", "status": "exited", "exit": 0 },
                     "output": { "output": "done", "cursor": 4, "size": 4, "truncated": false } }),
        ));
        assert!(matches!(ended, Fold::Commits(_)), "the live event settles the running shell");
    }

    /// G-221 block 5 — `session.tool.input.ended` (transport.ts:1260-
    /// 1272): the streaming caption row finalizes with the complete
    /// input detail; after `tool.called` (or with no streaming part)
    /// the event folds to nothing.
    #[test]
    fn tool_input_ended_finalizes_the_streaming_caption() {
        let mut state = TransportState { session_id: "s1".into(), ..Default::default() };
        // The REAL wire shape: `id`/`name` (no partID/tool keys).
        let started = state.fold(&event(
            "session.tool.input.started",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_1", "name": "bash" }),
        ));
        match started {
            Fold::Commit(commit) => {
                assert_eq!(commit.text, "Bash", "no input yet — the bare caption");
                assert_eq!(commit.tool.as_deref(), Some("bash"));
                assert_eq!(commit.part_id.as_deref(), Some("call_1"));
            }
            other => panic!("started folded: {other:?}"),
        }
        let ended = state.fold(&event(
            "session.tool.input.ended",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_1",
                     "text": "{\"command\":\"rm -rf /tmp/oc-g221\"}" }),
        ));
        match ended {
            Fold::Commit(commit) => {
                assert_eq!(commit.text, "Bash · rm -rf /tmp/oc-g221", "the complete input detail");
                assert_eq!(commit.phase, Phase::Progress);
                assert_eq!(commit.part_id.as_deref(), Some("call_1"));
            }
            other => panic!("ended folded: {other:?}"),
        }
        // A second ended (nothing streaming) folds to nothing.
        assert_eq!(
            state.fold(&event(
                "session.tool.input.ended",
                &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_1", "text": "{}" }),
            )),
            Fold::Ignored
        );
        // After `tool.called` the part is no longer streaming — an
        // ended event never re-fires (transport.ts:1260-1272's
        // streaming guard).
        state.fold(&event(
            "session.tool.input.started",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_2", "name": "read" }),
        ));
        state.fold(&event(
            "session.tool.called",
            &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_2", "input": { "file": "x.rs" } }),
        ));
        assert_eq!(
            state.fold(&event(
                "session.tool.input.ended",
                &json!({ "sessionID": "s1", "assistantMessageID": "msg_a", "id": "call_2", "text": "{\"file\":\"y.rs\"}" }),
            )),
            Fold::Ignored,
            "a called part is no longer streaming"
        );
    }
}
