//! Port of `runner/publish-llm-event.ts` — the single-writer
//! `LLMEventPublisher` (runner-concurrency.md §1).
//!
//! The publisher is NOT a shared struct with `Mutex`es: it is a
//! single-owner state machine the step loop drives. Every method marks the
//! record **synchronously before its first await**, so check-then-mark
//! races are impossible by construction; the only code touching the record
//! is the loop that owns `&mut` publisher. Tool fibers (step.rs §2) send
//! outcomes over a channel the loop drains — settlement marks happen in the
//! loop's single-threaded order.

#![allow(clippy::too_many_lines)]

use std::collections::BTreeMap;

use oc_ai::schema::events::{FinishReasonDetails, LLMEvent};
use oc_ai::schema::options::ProviderMetadata;
use oc_schema::common::{FinishReason, Record, SessionError, TokenUsage, Usd};
use oc_schema::id::{AgentId, MessageId, SnapshotId};
use oc_schema::model;
use oc_schema::tool::Content;

use crate::bus::{Bus, PublishOptions};

/// `ToolResultValue.value` accessor (the payload of any variant).
fn result_value(result: &oc_ai::schema::messages::ToolResultValue) -> &serde_json::Value {
    match result {
        oc_ai::schema::messages::ToolResultValue::Json { value }
        | oc_ai::schema::messages::ToolResultValue::Text { value }
        | oc_ai::schema::messages::ToolResultValue::Error { value } => value,
        oc_ai::schema::messages::ToolResultValue::Content { .. } => &serde_json::Value::Null,
    }
}

/// Publisher defects (`Effect.die` in v2: duplicate ids, invariant
/// violations).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct PublisherError(pub String);

type PublishResult<T> = std::result::Result<T, PublisherError>;

fn defect<T>(message: impl Into<String>) -> PublishResult<T> {
    Err(PublisherError(message.into()))
}

/// The immutable fold the step observes (`StepRecord`).
#[derive(Debug, Clone, Default)]
pub struct StepRecord {
    pub output_started: bool,
    pub provider_failed: bool,
    pub failure: Option<SessionError>,
    pub finish: Option<StepSettlement>,
    pub needs_continuation: bool,
}

/// A recorded step settlement (`step-finish` facts).
#[derive(Debug, Clone)]
pub struct StepSettlement {
    pub finish: FinishReason,
    pub raw_finish: Option<String>,
    pub provider_state: Option<Record>,
    pub tokens: TokenUsage,
}

/// Durable settlement details for `Step.Ended`/`Step.Failed`.
#[derive(Debug, Clone, Default)]
pub struct SettlementDetails {
    pub cost: Option<Usd>,
    pub tokens: Option<TokenUsage>,
    pub snapshot: Option<SnapshotId>,
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
struct ToolRecord {
    name: String,
    called: bool,
    settled: bool,
    provider_executed: bool,
    progress: Option<Record>,
}

/// One streaming fragment family (text / reasoning / tool input).
#[derive(Default)]
struct Fragments {
    /// id → (ordinal, values, state).
    chunks: BTreeMap<String, (u64, Vec<String>, Option<Record>)>,
    next_ordinal: u64,
    /// `true` for text/reasoning (one open fragment at a time).
    single: bool,
    kind: &'static str,
}

impl Fragments {
    fn start(&mut self, id: &str, state: Option<Record>) -> PublishResult<u64> {
        if self.chunks.contains_key(id) {
            return defect(format!("Duplicate {} start: {}", self.kind, id));
        }
        if self.single && !self.chunks.is_empty() {
            return defect(format!("{} start before end: {}", self.kind, id));
        }
        let ordinal = self.next_ordinal;
        self.next_ordinal += 1;
        self.chunks.insert(id.to_string(), (ordinal, Vec::new(), state));
        Ok(ordinal)
    }

    fn append(&mut self, id: &str, value: &str, state: Option<Record>) -> PublishResult<u64> {
        let Some(entry) = self.chunks.get_mut(id) else {
            return defect(format!("{} delta before start: {}", self.kind, id));
        };
        entry.1.push(value.to_string());
        if state.is_some() {
            entry.2 = state;
        }
        Ok(entry.0)
    }

