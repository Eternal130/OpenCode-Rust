//! Port of `schema/reference.ts` event surface — the ephemeral
//! `reference.updated` notification (empty payload) that invalidates the
//! docs/reference catalog.

use serde::{Deserialize, Serialize};

use crate::event::EphemeralEnvelope;
use crate::misc_events::Empty;

/// `reference.updated` (ephemeral, empty payload — reference.ts:8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ReferenceEvent {
    /// Reference sources changed.
    #[serde(rename = "reference.updated")]
    Updated(EphemeralEnvelope<Empty>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_updated_wire() {
        let event = ReferenceEvent::Updated(EphemeralEnvelope {
            id: crate::id::EventId::parse("evt_1").unwrap(),
            created: 1,
            metadata: None,
            location: None,
            data: Empty {},
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "evt_1", "created": 1, "type": "reference.updated", "data": {}
            })
        );
        let back: ReferenceEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }
}
