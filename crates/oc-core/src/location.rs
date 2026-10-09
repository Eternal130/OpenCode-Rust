//! Location-scoped service graph — the per-(directory, workspace) slice of
//! opencode's service DAG (port of the v2 `Instance.layer(ref)` 55-node
//! location layer, per core-deep-map §5).
//!
//! # Rust shape (graph v2 — oracle gate 4, finding 1)
//!
//! - [`LocationRef`] is the canonical map key: `{directory, workspace_id?}`
//!   with forward-slash normalization.
//! - [`crate::location_services::LocationServiceMap`] is an
//!   `Arc<DashMap<LocationRef, Arc<LocationGraph>>>`.
//! - [`LocationGraph`] holds **concrete** handles to the global services —
//!   [`Arc<Database>`](crate::database::Database) +
//!   [`Arc<Bus>`](crate::bus::Bus) + [`Arc<Catalog>`](crate::provider_catalog::Catalog)
//!   — plus the **per-location** state nodes:
//!   [`AgentRegistry`], [`ToolRegistryState`], [`ShellEnv`],
//!   [`FileAccessState`], [`InstructionSlots`], and the plugin
//!   [`HookRegistry`]. The Phase 3 placeholder port traits are gone: every
//!   consumer lives in this crate, so the trait seam bought nothing.
//! - **Build is fallible** ([`BootError`]): a missing directory fails the
//!   boot with a typed error (v2: "Missing directory ⇒ typed boot error"),
//!   and the database is probed before the graph is published.
//! - **Old-graph survival**: borrows hold an `Arc<LocationGraph>` clone;
//!   invalidation only removes the `DashMap` entry. The graph is **never
//!   mutated in place** — reload builds a NEW graph and swaps the entry, so
//!   running steps keep borrowing the old graph until they finish (v2:
//!   "running steps keep borrowing old graph until boundary").
//! - Invalidation runs the v2 shutdown sequence (`location-lifecycle.ts`):
//!   permission close → forms close (placeholder) → rpc close (placeholder)
//!   → publish `location.shutdown` (data `{}`, location-stamped).
//! - Plugin boot order (v2 `supervisor.ts`): internal **pre** plugins →
//!   host plugins (placeholder slot, `TODO(plugin-lane)`) → internal
//!   **post** plugins, with per-plugin failure containment.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use oc_util::wildcard;
use parking_lot::Mutex;

use crate::bus::Bus;
use crate::database::Database;
use crate::hooks::{HookRegistry, PluginFailure, RpcPort};
use crate::provider_catalog::Catalog;

// =============================================================================
// Location reference (the canonical key)
// =============================================================================

/// Canonical location key: `{directory, workspace_id?}`. The directory is
/// forward-slash normalized so Windows-style separators compare equal to
/// their POSIX spelling (v2 storage normalizes Windows↔POSIX the same way).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocationRef {
    pub directory: PathBuf,
    pub workspace_id: Option<String>,
}

impl LocationRef {
    /// Build a canonical key (normalizes backslashes to forward slashes).
    pub fn new(directory: impl Into<PathBuf>, workspace_id: Option<String>) -> Self {
        Self { directory: normalize_directory(&directory.into()), workspace_id }
    }

    /// The directory as a `&Path`.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.directory
    }
}

/// Forward-slash normalization: `\` → `/` (a no-op on POSIX inputs, where
/// backslash is a legal filename character — v2 tolerates the same
/// ambiguity in its path columns).
fn normalize_directory(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().replace('\\', "/");
    PathBuf::from(text)
}

impl fmt::Display for LocationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.workspace_id {
            Some(workspace) => write!(f, "{}#{}", self.directory.display(), workspace),
            None => write!(f, "{}", self.directory.display()),
        }
    }
}

impl From<oc_schema::location::Ref> for LocationRef {
    fn from(value: oc_schema::location::Ref) -> Self {
        Self::new(PathBuf::from(value.directory.0), value.workspace_id.map(|id| id.0))
    }
}

impl From<&oc_schema::location::Ref> for LocationRef {
    fn from(value: &oc_schema::location::Ref) -> Self {
        Self::new(
            PathBuf::from(value.directory.0.clone()),
            value.workspace_id.as_ref().map(|id| id.0.clone()),
        )
    }
}

// =============================================================================
// Boot failure
// =============================================================================

