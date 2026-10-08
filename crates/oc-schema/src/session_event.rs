//! Port of `session-event.ts` — all 51 session event definitions.
//!
//! Every event is a data struct plus an envelope; the [`Event`] enum is the
//! public union, internally tagged by `type`, mirroring v2's
//! `Schema.Union(...).pipe(Schema.toTaggedUnion("type"))`. Durable events
//! additionally carry the required `durable` envelope
//! (`{aggregateID, seq, version}`); every session aggregate is `"sessionID"`
//! with the version noted per event below.

use serde::{Deserialize, Serialize};

use crate::common::{
    FinishReason, NonNegativeInt, PositiveInt, Record, RelativePath, SessionError, TokenUsage, Usd,
};
use crate::event::{DurableEnvelope, EphemeralEnvelope};
use crate::id::AgentId;
use crate::id::{MessageId, ProjectId, SessionId, SkillId, SkillName, SnapshotId};
use crate::location;
use crate::model;
use crate::permission::Ruleset;
use crate::session_inbox::{self, Delivery};
use crate::session_message::{AssistantContent, CompactionReason};
use crate::session_support::{
    FileDiff, ForkBoundary, InstructionDelta, InstructionEntrySnapshot, InstructionValues,
    ProviderContext, Revert, SessionMetadata,
};
use crate::shell;
use crate::tool::Content;

/// Aggregate name for every durable session event.
pub const AGGREGATE: &str = "sessionID";

/// `session.created` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Created {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    pub location: location::Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<RelativePath>,
    #[serde(rename = "parentID", default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<SessionId>,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<model::Ref>,
    /// Host-supplied annotations resolved at creation, including inherited ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SessionMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Ruleset>,
    pub version: String,
}

/// `session.agent.selected` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentSelected {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub agent: AgentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<AgentId>,
}

/// `session.model.selected` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ModelSelected {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub model: model::Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<model::Ref>,
}

/// `session.moved` (durable v1) — `SessionInbox.MovePayload` fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Moved {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub location: location::Ref,
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<RelativePath>,
}

/// `session.renamed` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Renamed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub title: String,
}

/// `session.metadata.updated` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MetadataUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub metadata: SessionMetadata,
}

/// `session.permissions` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Permissions {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub permissions: Ruleset,
}

/// `session.viewed` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Viewed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Epoch-millisecond idle watermark the viewer observed.
    pub idle: crate::common::Num,
}

/// `session.message.content.updated` (durable v1, replay-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MessageContentUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "messageID")]
    pub message_id: MessageId,
    /// Public events are framed directly, so timestamps are already encoded.
    pub content: Vec<AssistantContent>,
}

/// `session.usage.recorded` (durable v1, internal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsageRecorded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub source: UsageSource,
    pub cost: Usd,
    pub tokens: TokenUsage,
}

/// `UsageRecorded.source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum UsageSource {
    Title,
    Compaction,
}

/// `session.usage.updated` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UsageUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub cost: Usd,
    pub tokens: TokenUsage,
}

/// `session.deleted` (durable v2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Deleted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `session.forked` (durable v2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Forked {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "parentID")]
    pub parent_id: SessionId,
    pub boundary: ForkBoundary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<InstructionValues>,
    #[serde(rename = "instructionEntries", default, skip_serializing_if = "Option::is_none")]
    pub instruction_entries: Option<InstructionEntrySnapshot>,
}

/// `session.inbox.delivered` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxDelivered {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "inboxID")]
    pub inbox_id: MessageId,
}

/// `session.inbox.enqueued` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxEnqueued {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "inboxID")]
    pub inbox_id: MessageId,
    pub item: session_inbox::Item,
}

/// `session.inbox.cancelled` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxCancelled {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "inboxID")]
    pub inbox_id: MessageId,
}

/// `session.inbox.delivery.changed` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InboxDeliveryChanged {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "inboxID")]
    pub inbox_id: MessageId,
    pub delivery: Delivery,
}

/// `session.execution.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `session.execution.succeeded` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionSucceeded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `session.execution.failed` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionFailed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub error: SessionError,
}

/// `session.execution.interrupted` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExecutionInterrupted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub reason: InterruptReason,
}

/// `ExecutionInterrupted.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InterruptReason {
    User,
    Shutdown,
    Superseded,
    Inactivity,
}

