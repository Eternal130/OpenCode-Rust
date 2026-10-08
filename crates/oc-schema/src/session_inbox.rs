//! Port of `session-inbox.ts`.

use serde::{Deserialize, Serialize};

use crate::common::{Record, RelativePath};
use crate::id::{MessageId, ProjectId, SessionId};
use crate::location;
use crate::prompt::{AgentAttachment, FileAttachment, Prompt, SkillAttachment};

/// `Session.Inbox.Delivery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    /// Inject into the running turn.
    Steer,
    /// Run after the current turn.
    Queue,
}

/// `Session.Inbox.UserPayload` — prompt fields plus metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UserPayload {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<AgentAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<SkillAttachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
}

/// `Session.Inbox.SyntheticPayload`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyntheticPayload {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
}

/// `Session.Inbox.CompactionPayload` — empty struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompactionPayload {}

/// `Session.Inbox.MovePayload`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MovePayload {
    pub location: location::Ref,
    #[serde(rename = "projectID")]
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<RelativePath>,
}

/// `Session.Inbox.Item` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Item {
    /// Queued user prompt.
    User { payload: UserPayload, delivery: Delivery },
    /// Queued synthetic entry.
    Synthetic { payload: SyntheticPayload, delivery: Delivery },
    /// Queued compaction request.
    Compaction { payload: CompactionPayload, delivery: Delivery },
    /// Queued location move.
    #[serde(rename = "move")]
    Move_ { payload: MovePayload, delivery: Delivery },
}

/// Time block shared by enqueued inbox records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EnqueuedTime {
    pub created: u64,
}

/// `Session.Inbox.Info` — tagged by `type`, adds `id`/`sessionID`/`time`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Info {
    /// Delivered user prompt record.
    User {
        id: MessageId,
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        time: EnqueuedTime,
        payload: UserPayload,
        delivery: Delivery,
    },
    /// Delivered synthetic record.
    Synthetic {
        id: MessageId,
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        time: EnqueuedTime,
        payload: SyntheticPayload,
        delivery: Delivery,
    },
    /// Delivered compaction record.
    Compaction {
        id: MessageId,
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        time: EnqueuedTime,
        payload: CompactionPayload,
        delivery: Delivery,
    },
    /// Delivered move record.
    #[serde(rename = "move")]
    Move {
        id: MessageId,
        #[serde(rename = "sessionID")]
        session_id: SessionId,
        time: EnqueuedTime,
        payload: MovePayload,
        delivery: Delivery,
    },
}

/// Re-export for prompt parity checks.
#[allow(dead_code)]
type _Prompt = Prompt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_wire() {
        let value = serde_json::json!({
            "type": "compaction",
            "payload": {},
            "delivery": "queue"
        });
        let parsed: Item = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Item::Compaction { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn info_move_wire() {
        let value = serde_json::json!({
            "type": "move",
            "id": "msg_1",
            "sessionID": "ses_1",
            "time": {"created": 5},
            "payload": {
                "location": {"directory": "/repo"},
                "projectID": "global"
            },
            "delivery": "steer"
        });
        let parsed: Info = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(parsed, Info::Move { .. }));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }
}