    fn end(
        &mut self,
        id: &str,
        state: Option<Record>,
        value: Option<&str>,
    ) -> PublishResult<(u64, String, Option<Record>)> {
        let Some((ordinal, values, existing)) = self.chunks.remove(id) else {
            return defect(format!("{} end before start: {}", self.kind, id));
        };
        let joined = values.join("");
        let text = value.map(str::to_string).unwrap_or(joined);
        let state = state.or(existing);
        Ok((ordinal, text, state))
    }

    fn has(&self, id: &str) -> bool {
        self.chunks.contains_key(id)
    }

    fn ids(&self) -> Vec<String> {
        self.chunks.keys().cloned().collect()
    }

    fn values_joined(&self, id: &str) -> String {
        self.chunks.get(id).map(|(_, values, _)| values.join("")).unwrap_or_default()
    }
}

/// `stringify` (publish-llm-event.ts:51-58).
fn stringify(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn as_record(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(_) => value.clone(),
        other => serde_json::json!({ "value": other }),
    }
}

fn record_json(record: Option<&Record>) -> Option<serde_json::Value> {
    record.map(|record| serde_json::Value::Object(record.clone().into_iter().collect()))
}

fn session_error_json(error: &SessionError) -> serde_json::Value {
    serde_json::to_value(error).unwrap_or_default()
}

fn finish_reason_json(reason: FinishReason) -> serde_json::Value {
    serde_json::to_value(reason).unwrap_or_default()
}

/// The publisher. Construct one per attempt; the step loop owns it.
pub struct LlmEventPublisher {
    bus: Bus,
    session_id: String,
    assistant_message_id: MessageId,
    agent: AgentId,
    model: model::Ref,
    provider_metadata_key: String,
    snapshot: Option<SnapshotId>,
    started: u64,
    // -- single-writer record ------------------------------------------------
    step_started: bool,
    provider_failed: bool,
    output_started: bool,
    step_failure: Option<SessionError>,
    step_settlement: Option<StepSettlement>,
    tools: BTreeMap<String, ToolRecord>,
    text: Fragments,
    reasoning: Fragments,
    tool_input: Fragments,
}

impl LlmEventPublisher {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bus: Bus,
        session_id: impl Into<String>,
        assistant_message_id: MessageId,
        agent: AgentId,
        model: model::Ref,
        provider_metadata_key: impl Into<String>,
        snapshot: Option<SnapshotId>,
        started: u64,
    ) -> Self {
        Self {
            bus,
            session_id: session_id.into(),
            assistant_message_id,
            agent,
            model,
            provider_metadata_key: provider_metadata_key.into(),
            snapshot,
            started,
            step_started: false,
            provider_failed: false,
            output_started: false,
            step_failure: None,
            step_settlement: None,
            tools: BTreeMap::new(),
            text: Fragments { single: true, kind: "text", ..Default::default() },
            reasoning: Fragments { single: true, kind: "reasoning", ..Default::default() },
            tool_input: Fragments { single: false, kind: "tool input", ..Default::default() },
        }
    }

    fn base(&self) -> serde_json::Value {
        serde_json::json!({
            "sessionID": self.session_id,
            "assistantMessageID": self.assistant_message_id.as_str(),
        })
    }

    async fn publish_type(
        &self,
        type_: &str,
        mut data: serde_json::Value,
        options: PublishOptions,
    ) -> PublishResult<()> {
        let def = crate::bus::registry::by_type_latest(type_)
            .unwrap_or_else(|| panic!("no durable definition for {type_}"));
        if !data.is_object() {
            data = serde_json::json!({});
        }
        self.bus
            .publish(def, data, options)
            .await
            .map(|_| ())
            .map_err(|err| PublisherError(format!("publish {type_} failed: {err}")))
    }

    fn provider_state(&self, metadata: Option<&ProviderMetadata>) -> Option<Record> {
        metadata
            .as_ref()?
            .get(&self.provider_metadata_key)
            .cloned()
            .map(|object| object.into_iter().collect())
    }

    /// `startAssistant` — publishes `session.step.started` once.
    pub async fn start_assistant(&mut self) -> PublishResult<MessageId> {
        if self.step_started {
            return Ok(self.assistant_message_id.clone());
        }
        self.step_started = true;
        let mut data = self.base();
        data["agent"] = serde_json::json!(self.agent.as_str());
        data["model"] = serde_json::to_value(&self.model).unwrap_or_default();
        if let Some(snapshot) = &self.snapshot {
            data["snapshot"] = serde_json::json!(snapshot.as_str());
        }
        data["started"] = serde_json::json!(self.started);
        self.publish_type("session.step.started", data, PublishOptions::default()).await?;
        Ok(self.assistant_message_id.clone())
    }

