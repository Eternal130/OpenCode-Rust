//! Mirror of v2's durable event manifest (`packages/schema/src/durable-event-manifest.ts`).
//!
//! The manifest is `Event.durableMap([...SessionEvent.DurableDefinitions,
//! Worktree.Event.Resolved])` keyed by the **versioned type**
//! `"{type}.{version}"`. This port keeps the full session event inventory
//! (52 definitions incl. the ephemeral `session.diff` remainder, durable +
//! ephemeral) plus `worktree.resolved`, exposing:
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
    // `session.diff` (schema/src/v1/session.ts:633-639): ephemeral
    // `{sessionID, diff}` published alongside the session.diff service.
    "session.diff",
];

fn session_def(type_: &str, version: u64) -> EventDefinition {
    EventDefinition {
        type_: type_.to_string(),
        durable: Some(DurableMeta { aggregate: SESSION_AGGREGATE.to_string(), version }),
    }
}

static DEFINITIONS: LazyLock<Vec<EventDefinition>> = LazyLock::new(|| {
    let mut defs = Vec::with_capacity(53);
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
        // BE-2b remainder: the ephemeral `session.diff` event
        // (schema/src/v1/session.ts:633-639) joins the registry so the
        // server can publish turn diffs on the session aggregate.
        "session.diff",
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

/// `SessionEvent.All.cases` membership (all 52 session event types).
static SESSION_TYPES: LazyLock<std::collections::HashSet<&'static str>> = LazyLock::new(|| {
    DEFINITIONS
        .iter()
        .filter(|def| def.type_.starts_with("session."))
        .map(|def| def.type_.as_str())
        .collect()
});

/// The full event inventory (52 session events — 51 in enum order + the
/// ephemeral `session.diff` remainder — + `worktree.resolved`).
#[must_use]
pub fn definitions() -> &'static [EventDefinition] {
    &DEFINITIONS
}

// ---------------------------------------------------------------------------
// Non-session events v2 publishes ad-hoc
// ---------------------------------------------------------------------------
// v2's `EventManifest.ServerDefinitions`/`Definitions` aggregate many
// per-domain ephemeral inventories (location-event.ts, config.ts,
// permission.ts, project.ts, …). Phase 5 (SSE) needs a single lookup for
// every type the server can emit, so the non-session types oc-core
// actually publishes today are centralized here — every one of them
// ephemeral, exactly like v2:
//
// - `location.shutdown` — `location-event.ts:6` (published by the location
//   shutdown sequence, `location-lifecycle.ts`),
// - `config.updated` — `config.ts:133-135`,
// - `permission.asked` / `permission.replied` — `permission.ts:44-49`,
// - `project.updated` — `project.ts:60-61`.
//
// `worktree.resolved` is already part of [`DEFINITIONS`] (durable v1).

