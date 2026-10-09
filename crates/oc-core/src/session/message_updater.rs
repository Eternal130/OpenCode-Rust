//! Port of `session/message-updater.ts` — the `session_message` part state
//! machine driven by every message-level durable event.
//!
//! The [`Adapter`] mirrors the TS interface over one transaction: reads go
//! to `session_message`/`session_v2`, writes update the assistant/shell/
//! compaction rows or append new ones. [`SessionMessageUpdater::update`]
//! ports the exhaustive `Match.discriminatorsExhaustive("type")` dispatch,
//! including the newest-incomplete-assistant rule, the
//! `findLast`-based part lookups, and the retry clearing at step/execution
//! terminals.

#![allow(
    clippy::needless_lifetimes, // adapter borrows stay explicit
    clippy::redundant_clone, // draft round-trips clone deliberately
    clippy::map_unwrap_or,
    clippy::match_same_arms, // the exhaustive dispatch mirrors the TS table
    clippy::assigning_clones,
    clippy::redundant_closure,
)]
#![allow(
    clippy::too_many_lines, // the exhaustive event dispatch is one table
    clippy::items_after_statements, // per-arm helpers mirror the TS closures
)]
use oc_schema::common::{FinishReason, Record, SessionError, Usd};
use oc_schema::common::{RelativePath, TokenUsage};
use oc_schema::id::{AgentId, MessageId, ProjectId, ShellId, SkillId, SkillName};
use oc_schema::location;
use oc_schema::model;
use oc_schema::session_event::{
    self as event, AgentSelected, CompactionEnded, CompactionFailed, CompactionStarted,
    MessageContentUpdated, ModelSelected, Moved, ReasoningEnded, ReasoningStarted, RetryScheduled,
    ShellEnded, ShellStarted, StepEnded, StepFailed, StepStarted, StepStreamed, Synthetic,
    TextEnded, ToolCalled, ToolFailed, ToolInputEnded, ToolInputStarted, ToolSuccess,
};
use oc_schema::session_message::{
    self as message, AssistantContent, AssistantRetry, AssistantSnapshot, AssistantTime,
    IdleOutcome, Metadata, ToolState, ToolTime,
};

use crate::bus::DurableEvent;
use crate::database::error::Result;
use crate::database::schema::decode_json;

/// Converts a JSON object value into a [`Record`] (metadata / provider state).
fn record(value: serde_json::Value) -> Record {
    match value {
        serde_json::Value::Object(object) => object.into_iter().collect(),
        _ => Record::default(),
    }
}

fn record_opt(value: Option<serde_json::Value>) -> Option<Record> {
    value.map(record)
}

/// Previous-location triple for `location-switched` messages.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviousLocation {
    pub location: location::Ref,
    pub project_id: Option<ProjectId>,
    pub subpath: Option<String>,
}

/// The persistence surface the updater needs (`message-updater.ts`
/// `Adapter`), bound to one event's session.
pub trait Adapter {
    fn get_agent(&mut self) -> Result<Option<AgentId>>;
    fn get_model(&mut self) -> Result<Option<model::Ref>>;
    fn get_location(&mut self) -> Result<Option<PreviousLocation>>;
    /// Newest assistant row when still incomplete (`getCurrentAssistant`).
    fn get_current_assistant(&mut self) -> Result<Option<message::Info>>;
    fn get_assistant(&mut self, message_id: &str) -> Result<Option<message::Info>>;
    fn get_shell(&mut self, shell_id: &str) -> Result<Option<message::Info>>;
    /// The newest `compaction` row still `running` (`getCompaction`).
    fn get_compaction(&mut self) -> Result<Option<message::Info>>;
    fn update_assistant(&mut self, message: &message::Info) -> Result<()>;
    fn update_shell(&mut self, message: &message::Info) -> Result<()>;
    fn update_compaction(&mut self, message: &message::Info) -> Result<()>;
    fn append_message(&mut self, message: &message::Info) -> Result<()>;
}