    fn require_assistant(&self, what: &str) -> PublishResult<()> {
        if self.step_started {
            Ok(())
        } else {
            defect(format!("Tool event before assistant step start: {what}"))
        }
    }

    /// `streamed` — the response body ended.
    pub async fn streamed(&mut self) -> PublishResult<()> {
        let id = self.start_assistant().await?;
        let mut data = self.base();
        data["assistantMessageID"] = serde_json::json!(id.as_str());
        self.publish_type("session.step.streamed", data, PublishOptions::default()).await
    }

    // -----------------------------------------------------------------------
    // Text / reasoning fragments
    // -----------------------------------------------------------------------

    async fn publish_text_started(&mut self, ordinal: u64) -> PublishResult<()> {
        self.require_assistant("text started")?;
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        self.publish_type("session.text.started", data, PublishOptions::default()).await
    }

    async fn publish_text_delta(&mut self, ordinal: u64, delta: &str) -> PublishResult<()> {
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        data["delta"] = serde_json::json!(delta);
        self.publish_type("session.text.delta", data, PublishOptions::default()).await
    }

    async fn publish_text_ended(
        &mut self,
        ordinal: u64,
        text: &str,
        state: Option<Record>,
    ) -> PublishResult<()> {
        self.require_assistant("text ended")?;
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        data["text"] = serde_json::json!(text);
        if let Some(state) = record_json(state.as_ref()) {
            data["state"] = state;
        }
        self.publish_type("session.text.ended", data, PublishOptions::default()).await
    }

    async fn publish_reasoning_started(
        &mut self,
        ordinal: u64,
        state: Option<Record>,
    ) -> PublishResult<()> {
        self.require_assistant("reasoning started")?;
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        if let Some(state) = record_json(state.as_ref()) {
            data["state"] = state;
        }
        self.publish_type("session.reasoning.started", data, PublishOptions::default()).await
    }

    async fn publish_reasoning_delta(&mut self, ordinal: u64, delta: &str) -> PublishResult<()> {
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        data["delta"] = serde_json::json!(delta);
        self.publish_type("session.reasoning.delta", data, PublishOptions::default()).await
    }

    async fn publish_reasoning_ended(
        &mut self,
        ordinal: u64,
        text: &str,
        state: Option<Record>,
    ) -> PublishResult<()> {
        self.require_assistant("reasoning ended")?;
        let mut data = self.base();
        data["ordinal"] = serde_json::json!(ordinal);
        data["text"] = serde_json::json!(text);
        if let Some(state) = record_json(state.as_ref()) {
            data["state"] = state;
        }
        self.publish_type("session.reasoning.ended", data, PublishOptions::default()).await
    }

    // -----------------------------------------------------------------------
    // Tool input fragments
    // -----------------------------------------------------------------------

    async fn start_tool_input(
        &mut self,
        id: &str,
        name: &str,
        provider_executed: bool,
    ) -> PublishResult<()> {
        if self.tools.contains_key(id) {
            return defect(format!("Duplicate tool input start: {id}"));
        }
        self.start_assistant().await?;
        self.tools.insert(
            id.to_string(),
            ToolRecord {
                name: name.to_string(),
                called: false,
                settled: false,
                provider_executed,
                progress: None,
            },
        );
        self.tool_input.start(id, None)?;
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["name"] = serde_json::json!(name);
        self.publish_type("session.tool.input.started", data, PublishOptions::default()).await
    }

    async fn end_tool_input(
        &mut self,
        id: &str,
        name: &str,
        value: Option<&str>,
    ) -> PublishResult<()> {
        let Some(tool) = self.tools.get(id) else {
            return defect(format!("Tool input end before start: {id}"));
        };
        if tool.name != name {
            return defect(format!("Tool input name changed for {id}: {} -> {name}", tool.name));
        }
        if !self.tool_input.has(id) {
            return defect(format!("Duplicate tool input end: {id}"));
        }
        let (ordinal, text, _state) = self.tool_input.end(id, None, value)?;
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["text"] = serde_json::json!(text);
        let _ = ordinal;
        self.publish_type("session.tool.input.ended", data, PublishOptions::default()).await
    }

