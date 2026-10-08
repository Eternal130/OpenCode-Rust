//! Port of `session-message.ts` — the full message + part tagged unions.
//!
//! Wire format: `Session.Message.Info` is a union of structs each carrying a
//! literal `type` field (`user`, `assistant`, `compaction`, …); serde mirrors
//! this with an internally-tagged enum. Nested unions (`Compaction`,
//! `AssistantContent`, `ToolState`) use their own tags (`status`, `type`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{
    FinishReason, PositiveInt, Record, RelativePath, SessionError, TokenUsage, Usd,
};
use crate::id::AgentId;
use crate::id::{MessageId, ProjectId, ShellId, SkillId, SkillName, SnapshotId};
use crate::location;
use crate::model;
use crate::prompt::{AgentAttachment, FileAttachment, SkillAttachment};
use crate::session_support::{ProviderContext, SessionMetadata};
use crate::shell;
use crate::tool::Content;

/// Message metadata: `Record<string, unknown>`.
pub type Metadata = Record;

/// Base `time` — creation only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Time {
    pub created: u64,
}

/// Shell message `time` — created plus optional completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShellTime {
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

/// Assistant message `time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssistantTime {
    pub created: u64,
    /// When the provider response body ended, before tool settlement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streamed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

/// `Session.Message.ToolState` — tagged by `status`.
///
/// State machine: `streaming` → `running` → `completed` | `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ToolState {
    /// Partial JSON input as received from the provider.
    Streaming {
        /// Raw accumulated input string.
        input: String,
    },
    /// Input parsed; the tool is executing.
    Running { input: Record, metadata: Record },
    /// Terminal success.
    Completed {
        input: Record,
        /// Non-empty on the wire (`NonEmptyArray<Content>`).
        content: Vec<Content>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Record>,
    },
    /// Terminal failure.
    Error {
        input: Record,
        error: SessionError,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Vec<Content>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Record>,
    },
}

/// Tool part `time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ToolTime {
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

/// Reasoning part `time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReasoningTime {
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

/// `Session.Message.AssistantContent` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[allow(clippy::large_enum_variant)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantContent {
    /// Model-visible text.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<Record>,
    },
    /// Provider reasoning trace.
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<Record>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<ReasoningTime>,
    },
    /// A tool invocation.
    Tool {
        /// Tool call id (provider-assigned).
        id: String,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executed: Option<bool>,
        #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
        provider_state: Option<Record>,
        #[serde(rename = "providerResultState", default, skip_serializing_if = "Option::is_none")]
        provider_result_state: Option<Record>,
        state: ToolState,
        time: ToolTime,
    },
}

/// `Session.Message.Assistant.Retry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssistantRetry {
    pub attempt: PositiveInt,
    pub at: u64,
    pub error: SessionError,
}

/// Assistant snapshot bracket.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AssistantSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<SnapshotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<SnapshotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<RelativePath>>,
}

/// `Session.Message.Compaction` — tagged by `status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[allow(clippy::large_enum_variant)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Compaction {
    /// Summarization request in flight.
    Running {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        reason: CompactionReason,
        summary: String,
        recent: String,
    },
    /// Summarization finished.
    Completed {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        reason: CompactionReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<model::Ref>,
        #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
        provider_state: Option<Record>,
        summary: String,
        recent: String,
        #[serde(rename = "providerContext", default, skip_serializing_if = "Option::is_none")]
        provider_context: Option<ProviderContext>,
        /// Usage of the compaction request itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<Usd>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
    },
    /// Summarization failed.
    Failed {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        reason: CompactionReason,
        error: SessionError,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<Usd>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
    },
}

/// `Compaction.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CompactionReason {
    Auto,
    Manual,
}

/// `Session.Message.Idle.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IdleOutcome {
    Succeeded,
    Failed,
    Interrupted,
}

/// Previous location triple carried by `location-switched`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LocationSwitchedPrevious {
    pub location: location::Ref,
    #[serde(rename = "projectID", default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<RelativePath>,
}