/// The updater (stateless dispatch over [`Adapter`]).
pub struct SessionMessageUpdater;

fn decode_data<T: serde::de::DeserializeOwned>(event: &DurableEvent) -> Result<T> {
    Ok(serde_json::from_value(event.data.clone())?)
}

/// Extracts a required string field from raw event data.
fn field_str(event: &DurableEvent, key: &str) -> Result<String> {
    event
        .data
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| crate::database::error::DbError::Pragma(format!("missing field {key}")))
}

/// Decodes a stored `data` column + id + type into a message.
pub(crate) fn decode_row(id: &str, type_: &str, data: &str) -> Result<message::Info> {
    let mut value = decode_json(data)?;
    let object = value.as_object_mut().ok_or_else(|| {
        crate::database::error::DbError::Pragma("message data not an object".into())
    })?;
    object.insert("id".into(), serde_json::json!(id));
    object.insert("type".into(), serde_json::json!(type_));
    Ok(serde_json::from_value(value)?)
}

/// The mutably-owned assistant projection (`WritableDraft<Assistant>`).
struct AssistantDraft {
    id: MessageId,
    metadata: Option<Metadata>,
    time: AssistantTime,
    agent: AgentId,
    model: model::Ref,
    content: Vec<AssistantContent>,
    snapshot: Option<AssistantSnapshot>,
    finish: Option<FinishReason>,
    raw_finish: Option<String>,
    provider_state: Option<Record>,
    cost: Option<Usd>,
    tokens: Option<TokenUsage>,
    error: Option<SessionError>,
    retry: Option<AssistantRetry>,
}

impl AssistantDraft {
    fn from_info(info: &message::Info) -> Option<Self> {
        match info {
            message::Info::Assistant {
                id,
                metadata,
                time,
                agent,
                model,
                content,
                snapshot,
                finish,
                raw_finish,
                provider_state,
                cost,
                tokens,
                error,
                retry,
            } => Some(Self {
                id: id.clone(),
                metadata: metadata.clone(),
                time: *time,
                agent: agent.clone(),
                model: model.clone(),
                content: content.clone(),
                snapshot: snapshot.clone(),
                finish: *finish,
                raw_finish: raw_finish.clone(),
                provider_state: provider_state.clone(),
                cost: *cost,
                tokens: tokens.clone(),
                error: error.clone(),
                retry: retry.clone(),
            }),
            _ => None,
        }
    }

    fn to_info(&self) -> message::Info {
        message::Info::Assistant {
            id: self.id.clone(),
            metadata: self.metadata.clone(),
            time: self.time,
            agent: self.agent.clone(),
            model: self.model.clone(),
            content: self.content.clone(),
            snapshot: self.snapshot.clone(),
            finish: self.finish,
            raw_finish: self.raw_finish.clone(),
            provider_state: self.provider_state.clone(),
            cost: self.cost,
            tokens: self.tokens.clone(),
            error: self.error.clone(),
            retry: self.retry.clone(),
        }
    }
}

/// `latestTool`: the last tool part with `id` (`findLast`).
fn latest_tool<'a>(
    assistant: &'a mut AssistantDraft,
    id: &str,
) -> Option<&'a mut AssistantContent> {
    assistant.content.iter_mut().rev().find(|item| match item {
        AssistantContent::Tool { id: tool_id, .. } => tool_id == id,
        _ => false,
    })
}

/// `latestText`.
fn latest_text<'a>(assistant: &'a mut AssistantDraft) -> Option<&'a mut AssistantContent> {
    assistant.content.iter_mut().rev().find(|item| matches!(item, AssistantContent::Text { .. }))
}

/// `latestReasoning`: the last incomplete reasoning part.
fn latest_reasoning<'a>(assistant: &'a mut AssistantDraft) -> Option<&'a mut AssistantContent> {
    assistant.content.iter_mut().rev().find(|item| match item {
        AssistantContent::Reasoning { time, .. } => {
            time.is_none_or(|time| time.completed.is_none())
        }
        _ => false,
    })
}