/// `session.instructions.updated` (durable v2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstructionsUpdated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub delta: InstructionDelta,
    /// Rendered chronological update shown to the model, frozen at emit time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// `session.synthetic` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Synthetic {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
}

/// `session.skill.activated` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SkillActivated {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub id: SkillId,
    pub name: SkillName,
    pub text: String,
}

/// `session.shell.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub shell: shell::Info,
}

/// `session.shell.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub shell: shell::Info,
    pub output: shell::Output,
}

/// `session.step.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StepStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub agent: AgentId,
    pub model: model::Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotId>,
    /// Request dispatch time, before waiting for provider output.
    pub started: NonNegativeInt,
}

/// `session.step.streamed` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StepStreamed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
}

/// `session.step.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StepEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub finish: FinishReason,
    #[serde(rename = "rawFinish", default, skip_serializing_if = "Option::is_none")]
    pub raw_finish: Option<String>,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
    pub cost: Usd,
    pub tokens: TokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<RelativePath>>,
}

/// `session.step.failed` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StepFailed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub error: SessionError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<StepFailFinish>,
    #[serde(rename = "rawFinish", default, skip_serializing_if = "Option::is_none")]
    pub raw_finish: Option<String>,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Usd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<RelativePath>>,
}

/// `StepFailed.finish` — the literal `"content-filter"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename = "content-filter")]
pub struct StepFailFinish;

/// `session.text.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TextStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
}

/// `session.text.delta` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TextDelta {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
    pub delta: String,
}

/// `session.text.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TextEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
    pub text: String,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
}

/// `session.reasoning.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReasoningStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
}

/// `session.reasoning.delta` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReasoningDelta {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
    pub delta: String,
}

/// `session.reasoning.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReasoningEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub ordinal: NonNegativeInt,
    pub text: String,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
}

/// `session.tool.input.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolInputStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub name: String,
}

/// `session.tool.input.delta` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolInputDelta {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub delta: String,
}

/// `session.tool.input.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolInputEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub text: String,
}

/// `session.tool.called` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolCalled {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub input: Record,
    pub executed: bool,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
}

/// `session.tool.progress` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolProgress {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub metadata: Record,
}

/// `session.tool.success` (durable v2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolSuccess {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub content: Vec<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    pub executed: bool,
    #[serde(rename = "resultState", default, skip_serializing_if = "Option::is_none")]
    pub result_state: Option<Record>,
}

/// `session.tool.failed` (durable v2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolFailed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub id: String,
    pub error: SessionError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<Content>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    pub executed: bool,
    #[serde(rename = "resultState", default, skip_serializing_if = "Option::is_none")]
    pub result_state: Option<Record>,
}

/// `session.retry.scheduled` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetryScheduled {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "assistantMessageID")]
    pub assistant_message_id: MessageId,
    pub attempt: PositiveInt,
    pub at: NonNegativeInt,
    pub error: SessionError,
}

/// `session.compaction.started` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionStarted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub reason: CompactionReason,
    pub recent: String,
    #[serde(rename = "inputID", default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<MessageId>,
}

/// `session.compaction.delta` (ephemeral).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionDelta {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub text: String,
}

/// `session.compaction.ended` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionEnded {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub reason: CompactionReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<model::Ref>,
    #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
    pub provider_state: Option<Record>,
    #[serde(rename = "providerContext", default, skip_serializing_if = "Option::is_none")]
    pub provider_context: Option<ProviderContext>,
    pub text: String,
    pub recent: String,
    /// Repeats the internal `session.usage.recorded` figures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Usd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
}

/// `session.compaction.failed` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionFailed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub reason: CompactionReason,
    pub error: SessionError,
    #[serde(rename = "inputID", default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Usd>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenUsage>,
}

/// `session.revert.staged` (durable v1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevertStaged {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub revert: Revert,
}

/// `session.revert.cleared` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevertCleared {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
}

/// `session.revert.committed` (durable v1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevertCommitted {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub to: MessageId,
}