/// `Session.Message.Info` — tagged by `type`.
///
/// The variant tag and the per-variant fields match v2's wire format exactly;
/// the outer `type` is injected by serde's internal tagging and the nested
/// `Compaction` union injects its own `status` tag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Info {
    /// Agent switched mid-session.
    AgentSwitched {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        agent: AgentId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<AgentId>,
    },
    /// Model switched mid-session.
    ModelSwitched {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        model: model::Ref,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<model::Ref>,
    },
    /// Location switched mid-session (session move).
    LocationSwitched {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        location: location::Ref,
        #[serde(rename = "projectID", default, skip_serializing_if = "Option::is_none")]
        project_id: Option<ProjectId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subpath: Option<RelativePath>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous: Option<LocationSwitchedPrevious>,
    },
    /// A user turn.
    User {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        files: Option<Vec<FileAttachment>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agents: Option<Vec<AgentAttachment>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skills: Option<Vec<SkillAttachment>>,
    },
    /// Synthetic (non-user-origin) transcript entry.
    Synthetic {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Model-facing system update.
    System {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        /// The model-facing update text, frozen at emit time.
        text: String,
        /// A short human-readable summary for transcript display.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Skill activated into the transcript.
    Skill {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        skill: SkillId,
        name: SkillName,
        text: String,
    },
    /// Shell execution transcript entry.
    Shell {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        #[serde(rename = "shellID")]
        shell_id: ShellId,
        command: String,
        status: shell::Status,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit: Option<crate::common::Num>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<shell::Output>,
        time: ShellTime,
    },
    /// An assistant turn.
    Assistant {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: AssistantTime,
        agent: AgentId,
        model: model::Ref,
        content: Vec<AssistantContent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<AssistantSnapshot>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        finish: Option<FinishReason>,
        #[serde(rename = "rawFinish", default, skip_serializing_if = "Option::is_none")]
        raw_finish: Option<String>,
        #[serde(rename = "providerState", default, skip_serializing_if = "Option::is_none")]
        provider_state: Option<Record>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<Usd>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens: Option<TokenUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<SessionError>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry: Option<AssistantRetry>,
    },
    /// Context compaction marker.
    Compaction(Compaction),
    /// Turn boundary: everything since the previous idle marker is one turn.
    Idle {
        id: MessageId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Metadata>,
        time: Time,
        outcome: IdleOutcome,
    },
}

/// JSON re-export for provider state values.
#[allow(dead_code)]
type _Value = Value;

/// Session metadata re-export used by messages.
#[allow(dead_code)]
type _SessionMetadata = SessionMetadata;

#[cfg(test)]
mod tests {
    use super::*;

    fn user_message() -> Info {
        Info::User {
            id: MessageId::parse("msg_1").unwrap(),
            metadata: None,
            time: Time { created: 1 },
            text: "Run it".into(),
            files: None,
            agents: None,
            skills: None,
        }
    }

    #[test]
    fn user_wire() {
        let json = serde_json::to_value(user_message()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "msg_1",
                "time": {"created": 1},
                "text": "Run it",
                "type": "user"
            })
        );
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, user_message());
    }

    #[test]
    fn agent_switched_wire() {
        let message = Info::AgentSwitched {
            id: MessageId::parse("msg_2").unwrap(),
            metadata: None,
            time: Time { created: 2 },
            agent: AgentId::parse("explore"),
            previous: Some(AgentId::parse("build")),
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "agent-switched");
        assert_eq!(json["agent"], "explore");
        assert_eq!(json["previous"], "build");
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn assistant_tool_state_machine_wire() {
        let streaming = ToolState::Streaming { input: "{\"cmd\"".into() };
        assert_eq!(
            serde_json::to_value(&streaming).unwrap(),
            serde_json::json!({"status": "streaming", "input": "{\"cmd\""})
        );
        let running = ToolState::Running {
            input: serde_json::from_value(serde_json::json!({"cmd": "ls"})).unwrap(),
            metadata: Record::default(),
        };
        assert_eq!(
            serde_json::to_value(&running).unwrap(),
            serde_json::json!({"status": "running", "input": {"cmd": "ls"}, "metadata": {}})
        );
        let completed = ToolState::Completed {
            input: Record::default(),
            content: vec![Content::Text { text: "done".into() }],
            metadata: None,
        };
        assert_eq!(
            serde_json::to_value(&completed).unwrap(),
            serde_json::json!({
                "status": "completed",
                "input": {},
                "content": [{"type": "text", "text": "done"}]
            })
        );
        let error = ToolState::Error {
            input: Record::default(),
            error: SessionError {
                error_type: "ToolError".into(),
                message: "failed".into(),
                status: None,
                response: None,
            },
            content: None,
            metadata: None,
        };
        assert_eq!(error, serde_json::from_value(serde_json::to_value(&error).unwrap()).unwrap());
    }

    #[test]
    fn assistant_full_wire() {
        let message = Info::Assistant {
            id: MessageId::parse("msg_3").unwrap(),
            metadata: None,
            time: AssistantTime { created: 2, streamed: None, completed: Some(3) },
            agent: AgentId::parse("build"),
            model: model::Ref::parse("anthropic/claude").unwrap(),
            content: vec![
                AssistantContent::Text { text: "Working".into(), state: None },
                AssistantContent::Tool {
                    id: "call_1".into(),
                    name: "bash".into(),
                    executed: Some(true),
                    provider_state: None,
                    provider_result_state: None,
                    state: ToolState::Running {
                        input: Record::default(),
                        metadata: Record::default(),
                    },
                    time: ToolTime { created: 2, ran: Some(3), completed: None },
                },
            ],
            snapshot: None,
            finish: Some(FinishReason::Stop),
            raw_finish: None,
            provider_state: None,
            cost: Some(Usd(crate::common::Num(0.012))),
            tokens: Some(TokenUsage::default()),
            error: None,
            retry: None,
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "assistant");
        assert_eq!(json["model"]["providerID"], "anthropic");
        assert_eq!(json["content"][1]["state"]["status"], "running");
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn compaction_wire() {
        let message = Info::Compaction(Compaction::Completed {
            id: MessageId::parse("msg_4").unwrap(),
            metadata: None,
            time: Time { created: 9 },
            reason: CompactionReason::Auto,
            model: Some(model::Ref::parse("opencode/grok-code").unwrap()),
            provider_state: None,
            summary: "summary text".into(),
            recent: "recent text".into(),
            provider_context: None,
            cost: None,
            tokens: None,
        });
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["type"], "compaction");
        assert_eq!(json["status"], "completed");
        assert_eq!(json["reason"], "auto");
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
    }

    #[test]
    fn idle_wire() {
        let message = Info::Idle {
            id: MessageId::parse("msg_5").unwrap(),
            metadata: None,
            time: Time { created: 10 },
            outcome: IdleOutcome::Interrupted,
        };
        assert_eq!(
            serde_json::to_value(&message).unwrap(),
            serde_json::json!({
                "id": "msg_5",
                "time": {"created": 10},
                "type": "idle",
                "outcome": "interrupted"
            })
        );
    }
}