impl SessionMessageUpdater {
    /// Ports `SessionMessageUpdater.update(adapter, event)`.
    pub fn update(adapter: &mut dyn Adapter, event: &DurableEvent) -> Result<()> {
        let created = event.created;
        let event_message_id = MessageId::from_event(&event.id);

        // updateOwnedAssistant: read by id, apply recipe when present.
        fn owned(
            adapter: &mut dyn Adapter,
            message_id: &str,
            recipe: impl FnOnce(&mut AssistantDraft),
        ) -> Result<()> {
            let Some(info) = adapter.get_assistant(message_id)? else { return Ok(()) };
            let Some(mut draft) = AssistantDraft::from_info(&info) else {
                return Ok(());
            };
            recipe(&mut draft);
            adapter.update_assistant(&draft.to_info())
        }

        match event.type_.as_str() {
            "session.created"
            | "session.viewed"
            | "session.usage.recorded"
            | "session.renamed"
            | "session.metadata.updated"
            | "session.permissions"
            | "session.deleted"
            | "session.forked"
            | "session.inbox.delivered"
            | "session.inbox.enqueued"
            | "session.inbox.cancelled"
            | "session.inbox.delivery.changed"
            | "session.execution.started"
            | "session.revert.staged"
            | "session.revert.cleared"
            | "session.revert.committed" => Ok(()),

            "session.message.content.updated" => {
                let data: MessageContentUpdated = decode_data(event)?;
                owned(adapter, data.message_id.as_str(), |draft| {
                    draft.content = data.content.clone();
                })
            }

            "session.agent.selected" => {
                let data: AgentSelected = decode_data(event)?;
                let previous = match data.previous.clone() {
                    Some(previous) => Some(previous),
                    None => adapter.get_agent()?,
                };
                adapter.append_message(&message::Info::AgentSwitched {
                    id: event_message_id,
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    agent: data.agent,
                    previous,
                })
            }

            "session.model.selected" => {
                let data: ModelSelected = decode_data(event)?;
                let previous = match data.previous.clone() {
                    Some(previous) => Some(previous),
                    None => adapter.get_model()?,
                };
                adapter.append_message(&message::Info::ModelSwitched {
                    id: event_message_id,
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    model: data.model,
                    previous,
                })
            }

            "session.moved" => {
                let data: Moved = decode_data(event)?;
                let previous = adapter.get_location()?;
                adapter.append_message(&message::Info::LocationSwitched {
                    id: event_message_id,
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    location: data.location,
                    project_id: Some(data.project_id),
                    subpath: data.subpath.map(|path| RelativePath(path.0)),
                    previous: previous.map(|previous| message::LocationSwitchedPrevious {
                        location: previous.location,
                        project_id: previous.project_id,
                        subpath: previous.subpath.map(|path| RelativePath(path)),
                    }),
                })
            }

            "session.execution.succeeded" => idle(adapter, event, IdleOutcome::Succeeded),
            "session.execution.failed" => idle(adapter, event, IdleOutcome::Failed),

            // Shutdown keeps the execution claim; the resumed drain continues
            // the turn, so the current assistant's retry marker just clears.
            "session.execution.interrupted" => {
                let data: event::ExecutionInterrupted = decode_data(event)?;
                if data.reason == event::InterruptReason::Shutdown {
                    clear_current_retry(adapter)
                } else {
                    idle(adapter, event, IdleOutcome::Interrupted)
                }
            }

            "session.instructions.updated" => {
                let data: event::InstructionsUpdated = decode_data(event)?;
                let Some(text) = data.text else { return Ok(()) };
                let keys: Vec<String> = data.delta.keys().cloned().collect();
                let mut metadata = record_opt(event.metadata.clone()).unwrap_or_default();
                metadata.insert("notice".into(), serde_json::json!("instructions"));
                metadata.insert("instructionSources".into(), serde_json::json!(keys));
                adapter.append_message(&message::Info::System {
                    id: event_message_id,
                    metadata: Some(metadata),
                    time: message::Time { created },
                    text,
                    description: Some(format!("Instructions updated: {}", keys.join(", "))),
                })
            }

            "session.synthetic" => {
                let data: Synthetic = decode_data(event)?;
                adapter.append_message(&message::Info::Synthetic {
                    id: event_message_id,
                    metadata: data.metadata.clone(),
                    time: message::Time { created },
                    text: data.text,
                    description: data.description,
                })
            }

            "session.skill.activated" => {
                let data: event::SkillActivated = decode_data(event)?;
                adapter.append_message(&message::Info::Skill {
                    id: event_message_id,
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    skill: SkillId::parse(data.id.as_str().to_string()),
                    name: SkillName::parse(data.name.as_str().to_string()),
                    text: data.text,
                })
            }

            "session.shell.started" => {
                let data: ShellStarted = decode_data(event)?;
                let background = data
                    .shell
                    .metadata
                    .get("background")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let metadata = if background {
                    let mut merged =
                        event.metadata.clone().unwrap_or_else(|| serde_json::json!({}));
                    if let Some(object) = merged.as_object_mut() {
                        object.insert("background".into(), serde_json::json!(true));
                    }
                    Some(record(merged))
                } else {
                    record_opt(event.metadata.clone())
                };
                adapter.append_message(&message::Info::Shell {
                    id: event_message_id,
                    metadata,
                    shell_id: ShellId::parse(data.shell.id.as_str().to_string())
                        .unwrap_or_else(|err| ShellId(err.value)),
                    command: data.shell.command,
                    status: data.shell.status,
                    exit: data.shell.exit,
                    output: None,
                    time: message::ShellTime { created, completed: None },
                })
            }

            "session.shell.ended" => {
                let data: ShellEnded = decode_data(event)?;
                let Some(current) = adapter.get_shell(data.shell.id.as_str())? else {
                    return Ok(());
                };
                let message::Info::Shell { id, metadata, shell_id, command, time, .. } = current
                else {
                    return Ok(());
                };
                adapter.update_shell(&message::Info::Shell {
                    id,
                    metadata,
                    shell_id,
                    command,
                    status: data.shell.status,
                    exit: data.shell.exit,
                    output: Some(data.output),
                    time: message::ShellTime { created: time.created, completed: Some(created) },
                })
            }

            "session.step.started" => {
                let data: StepStarted = decode_data(event)?;
                if adapter.get_assistant(data.assistant_message_id.as_str())?.is_some() {
                    let message_id = data.assistant_message_id.clone();
                    return owned(adapter, message_id.as_str(), |draft| {
                        draft.agent = data.agent.clone();
                        draft.model = data.model.clone();
                        draft.retry = None;
                        draft.error = None;
                        draft.finish = None;
                        draft.raw_finish = None;
                        draft.provider_state = None;
                        draft.time = AssistantTime {
                            created: data.started,
                            streamed: None,
                            completed: None,
                        };
                        if let Some(snapshot) = data.snapshot {
                            let mut next = draft.snapshot.clone().unwrap_or_default();
                            next.start = Some(snapshot);
                            draft.snapshot = Some(next);
                        }
                    });
                }
                if let Some(current) = adapter.get_current_assistant()? {
                    if let Some(mut draft) = AssistantDraft::from_info(&current) {
                        draft.retry = None;
                        draft.time.completed = Some(created);
                        adapter.update_assistant(&draft.to_info())?;
                    }
                }
                adapter.append_message(&message::Info::Assistant {
                    id: data.assistant_message_id.clone(),
                    metadata: record_opt(event.metadata.clone()),
                    time: AssistantTime { created: data.started, streamed: None, completed: None },
                    agent: data.agent,
                    model: data.model,
                    content: Vec::new(),
                    snapshot: data.snapshot.map(|snapshot| AssistantSnapshot {
                        start: Some(snapshot),
                        end: None,
                        files: None,
                    }),
                    finish: None,
                    raw_finish: None,
                    provider_state: None,
                    cost: None,
                    tokens: None,
                    error: None,
                    retry: None,
                })
            }

            "session.step.streamed" => {
                let data: StepStreamed = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.time.streamed = Some(created);
                })
            }