/// The 51 session events, internally tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum Event {
    /// `session.created` (durable v1).
    #[serde(rename = "session.created")]
    Created(DurableEnvelope<Created>),
    /// `session.agent.selected` (durable v1).
    #[serde(rename = "session.agent.selected")]
    AgentSelected(DurableEnvelope<AgentSelected>),
    /// `session.model.selected` (durable v1).
    #[serde(rename = "session.model.selected")]
    ModelSelected(DurableEnvelope<ModelSelected>),
    /// `session.moved` (durable v1).
    #[serde(rename = "session.moved")]
    Moved(DurableEnvelope<Moved>),
    /// `session.renamed` (durable v1).
    #[serde(rename = "session.renamed")]
    Renamed(DurableEnvelope<Renamed>),
    /// `session.metadata.updated` (durable v1).
    #[serde(rename = "session.metadata.updated")]
    MetadataUpdated(DurableEnvelope<MetadataUpdated>),
    /// `session.permissions` (durable v1).
    #[serde(rename = "session.permissions")]
    Permissions(DurableEnvelope<Permissions>),
    /// `session.viewed` (durable v1).
    #[serde(rename = "session.viewed")]
    Viewed(DurableEnvelope<Viewed>),
    /// `session.usage.updated` (ephemeral).
    #[serde(rename = "session.usage.updated")]
    UsageUpdated(EphemeralEnvelope<UsageUpdated>),
    /// `session.deleted` (durable v2).
    #[serde(rename = "session.deleted")]
    Deleted(DurableEnvelope<Deleted>),
    /// `session.forked` (durable v2).
    #[serde(rename = "session.forked")]
    Forked(DurableEnvelope<Forked>),
    /// `session.inbox.delivered` (durable v1).
    #[serde(rename = "session.inbox.delivered")]
    InboxDelivered(DurableEnvelope<InboxDelivered>),
    /// `session.inbox.enqueued` (durable v1).
    #[serde(rename = "session.inbox.enqueued")]
    InboxEnqueued(DurableEnvelope<InboxEnqueued>),
    /// `session.inbox.cancelled` (durable v1).
    #[serde(rename = "session.inbox.cancelled")]
    InboxCancelled(DurableEnvelope<InboxCancelled>),
    /// `session.inbox.delivery.changed` (durable v1).
    #[serde(rename = "session.inbox.delivery.changed")]
    InboxDeliveryChanged(DurableEnvelope<InboxDeliveryChanged>),
    /// `session.execution.started` (durable v1).
    #[serde(rename = "session.execution.started")]
    ExecutionStarted(DurableEnvelope<ExecutionStarted>),
    /// `session.execution.succeeded` (durable v1).
    #[serde(rename = "session.execution.succeeded")]
    ExecutionSucceeded(DurableEnvelope<ExecutionSucceeded>),
    /// `session.execution.failed` (durable v1).
    #[serde(rename = "session.execution.failed")]
    ExecutionFailed(DurableEnvelope<ExecutionFailed>),
    /// `session.execution.interrupted` (durable v1).
    #[serde(rename = "session.execution.interrupted")]
    ExecutionInterrupted(DurableEnvelope<ExecutionInterrupted>),
    /// `session.instructions.updated` (durable v2).
    #[serde(rename = "session.instructions.updated")]
    InstructionsUpdated(DurableEnvelope<InstructionsUpdated>),
    /// `session.synthetic` (durable v1).
    #[serde(rename = "session.synthetic")]
    Synthetic(DurableEnvelope<Synthetic>),
    /// `session.skill.activated` (durable v1).
    #[serde(rename = "session.skill.activated")]
    SkillActivated(DurableEnvelope<SkillActivated>),
    /// `session.shell.started` (durable v1).
    #[serde(rename = "session.shell.started")]
    ShellStarted(DurableEnvelope<ShellStarted>),
    /// `session.shell.ended` (durable v1).
    #[serde(rename = "session.shell.ended")]
    ShellEnded(DurableEnvelope<ShellEnded>),
    /// `session.step.started` (durable v1).
    #[serde(rename = "session.step.started")]
    StepStarted(DurableEnvelope<StepStarted>),
    /// `session.step.streamed` (durable v1).
    #[serde(rename = "session.step.streamed")]
    StepStreamed(DurableEnvelope<StepStreamed>),
    /// `session.step.ended` (durable v1).
    #[serde(rename = "session.step.ended")]
    StepEnded(DurableEnvelope<StepEnded>),
    /// `session.step.failed` (durable v1).
    #[serde(rename = "session.step.failed")]
    StepFailed(DurableEnvelope<StepFailed>),
    /// `session.text.started` (durable v1).
    #[serde(rename = "session.text.started")]
    TextStarted(DurableEnvelope<TextStarted>),
    /// `session.text.delta` (ephemeral).
    #[serde(rename = "session.text.delta")]
    TextDelta(EphemeralEnvelope<TextDelta>),
    /// `session.text.ended` (durable v1).
    #[serde(rename = "session.text.ended")]
    TextEnded(DurableEnvelope<TextEnded>),
    /// `session.reasoning.started` (durable v1).
    #[serde(rename = "session.reasoning.started")]
    ReasoningStarted(DurableEnvelope<ReasoningStarted>),
    /// `session.reasoning.delta` (ephemeral).
    #[serde(rename = "session.reasoning.delta")]
    ReasoningDelta(EphemeralEnvelope<ReasoningDelta>),
    /// `session.reasoning.ended` (durable v1).
    #[serde(rename = "session.reasoning.ended")]
    ReasoningEnded(DurableEnvelope<ReasoningEnded>),
    /// `session.tool.input.started` (durable v1).
    #[serde(rename = "session.tool.input.started")]
    ToolInputStarted(DurableEnvelope<ToolInputStarted>),
    /// `session.tool.input.delta` (ephemeral).
    #[serde(rename = "session.tool.input.delta")]
    ToolInputDelta(EphemeralEnvelope<ToolInputDelta>),
    /// `session.tool.input.ended` (durable v1).
    #[serde(rename = "session.tool.input.ended")]
    ToolInputEnded(DurableEnvelope<ToolInputEnded>),
    /// `session.tool.called` (durable v1).
    #[serde(rename = "session.tool.called")]
    ToolCalled(DurableEnvelope<ToolCalled>),
    /// `session.tool.progress` (ephemeral).
    #[serde(rename = "session.tool.progress")]
    ToolProgress(EphemeralEnvelope<ToolProgress>),
    /// `session.tool.success` (durable v2).
    #[serde(rename = "session.tool.success")]
    ToolSuccess(DurableEnvelope<ToolSuccess>),
    /// `session.tool.failed` (durable v2).
    #[serde(rename = "session.tool.failed")]
    ToolFailed(DurableEnvelope<ToolFailed>),
    /// `session.retry.scheduled` (durable v1).
    #[serde(rename = "session.retry.scheduled")]
    RetryScheduled(DurableEnvelope<RetryScheduled>),
    /// `session.compaction.started` (durable v1).
    #[serde(rename = "session.compaction.started")]
    CompactionStarted(DurableEnvelope<CompactionStarted>),
    /// `session.compaction.delta` (ephemeral).
    #[serde(rename = "session.compaction.delta")]
    CompactionDelta(EphemeralEnvelope<CompactionDelta>),
    /// `session.compaction.ended` (durable v1).
    #[serde(rename = "session.compaction.ended")]
    CompactionEnded(DurableEnvelope<CompactionEnded>),
    /// `session.compaction.failed` (durable v1).
    #[serde(rename = "session.compaction.failed")]
    CompactionFailed(DurableEnvelope<CompactionFailed>),
    /// `session.revert.staged` (durable v1).
    #[serde(rename = "session.revert.staged")]
    RevertStaged(DurableEnvelope<RevertStaged>),
    /// `session.revert.cleared` (durable v1).
    #[serde(rename = "session.revert.cleared")]
    RevertCleared(DurableEnvelope<RevertCleared>),
    /// `session.revert.committed` (durable v1).
    #[serde(rename = "session.revert.committed")]
    RevertCommitted(DurableEnvelope<RevertCommitted>),
    /// `session.usage.recorded` (durable v1, internal — excluded from the
    /// public manifest but part of the durable log).
    #[serde(rename = "session.usage.recorded")]
    UsageRecorded(DurableEnvelope<UsageRecorded>),
    /// `session.message.content.updated` (durable v1, replay-only).
    #[serde(rename = "session.message.content.updated")]
    MessageContentUpdated(DurableEnvelope<MessageContentUpdated>),
}

