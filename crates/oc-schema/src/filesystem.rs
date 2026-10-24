//! Port of `schema/filesystem.ts` event surface — the ephemeral
//! `filesystem.changed` notification published by the config/vcs
//! watchers.

use serde::{Deserialize, Serialize};

use crate::event::EphemeralEnvelope;

/// `filesystem.changed` data (filesystem.ts:8-14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Changed {
    pub file: String,
    pub event: Change,
}

/// `filesystem.changed.event` — `"add" | "change" | "unlink"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Add,
    Change,
    Unlink,
}

/// `filesystem.changed` (ephemeral).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum FilesystemEvent {
    /// A watched file changed.
    #[serde(rename = "filesystem.changed")]
    Changed(EphemeralEnvelope<Changed>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filesystem_changed_wire() {
        let event = FilesystemEvent::Changed(EphemeralEnvelope {
            id: crate::id::EventId::parse("evt_1").unwrap(),
            created: 1,
            metadata: None,
            location: None,
            data: Changed { file: "/repo/opencode.json".into(), event: Change::Change },
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "filesystem.changed");
        assert_eq!(json["data"]["file"], "/repo/opencode.json");
        assert_eq!(json["data"]["event"], "change");
        let back: FilesystemEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn filesystem_changed_rejects_unknown_kind() {
        let json = serde_json::json!({
            "type": "filesystem.changed",
            "id": "evt_1",
            "created": 1,
            "data": { "file": "/x", "event": "rename" }
        });
        assert!(serde_json::from_value::<FilesystemEvent>(json).is_err());
    }
}
