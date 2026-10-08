//! Location-scoped service graph — the per-(directory, workspace) slice of
//! opencode's service DAG (port of the v2 `Instance.layer(ref)` 55-node
//! location layer, per core-deep-map §5).
//!
//! # Rust shape (as prescribed by core-deep-map §5)
//!
//! - [`LocationRef`] is the canonical map key: `{directory, workspace_id?}`
//!   with forward-slash normalization.
//! - [`crate::location_services::LocationServiceMap`] is an
//!   `Arc<DashMap<LocationRef, Arc<LocationGraph>>>`.
//! - [`LocationGraph`] holds `Arc` references to the **global** services
//!   (database port, event-bus sink, project store, provider catalog) plus
//!   the **per-location** state (a [`crate::hooks::HookRegistry`] instance,
//!   config cache handle, permission pending handle).
//! - **Old-graph survival**: borrows hold an `Arc<LocationGraph>` clone;
//!   invalidation only removes the `DashMap` entry. The graph is **never
//!   mutated in place** — reload builds a NEW graph and swaps the entry, so
//!   running steps keep borrowing the old graph until they finish (v2:
//!   "running steps keep borrowing old graph until boundary").
//! - Invalidation runs the v2 shutdown sequence: permission close → forms
//!   close (placeholder) → rpc close (placeholder) → publish
//!   `location.shutdown`.
//!
//! The global-service and per-location traits below are the *minimal* seams
//! this lane needs; the sibling lanes (database, config, permission,
//! bus/event) plug their concrete types in — `TODO(lane-integration)`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::hooks::{EventEnvelope, HookRegistry, RpcPort};
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
// Global-service ports (minimal seams this lane needs)
// =============================================================================
// TODO(lane-integration): the database/bus/config/permission lanes provide
// concrete implementations of these traits on the global graph.

/// Database access port (the global `Db` node). Minimal surface for graph
/// wiring; the session/database lanes own the real API.
///
/// `TODO(lane-integration)`.
#[async_trait]
pub trait DatabasePort: Send + Sync {
    /// Liveness probe used by old-graph-survival checks and tests.
    async fn healthy(&self) -> bool;
}

/// Event-bus sink (the global `Bus` node, publish side). Publishing an
/// event routes it through location scoping — implemented by the bus lane.
///
/// `TODO(lane-integration)`.
#[async_trait]
pub trait EventBusSink: Send + Sync {
    /// Publish one event (ephemeral unless the type is durable; the bus lane
    /// owns durability).
    async fn publish(&self, event: EventEnvelope);
}

/// Project identity for a directory (the global `Project` node's lookup).
///
/// `TODO(lane-integration)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIdentity {
    pub id: String,
    pub canonical: PathBuf,
}

/// Project store port.
///
/// `TODO(lane-integration)`.
#[async_trait]
pub trait ProjectStore: Send + Sync {
    /// Resolve the project a directory belongs to (`None` before discovery).
    async fn project_for(&self, directory: &Path) -> Option<ProjectIdentity>;
}

// =============================================================================
// Per-location ports
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
// Location graph
// =============================================================================

/// The per-location service graph: shared global services (by `Arc`) plus
/// location-local state. Constructed fresh per build; never mutated in
/// place.
pub struct LocationGraph {
    /// Canonical key this graph serves.
    pub location: LocationRef,
    // -- global services (Arc-shared across every location graph) ----------
    /// Database port.
    pub db: Arc<dyn DatabasePort>,
    /// Event-bus sink (publishes `location.shutdown` etc.).
    pub events: Arc<dyn EventBusSink>,
    /// Project store.
    pub projects: Arc<dyn ProjectStore>,
    /// Provider catalog (models.dev snapshot + refresh).
    pub catalog: Arc<Catalog>,
    // -- per-location state ------------------------------------------------
    /// This location's plugin hook registry (v2 `PluginHooks`).
    pub hooks: Arc<HookRegistry>,
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
}

impl fmt::Debug for LocationGraph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocationGraph")
            .field("location", &self.location.to_string())
            .field("built_at_generation", &self.built_at_generation)
            .finish_non_exhaustive()
    }
}

impl LocationGraph {
    /// Build a fresh graph for `location` from the shared globals (v2
    /// `Instance.layer(ref)` — location-tagged subgraph, fresh per build).
    #[must_use]
    pub fn build(
        location: LocationRef,
        globals: &crate::location_services::GlobalServices,
        generation: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            location,
            db: Arc::clone(&globals.db),
            events: Arc::clone(&globals.events),
            projects: Arc::clone(&globals.projects),
            catalog: Arc::clone(&globals.catalog),
            hooks: Arc::new(HookRegistry::new()),
            config: Arc::clone(&globals.config_cache),
            permissions: Arc::clone(&globals.permissions),
            forms: Arc::clone(&globals.forms),
            rpc: Arc::clone(&globals.rpc),
            built_at_generation: generation,
        })
    }

    /// v2 `LocationLifecycle.shutdown` sequence:
    /// permission close → forms close → rpc close → publish
    /// [`LOCATION_SHUTDOWN_EVENT`]. Idempotent per graph instance (each step
    /// is delegated to the owning service).
    pub async fn shutdown(&self) {
        self.permissions.close().await;
        self.forms.close().await;
        self.rpc.close().await;
        self.events
            .publish(EventEnvelope {
                id: format!("location-shutdown:{}", self.location),
                created: 0,
                r#type: LOCATION_SHUTDOWN_EVENT.into(),
                location: Some(self.schema_location()),
                data: serde_json::json!({"directory": self.location.directory}),
            })
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
}