            "session.step.ended" => {
                let data: StepEnded = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.time.completed = Some(created);
                    draft.finish = Some(data.finish);
                    draft.raw_finish = data.raw_finish.clone();
                    draft.provider_state = data.provider_state.clone();
                    draft.cost = Some(data.cost);
                    draft.tokens = Some(data.tokens.clone());
                    project_terminal_snapshot(draft, data.snapshot, data.files.as_deref());
                })
            }

            "session.step.failed" => {
                let data: StepFailed = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.time.completed = Some(created);
                    draft.finish = Some(
                        data.finish
                            .map(|_| FinishReason::ContentFilter)
                            .unwrap_or(FinishReason::Error),
                    );
                    draft.raw_finish = data.raw_finish.clone();
                    draft.provider_state = data.provider_state.clone();
                    draft.error = Some(data.error.clone());
                    draft.retry = None;
                    if let (Some(cost), Some(tokens)) = (data.cost, data.tokens.clone()) {
                        draft.cost = Some(cost);
                        draft.tokens = Some(tokens);
                    }
                    project_terminal_snapshot(draft, data.snapshot, data.files.as_deref());
                })
            }

            "session.text.started" => {
                let id = field_str(event, "assistantMessageID")?;
                owned(adapter, &id, |draft| {
                    draft.content.push(AssistantContent::Text { text: String::new(), state: None });
                })
            }

            "session.text.ended" => {
                let data: TextEnded = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Text {
                        text: target, state: target_state, ..
                    }) = latest_text(draft)
                    {
                        *target = data.text.clone();
                        *target_state = data.provider_state.clone();
                    }
                })
            }

            "session.tool.input.started" => {
                let data: ToolInputStarted = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.content.push(AssistantContent::Tool {
                        id: data.id.clone(),
                        name: data.name.clone(),
                        executed: None,
                        provider_state: None,
                        provider_result_state: None,
                        state: ToolState::Streaming { input: String::new() },
                        time: ToolTime { created, ran: None, completed: None },
                    });
                })
            }

            "session.tool.input.ended" => {
                let data: ToolInputEnded = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Tool {
                        state: ToolState::Streaming { input },
                        ..
                    }) = latest_tool(draft, &data.id)
                    {
                        *input = data.text.clone();
                    }
                })
            }

            "session.tool.called" => {
                let data: ToolCalled = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Tool {
                        executed,
                        provider_state,
                        time,
                        state,
                        ..
                    }) = latest_tool(draft, &data.id)
                    {
                        *executed = Some(data.executed);
                        // The typed field reads the wire `state` key (Wave C1
                        // oc-schema fix); this is the projection that
                        // previously dropped tool.called provider state.
                        *provider_state = data.provider_state.clone();
                        time.ran = Some(created);
                        *state = ToolState::Running {
                            input: data.input.clone(),
                            metadata: Record::default(),
                        };
                    }
                })
            }

            // Terminal tool events are self-contained: projection is a direct
            // copy and never reaches into ephemeral progress history.
            "session.tool.success" => {
                let data: ToolSuccess = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Tool {
                        executed,
                        provider_result_state,
                        time,
                        state,
                        ..
                    }) = latest_tool(draft, &data.id)
                    {
                        if let ToolState::Running { input, .. } = state {
                            let input = input.clone();
                            finish_tool(
                                executed,
                                provider_result_state,
                                time,
                                state,
                                data.executed,
                                data.result_state.clone(),
                                ToolState::Completed {
                                    input,
                                    content: data.content.clone(),
                                    metadata: data.metadata.clone(),
                                },
                                created,
                            );
                        }
                    }
                })
            }

            "session.tool.failed" => {
                let data: ToolFailed = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Tool {
                        executed,
                        provider_result_state,
                        time,
                        state,
                        ..
                    }) = latest_tool(draft, &data.id)
                    {
                        let unsettled = matches!(
                            state,
                            ToolState::Streaming { .. } | ToolState::Running { .. }
                        );
                        if unsettled {
                            let input = match state {
                                ToolState::Streaming { .. } => Record::default(),
                                ToolState::Running { input, .. } => input.clone(),
                                _ => Record::default(),
                            };
                            finish_tool(
                                executed,
                                provider_result_state,
                                time,
                                state,
                                data.executed,
                                data.result_state.clone(),
                                ToolState::Error {
                                    input,
                                    error: data.error.clone(),
                                    content: data.content.clone(),
                                    metadata: data.metadata.clone(),
                                },
                                created,
                            );
                        }
                    }
                })
            }

            "session.reasoning.started" => {
                let data: ReasoningStarted = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.content.push(AssistantContent::Reasoning {
                        text: String::new(),
                        state: data.provider_state.clone(),
                        time: None,
                    });
                })
            }

            "session.reasoning.ended" => {
                let data: ReasoningEnded = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    if let Some(AssistantContent::Reasoning {
                        text: target,
                        state: target_state,
                        time,
                        ..
                    }) = latest_reasoning(draft)
                    {
                        *target = data.text.clone();
                        let original = time.map_or(created, |time| time.created);
                        *time = Some(message::ReasoningTime {
                            created: original,
                            completed: Some(created),
                        });
                        if let Some(next) = data.provider_state.clone() {
                            *target_state = Some(next);
                        }
                    }
                })
            }

            "session.retry.scheduled" => {
                let data: RetryScheduled = decode_data(event)?;
                let id = data.assistant_message_id.clone();
                owned(adapter, id.as_str(), |draft| {
                    draft.retry = Some(AssistantRetry {
                        attempt: data.attempt,
                        at: data.at,
                        error: data.error,
                    });
                })
            }

            "session.compaction.started" => {
                let data: CompactionStarted = decode_data(event)?;
                adapter.append_message(&message::Info::Compaction(message::Compaction::Running {
                    id: data.input_id.clone().unwrap_or_else(|| event_message_id.clone()),
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    reason: data.reason,
                    summary: String::new(),
                    recent: data.recent.clone(),
                }))
            }

            "session.compaction.ended" => {
                let data: CompactionEnded = decode_data(event)?;
                let current = adapter.get_compaction()?;
                if let Some(message::Info::Compaction(message::Compaction::Running {
                    id,
                    metadata,
                    time,
                    ..
                })) = current
                {
                    let merged = match (metadata, event.metadata.clone()) {
                        (Some(mut left), Some(right)) => {
                            if let serde_json::Value::Object(right) = right {
                                for (key, value) in right {
                                    left.insert(key, value);
                                }
                            }
                            Some(left)
                        }
                        (Some(left), None) => Some(left),
                        (None, Some(right)) => Some(record(right)),
                        (None, None) => None,
                    };
                    return adapter.update_compaction(&message::Info::Compaction(
                        message::Compaction::Completed {
                            id,
                            metadata: merged,
                            time,
                            reason: data.reason,
                            model: data.model,
                            provider_state: data.provider_state,
                            summary: data.text,
                            recent: data.recent,
                            provider_context: data.provider_context,
                            cost: data.cost,
                            tokens: data.tokens,
                        },
                    ));
                }
                adapter.append_message(&message::Info::Compaction(message::Compaction::Completed {
                    id: event_message_id,
                    metadata: record_opt(event.metadata.clone()),
                    time: message::Time { created },
                    reason: data.reason,
                    model: data.model,
                    provider_state: data.provider_state,
                    summary: data.text,
                    recent: data.recent,
                    provider_context: data.provider_context,
                    cost: data.cost,
                    tokens: data.tokens,
                }))
            }

            "session.compaction.failed" => {
                let data: CompactionFailed = decode_data(event)?;
                let current = adapter.get_compaction()?;
                let failed = message::Info::Compaction(message::Compaction::Failed {
                    id: current
                        .as_ref()
                        .map(|info| message_id_of(info))
                        .or_else(|| data.input_id.clone())
                        .unwrap_or_else(|| event_message_id.clone()),
                    metadata: match &current {
                        Some(message::Info::Compaction(message::Compaction::Running {
                            metadata,
                            ..
                        })) => metadata.clone(),
                        _ => record_opt(event.metadata.clone()),
                    },
                    time: match &current {
                        Some(message::Info::Compaction(message::Compaction::Running {
                            time,
                            ..
                        })) => *time,
                        _ => message::Time { created },
                    },
                    reason: data.reason,
                    error: data.error,
                    cost: data.cost,
                    tokens: data.tokens,
                });
                match current {
                    Some(message::Info::Compaction(message::Compaction::Running { .. })) => {
                        adapter.update_compaction(&failed)
                    }
                    _ => adapter.append_message(&failed),
                }
            }

            // Ephemeral deltas never reach a durable projector.
            other => Err(crate::database::error::DbError::Pragma(format!(
                "message updater received unsupported event type {other}"
            ))),
        }
    }
}