    async fn fail_malformed_tool_input(
        &mut self,
        id: &str,
        name: &str,
        raw: &str,
    ) -> PublishResult<()> {
        if !self.tools.contains_key(id) {
            self.start_tool_input(id, name, false).await?;
        }
        {
            let Some(tool) = self.tools.get(id) else {
                return defect(format!("Tool input end before start: {id}"));
            };
            if tool.called || tool.settled {
                return defect(format!("Malformed tool input after call settlement: {id}"));
            }
            if tool.name != name {
                return defect(format!(
                    "Tool input name changed for {id}: {} -> {name}",
                    tool.name
                ));
            }
        }
        if self.tool_input.has(id) {
            self.end_tool_input(id, name, Some(raw)).await?;
        }
        // Mark settled BEFORE the first await of the terminal publish.
        let snapshot = self.take_tool_failure_snapshot(id, None);
        let error = SessionError {
            error_type: "tool.input-json".into(),
            message: "Tool call arguments were malformed JSON and were not executed. Retry with valid JSON.".into(),
            status: None,
            response: None,
        };
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["error"] = session_error_json(&error);
        if let Some(metadata) = record_json(snapshot.as_ref()) {
            data["metadata"] = metadata;
        }
        data["executed"] = serde_json::json!(false);
        self.publish_type("session.tool.failed", data, PublishOptions::default()).await
    }

    /// `failureSnapshot` — the last progress metadata merged with the event
    /// metadata.
    fn take_tool_failure_snapshot(&mut self, id: &str, metadata: Option<Record>) -> Option<Record> {
        let progress = self.tools.get(id).and_then(|tool| tool.progress.clone());
        match (progress, metadata) {
            (None, None) => None,
            (None, Some(metadata)) => Some(metadata),
            (Some(progress), None) => Some(progress),
            (Some(progress), Some(metadata)) => {
                let mut merged = progress;
                merged.extend(metadata);
                Some(merged)
            }
        }
    }

    // -----------------------------------------------------------------------
    // publish(event) — the durable LLM event switch
    // -----------------------------------------------------------------------