/// Typed location-boot failure (v2 "Missing directory ⇒ typed boot error";
/// the graph is not published to the map, so a later `get_or_build` retries).
#[derive(Debug, thiserror::Error)]
pub enum BootError {
    /// The location directory does not exist (or is not a directory).
    #[error("location directory does not exist: {}", .0.display())]
    MissingDirectory(PathBuf),
    /// The shared database failed its boot probe.
    #[error("location database check failed: {0}")]
    Database(#[from] crate::database::DbError),
}

// =============================================================================
// Per-location handles supplied by the owning lanes
// =============================================================================

/// Config cache handle: hot-reload marker for the per-location `Config`
/// node. The generation increments whenever a config document changes
/// (`config.updated`); a stale graph is invalidated and rebuilt.
///
/// `TODO(lane-integration)` (config lane).
pub trait ConfigCache: Send + Sync {
    /// Monotonic config generation for this location.
    fn generation(&self) -> u64;
}

/// Pending-permission handle: the per-location `Permission` node's pending
/// request set. `close` rejects every pending request (v2 shutdown:
/// reject-all + `permission.replied` + `DeclinedError`).
///
/// `TODO(lane-integration)` (permission lane).
#[async_trait]
pub trait PermissionPending: Send + Sync {
    /// Reject all pending requests; returns the rejected request ids.
    async fn close(&self) -> Vec<String>;
    /// Number of currently parked permission requests.
    async fn pending_count(&self) -> usize;
}

/// Forms port (per-location `Form` node close). Placeholder seam.
///
/// `TODO(lane-integration)` (server/forms lane).
#[async_trait]
pub trait FormsPort: Send + Sync {
    async fn close(&self) -> Vec<String>;
}

/// Session interrupt port: interrupt active executions for a location (the
/// activity-TTL sweeper's "interrupt with reason `inactivity`, await
/// settlement" step; the session-runner lane implements it).
///
/// `TODO(lane-integration)`.
#[async_trait]
pub trait SessionInterruptor: Send + Sync {
    /// Interrupt running session steps for `location`; resolves once the
    /// executions settled.
    async fn interrupt(&self, location: &LocationRef, reason: &str);
}

// =============================================================================
// Per-location service state nodes
// =============================================================================

/// `agent.ts` selection result (`Agent.select`).
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSelection {
    pub id: oc_schema::id::AgentId,
    pub info: Option<oc_schema::agent::Info>,
}

/// The per-location agent registry (`Agent` node) — in-memory agent state
/// with the v2 `select`/`list` semantics (core-deep-map §5 / agent.ts):
///
/// - **selectable**: `mode !== "subagent" && !hidden`;
/// - **selected default**: configured default → `build` → first selectable
///   (in id order — the Rust table is a `BTreeMap`, v2 uses insertion
///   order; Wave 0 has no insertion-order observers);
/// - **`select(id?)`**: an explicit id resolves verbatim (`info` may be
///   `None`, matching v2); otherwise the selected default, falling back to
///   `{id: "build", info: None}`;
/// - **`list()`**: the selected agent first, then every other agent.
///
/// The table starts **empty**; markdown agent ingestion arrives with the
/// config lane via [`AgentRegistry::set`]/[`AgentRegistry::update`], and
/// the `opencode.agent` internal plugin seeds the built-ins.
pub struct AgentRegistry {
    agents: Mutex<std::collections::BTreeMap<oc_schema::id::AgentId, oc_schema::agent::Info>>,
    default_id: oc_schema::id::AgentId,
    configured_default: Mutex<Option<oc_schema::id::AgentId>>,
}

impl fmt::Debug for AgentRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.agents.lock();
        let ids: Vec<&str> = guard.keys().map(oc_schema::id::AgentId::as_str).collect();
        write!(f, "AgentRegistry({ids:?})")
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    /// Fresh empty registry with the v2 default id (`build`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            agents: Mutex::new(std::collections::BTreeMap::new()),
            default_id: oc_schema::id::AgentId::parse("build"),
            configured_default: Mutex::new(None),
        }
    }

    /// Insert or replace one agent (config-lane markdown ingestion, or the
    /// agent plugin's built-in seeds).
    pub fn set(&self, agent: oc_schema::agent::Info) {
        self.agents.lock().insert(agent.id.clone(), agent);
    }

    /// Edit one agent in place (`ctx.agent.update` editor shape). `None`
    /// when the id is unknown.
    pub fn update(
        &self,
        id: &oc_schema::id::AgentId,
        editor: impl FnOnce(&mut oc_schema::agent::Info),
    ) -> Option<()> {
        let mut agents = self.agents.lock();
        let agent = agents.get_mut(id)?;
        editor(agent);
        Some(())
    }

    /// Remove one agent.
    pub fn remove(&self, id: &oc_schema::id::AgentId) {
        self.agents.lock().remove(id);
    }

    /// Set the configured default agent (`config.agent.default`); `None`
    /// clears it.
    pub fn set_default(&self, id: Option<oc_schema::id::AgentId>) {
        *self.configured_default.lock() = id;
    }

    /// `Agent.get`.
    #[must_use]
    pub fn get(&self, id: &oc_schema::id::AgentId) -> Option<oc_schema::agent::Info> {
        self.agents.lock().get(id).cloned()
    }

    /// A fresh agent id-agnostic count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.lock().len()
    }

    /// Whether the registry holds no agents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.lock().is_empty()
    }

    fn selectable(info: &oc_schema::agent::Info) -> bool {
        info.mode != oc_schema::agent::Mode::Subagent && !info.hidden
    }

    /// The selected default agent (agent.ts `selectedDefault`):
    /// configured default → `build` → first selectable.
    #[must_use]
    pub fn selected_default(&self) -> Option<oc_schema::agent::Info> {
        let agents = self.agents.lock();
        if let Some(configured) = self.configured_default.lock().as_ref() {
            if let Some(info) = agents.get(configured) {
                if Self::selectable(info) {
                    return Some(info.clone());
                }
            }
        }
        if let Some(build) = agents.get(&self.default_id) {
            if Self::selectable(build) {
                return Some(build.clone());
            }
        }
        agents.values().find(|info| Self::selectable(info)).cloned()
    }

    /// `Agent.resolve`: an explicit id resolves verbatim (selectability not
    /// consulted); `None` resolves the selected default.
    #[must_use]
    pub fn resolve(&self, id: Option<&str>) -> Option<oc_schema::agent::Info> {
        match id {
            Some(id) => self.get(&oc_schema::id::AgentId::parse(id)),
            None => self.selected_default(),
        }
    }

    /// `Agent.select`: `{id, info}` — explicit ids keep the id even when
    /// unknown; the default selection falls back to the default id.
    #[must_use]
    pub fn select(&self, id: Option<&str>) -> AgentSelection {
        match id {
            Some(id) => {
                let id = oc_schema::id::AgentId::parse(id);
                AgentSelection { info: self.get(&id), id }
            }
            None => match self.selected_default() {
                Some(info) => AgentSelection { id: info.id.clone(), info: Some(info) },
                None => AgentSelection { id: self.default_id.clone(), info: None },
            },
        }
    }

    /// `Agent.list`: the selected agent first, then every other agent.
    #[must_use]
    pub fn list(&self) -> Vec<oc_schema::agent::Info> {
        let agents: Vec<oc_schema::agent::Info> = self.agents.lock().values().cloned().collect();
        match self.selected_default() {
            Some(selected) => {
                let mut rest: Vec<oc_schema::agent::Info> =
                    agents.into_iter().filter(|info| info.id != selected.id).collect();
                std::iter::once(selected).chain(rest.drain(..)).collect()
            }
            None => agents,
        }
    }
}

