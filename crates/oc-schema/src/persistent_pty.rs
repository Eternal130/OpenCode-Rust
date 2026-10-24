//! Port of `schema/persistent-pty.ts` event surface — the ephemeral
//! `persistent-pty.added` / `persistent-pty.removed` events and the
//! `PersistentPty.Info` wire shape they carry (camelCase keys,
//! `size: {cols, rows}`, `output: {head, tail}`, nullable
//! `foregroundProcess`, `exitCode` only once exited).

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;
use crate::event::EphemeralEnvelope;
use crate::id::SessionId;
use crate::pty;

/// `PersistentPty.Info` (persistent-pty.ts:13-19) — `Pty.Info` fields
/// plus session grouping, foreground process, size and output window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    #[serde(flatten)]
    pub base: pty::Info,
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    /// Nullable — `null` while no foreground process is detected.
    #[serde(rename = "foregroundProcess")]
    pub foreground_process: Option<String>,
    pub size: Size,
    pub output: Output,
}

/// `PersistentPty.Info.size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// `PersistentPty.Info.output` — absolute byte window `[head, tail)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Output {
    pub head: NonNegativeInt,
    pub tail: NonNegativeInt,
}

/// `persistent-pty.added` data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Added {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    pub terminal: Info,
}

/// `persistent-pty.removed` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Removed {
    #[serde(rename = "sessionID")]
    pub session_id: SessionId,
    #[serde(rename = "ptyID")]
    pub pty_id: String,
}

/// `persistent-pty.*` (ephemeral) tagged union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum PersistentPtyEvent {
    /// Terminal attached to a session.
    #[serde(rename = "persistent-pty.added")]
    Added(EphemeralEnvelope<Added>),
    /// Terminal detached/terminated.
    #[serde(rename = "persistent-pty.removed")]
    Removed(EphemeralEnvelope<Removed>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(exit_code: Option<u64>) -> Info {
        Info {
            base: pty::Info {
                id: "pty_persistent_1".into(),
                title: "workbench".into(),
                command: "/bin/sh".into(),
                args: vec!["-c".into()],
                cwd: "/tmp".into(),
                status: pty::Status::Running,
                pid: 7,
                exit_code: exit_code.map(|code| code as NonNegativeInt),
            },
            session_id: SessionId::parse("ses_1").unwrap(),
            foreground_process: None,
            size: Size { cols: 80, rows: 24 },
            output: Output { head: 0, tail: 128 },
        }
    }

    #[test]
    fn persistent_pty_info_wire() {
        let json = serde_json::to_value(info(None)).unwrap();
        assert_eq!(json["sessionID"], "ses_1");
        assert_eq!(json["foregroundProcess"], serde_json::Value::Null);
        assert_eq!(json["size"]["cols"], 80);
        assert_eq!(json["size"]["rows"], 24);
        assert_eq!(json["output"]["head"], 0);
        assert_eq!(json["output"]["tail"], 128);
        assert!(json.get("exitCode").is_none());
        assert_eq!(json["id"], "pty_persistent_1");
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, info(None));
    }

    #[test]
    fn persistent_pty_info_wire_with_exit_code() {
        let json = serde_json::to_value(info(Some(3))).unwrap();
        assert_eq!(json["exitCode"], 3);
        assert_eq!(json["status"], "running");
    }

    #[test]
    fn persistent_pty_event_round_trip() {
        let event = PersistentPtyEvent::Added(EphemeralEnvelope {
            id: crate::id::EventId::parse("evt_2").unwrap(),
            created: 2,
            metadata: None,
            location: None,
            data: Added { session_id: SessionId::parse("ses_1").unwrap(), terminal: info(None) },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "persistent-pty.added");
        assert_eq!(json["data"]["sessionID"], "ses_1");
        assert_eq!(json["data"]["terminal"]["output"]["tail"], 128);
        let back: PersistentPtyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn persistent_pty_removed_wire_and_rejection() {
        let event = PersistentPtyEvent::Removed(EphemeralEnvelope {
            id: crate::id::EventId::parse("evt_3").unwrap(),
            created: 3,
            metadata: None,
            location: None,
            data: Removed {
                session_id: SessionId::parse("ses_1").unwrap(),
                pty_id: "pty_persistent_1".into(),
            },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["data"]["ptyID"], "pty_persistent_1");
        let back: PersistentPtyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);

        let bad = serde_json::json!({
            "type": "persistent-pty.removed",
            "id": "evt_3",
            "created": 3,
            "data": { "sessionID": "ses_1", "ptyID": ["pty_persistent_1"] }
        });
        assert!(serde_json::from_value::<PersistentPtyEvent>(bad).is_err());
    }
}