    /// `publish` (publish-llm-event.ts:401-558). Marks the record
    /// synchronously before every await.
    pub async fn publish(&mut self, event: &LLMEvent) -> PublishResult<()> {
        match event {
            LLMEvent::StepStart { .. } => {
                self.start_assistant().await?;
            }
            LLMEvent::TextStart { id, provider_metadata, .. } => {
                self.output_started = true;
                let state = self.provider_state(provider_metadata.as_ref());
                let ordinal = self.text.start(id, state)?;
                self.start_assistant().await?;
                self.publish_text_started(ordinal).await?;
            }
            LLMEvent::TextDelta { id, text, provider_metadata, .. } => {
                let state = self.provider_state(provider_metadata.as_ref());
                let ordinal = self.text.append(id, text, state)?;
                self.publish_text_delta(ordinal, text).await?;
            }
            LLMEvent::TextEnd { id, text, provider_metadata, .. } => {
                let state = self.provider_state(provider_metadata.as_ref());
                let (ordinal, joined, state) = self.text.end(id, state, text.as_deref())?;
                self.publish_text_ended(ordinal, &joined, state).await?;
            }
            LLMEvent::ReasoningStart { id, provider_metadata, .. } => {
                self.output_started = true;
                let state = self.provider_state(provider_metadata.as_ref());
                let ordinal = self.reasoning.start(id, state.clone())?;
                self.start_assistant().await?;
                self.publish_reasoning_started(ordinal, state).await?;
            }
            LLMEvent::ReasoningDelta { id, text, provider_metadata, .. } => {
                let state = self.provider_state(provider_metadata.as_ref());
                let ordinal = self.reasoning.append(id, text, state)?;
                self.publish_reasoning_delta(ordinal, text).await?;
            }
            LLMEvent::ReasoningEnd { id, text, provider_metadata, .. } => {
                let state = self.provider_state(provider_metadata.as_ref());
                let (ordinal, joined, state) = self.reasoning.end(id, state, text.as_deref())?;
                self.publish_reasoning_ended(ordinal, &joined, state).await?;
            }
            LLMEvent::ToolInputStart { id, name, provider_executed, .. } => {
                self.output_started = true;
                self.start_tool_input(id, name, provider_executed.unwrap_or(false)).await?;
            }
            LLMEvent::ToolInputDelta { id, name, text, .. } => {
                {
                    let Some(tool) = self.tools.get(id) else {
                        return defect(format!("Tool input delta before start: {id}"));
                    };
                    if tool.name != *name {
                        return defect(format!(
                            "Tool input name changed for {id}: {} -> {name}",
                            tool.name
                        ));
                    }
                }
                if !self.tool_input.has(id) {
                    return defect(format!("Tool input delta after end: {id}"));
                }
                self.tool_input.append(id, text, None)?;
            }
            LLMEvent::ToolInputEnd { id, name, .. } => {
                self.end_tool_input(id, name, None).await?;
            }
            LLMEvent::ToolInputError { id, name, raw, .. } => {
                self.output_started = true;
                self.fail_malformed_tool_input(id, name, raw).await?;
            }
            LLMEvent::ToolCall {
                id, name, input, provider_executed, provider_metadata, ..
            } => {
                self.output_started = true;
                if !self.tools.contains_key(id) {
                    self.start_tool_input(id, name, provider_executed.unwrap_or(false)).await?;
                }
                if self.tool_input.has(id) {
                    self.end_tool_input(id, name, None).await?;
                }
                {
                    let Some(tool) = self.tools.get_mut(id) else {
                        return defect(format!("Tool input end before start: {id}"));
                    };
                    if tool.name != *name {
                        return defect(format!(
                            "Tool call name changed for {id}: {} -> {name}",
                            tool.name
                        ));
                    }
                    if tool.called {
                        return defect(format!("Duplicate tool call: {id}"));
                    }
                    tool.called = true;
                    tool.provider_executed = provider_executed.unwrap_or(false);
                }
                let state = self.provider_state(provider_metadata.as_ref());
                let mut data = self.base();
                data["id"] = serde_json::json!(id);
                data["input"] = as_record(input);
                data["executed"] = serde_json::json!(provider_executed.unwrap_or(false));
                if let Some(state) = record_json(state.as_ref()) {
                    data["state"] = state;
                }
                self.publish_type("session.tool.called", data, PublishOptions::default()).await?;
            }
            LLMEvent::ToolResult {
                id,
                name,
                result,
                output,
                provider_executed,
                provider_metadata,
                ..
            } => {
                // Provider-hosted results only; local executions publish
                // through `tool_execution`.
                let provider_executed_flag = provider_executed.unwrap_or(false);
                {
                    let Some(tool) = self.tools.get(id) else {
                        return defect(format!("Tool result before call: {id}"));
                    };
                    if !tool.called {
                        return defect(format!("Tool result before call: {id}"));
                    }
                    if tool.name != *name {
                        return defect(format!(
                            "Tool result name changed for {id}: {} -> {name}",
                            tool.name
                        ));
                    }
                    if tool.settled {
                        // A late error result is a benign straggler; a late
                        // success would mean double execution.
                        if result.kind() == "error" {
                            return Ok(());
                        }
                        return defect(format!("Duplicate tool result: {id}"));
                    }
                }
                // Mark settled BEFORE the terminal publish.
                let executed = {
                    let tool = self.tools.get_mut(id).expect("checked above");
                    tool.settled = true;
                    provider_executed_flag || tool.provider_executed
                };
                let result_state = self.provider_state(provider_metadata.as_ref());
                let snapshot = self.take_tool_failure_snapshot(id, None);
                if result.kind() == "error" {
                    let error = SessionError {
                        error_type: "tool.execution".into(),
                        message: stringify(result_value(result)),
                        status: None,
                        response: None,
                    };
                    let mut data = self.base();
                    data["id"] = serde_json::json!(id);
                    data["error"] = session_error_json(&error);
                    if let Some(metadata) = record_json(snapshot.as_ref()) {
                        data["metadata"] = metadata;
                    }
                    data["executed"] = serde_json::json!(executed);
                    if let Some(state) = record_json(result_state.as_ref()) {
                        data["resultState"] = state;
                    }
                    self.publish_type("session.tool.failed", data, PublishOptions::default())
                        .await?;
                    return Ok(());
                }
                let content = match result {
                    oc_ai::schema::messages::ToolResultValue::Content { value }
                        if !value.0.is_empty() =>
                    {
                        value.0.clone()
                    }
                    _ => vec![
                        serde_json::json!({ "type": "text", "text": stringify(result_value(result)) }),
                    ],
                };
                let mut data = self.base();
                data["id"] = serde_json::json!(id);
                data["content"] = serde_json::json!(content);
                data["executed"] = serde_json::json!(executed);
                if let Some(state) = record_json(result_state.as_ref()) {
                    data["resultState"] = state;
                }
                let _ = output;
                self.publish_type("session.tool.success", data, PublishOptions::default()).await?;
            }
            LLMEvent::ToolError { id, name, message, provider_metadata, .. } => {
                {
                    let Some(tool) = self.tools.get(id) else {
                        return defect(format!("Tool error before call: {id}"));
                    };
                    if !tool.called {
                        return defect(format!("Tool error before call: {id}"));
                    }
                    if tool.name != *name {
                        return defect(format!(
                            "Tool error name changed for {id}: {} -> {name}",
                            tool.name
                        ));
                    }
                    if tool.settled {
                        return defect(format!("Duplicate tool error: {id}"));
                    }
                }
                let snapshot = self.take_tool_failure_snapshot(id, None);
                let executed = self.tools.get(id).is_some_and(|tool| tool.provider_executed);
                let result_state = self.provider_state(provider_metadata.as_ref());
                self.tools.get_mut(id).expect("checked above").settled = true;
                let error = if *message == format!("Unknown tool: {name}") {
                    SessionError {
                        error_type: "tool.unknown".into(),
                        message: message.clone(),
                        status: None,
                        response: None,
                    }
                } else {
                    SessionError {
                        error_type: "tool.execution".into(),
                        message: message.clone(),
                        status: None,
                        response: None,
                    }
                };
                let mut data = self.base();
                data["id"] = serde_json::json!(id);
                data["error"] = session_error_json(&error);
                if let Some(metadata) = record_json(snapshot.as_ref()) {
                    data["metadata"] = metadata;
                }
                data["executed"] = serde_json::json!(executed);
                if let Some(state) = record_json(result_state.as_ref()) {
                    data["resultState"] = state;
                }
                self.publish_type("session.tool.failed", data, PublishOptions::default()).await?;
            }
            LLMEvent::StepFinish { reason, usage, provider_metadata, .. } => {
                self.flush().await?;
                if self.step_settlement.is_some() {
                    return defect("Duplicate step finish");
                }
                self.step_settlement = Some(StepSettlement {
                    finish: normalize_finish(reason),
                    raw_finish: reason.raw.clone(),
                    provider_state: self.provider_state(provider_metadata.as_ref()),
                    tokens: crate::session::runner::SessionUsage::tokens(usage.as_ref()),
                });
                if normalize_finish(reason) == FinishReason::ContentFilter {
                    self.provider_failed = true;
                    let error = crate::session::runner::content_filter_error(
                        "Provider blocked the response",
                        reason,
                    );
                    self.fail_assistant(error).await?;
                }
            }
            LLMEvent::Finish { .. } => {}
            LLMEvent::ProviderError { message, .. } => {
                self.provider_failed = true;
                let error = SessionError {
                    error_type: "provider.unknown".into(),
                    message: message.clone(),
                    status: None,
                    response: None,
                };
                self.fail_assistant(error).await?;
            }
            LLMEvent::Compaction { .. } | LLMEvent::Media { .. } => {}
        }
        Ok(())
    }