fn message_id_of(info: &message::Info) -> MessageId {
    match info {
        message::Info::Compaction(
            message::Compaction::Running { id, .. }
            | message::Compaction::Completed { id, .. }
            | message::Compaction::Failed { id, .. },
        ) => id.clone(),
        _ => MessageId::parse("msg_unknown").unwrap_or_else(|err| MessageId(err.value)),
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_tool(
    executed: &mut Option<bool>,
    provider_result_state: &mut Option<Record>,
    time: &mut ToolTime,
    state: &mut ToolState,
    event_executed: bool,
    event_result_state: Option<Record>,
    next: ToolState,
    created: u64,
) {
    *executed = Some(event_executed || *executed == Some(true));
    *provider_result_state = event_result_state;
    time.completed = Some(created);
    *state = next;
}

/// `projectTerminalSnapshot` (message-updater.ts:22-29).
fn project_terminal_snapshot(
    draft: &mut AssistantDraft,
    snapshot: Option<oc_schema::id::SnapshotId>,
    files: Option<&[RelativePath]>,
) {
    if snapshot.is_none() && files.is_none() {
        return;
    }
    let mut next = draft.snapshot.clone().unwrap_or_default();
    next.end = snapshot;
    if let Some(files) = files {
        if !files.is_empty() {
            next.files = Some(files.to_vec());
        }
    }
    draft.snapshot = Some(next);
}

/// `clearCurrentRetry`.
fn clear_current_retry(adapter: &mut dyn Adapter) -> Result<()> {
    let Some(current) = adapter.get_current_assistant()? else { return Ok(()) };
    let Some(mut draft) = AssistantDraft::from_info(&current) else { return Ok(()) };
    if draft.retry.is_none() {
        return Ok(());
    }
    draft.retry = None;
    adapter.update_assistant(&draft.to_info())
}

/// `idle(outcome)`: clear the current retry then append the idle marker.
fn idle(adapter: &mut dyn Adapter, event: &DurableEvent, outcome: IdleOutcome) -> Result<()> {
    clear_current_retry(adapter)?;
    adapter.append_message(&message::Info::Idle {
        id: MessageId::from_event(&event.id),
        metadata: record_opt(event.metadata.clone()),
        time: message::Time { created: event.created },
        outcome,
    })
}
