//! Port of `schema/pty.ts` — `Pty.Info` plus the ephemeral
//! `pty.created` / `pty.updated` / `pty.exited` / `pty.deleted` events.
//! IDs are plain `pty_…`-prefixed strings (matching core's `PtyId`).

use serde::{Deserialize, Serialize};

use crate::common::NonNegativeInt;
use crate::event::EphemeralEnvelope;

/// `Pty.Info` (pty.ts:22-31).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    /// `pty_…`-prefixed identifier.
    pub id: String,
    pub title: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub status: Status,
    pub pid: NonNegativeInt,
    /// Present once the process exited.
    #[serde(default, rename = "exitCode", skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<NonNegativeInt>,
}

/// `Pty.Info.status` — `"running" | "exited"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Exited,
}

/// `pty.created` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Created {
    pub info: Info,
}

/// `pty.updated` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Updated {
    pub info: Info,
}

/// `pty.exited` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Exited {
    pub id: String,
    #[serde(rename = "exitCode")]
    pub exit_code: NonNegativeInt,
}

/// `pty.deleted` data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Deleted {
    pub id: String,
}

/// `pty.*` (ephemeral) tagged union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum PtyEvent {
    /// Process started.
    #[serde(rename = "pty.created")]
    Created(EphemeralEnvelope<Created>),
    /// Title/size changed.
    #[serde(rename = "pty.updated")]
    Updated(EphemeralEnvelope<Updated>),
    /// Process reached a terminal status.
    #[serde(rename = "pty.exited")]
    Exited(EphemeralEnvelope<Exited>),
    /// Session dropped from the registry.
    #[serde(rename = "pty.deleted")]
    Deleted(EphemeralEnvelope<Deleted>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_info_wire_is_camel_case() {
        let info = Info {
            id: "pty_1".into(),
            title: "t".into(),
            command: "/bin/sh".into(),
            args: vec!["-c".into()],
            cwd: "/tmp".into(),
            status: Status::Exited,
            pid: 42,
            exit_code: Some(3),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["exitCode"], 3);
        assert!(json.get("exit_code").is_none());
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn running_info_omits_exit_code() {
        let info = Info {
            id: "pty_2".into(),
            title: "t".into(),
            command: "/bin/sh".into(),
            args: vec![],
            cwd: "/tmp".into(),
            status: Status::Running,
            pid: 1,
            exit_code: None,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("exitCode").is_none());
    }

    #[test]
    fn pty_event_round_trip() {
        let event = PtyEvent::Exited(EphemeralEnvelope {
            id: crate::id::EventId::parse("evt_1").unwrap(),
            created: 1,
            metadata: None,
            location: None,
            data: Exited { id: "pty_1".into(), exit_code: 0 },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "pty.exited");
        assert_eq!(json["data"]["exitCode"], 0);
        let back: PtyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn pty_event_rejects_wrong_typed_field() {
        let json = serde_json::json!({
            "type": "pty.exited",
            "id": "evt_1",
            "created": 1,
            "data": { "id": "pty_1", "exitCode": "zero" }
        });
        assert!(serde_json::from_value::<PtyEvent>(json).is_err());
    }
}