impl Event {
    /// The wire `type` of this event.
    pub fn event_type(&self) -> &'static str {
        match self {
            Event::Created(_) => "session.created",
            Event::AgentSelected(_) => "session.agent.selected",
            Event::ModelSelected(_) => "session.model.selected",
            Event::Moved(_) => "session.moved",
            Event::Renamed(_) => "session.renamed",
            Event::MetadataUpdated(_) => "session.metadata.updated",
            Event::Permissions(_) => "session.permissions",
            Event::Viewed(_) => "session.viewed",
            Event::UsageUpdated(_) => "session.usage.updated",
            Event::Deleted(_) => "session.deleted",
            Event::Forked(_) => "session.forked",
            Event::InboxDelivered(_) => "session.inbox.delivered",
            Event::InboxEnqueued(_) => "session.inbox.enqueued",
            Event::InboxCancelled(_) => "session.inbox.cancelled",
            Event::InboxDeliveryChanged(_) => "session.inbox.delivery.changed",
            Event::ExecutionStarted(_) => "session.execution.started",
            Event::ExecutionSucceeded(_) => "session.execution.succeeded",
            Event::ExecutionFailed(_) => "session.execution.failed",
            Event::ExecutionInterrupted(_) => "session.execution.interrupted",
            Event::InstructionsUpdated(_) => "session.instructions.updated",
            Event::Synthetic(_) => "session.synthetic",
            Event::SkillActivated(_) => "session.skill.activated",
            Event::ShellStarted(_) => "session.shell.started",
            Event::ShellEnded(_) => "session.shell.ended",
            Event::StepStarted(_) => "session.step.started",
            Event::StepStreamed(_) => "session.step.streamed",
            Event::StepEnded(_) => "session.step.ended",
            Event::StepFailed(_) => "session.step.failed",
            Event::TextStarted(_) => "session.text.started",
            Event::TextDelta(_) => "session.text.delta",
            Event::TextEnded(_) => "session.text.ended",
            Event::ReasoningStarted(_) => "session.reasoning.started",
            Event::ReasoningDelta(_) => "session.reasoning.delta",
            Event::ReasoningEnded(_) => "session.reasoning.ended",
            Event::ToolInputStarted(_) => "session.tool.input.started",
            Event::ToolInputDelta(_) => "session.tool.input.delta",
            Event::ToolInputEnded(_) => "session.tool.input.ended",
            Event::ToolCalled(_) => "session.tool.called",
            Event::ToolProgress(_) => "session.tool.progress",
            Event::ToolSuccess(_) => "session.tool.success",
            Event::ToolFailed(_) => "session.tool.failed",
            Event::RetryScheduled(_) => "session.retry.scheduled",
            Event::CompactionStarted(_) => "session.compaction.started",
            Event::CompactionDelta(_) => "session.compaction.delta",
            Event::CompactionEnded(_) => "session.compaction.ended",
            Event::CompactionFailed(_) => "session.compaction.failed",
            Event::RevertStaged(_) => "session.revert.staged",
            Event::RevertCleared(_) => "session.revert.cleared",
            Event::RevertCommitted(_) => "session.revert.committed",
            Event::UsageRecorded(_) => "session.usage.recorded",
            Event::MessageContentUpdated(_) => "session.message.content.updated",
        }
    }

    /// The event id.
    pub fn event_id(&self) -> &crate::id::EventId {
        match self {
            Event::Created(event) => &event.id,
            Event::AgentSelected(event) => &event.id,
            Event::ModelSelected(event) => &event.id,
            Event::Moved(event) => &event.id,
            Event::Renamed(event) => &event.id,
            Event::MetadataUpdated(event) => &event.id,
            Event::Permissions(event) => &event.id,
            Event::Viewed(event) => &event.id,
            Event::UsageUpdated(event) => &event.id,
            Event::Deleted(event) => &event.id,
            Event::Forked(event) => &event.id,
            Event::InboxDelivered(event) => &event.id,
            Event::InboxEnqueued(event) => &event.id,
            Event::InboxCancelled(event) => &event.id,
            Event::InboxDeliveryChanged(event) => &event.id,
            Event::ExecutionStarted(event) => &event.id,
            Event::ExecutionSucceeded(event) => &event.id,
            Event::ExecutionFailed(event) => &event.id,
            Event::ExecutionInterrupted(event) => &event.id,
            Event::InstructionsUpdated(event) => &event.id,
            Event::Synthetic(event) => &event.id,
            Event::SkillActivated(event) => &event.id,
            Event::ShellStarted(event) => &event.id,
            Event::ShellEnded(event) => &event.id,
            Event::StepStarted(event) => &event.id,
            Event::StepStreamed(event) => &event.id,
            Event::StepEnded(event) => &event.id,
            Event::StepFailed(event) => &event.id,
            Event::TextStarted(event) => &event.id,
            Event::TextDelta(event) => &event.id,
            Event::TextEnded(event) => &event.id,
            Event::ReasoningStarted(event) => &event.id,
            Event::ReasoningDelta(event) => &event.id,
            Event::ReasoningEnded(event) => &event.id,
            Event::ToolInputStarted(event) => &event.id,
            Event::ToolInputDelta(event) => &event.id,
            Event::ToolInputEnded(event) => &event.id,
            Event::ToolCalled(event) => &event.id,
            Event::ToolProgress(event) => &event.id,
            Event::ToolSuccess(event) => &event.id,
            Event::ToolFailed(event) => &event.id,
            Event::RetryScheduled(event) => &event.id,
            Event::CompactionStarted(event) => &event.id,
            Event::CompactionDelta(event) => &event.id,
            Event::CompactionEnded(event) => &event.id,
            Event::CompactionFailed(event) => &event.id,
            Event::RevertStaged(event) => &event.id,
            Event::RevertCleared(event) => &event.id,
            Event::RevertCommitted(event) => &event.id,
            Event::UsageRecorded(event) => &event.id,
            Event::MessageContentUpdated(event) => &event.id,
        }
    }
}