/// `Tool.Info` skeleton — the registry row without the executor. Wave C
/// (tools) fills `execute` wiring via its own handle type; Wave 0 pins the
/// identity + schema surface the permission filter needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRegistration {
    /// Bare tool name (`^[A-Za-z0-9_-]{1,128}$`; `execute` is reserved for
    /// `CodeMode`).
    pub name: String,
    pub description: String,
    /// JSON-Schema of the input.
    pub input: serde_json::Value,
    /// `options.namespace` — namespacing prefixes the effective name.
    pub namespace: Option<String>,
    /// `options.permission` — the permission action; defaults to the
    /// effective tool name.
    pub permission: Option<String>,
    /// `options.codemode !== false` (direct tools opt out with `false`).
    pub codemode: bool,
}

/// A locally executed tool call's terminal success (Wave C owns the full
/// decode → execute → encode pipeline; Wave B's runner carries this shape).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRunResult {
    /// Non-empty content parts (`Tool.NormalizedResult.content`).
    pub content: Vec<oc_schema::tool::Content>,
    /// Optional tool metadata (`Tool.NormalizedResult.metadata`).
    pub metadata: Option<oc_schema::common::Record>,
}

/// A local tool execution failure (session-runner lane, step.ts §2).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolRunFailure {
    /// `Tool.Error` — settles through `session.tool.failed` with the mapped
    /// error.
    Error { message: String, metadata: Option<oc_schema::common::Record> },
    /// `Permission.DeclinedError` / `QuestionTool.CancelledError` — declines
    /// end the step as an interrupt.
    Declined { message: String },
}

/// The invocation identity handed to context-aware tool executors
/// (`Tool.Context`: session, agent, assistant message, call id).
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    pub session_id: oc_schema::id::SessionId,
    pub agent: oc_schema::id::AgentId,
    pub message_id: String,
    pub call_id: String,
}

/// The local execute seam (Wave C replaces this with the full tool
/// pipeline; Wave B's runner + tests register stub executors here).
#[async_trait::async_trait]
pub trait ToolExecute: Send + Sync {
    async fn execute(&self, input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure>;

    /// Context-aware surface (Wave C1 tool leaves): default delegates to the
    /// bare call so stub executors keep working unchanged.
    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        let _ = context;
        self.execute(input).await
    }
}

