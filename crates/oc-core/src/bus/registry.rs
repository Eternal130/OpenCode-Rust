//! Mirror of v2's durable event manifest (`packages/schema/src/durable-event-manifest.ts`).
//!
//! The manifest is `Event.durableMap([...SessionEvent.DurableDefinitions,
//! Worktree.Event.Resolved])` keyed by the **versioned type**
//! `"{type}.{version}"`. This port keeps the full session event inventory
//! (51 definitions, durable + ephemeral) plus `worktree.resolved`, exposing:
//!
//! - [`definitions`] — the ordered inventory (enum order of
//!   `oc_schema::session_event::Event`, then `worktree.resolved`).
//! - [`by_versioned_type`] — the durable manifest lookup used to decode
//!   stored `event.type` rows (`"session.deleted.2"` → definition). Only
//!   durable definitions resolve here, exactly like v2's `Durable.get`.
//! - [`by_type_latest`] — `Event.latest`: highest version per base type.
//! - [`is_session_event`] — membership in `SessionEvent.All.cases`, which
//!   gates the bus's location routing.

use std::collections::HashMap;
use std::sync::LazyLock;

use oc_schema::event::{versioned_type, DurableInfo};

/// Aggregate field of every durable session event.
pub const SESSION_AGGREGATE: &str = "sessionID";

/// Aggregate field of `worktree.resolved`.
pub const WORKTREE_AGGREGATE: &str = "projectID";

/// Durable metadata attached to an [`EventDefinition`] (v2
/// `Event.durable({ durable: { aggregate, version } })`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableMeta {
    /// Data field holding the aggregate id (e.g. `"sessionID"`).
    pub aggregate: String,
    /// Durable schema version of this event type (>= 1).
    pub version: u64,
}

/// One event definition from the inventory.
///
/// Encoded (stored) type of a durable definition is
/// [`versioned_type`](`oc_schema::event::versioned_type`)`("{type}", {version}")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDefinition {
    /// Wire `type` (e.g. `"session.deleted"`).
    pub type_: String,
    /// `Some` for durable events, `None` for ephemeral ones.
    pub durable: Option<DurableMeta>,
}

impl EventDefinition {
    /// An ephemeral definition (no durable metadata).
    #[must_use]
    pub fn ephemeral(type_: &str) -> Self {
        Self { type_: type_.to_string(), durable: None }
    }

    /// A durable definition.
    #[must_use]
    pub fn durable(type_: &str, aggregate: &str, version: u64) -> Self {
        Self {
            type_: type_.to_string(),
            durable: Some(DurableMeta { aggregate: aggregate.to_string(), version }),
        }
    }

    /// The encoded `"{type}.{version}"` key, when durable.
    #[must_use]
    pub fn versioned_type(&self) -> Option<String> {
        self.durable.as_ref().map(|meta| versioned_type(&self.type_, meta.version))
    }

    /// Builds a [`DurableInfo`] envelope for this definition.
    #[must_use]
    pub fn envelope(&self, aggregate_id: &str, seq: i64) -> Option<DurableInfo> {
        self.durable.as_ref().map(|meta| DurableInfo {
            aggregate_id: aggregate_id.to_string(),
            seq: u64::try_from(seq).unwrap_or(0),
            version: meta.version,
        })
    }
}

/// Durable session events at version 1 (v2 `options` block,
/// `session-event.ts:42-47`).
const SESSION_V1: &[&str] = &[
    "session.created",
    "session.agent.selected",
    "session.model.selected",
    "session.moved",
    "session.renamed",
    "session.metadata.updated",
    "session.permissions",
    "session.viewed",
    "session.inbox.delivered",
    "session.inbox.enqueued",
    "session.inbox.cancelled",
    "session.inbox.delivery.changed",
    "session.execution.started",
    "session.execution.succeeded",
    "session.execution.failed",
    "session.execution.interrupted",
    "session.synthetic",
    "session.skill.activated",
    "session.shell.started",
    "session.shell.ended",
    "session.step.started",
    "session.step.streamed",
    "session.step.ended",
    "session.step.failed",
    "session.text.started",
    "session.text.ended",
    "session.reasoning.started",
    "session.reasoning.ended",
    "session.tool.input.started",
    "session.tool.input.ended",
    "session.tool.called",
    "session.retry.scheduled",
    "session.compaction.started",
    "session.compaction.ended",
    "session.compaction.failed",
    "session.revert.staged",
    "session.revert.cleared",
    "session.revert.committed",
    // Internal + replay-only events: excluded from the public manifest but
    // part of `DurableDefinitions` (session-event.ts:704-709).
    "session.usage.recorded",
    "session.message.content.updated",
];

/// Durable session events at version 2 (explicit version blocks:
/// `session.deleted`, `session.forked`, `session.instructions.updated`,
/// `session.tool.success`, `session.tool.failed`).
const SESSION_V2: &[&str] = &[
    "session.deleted",
    "session.forked",
    "session.instructions.updated",
    "session.tool.success",
    "session.tool.failed",
];