/// Type of the location shutdown event (`location-event.ts`).
pub const LOCATION_SHUTDOWN: &str = "location.shutdown";
/// Type of the config hot-reload event (`config.ts`).
pub const CONFIG_UPDATED: &str = "config.updated";
/// Type of the permission prompt event (`permission.ts`).
pub const PERMISSION_ASKED: &str = "permission.asked";
/// Type of the permission answer event (`permission.ts`).
pub const PERMISSION_REPLIED: &str = "permission.replied";
/// Type of the project registry event (`project.ts`).
pub const PROJECT_UPDATED: &str = "project.updated";
/// Type of the MCP tool-list event (`mcp-event.ts`, wave D).
pub const MCP_TOOLS_CHANGED: &str = "mcp.tools.changed";
/// Type of the MCP resource-list event (`mcp-event.ts`, wave D).
pub const MCP_RESOURCES_CHANGED: &str = "mcp.resources.changed";
/// Type of the MCP status event (`mcp-event.ts`, wave D).
pub const MCP_STATUS_CHANGED: &str = "mcp.status.changed";
/// Type of the MCP prompt-list event (`mcp/index.ts`, wave D).
pub const MCP_PROMPTS_CHANGED: &str = "mcp.prompts.changed";
/// Type of the PTY lifecycle event (`schema/pty.ts`, wave D).
pub const PTY_CREATED: &str = "pty.created";
/// Type of the PTY lifecycle event (`schema/pty.ts`, wave D).
pub const PTY_UPDATED: &str = "pty.updated";
/// Type of the PTY lifecycle event (`schema/pty.ts`, wave D).
pub const PTY_EXITED: &str = "pty.exited";
/// Type of the PTY lifecycle event (`schema/pty.ts`, wave D).
pub const PTY_DELETED: &str = "pty.deleted";
/// Type of the persistent-PTY lifecycle event (`schema/persistent-pty.ts`, wave D).
pub const PERSISTENT_PTY_ADDED: &str = "persistent-pty.added";
/// Type of the persistent-PTY lifecycle event (`schema/persistent-pty.ts`, wave D).
pub const PERSISTENT_PTY_REMOVED: &str = "persistent-pty.removed";
/// Type of the form lifecycle events (`schema/form.ts`, `Event.Definitions`).
pub const FORM_CREATED: &str = "form.created";
/// Type of the form lifecycle events (`schema/form.ts`, `Event.Definitions`).
pub const FORM_REPLIED: &str = "form.replied";
/// Type of the form lifecycle events (`schema/form.ts`, `Event.Definitions`).
pub const FORM_CANCELLED: &str = "form.cancelled";
/// Type of the shell lifecycle events (`schema/shell.ts:55-58`).
pub const SHELL_CREATED: &str = "shell.created";
/// Type of the shell lifecycle events (`schema/shell.ts:55-58`).
pub const SHELL_EXITED: &str = "shell.exited";
/// Type of the shell lifecycle events (`schema/shell.ts:55-58`).
pub const SHELL_DELETED: &str = "shell.deleted";
/// `session.status` `{sessionID, status}` (`schema/session-status-event.ts:35-43`).
pub const SESSION_STATUS: &str = "session.status";
/// Deprecated `session.idle` `{sessionID}` (`session-status-event.ts:46-51`).
pub const SESSION_IDLE: &str = "session.idle";
/// `filesystem.changed` `{file, event}` (`schema/filesystem.ts`).
pub const FILESYSTEM_CHANGED: &str = "filesystem.changed";
/// `worktree.updated` `{projectID}` (`schema/worktree.ts:55-58`).
pub const WORKTREE_UPDATED: &str = "worktree.updated";
/// `vcs.branch.updated` `{branch?}` (`schema/vcs-event.ts`).
pub const VCS_BRANCH_UPDATED: &str = "vcs.branch.updated";
/// `credential.updated` `{}` (`schema/credential.ts:17-20`).
pub const CREDENTIAL_UPDATED: &str = "credential.updated";
/// `credential.switched` `{integrationID, credentialID}` (`credential.ts:21-24`).
pub const CREDENTIAL_SWITCHED: &str = "credential.switched";
/// `integration.updated` `{}` (`schema/integration.ts:57-60`): the
/// integration registry transform notify (BE-3).
pub const INTEGRATION_UPDATED: &str = "integration.updated";
/// `models-dev.refreshed` `{}` (`schema/models-dev.ts:5-8`).
pub const MODELS_DEV_REFRESHED: &str = "models-dev.refreshed";
/// `tui.prompt.append` `{text}` (`schema/tui-event.ts`).
pub const TUI_PROMPT_APPEND: &str = "tui.prompt.append";
/// `tui.command.execute` `{command}` (`schema/tui-event.ts`).
pub const TUI_COMMAND_EXECUTE: &str = "tui.command.execute";
/// `tui.toast.show` `{title?, message, variant, duration}` (`schema/tui-event.ts`).
pub const TUI_TOAST_SHOW: &str = "tui.toast.show";
/// `tui.session.select` `{sessionID}` (`schema/tui-event.ts`).
pub const TUI_SESSION_SELECT: &str = "tui.session.select";
/// `installation.updated` `{version}` (`schema/installation-event.ts`).
pub const INSTALLATION_UPDATED: &str = "installation.updated";
/// `installation.update-available` `{version}` (`installation-event.ts`).
pub const INSTALLATION_UPDATE_AVAILABLE: &str = "installation.update-available";