/// One boxed tool executor (test/registry storage).
pub type ToolExecutorHandle = Arc<dyn ToolExecute>;

impl ToolRegistration {
    /// Minimal registration (codemode-enabled, no namespace).
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input,
            namespace: None,
            permission: None,
            codemode: true,
        }
    }

    /// The effective permission action (`options.permission ?? name`).
    #[must_use]
    pub fn permission_action(&self) -> &str {
        self.permission.as_deref().unwrap_or(&self.name)
    }
}

/// `whollyDisabled` (tool.ts:292-295): the LAST rule whose action
/// wildcard-matches `action` has resource `"*"` and effect `deny`.
#[must_use]
pub fn wholly_denied(action: &str, rules: &oc_schema::permission::Ruleset) -> bool {
    let matched = rules.iter().rev().find(|rule| wildcard::matches(action, &rule.action));
    matches!(matched, Some(rule) if rule.resource == "*" && rule.effect == oc_schema::permission::Effect::Deny)
}

/// The per-location tool registry (`Tool` node) — Wave 0 carries the empty
/// table plus the `snapshot(permissions)` **skeleton**: wholly-denied tools
/// are dropped from the snapshot exactly like v2 (`tool.ts:225-233`).
/// Definitions sort by name (`BTreeMap`), direct-vs-CodeMode splitting and
/// the execute-tool wrapper land with Wave C. Wave B adds the
/// **execute-handle storage** the session runner's step fibers use
/// (`runner/step.rs` §2).
#[derive(Default)]
pub struct ToolRegistryState {
    tools: Mutex<std::collections::BTreeMap<String, ToolRegistration>>,
    /// Local executors keyed by bare name (Wave C folds these into the full
    /// pipeline; the runner resolves them at tool-call time).
    executors: Mutex<std::collections::BTreeMap<String, ToolExecutorHandle>>,
}

impl fmt::Debug for ToolRegistryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.tools.lock();
        let names: Vec<&str> = guard.keys().map(String::as_str).collect();
        write!(f, "ToolRegistryState({names:?})")
    }
}

impl ToolRegistryState {
    /// Fresh empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace one registration (keyed by name).
    pub fn register(&self, tool: ToolRegistration) {
        self.tools.lock().insert(tool.name.clone(), tool);
    }

    /// Remove one registration.
    pub fn remove(&self, name: &str) {
        self.tools.lock().remove(name);
    }

    /// One registration.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<ToolRegistration> {
        self.tools.lock().get(name).cloned()
    }

    /// Every registration, sorted by name.
    #[must_use]
    pub fn list(&self) -> Vec<ToolRegistration> {
        self.tools.lock().values().cloned().collect()
    }

    /// Number of registrations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.lock().len()
    }

    /// Whether the registry holds no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.lock().is_empty()
    }

    /// `Tool.snapshot(permissions)` skeleton: name-sorted registrations with
    /// wholly-denied tools dropped (`tool.ts:225-233` — a tool is dropped
    /// when its permission action is wholly denied over the ruleset;
    /// `None` behaves like an empty ruleset, keeping everything).
    #[must_use]
    pub fn snapshot(
        &self,
        permissions: Option<&oc_schema::permission::Ruleset>,
    ) -> Vec<ToolRegistration> {
        static EMPTY: oc_schema::permission::Ruleset = Vec::new();
        let rules = permissions.unwrap_or(&EMPTY);
        self.tools
            .lock()
            .values()
            .filter(|tool| !wholly_denied(tool.permission_action(), rules))
            .cloned()
            .collect()
    }

    /// Registers a local executor under a bare name (Wave B runner seam).
    pub fn register_executor(&self, name: impl Into<String>, executor: ToolExecutorHandle) {
        self.executors.lock().insert(name.into(), executor);
    }

    /// Removes a local executor.
    pub fn remove_executor(&self, name: &str) {
        self.executors.lock().remove(name);
    }

    /// The registered local executor for a bare name.
    #[must_use]
    pub fn executor(&self, name: &str) -> Option<ToolExecutorHandle> {
        self.executors.lock().get(name).cloned()
    }
}

/// Per-location shell environment node (`Shell*` in the v2 layer) —
/// placeholder. Wave C (shell tool) and Wave D (PTY) populate env plumbing
/// and process state; Wave 0 only pins the working directory.
#[derive(Debug, Clone)]
pub struct ShellEnv {
    directory: PathBuf,
}