/// `FileDiff` re-export for revert staging.
#[allow(dead_code)]
type _FileDiff = FileDiff;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::DurableInfo;
    use crate::id::EventId;

    #[test]
    fn created_event_wire() {
        let event = Event::Created(DurableEnvelope {
            id: EventId::parse("evt_1").unwrap(),
            created: 1_700_000_000_000,
            metadata: None,
            durable: DurableInfo { aggregate_id: "ses_1".into(), seq: 0, version: 1 },
            location: Some(location::Ref {
                directory: crate::common::AbsolutePath::parse("/repo"),
                workspace_id: None,
            }),
            data: Created {
                session_id: SessionId::parse("ses_1").unwrap(),
                project_id: ProjectId::global(),
                location: location::Ref {
                    directory: crate::common::AbsolutePath::parse("/repo"),
                    workspace_id: None,
                },
                subpath: None,
                parent_id: None,
                slug: "timeline-stability".into(),
                title: None,
                agent: Some(AgentId::parse("build")),
                model: None,
                metadata: None,
                permissions: None,
                version: "2.0.24".into(),
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "session.created");
        assert_eq!(json["durable"]["aggregateID"], "ses_1");
        assert_eq!(json["data"]["sessionID"], "ses_1");
        assert_eq!(json["data"]["projectID"], "global");
        assert_eq!(json["data"]["agent"], "build");
        assert_eq!(json["location"]["directory"], "/repo");
        let back: Event = serde_json::from_value(json).unwrap();
        assert_eq!(back.event_type(), "session.created");
        assert_eq!(back, event);
    }

    #[test]
    fn ephemeral_delta_wire() {
        let event = Event::TextDelta(EphemeralEnvelope {
            id: EventId::parse("evt_2").unwrap(),
            created: 1_700_000_000_001,
            metadata: None,
            location: None,
            data: TextDelta {
                session_id: SessionId::parse("ses_1").unwrap(),
                assistant_message_id: MessageId::parse("msg_1").unwrap(),
                ordinal: 0,
                delta: "Hello".into(),
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "evt_2",
                "created": 1_700_000_000_001u64,
                "type": "session.text.delta",
                "data": {
                    "sessionID": "ses_1",
                    "assistantMessageID": "msg_1",
                    "ordinal": 0,
                    "delta": "Hello"
                }
            })
        );
        let back: Event = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn tool_success_wire() {
        let event = Event::ToolSuccess(DurableEnvelope {
            id: EventId::parse("evt_3").unwrap(),
            created: 5,
            metadata: None,
            durable: DurableInfo { aggregate_id: "ses_1".into(), seq: 9, version: 2 },
            location: None,
            data: ToolSuccess {
                session_id: SessionId::parse("ses_1").unwrap(),
                assistant_message_id: MessageId::parse("msg_1").unwrap(),
                id: "call_1".into(),
                content: vec![Content::Text { text: "ok".into() }],
                metadata: None,
                executed: true,
                result_state: None,
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "session.tool.success");
        assert_eq!(json["durable"]["version"], 2);
        assert_eq!(json["data"]["content"][0]["text"], "ok");
        assert_eq!(json["data"]["executed"], true);
    }
}