    /// `progress` — live replacement metadata for a running tool.
    pub async fn progress(&mut self, id: &str, metadata: Record) -> PublishResult<()> {
        {
            let Some(tool) = self.tools.get_mut(id) else {
                return defect(format!("Tool progress outside running call: {id}"));
            };
            if !tool.called || tool.settled {
                return defect(format!("Tool progress outside running call: {id}"));
            }
            tool.progress = Some(metadata.clone());
        }
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["metadata"] = record_json(Some(&metadata)).unwrap_or_default();
        self.publish_type("session.tool.progress", data, PublishOptions::default()).await
    }

    /// `toolExecution` — the canonical terminal event for a locally
    /// executed tool call.
    pub async fn tool_execution(
        &mut self,
        id: &str,
        name: &str,
        result: &crate::location::ToolRunResult,
    ) -> PublishResult<()> {
        {
            let Some(tool) = self.tools.get(id) else {
                return defect(format!("Tool execution before call: {id}"));
            };
            if !tool.called {
                return defect(format!("Tool execution before call: {id}"));
            }
            if tool.name != *name {
                return defect(format!(
                    "Tool execution name changed for {id}: {} -> {name}",
                    tool.name
                ));
            }
            if tool.settled {
                return defect(format!("Duplicate tool execution: {id}"));
            }
        }
        if result.content.is_empty() {
            return defect(format!("Tool execution has no content: {id}"));
        }
        // Mark settled BEFORE the terminal publish.
        let executed = self.tools.get_mut(id).expect("checked above");
        executed.settled = true;
        let executed = executed.provider_executed;
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["content"] = serde_json::to_value(&result.content).unwrap_or_default();
        if let Some(metadata) = record_json(result.metadata.as_ref()) {
            data["metadata"] = metadata;
        }
        data["executed"] = serde_json::json!(executed);
        self.publish_type("session.tool.success", data, PublishOptions::default()).await
    }