impl ShellEnv {
    /// Environment bound to the location directory.
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// The working directory shell commands resolve against.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Per-location file-access boundary (`FileAccess` node) — wraps
/// [`crate::file_access`] with this location's `directory`. The boundary
/// half ([`FileAccessState::resolve`]) runs without a permission engine;
/// the approval half needs the per-location [`Permission`] engine, bound at
/// tool time via [`FileAccessState::bound`].
pub struct FileAccessState {
    directory: PathBuf,
    /// `location.project.directory` — until project resolution lands in the
    /// graph this is the location directory itself (the v2 boundary for
    /// untracked directories).
    project_directory: PathBuf,
    home: PathBuf,
}

impl fmt::Debug for FileAccessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileAccessState")
            .field("directory", &self.directory)
            .field("project_directory", &self.project_directory)
            .finish_non_exhaustive()
    }
}

impl FileAccessState {
    /// Boundary bound to `directory` (project boundary defaults to it).
    #[must_use]
    pub fn new(directory: PathBuf, project_directory: PathBuf, home: PathBuf) -> Self {
        Self { directory, project_directory, home }
    }

    /// The location directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The project (worktree) boundary directory.
    #[must_use]
    pub fn project_directory(&self) -> &Path {
        &self.project_directory
    }

    /// The full [`crate::file_access::FileAccess`] service, bound to the
    /// per-location permission engine (the tool waves' approval path).
    #[must_use]
    pub fn bound(
        &self,
        permission: crate::permission::Permission,
    ) -> crate::file_access::FileAccess {
        crate::file_access::FileAccess::new(
            permission,
            self.directory.clone(),
            self.project_directory.clone(),
            self.home.clone(),
        )
    }

    /// `FileAccess.resolve` — the boundary half, mirroring
    /// [`crate::file_access::FileAccess::resolve`] exactly (lexical
    /// resolution, internal/external classification, external directory
    /// authorization metadata). Never touches the permission engine.
    #[must_use]
    pub fn resolve(&self, input: &crate::file_access::ResolveInput) -> crate::file_access::Target {
        use crate::file_access::{path_contains, project_root, resolve_path};

        let absolute = resolve_path(&self.directory, &input.path, &self.home);
        let worktree = crate::file_access::lexical_resolve(Path::new("/"), &self.project_directory);
        let fs_root = worktree.parent().is_none();
        let internal = path_contains(&self.directory, &absolute)
            || (!fs_root && path_contains(&worktree, &absolute));
        if internal {
            return crate::file_access::Target {
                resource: crate::file_access::lexical_relative(&self.directory, &absolute),
                absolute,
                external_directory: None,
            };
        }
        let is_dir = match input.kind {
            Some(crate::file_access::Kind::Directory) => true,
            Some(crate::file_access::Kind::File) => false,
            None => std::fs::metadata(&absolute).is_ok_and(|meta| meta.is_dir()),
        };
        let directory = if is_dir {
            absolute.clone()
        } else {
            absolute.parent().map_or_else(|| absolute.clone(), Path::to_path_buf)
        };
        let save_root = project_root(&directory).unwrap_or_else(|| directory.clone());
        let slash = |path: &Path| path.to_string_lossy().replace('\\', "/");
        crate::file_access::Target {
            resource: slash(&absolute),
            external_directory: Some(crate::file_access::ExternalDirectoryAuthorization {
                action: crate::file_access::EXTERNAL_DIRECTORY,
                directory: directory.clone(),
                resource: slash(&directory.join("*")),
                save: slash(&save_root.join("*")),
            }),
            absolute,
        }
    }
}

/// Per-location instruction slots (`InstructionDiscovery` node) —
/// placeholder. Wave C fills discovery + slot naming.
#[derive(Debug, Default)]
pub struct InstructionSlots {
    slots: Mutex<Vec<String>>,
}

impl InstructionSlots {
    /// Fresh empty slot set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the slot list (Wave C discovery writes here).
    pub fn set_slots(&self, slots: Vec<String>) {
        *self.slots.lock() = slots;
    }

    /// Current slots.
    #[must_use]
    pub fn slots(&self) -> Vec<String> {
        self.slots.lock().clone()
    }
}

// =============================================================================
// Location graph
// =============================================================================

