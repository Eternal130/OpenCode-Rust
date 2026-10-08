//! Port of `session-provider-context.ts`, `session-fork.ts`,
//! `session-metadata.ts`, `file-diff.ts`, `instruction.ts`,
//! `instruction-entry.ts`, `session-revert.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{AbsolutePath, NonNegativeInt};
use crate::id::{MessageId, ProviderId, SnapshotId};

/// Serde helper for `Schema.Literal(1)` fields.
#[allow(clippy::trivially_copy_pass_by_ref)]
mod literal_one {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(_: &(), serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(1)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
        let value = u64::deserialize(deserializer)?;
        if value == 1 {
            Ok(())
        } else {
            Err(serde::de::Error::custom("expected literal 1"))
        }
    }
}

/// `Session.ProviderContext.Provenance` — exact producing model/deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Provenance {
    #[serde(rename = "providerID")]
    pub provider_id: ProviderId,
    pub provider: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
    pub route: String,
    pub protocol: String,
    /// Digest of the configured endpoint; raw URLs are not persisted.
    pub endpoint: String,
}

/// `Session.ProviderContext` (`Info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProviderContext {
    /// Always `1` on the wire.
    #[serde(with = "literal_one")]
    #[schemars(with = "u64")]
    pub version: (),
    pub provenance: Provenance,
    /// Versioned canonical AI `Message[]` payload.
    pub messages: Value,
}

/// `Session.Metadata` — host-supplied annotations, JSON values.
pub type SessionMetadata = BTreeMap<String, Value>;

/// `Session.ForkBoundary` — tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ForkBoundary {
    /// Fork everything strictly before this message.
    Before {
        #[serde(rename = "messageID")]
        message_id: MessageId,
    },
    /// Fork everything through this message.
    Through {
        #[serde(rename = "messageID")]
        message_id: MessageId,
    },
}

/// `FileDiff.Info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FileDiff {
    pub file: String,
    pub patch: String,
    pub additions: NonNegativeInt,
    pub deletions: NonNegativeInt,
    pub status: FileDiffStatus,
}

/// `FileDiff.Info.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileDiffStatus {
    Added,
    Deleted,
    Modified,
}

/// `FileDiff.LegacyInfo` — V1 snapshot shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LegacyFileDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub additions: crate::common::Num,
    pub deletions: crate::common::Num,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FileDiffStatus>,
}

/// `Instruction.Hash` — `^[a-f0-9]{64}$`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct InstructionHash(pub String);

/// `Instruction.Values` — full instruction key → hash map.
pub type InstructionValues = BTreeMap<String, InstructionHash>;

/// `Instruction.Delta` — per-key update: new hash or the literal `"removed"`.
///
/// Serde's untagged union cannot distinguish the `"removed"` string from a
/// hash value, so this carries hand-written impls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionDeltaValue {
    Hash(InstructionHash),
    Removed,
}

impl Serialize for InstructionDeltaValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            InstructionDeltaValue::Hash(hash) => hash.serialize(serializer),
            InstructionDeltaValue::Removed => serializer.serialize_str("removed"),
        }
    }
}

impl<'de> Deserialize<'de> for InstructionDeltaValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value == "removed" {
            Ok(InstructionDeltaValue::Removed)
        } else {
            Ok(InstructionDeltaValue::Hash(InstructionHash(value)))
        }
    }
}

impl schemars::JsonSchema for InstructionDeltaValue {
    fn schema_name() -> String {
        "Instruction.DeltaValue".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        serde_json::from_value(serde_json::json!({
            "anyOf": [
                {"type": "string", "pattern": "^[a-f0-9]{64}$"},
                {"type": "string", "enum": ["removed"]}
            ]
        }))
        .expect("static delta schema")
    }
}

/// `Instruction.Delta` map.
pub type InstructionDelta = BTreeMap<String, InstructionDeltaValue>;

/// `InstructionEntry.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstructionEntry {
    /// Slash-free client-facing key (`^[a-z0-9][a-z0-9._-]*$`).
    pub key: String,
    /// JSON value attached to the session's instructions.
    pub value: Value,
}

/// `InstructionEntry.Snapshot` — ordered entries plus removal markers.
pub type InstructionEntrySnapshot = Vec<InstructionEntrySnapshotItem>;

/// One entry of [`InstructionEntrySnapshot`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InstructionEntrySnapshotItem {
    pub key: String,
    pub value: Value,
    pub removed: bool,
}

/// `Session.Revert`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Revert {
    #[serde(rename = "messageID")]
    pub message_id: MessageId,
    /// Legacy V1 compatibility state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<SnapshotId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<FileDiff>>,
}

/// `Session.RevertV1` — persisted legacy shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RevertV1 {
    #[serde(rename = "messageID")]
    pub message_id: MessageId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<LegacyFileDiff>>,
}

/// Keep imports referenced for parity.
#[allow(dead_code)]
type _Path = AbsolutePath;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_context_wire() {
        let context = ProviderContext {
            version: (),
            provenance: Provenance {
                provider_id: ProviderId::anthropic(),
                provider: "anthropic".into(),
                model_id: "claude".into(),
                route: "chat".into(),
                protocol: "anthropic-messages".into(),
                endpoint: "sha256:abc".into(),
            },
            messages: serde_json::json!([{"role": "user", "content": "hi"}]),
        };
        let json = serde_json::to_value(&context).unwrap();
        assert_eq!(json["version"], serde_json::json!(1));
        let back: ProviderContext = serde_json::from_value(json).unwrap();
        assert_eq!(back, context);
        assert!(serde_json::from_value::<ProviderContext>(
            serde_json::json!({"version": 2, "provenance": null, "messages": null})
        )
        .is_err());
    }

    #[test]
    fn fork_boundary_wire() {
        let value = serde_json::json!({"type": "through", "messageID": "msg_2"});
        let parsed: ForkBoundary = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    }

    #[test]
    fn instruction_delta_wire() {
        let delta = serde_json::json!({
            "api/style": "a".repeat(64),
            "old/entry": "removed"
        });
        let parsed: InstructionDelta = serde_json::from_value(delta).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed.get("old/entry"), Some(&InstructionDeltaValue::Removed));
    }
}