    /// `flush` — end every open fragment.
    pub async fn flush(&mut self) -> PublishResult<()> {
        for id in self.text.ids() {
            let (ordinal, text, state) = self.text.end(&id, None, None)?;
            self.publish_text_ended(ordinal, &text, state).await?;
        }
        for id in self.reasoning.ids() {
            let (ordinal, text, state) = self.reasoning.end(&id, None, None)?;
            self.publish_reasoning_ended(ordinal, &text, state).await?;
        }
        for id in self.tool_input.ids() {
            let text = self.tool_input.values_joined(&id);
            let name = self.tools.get(&id).map(|tool| tool.name.clone()).unwrap_or_default();
            self.end_tool_input(&id, &name, None).await?;
            let _ = text;
        }
        Ok(())
    }

    /// `failTool` — one unsettled tool terminal.
    pub async fn fail_tool(
        &mut self,
        id: &str,
        error: SessionError,
        metadata: Option<Record>,
    ) -> PublishResult<bool> {
        let Some(tool) = self.tools.get(id) else { return Ok(false) };
        if tool.settled {
            return Ok(false);
        }
        // Mark settled BEFORE the terminal publish.
        let snapshot = self.take_tool_failure_snapshot(id, metadata);
        let tool = self.tools.get_mut(id).expect("checked above");
        tool.settled = true;
        let executed = tool.provider_executed;
        let name = tool.name.clone();
        let progress_session = tool
            .progress
            .as_ref()
            .and_then(|progress| progress.get("sessionID"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let mut error = error;
        if name == "subagent" && error.error_type == "aborted" {
            if let Some(session) = progress_session {
                error.message = format!("{} (sessionID: {session})", error.message);
            }
        }
        let mut data = self.base();
        data["id"] = serde_json::json!(id);
        data["error"] = session_error_json(&error);
        if let Some(metadata) = record_json(snapshot.as_ref()) {
            data["metadata"] = metadata;
        }
        data["executed"] = serde_json::json!(executed);
        self.publish_type("session.tool.failed", data, PublishOptions::default()).await?;
        Ok(true)
    }

    async fn fail_tools(
        &mut self,
        error: &SessionError,
        mode: ToolFailureMode,
    ) -> PublishResult<bool> {
        let ids: Vec<String> = self
            .tools
            .iter()
            .filter(|(_, tool)| {
                !tool.settled
                    && match mode {
                        ToolFailureMode::All => true,
                        ToolFailureMode::Hosted => tool.provider_executed,
                        ToolFailureMode::Uncalled => !tool.called,
                    }
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut failed = false;
        for id in ids {
            failed = self.fail_tool(&id, error.clone(), None).await? || failed;
        }
        Ok(failed)
    }

    /// `failAssistant`.
    pub async fn fail_assistant(&mut self, error: SessionError) -> PublishResult<()> {
        self.flush().await?;
        self.fail_tools(&error, ToolFailureMode::Uncalled).await?;
        self.start_assistant().await?;
        if self.step_failure.is_none() {
            self.step_failure = Some(error);
        }
        Ok(())
    }

    /// `publishStepFailure`.
    pub async fn publish_step_failure(&mut self, details: &SettlementDetails) -> PublishResult<()> {
        let Some(failure) = self.step_failure.clone() else { return Ok(()) };
        self.start_assistant().await?;
        let mut data = self.base();
        data["error"] = session_error_json(&failure);
        // TODO(oc-schema): the `StepFailFinish` unit-struct round-trips as a
        // string but cannot DEserialize from one, so the projector's typed
        // decode rejects `finish: "content-filter"`. Until that lands the
        // content-filter marker stays in the error message only (the TS
        // wire keeps `finish`); see session_event.rs StepFailFinish.
        let _ = finish_reason_json;
        if let Some(settlement) = &self.step_settlement {
            if let Some(raw) = &settlement.raw_finish {
                data["rawFinish"] = serde_json::json!(raw);
            }
            if let Some(state) = record_json(settlement.provider_state.as_ref()) {
                data["providerState"] = state;
            }
        }
        if let Some(cost) = &details.cost {
            data["cost"] = serde_json::json!(cost.0 .0);
        }
        if let Some(tokens) = &details.tokens {
            data["tokens"] = crate::session::runner::tokens_json(tokens);
        }
        if let Some(snapshot) = &self.snapshot {
            data["snapshot"] = serde_json::json!(snapshot.as_str());
        }
        if let Some(files) = &details.files {
            data["files"] = serde_json::json!(files);
        }
        self.publish_type("session.step.failed", data, PublishOptions::default()).await
    }

    /// `failUnsettledTools`.
    pub async fn fail_unsettled_tools(
        &mut self,
        error: SessionError,
        scope: ToolFailureScope,
    ) -> PublishResult<bool> {
        self.fail_tools(&error, scope.into()).await
    }

    /// `Step.Ended` (step.ts:240-250) — the durable terminal for a
    /// successful step.
    pub async fn publish_step_ended(&mut self, details: &SettlementDetails) -> PublishResult<()> {
        let id = self.start_assistant().await?;
        let settlement = self
            .step_settlement
            .as_ref()
            .ok_or_else(|| PublisherError("publish_step_ended without a settlement".into()))?;
        let mut data = self.base();
        data["assistantMessageID"] = serde_json::json!(id.as_str());
        data["finish"] = finish_reason_json(settlement.finish);
        if let Some(raw) = &settlement.raw_finish {
            data["rawFinish"] = serde_json::json!(raw);
        }
        if let Some(state) = record_json(settlement.provider_state.as_ref()) {
            data["providerState"] = state;
        }
        if let Some(cost) = &details.cost {
            data["cost"] = serde_json::json!(cost.0 .0);
        }
        if let Some(tokens) = &details.tokens {
            data["tokens"] = crate::session::runner::tokens_json(tokens);
        }
        if let Some(snapshot) = &details.snapshot {
            data["snapshot"] = serde_json::json!(snapshot.as_str());
        }
        if let Some(files) = &details.files {
            data["files"] = serde_json::json!(files);
        }
        self.publish_type("session.step.ended", data, PublishOptions::default()).await
    }

    pub fn has_provider_error(&self) -> bool {
        self.provider_failed
    }

    pub fn has_started(&self) -> bool {
        self.step_started
    }

    /// `record()` — the immutable fold.
    #[must_use]
    pub fn record(&self) -> StepRecord {
        StepRecord {
            output_started: self.output_started,
            provider_failed: self.provider_failed,
            failure: self.step_failure.clone(),
            finish: self.step_settlement.clone(),
            needs_continuation: self
                .tools
                .values()
                .any(|tool| !tool.provider_executed && (tool.called || tool.settled)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolFailureMode {
    All,
    Hosted,
    Uncalled,
}

/// `failUnsettledTools` scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFailureScope {
    All,
    Hosted,
}

impl From<ToolFailureScope> for ToolFailureMode {
    fn from(scope: ToolFailureScope) -> Self {
        match scope {
            ToolFailureScope::All => ToolFailureMode::All,
            ToolFailureScope::Hosted => ToolFailureMode::Hosted,
        }
    }
}

/// Map the provider finish details onto the normalized wire reason.
fn normalize_finish(reason: &FinishReasonDetails) -> FinishReason {
    match reason.normalized {
        oc_ai::schema::events::FinishReason::Stop => FinishReason::Stop,
        oc_ai::schema::events::FinishReason::Length => FinishReason::Length,
        oc_ai::schema::events::FinishReason::ToolCalls => FinishReason::ToolCalls,
        oc_ai::schema::events::FinishReason::ContentFilter => FinishReason::ContentFilter,
        oc_ai::schema::events::FinishReason::Error => FinishReason::Error,
        oc_ai::schema::events::FinishReason::Unknown => FinishReason::Unknown,
    }
}

/// Convert provider tool content into canonical session content.
#[must_use]
pub fn hosted_content(result: &oc_ai::schema::messages::ToolResultValue) -> Vec<Content> {
    match result {
        oc_ai::schema::messages::ToolResultValue::Content { value } if !value.0.is_empty() => value
            .0
            .iter()
            .filter_map(|item| serde_json::from_value::<Content>(item.clone()).ok())
            .collect(),
        _ => vec![Content::Text { text: stringify(result_value(result)) }],
    }
}