/// The per-location service graph: shared global services (by `Arc`) plus
/// location-local state. Constructed fresh per build; never mutated in
/// place.
pub struct LocationGraph {
    /// Canonical key this graph serves.
    pub location: LocationRef,
    // -- global services (Arc-shared across every location graph) ----------
    /// Database handle (the global `Db` node).
    pub db: Arc<Database>,
    /// Event bus (publishes `location.shutdown` etc.).
    pub bus: Arc<Bus>,
    /// Provider catalog (models.dev snapshot + refresh).
    pub catalog: Arc<Catalog>,
    // -- per-location state ------------------------------------------------
    /// This location's plugin hook registry (v2 `PluginHooks`).
    pub hooks: Arc<HookRegistry>,
    /// Agent table + selection semantics (v2 `Agent`).
    pub agents: AgentRegistry,
    /// Tool table + permission-filtered snapshots (v2 `Tool`).
    pub tools: ToolRegistryState,
    /// Shell environment node (placeholder).
    pub shell: ShellEnv,
    /// File-access boundary for this location's directory.
    pub files: FileAccessState,
    /// Instruction discovery slots (placeholder).
    pub instructions: InstructionSlots,
    /// Config hot-reload marker.
    pub config: Arc<dyn ConfigCache>,
    /// Pending-permission handle (closed first on shutdown).
    pub permissions: Arc<dyn PermissionPending>,
    /// Forms placeholder (closed second on shutdown).
    pub forms: Arc<dyn FormsPort>,
    /// RPC port placeholder (closed third on shutdown).
    pub rpc: Arc<dyn RpcPort>,
    /// Generation of the map at build time (diagnostics).
    pub built_at_generation: u64,
    /// Plugin boot failures (contained — the graph booted regardless).
    pub plugin_failures: Vec<PluginFailure>,
}

impl fmt::Debug for LocationGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocationGraph")
            .field("location", &self.location.to_string())
            .field("built_at_generation", &self.built_at_generation)
            .field("plugin_failures", &self.plugin_failures.len())
            .finish_non_exhaustive()
    }
}

/// The plugin-visible view of one graph — what `InternalPlugin::register`
/// receives (the v2 plugin `ctx`, scoped to Wave 0's surfaces).
pub struct LocationPorts<'a> {
    pub location: &'a LocationRef,
    pub db: &'a Arc<Database>,
    pub bus: &'a Arc<Bus>,
    pub catalog: &'a Catalog,
    pub agents: &'a AgentRegistry,
    pub tools: &'a ToolRegistryState,
    pub shell: &'a ShellEnv,
    pub files: &'a FileAccessState,
    pub instructions: &'a InstructionSlots,
}

impl LocationGraph {
    /// Build a fresh graph for `location` from the shared globals (v2
    /// `Instance.layer(ref)` — location-tagged subgraph, fresh per build).
    ///
    /// Fallible (oracle gate 4, finding 1): a missing directory fails with
    /// [`BootError::MissingDirectory`]; a failed database probe fails with
    /// [`BootError::Database`]. On success the plugin supervisor boots
    /// (pre → host placeholder → post) with failure containment — a failed
    /// plugin is reported in [`LocationGraph::plugin_failures`], never
    /// failing the location.
    pub async fn build(
        location: LocationRef,
        globals: &crate::location_services::GlobalServices,
        generation: u64,
    ) -> Result<Arc<Self>, BootError> {
        if !location.directory.is_dir() {
            return Err(BootError::MissingDirectory(location.directory.clone()));
        }
        // Database probe (serialized connection): catches locked/corrupt
        // files before the graph is published to the map.
        globals.db.migration_ids().await?;

        let hooks = Arc::new(HookRegistry::new());
        let agents = AgentRegistry::new();
        let tools = ToolRegistryState::new();
        let shell = ShellEnv::new(location.directory.clone());
        let files = FileAccessState::new(
            location.directory.clone(),
            location.directory.clone(),
            globals.home.clone(),
        );
        let instructions = InstructionSlots::new();

        // Plugin boot: pre → host placeholder → post, borrowing the state
        // pieces before the graph is assembled. Failures contained.
        let ports = LocationPorts {
            location: &location,
            db: &globals.db,
            bus: &globals.bus,
            catalog: &globals.catalog,
            agents: &agents,
            tools: &tools,
            shell: &shell,
            files: &files,
            instructions: &instructions,
        };
        let outcome = globals.plugins.boot(&hooks, &ports);

        Ok(Arc::new(Self {
            location,
            db: Arc::clone(&globals.db),
            bus: Arc::clone(&globals.bus),
            catalog: Arc::clone(&globals.catalog),
            hooks,
            agents,
            tools,
            shell,
            files,
            instructions,
            config: Arc::clone(&globals.config_cache),
            permissions: Arc::clone(&globals.permissions),
            forms: Arc::clone(&globals.forms),
            rpc: Arc::clone(&globals.rpc),
            built_at_generation: generation,
            plugin_failures: outcome.failures,
        }))
    }