static NON_SESSION: LazyLock<Vec<EventDefinition>> = LazyLock::new(|| {
    vec![
        EventDefinition::ephemeral(LOCATION_SHUTDOWN),
        EventDefinition::ephemeral(CONFIG_UPDATED),
        EventDefinition::ephemeral(PERMISSION_ASKED),
        EventDefinition::ephemeral(PERMISSION_REPLIED),
        EventDefinition::ephemeral(PROJECT_UPDATED),
        EventDefinition::ephemeral(MCP_TOOLS_CHANGED),
        EventDefinition::ephemeral(MCP_RESOURCES_CHANGED),
        EventDefinition::ephemeral(MCP_STATUS_CHANGED),
        EventDefinition::ephemeral(MCP_PROMPTS_CHANGED),
        EventDefinition::ephemeral(PTY_CREATED),
        EventDefinition::ephemeral(PTY_UPDATED),
        EventDefinition::ephemeral(PTY_EXITED),
        EventDefinition::ephemeral(PTY_DELETED),
        EventDefinition::ephemeral(PERSISTENT_PTY_ADDED),
        EventDefinition::ephemeral(PERSISTENT_PTY_REMOVED),
        // The six published-but-SSE-filtered types (auditD §3.4): oc-core
        // forms/shell publish them; the public-event lookup must resolve
        // them so the SSE feed stops dropping the frames.
        EventDefinition::ephemeral(FORM_CREATED),
        EventDefinition::ephemeral(FORM_REPLIED),
        EventDefinition::ephemeral(FORM_CANCELLED),
        EventDefinition::ephemeral(SHELL_CREATED),
        EventDefinition::ephemeral(SHELL_EXITED),
        EventDefinition::ephemeral(SHELL_DELETED),
        // The fourteen v2 public types the port never published (auditD
        // §3.4): registered here so publishers (execution hooks, credential
        // handlers, worktree/vcs, catalog refresh, installation) reach the
        // public stream, exactly like v2's `EventManifest.ServerDefinitions`.
        EventDefinition::ephemeral(SESSION_STATUS),
        EventDefinition::ephemeral(SESSION_IDLE),
        EventDefinition::ephemeral(FILESYSTEM_CHANGED),
        EventDefinition::ephemeral(WORKTREE_UPDATED),
        EventDefinition::ephemeral(VCS_BRANCH_UPDATED),
        EventDefinition::ephemeral(CREDENTIAL_UPDATED),
        EventDefinition::ephemeral(CREDENTIAL_SWITCHED),
        EventDefinition::ephemeral(MODELS_DEV_REFRESHED),
        EventDefinition::ephemeral(TUI_PROMPT_APPEND),
        EventDefinition::ephemeral(TUI_COMMAND_EXECUTE),
        EventDefinition::ephemeral(TUI_TOAST_SHOW),
        EventDefinition::ephemeral(TUI_SESSION_SELECT),
        EventDefinition::ephemeral(INSTALLATION_UPDATED),
        EventDefinition::ephemeral(INSTALLATION_UPDATE_AVAILABLE),
        // The integration-host registry notify (BE-3; append-only — the
        // positional accessors above are unaffected).
        EventDefinition::ephemeral(INTEGRATION_UPDATED),
    ]
});

static BY_NON_SESSION_TYPE: LazyLock<HashMap<String, usize>> = LazyLock::new(|| {
    NON_SESSION.iter().enumerate().map(|(index, def)| (def.type_.clone(), index)).collect()
});

/// The non-session definitions oc-core publishes (all ephemeral). Session +
/// `worktree.resolved` definitions live in [`definitions`].
#[must_use]
pub fn non_session_definitions() -> &'static [EventDefinition] {
    &NON_SESSION
}

/// The `location.shutdown` definition (location shutdown sequence).
#[must_use]
pub fn location_shutdown() -> &'static EventDefinition {
    &NON_SESSION[0]
}

/// The `config.updated` definition (config hot reload).
#[must_use]
pub fn config_updated() -> &'static EventDefinition {
    &NON_SESSION[1]
}

/// The `permission.asked` definition.
#[must_use]
pub fn permission_asked() -> &'static EventDefinition {
    &NON_SESSION[2]
}

/// The `permission.replied` definition.
#[must_use]
pub fn permission_replied() -> &'static EventDefinition {
    &NON_SESSION[3]
}

/// The `project.updated` definition.
#[must_use]
pub fn project_updated() -> &'static EventDefinition {
    &NON_SESSION[4]
}

/// The `mcp.tools.changed` definition.
#[must_use]
pub fn mcp_tools_changed() -> &'static EventDefinition {
    &NON_SESSION[5]
}

/// The `mcp.resources.changed` definition.
#[must_use]
pub fn mcp_resources_changed() -> &'static EventDefinition {
    &NON_SESSION[6]
}

/// The `mcp.status.changed` definition.
#[must_use]
pub fn mcp_status_changed() -> &'static EventDefinition {
    &NON_SESSION[7]
}

/// Single lookup across the whole known inventory — session events
/// (highest version, `Event.latest`), `worktree.resolved`, and the
/// non-session ephemeral types. This is the lookup the Phase 5 SSE encoder
/// uses to map a wire `type` to its definition; unknown types return
/// `None` (encoded verbatim, like v2's untagged server events).
#[must_use]
pub fn by_type(type_: &str) -> Option<&'static EventDefinition> {
    by_type_latest(type_)
        .or_else(|| BY_NON_SESSION_TYPE.get(type_).map(|&index| &NON_SESSION[index]))
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

/// Whether `type_` is one of the 52 session event types (gates routing).
#[must_use]
pub fn is_session_event(type_: &str) -> bool {
    SESSION_TYPES.contains(type_)
}