/// Ephemeral session events.
const SESSION_EPHEMERAL: &[&str] = &[
    "session.usage.updated",
    "session.text.delta",
    "session.reasoning.delta",
    "session.tool.input.delta",
    "session.tool.progress",
    "session.compaction.delta",
];

fn session_def(type_: &str, version: u64) -> EventDefinition {
    EventDefinition {
        type_: type_.to_string(),
        durable: Some(DurableMeta { aggregate: SESSION_AGGREGATE.to_string(), version }),
    }
}

static DEFINITIONS: LazyLock<Vec<EventDefinition>> = LazyLock::new(|| {
    let mut defs = Vec::with_capacity(52);
    // Enum order of oc_schema::session_event::Event.
    let order: &[&str] = &[
        "session.created",
        "session.agent.selected",
        "session.model.selected",
        "session.moved",
        "session.renamed",
        "session.metadata.updated",
        "session.permissions",
        "session.viewed",
        "session.usage.updated",
        "session.deleted",
        "session.forked",
        "session.inbox.delivered",
        "session.inbox.enqueued",
        "session.inbox.cancelled",
        "session.inbox.delivery.changed",
        "session.execution.started",
        "session.execution.succeeded",
        "session.execution.failed",
        "session.execution.interrupted",
        "session.instructions.updated",
        "session.synthetic",
        "session.skill.activated",
        "session.shell.started",
        "session.shell.ended",
        "session.step.started",
        "session.step.streamed",
        "session.step.ended",
        "session.step.failed",
        "session.text.started",
        "session.text.delta",
        "session.text.ended",
        "session.reasoning.started",
        "session.reasoning.delta",
        "session.reasoning.ended",
        "session.tool.input.started",
        "session.tool.input.delta",
        "session.tool.input.ended",
        "session.tool.called",
        "session.tool.progress",
        "session.tool.success",
        "session.tool.failed",
        "session.retry.scheduled",
        "session.compaction.started",
        "session.compaction.delta",
        "session.compaction.ended",
        "session.compaction.failed",
        "session.revert.staged",
        "session.revert.cleared",
        "session.revert.committed",
        "session.usage.recorded",
        "session.message.content.updated",
    ];
    for type_ in order {
        if SESSION_V1.contains(type_) {
            defs.push(session_def(type_, 1));
        } else if SESSION_V2.contains(type_) {
            defs.push(session_def(type_, 2));
        } else if SESSION_EPHEMERAL.contains(type_) {
            defs.push(EventDefinition::ephemeral(type_));
        } else {
            unreachable!("unclassified session event {type_}");
        }
    }
    // Worktree.Event.Resolved — durable v1 on the projectID aggregate.
    defs.push(EventDefinition {
        type_: "worktree.resolved".to_string(),
        durable: Some(DurableMeta { aggregate: WORKTREE_AGGREGATE.to_string(), version: 1 }),
    });
    defs
});

/// Durable-manifest index: versioned type → definition.
static BY_VERSIONED: LazyLock<HashMap<String, usize>> = LazyLock::new(|| {
    DEFINITIONS
        .iter()
        .enumerate()
        .filter_map(|(index, def)| def.versioned_type().map(|key| (key, index)))
        .collect()
});

/// `Event.latest` index: base type → highest-version definition.
static BY_TYPE_LATEST: LazyLock<HashMap<String, usize>> = LazyLock::new(|| {
    let mut latest: HashMap<String, (u64, usize)> = HashMap::new();
    for (index, def) in DEFINITIONS.iter().enumerate() {
        let version = def.durable.as_ref().map_or(0, |meta| meta.version);
        match latest.get(&def.type_) {
            Some((current, _)) if *current >= version => {}
            _ => {
                latest.insert(def.type_.clone(), (version, index));
            }
        }
    }
    latest.into_iter().map(|(type_, (_, index))| (type_, index)).collect()
});

/// `SessionEvent.All.cases` membership (all 51 session event types).
static SESSION_TYPES: LazyLock<std::collections::HashSet<&'static str>> = LazyLock::new(|| {
    DEFINITIONS
        .iter()
        .filter(|def| def.type_.starts_with("session."))
        .map(|def| def.type_.as_str())
        .collect()
});

/// The full event inventory (51 session events in enum order + `worktree.resolved`).
#[must_use]
pub fn definitions() -> &'static [EventDefinition] {
    &DEFINITIONS
}

/// The durable manifest lookup (v2 `Durable.get`): decodes stored
/// `event.type` values. Returns `None` for ephemeral and unknown types.
#[must_use]
pub fn by_versioned_type(encoded: &str) -> Option<&'static EventDefinition> {
    BY_VERSIONED.get(encoded).map(|&index| &DEFINITIONS[index])
}

/// `Event.latest`: the highest-version definition of a base type.
#[must_use]
pub fn by_type_latest(type_: &str) -> Option<&'static EventDefinition> {
    BY_TYPE_LATEST.get(type_).map(|&index| &DEFINITIONS[index])
}

/// Whether `type_` is one of the 51 session event types (gates routing).
#[must_use]
pub fn is_session_event(type_: &str) -> bool {
    SESSION_TYPES.contains(type_)
}