    /// The plugin-visible ports of this graph.
    #[must_use]
    pub fn ports(&self) -> LocationPorts<'_> {
        LocationPorts {
            location: &self.location,
            db: &self.db,
            bus: &self.bus,
            catalog: &self.catalog,
            agents: &self.agents,
            tools: &self.tools,
            shell: &self.shell,
            files: &self.files,
            instructions: &self.instructions,
        }
    }

    /// v2 `LocationLifecycle.shutdown` sequence:
    /// permission close → forms close → rpc close → publish
    /// [`LOCATION_SHUTDOWN_EVENT`] (data `{}`, location-stamped —
    /// `location-lifecycle.ts`). Idempotent per graph instance (each step
    /// is delegated to the owning service).
    pub async fn shutdown(&self) {
        self.permissions.close().await;
        self.forms.close().await;
        self.rpc.close().await;
        let _ = self
            .bus
            .publish(
                crate::bus::registry::location_shutdown(),
                serde_json::json!({}),
                crate::bus::PublishOptions {
                    location: Some(self.schema_location()),
                    ..Default::default()
                },
            )
            .await;
    }

    /// The schema-crate location ref for event payloads.
    #[must_use]
    pub fn schema_location(&self) -> oc_schema::location::Ref {
        oc_schema::location::Ref {
            directory: oc_schema::common::AbsolutePath::parse(
                self.location.directory.to_string_lossy().into_owned(),
            ),
            workspace_id: self
                .location
                .workspace_id
                .as_ref()
                .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
        }
    }
}

