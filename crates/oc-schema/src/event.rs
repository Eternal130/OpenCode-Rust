//! Port of `event.ts` — the public event envelope shared by every event kind.
//!
//! Wire shapes (verified against v2's framing):
//!
//! Durable event:
//! `{ "id": "evt_…", "created": <millis>, "metadata"?: {}, "type": "<event.type>",
//!    "durable": { "aggregateID": "…", "seq": 0, "version": 1 }, "location"?: {}, "data": {} }`
//!
//! Ephemeral event: same without the `durable` key.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::Record;
use crate::id::EventId;
use crate::location;

/// `Event.Seq` — position in one aggregate's durable log (>= 0).
pub type Seq = u64;

/// `Event.Version` — durable schema version of one event type (>= 1).
pub type Version = u64;

/// `Event.DurableEnvelope`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DurableInfo {
    #[serde(rename = "aggregateID")]
    pub aggregate_id: String,
    pub seq: Seq,
    pub version: Version,
}

/// Envelope for durable events: `Event.durable` output shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DurableEnvelope<D> {
    pub id: EventId,
    /// Epoch milliseconds.
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    pub durable: DurableInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<location::Ref>,
    pub data: D,
}

/// Envelope for ephemeral events: `Event.ephemeral` output shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EphemeralEnvelope<D> {
    pub id: EventId,
    /// Epoch milliseconds.
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<location::Ref>,
    pub data: D,
}

/// `Event.versionedType` — `"{type}.{version}"`.
pub fn versioned_type(event_type: &str, version: Version) -> String {
    format!("{event_type}.{version}")
}

/// Arbitrary JSON re-export for event payloads.
#[allow(dead_code)]
type _Value = Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_envelope_wire() {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        struct Data {
            value: u64,
        }
        let envelope = DurableEnvelope {
            id: EventId::parse("evt_1").unwrap(),
            created: 1_700_000_000_000,
            metadata: None,
            durable: DurableInfo { aggregate_id: "ses_1".into(), seq: 3, version: 1 },
            location: Some(location::Ref {
                directory: crate::common::AbsolutePath::parse("/repo"),
                workspace_id: None,
            }),
            data: Data { value: 7 },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "evt_1",
                "created": 1_700_000_000_000u64,
                "durable": {"aggregateID": "ses_1", "seq": 3, "version": 1},
                "location": {"directory": "/repo"},
                "data": {"value": 7}
            })
        );
        let back: DurableEnvelope<Data> = serde_json::from_value(json).unwrap();
        assert_eq!(back, envelope);
    }
}