/// Event type published at the end of a location shutdown.
pub const LOCATION_SHUTDOWN_EVENT: &str = "location.shutdown";

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use oc_schema::agent::{Info as AgentInfo, Mode};
    use oc_schema::id::AgentId;
    use oc_schema::permission::{Effect, Rule};

    fn agent(id: &str, mode: Mode, hidden: bool) -> AgentInfo {
        AgentInfo {
            id: AgentId::parse(id),
            name: oc_schema::id::AgentName::parse(id),
            model: None,
            request: oc_schema::provider::Request::default(),
            system: None,
            description: None,
            mode,
            hidden,
            color: None,
            steps: None,
            permissions: Vec::new(),
        }
    }

    fn rules(entries: &[(&str, &str, Effect)]) -> oc_schema::permission::Ruleset {
        entries
            .iter()
            .map(|(action, resource, effect)| Rule {
                action: (*action).to_string(),
                resource: (*resource).to_string(),
                effect: *effect,
            })
            .collect()
    }

    #[test]
    fn location_ref_normalizes_backslashes() {
        let a = LocationRef::new(PathBuf::from("/repo\\work"), None);
        let b = LocationRef::new(PathBuf::from("/repo/work"), None);
        assert_eq!(a, b);
        // equal keys must hash equal — HashSet roundtrip
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn location_ref_workspace_participates_in_key() {
        let plain = LocationRef::new(PathBuf::from("/repo"), None);
        let workspace = LocationRef::new(PathBuf::from("/repo"), Some("wrk_1".into()));
        assert_ne!(plain, workspace);
        assert_eq!(workspace.to_string(), "/repo#wrk_1");
    }

    #[test]
    fn location_ref_from_schema_ref() {
        let schema = oc_schema::location::Ref {
            directory: oc_schema::common::AbsolutePath::parse("/proj"),
            workspace_id: Some(oc_schema::id::WorkspaceId::parse("wrk_9").unwrap()),
        };
        let key = LocationRef::from(&schema);
        assert_eq!(key.directory, PathBuf::from("/proj"));
        assert_eq!(key.workspace_id.as_deref(), Some("wrk_9"));
    }

    // ------------------------------------------------------------ agents

    #[test]
    fn agents_select_defaults_configured_then_build_then_first() {
        let registry = AgentRegistry::new();
        assert!(registry.select(None).info.is_none());
        assert_eq!(registry.select(None).id.0, "build", "empty table falls back to build");

        registry.set(agent("explore", Mode::Subagent, false));
        assert!(registry.select(None).info.is_none(), "subagents are never selectable");

        registry.set(agent("general", Mode::All, false));
        let selected = registry.select(None);
        assert_eq!(selected.id.0, "general", "first selectable wins without build");

        registry.set(agent("build", Mode::Primary, false));
        assert_eq!(registry.select(None).id.0, "build", "build outranks first selectable");

        registry.set_default(Some(AgentId::parse("custom")));
        registry.set(agent("custom", Mode::Primary, false));
        assert_eq!(registry.select(None).id.0, "custom", "configured default wins");

        // Hidden agents are not selectable.
        registry.update(&AgentId::parse("custom"), |info| info.hidden = true).unwrap();
        assert_eq!(registry.select(None).id.0, "build");
    }

    #[test]
    fn agents_select_explicit_id_resolves_verbatim() {
        let registry = AgentRegistry::new();
        registry.set(agent("build", Mode::Primary, false));
        registry.set(agent("explore", Mode::Subagent, false));
        let selection = registry.select(Some("explore"));
        assert_eq!(selection.id.0, "explore");
        assert!(selection.info.is_some(), "explicit ids resolve even for subagents");
        let unknown = registry.select(Some("nope"));
        assert_eq!(unknown.id.0, "nope");
        assert!(unknown.info.is_none(), "unknown explicit id keeps the id, no info");
    }

    #[test]
    fn agents_list_is_selected_first() {
        let registry = AgentRegistry::new();
        registry.set(agent("build", Mode::Primary, false));
        registry.set(agent("aaa", Mode::All, false));
        registry.set(agent("zzz", Mode::Subagent, false));
        let ids: Vec<String> = registry.list().into_iter().map(|info| info.id.0).collect();
        assert_eq!(ids, vec!["build".to_string(), "aaa".to_string(), "zzz".to_string()]);
    }

    // ------------------------------------------------------------- tools

    #[test]
    fn tools_snapshot_drops_wholly_denied() {
        let state = ToolRegistryState::new();
        state.register(ToolRegistration::new("bash", "shell", serde_json::json!({})));
        state.register(ToolRegistration::new("read", "read", serde_json::json!({})));
        let mut webfetch = ToolRegistration::new("fetch", "webfetch", serde_json::json!({}));
        webfetch.permission = Some("webfetch".into());
        state.register(webfetch);

        // No ruleset: everything visible.
        assert_eq!(state.snapshot(None).len(), 3);

        // Last matching rule with resource "*" deny drops the tool.
        let ruleset = rules(&[("bash", "*", Effect::Deny)]);
        let names: Vec<String> =
            state.snapshot(Some(&ruleset)).into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, vec!["fetch".to_string(), "read".to_string()]);

        // A later allow over the same action rescues it (last match wins).
        let ruleset = rules(&[("bash", "*", Effect::Deny), ("bash *", "*", Effect::Allow)]);
        assert_eq!(state.snapshot(Some(&ruleset)).len(), 3);

        // Non-star resources never wholly deny.
        let ruleset = rules(&[("bash", "/tmp/*", Effect::Deny)]);
        assert_eq!(state.snapshot(Some(&ruleset)).len(), 3);

        // Permission action override routes the check.
        let ruleset = rules(&[("webfetch", "*", Effect::Deny)]);
        assert_eq!(state.snapshot(Some(&ruleset)).len(), 2);

        // Direct (codemode=false) registrations stay in the flat skeleton.
        let mut direct = ToolRegistration::new("direct", "direct", serde_json::json!({}));
        direct.codemode = false;
        state.register(direct);
        assert_eq!(state.snapshot(None).len(), 4);
    }

    #[test]
    fn tools_wholly_denied_matches_action_wildcards() {
        // Action pattern "bash *" matches "bash rm" — denied.
        assert!(wholly_denied("bash rm", &rules(&[("bash *", "*", Effect::Deny)])));
        // Bare "bash" matches "bash *" under v2 wildcard semantics.
        assert!(wholly_denied("bash", &rules(&[("bash *", "*", Effect::Deny)])));
        // Deny with a narrower resource is not wholesale.
        assert!(!wholly_denied("bash", &rules(&[("bash", "rm *", Effect::Deny)])));
        // Allow-only rulesets never deny.
        assert!(!wholly_denied("bash", &rules(&[("*", "*", Effect::Allow)])));
    }

    // -------------------------------------------------------- file access

    #[test]
    fn file_access_state_resolves_boundary_like_file_access() {
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let project = tmp.path().join("proj");
        let work = project.join("work");
        let other = tmp.path().join("other");
        std::fs::create_dir_all(work.join("sub")).unwrap();
        std::fs::create_dir_all(project.join("lib")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir(project.join(".git")).unwrap();
        std::fs::create_dir(other.join(".git")).unwrap();
        std::fs::write(work.join("sub").join("f.txt"), "x").unwrap();

        let state = FileAccessState::new(work.clone(), project.clone(), tmp.path().to_path_buf());
        let internal = state
            .resolve(&crate::file_access::ResolveInput { path: "sub/f.txt".into(), kind: None });
        assert_eq!(internal.resource, "sub/f.txt");
        assert!(internal.external_directory.is_none());

        // Inside the project but outside the location directory: internal
        // via the project boundary, resource relative to the location.
        let sibling = state
            .resolve(&crate::file_access::ResolveInput { path: "../lib/x.rs".into(), kind: None });
        assert_eq!(sibling.resource, "../lib/x.rs");
        assert!(sibling.external_directory.is_none());

        // Outside the project tree entirely: external, save anchored at the
        // target's own project root (the `other` checkout).
        let external = state.resolve(&crate::file_access::ResolveInput {
            path: "../../other/x.rs".into(),
            kind: None,
        });
        let auth = external.external_directory.expect("external");
        assert_eq!(auth.resource, format!("{}/*", other.to_string_lossy()));
        assert_eq!(
            auth.save,
            format!("{}/*", other.to_string_lossy()),
            "save anchored at .git root"
        );
    }
}
