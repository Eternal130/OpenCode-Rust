//! The location service map + activity TTL sweeper (port of v2
//! `LocationServiceMap` / `LocationActivity`, core-deep-map §5).
//!
//! - [`GlobalServices`] carries the process-wide services every location
//!   graph shares (global subgraph constructed once per process) — the
//!   **concrete** [`Arc<Database>`](crate::database::Database) +
//!   [`Arc<Bus>`](crate::bus::Bus) + [`Arc<Catalog>`](crate::provider_catalog::Catalog)
//!   handles plus the per-location trait seams (graph v2, oracle gate 4
//!   finding 1).
//! - [`LocationServiceMap`] is `DashMap<LocationRef, Arc<LocationGraph>>`
//!   with build / invalidate / reload semantics. Invalidation never mutates
//!   a live graph: it removes the entry and runs the shutdown sequence;
//!   a **new** graph is built on the next `get_or_build` (or immediately by
//!   [`LocationServiceMap::reload`]). Borrowers keep their `Arc` clone —
//!   old-graph survival.
//! - **Boot failure evicts + invalidates (retry possible)**:
//!   [`LocationServiceMap::get_or_build`] is fallible
//!   ([`BootError`]); a failed build never publishes a graph, so the next
//!   call retries.
//! - [`LocationActivity`] + [`spawn_activity_sweeper`] implement the v2
//!   activity TTL: default 60-minute idle expiry, 1-minute sweep, touched
//!   on session activity; expiry interrupts active executions with reason
//!   `"inactivity"` and then invalidates the location.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use oc_util::determinism::Clock;

use crate::bus::Bus;
use crate::database::Database;
use crate::hooks::{PluginSupervisor, RpcPort};
use crate::location::{
    BootError, ConfigCache, FormsPort, LocationGraph, LocationRef, PermissionPending,
    SessionInterruptor,
};
use crate::project::ProjectStore;
use crate::provider_catalog::Catalog;

/// Default idle TTL for an inactive location (v2: 60 minutes).
pub const LOCATION_TTL_DEFAULT: Duration = Duration::from_secs(3600);

/// Default sweep interval of the activity sweeper (v2: 1 minute).
pub const LOCATION_SWEEP_DEFAULT: Duration = Duration::from_secs(60);

/// Reason recorded when the TTL sweeper interrupts executions.
pub const INACTIVITY_REASON: &str = "inactivity";

// =============================================================================
// Global services
// =============================================================================

/// Process-wide services shared (by `Arc`) with every [`LocationGraph`].
///
/// The database/bus/catalog handles are the **concrete** types (graph v2:
/// the trait seams bought nothing — every consumer lives in this crate);
/// the per-location trait fields remain the seams their owning lanes fill
/// (`TODO(lane-integration)`).
pub struct GlobalServices {
    /// Database (global `Db` node), shared with the bus.
    pub db: Arc<Database>,
    /// Event bus (global `Bus` node).
    pub bus: Arc<Bus>,
    /// Project store (global `Project` node; sqlite-backed in production).
    pub projects: Arc<dyn ProjectStore>,
    /// Provider catalog (global `ModelsDev` node).
    pub catalog: Arc<Catalog>,
    /// Wall clock (injectable for tests; oc-util `Clock`).
    pub clock: Arc<dyn Clock>,
    /// Home directory (`~` expansion for the per-location file boundary).
    pub home: PathBuf,
    /// Session interruptor (activity-expiry interrupt + await settlement).
    pub interruptor: Arc<dyn SessionInterruptor>,
    /// Config hot-reload handles, resolved per location by the config lane.
    pub config_cache: Arc<dyn ConfigCache>,
    /// Pending-permission handles, resolved per location by the permission
    /// lane.
    pub permissions: Arc<dyn PermissionPending>,
    /// Forms close placeholder.
    pub forms: Arc<dyn FormsPort>,
    /// RPC port placeholder.
    pub rpc: Arc<dyn RpcPort>,
    /// Location activity tracker (feeds the TTL sweeper).
    pub activity: Arc<LocationActivity>,
    /// Internal plugin slots (boot order pre → host → post).
    pub plugins: PluginSupervisor,
}

impl GlobalServices {
    /// Assemble globals. The database handle is cloned into the bus by the
    /// caller (the bus owns its connection share); per-location trait
    /// handles are supplied once here — concrete lanes may swap in
    /// per-location dispatch internally.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<Database>,
        bus: Arc<Bus>,
        projects: Arc<dyn ProjectStore>,
        catalog: Arc<Catalog>,
        clock: Arc<dyn Clock>,
        home: PathBuf,
        interruptor: Arc<dyn SessionInterruptor>,
        config_cache: Arc<dyn ConfigCache>,
        permissions: Arc<dyn PermissionPending>,
        forms: Arc<dyn FormsPort>,
        rpc: Arc<dyn RpcPort>,
        plugins: PluginSupervisor,
    ) -> Self {
        let activity = Arc::new(LocationActivity::new(Arc::clone(&clock)));
        Self {
            db,
            bus,
            projects,
            catalog,
            clock,
            home,
            interruptor,
            config_cache,
            permissions,
            forms,
            rpc,
            activity,
            plugins,
        }
    }
}

// =============================================================================
// Location activity
// =============================================================================

/// Last-touch timestamps per location (v2 `LocationActivity`). Session
/// events call [`LocationActivity::touch`]; the sweeper expires entries
/// older than the TTL.
///
/// `TODO(lane-integration)`: the session runner lane touches on session
/// activity events.
pub struct LocationActivity {
    last_touch: DashMap<LocationRef, i64>,
    clock: Arc<dyn Clock>,
}

impl LocationActivity {
    /// Tracker reading `now` from `clock`.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { last_touch: DashMap::new(), clock }
    }

    /// Record activity for a location (v2 "session events touch").
    pub fn touch(&self, location: &LocationRef) {
        let now = self.clock.unix_ms();
        self.last_touch.insert(location.clone(), now);
    }

    /// Seed a location's last-touch to an explicit timestamp (restores /
    /// tests).
    pub fn touch_at(&self, location: &LocationRef, unix_ms: i64) {
        self.last_touch.insert(location.clone(), unix_ms);
    }

    /// Last touch time, if tracked.
    #[must_use]
    pub fn last_touch(&self, location: &LocationRef) -> Option<i64> {
        self.last_touch.get(location).map(|entry| *entry)
    }

    /// Locations idle for longer than `ttl_ms` (measured at `now_ms`).
    #[must_use]
    pub fn expired(&self, now_ms: i64, ttl_ms: i64) -> Vec<LocationRef> {
        self.last_touch
            .iter()
            .filter(|entry| now_ms.saturating_sub(*entry.value()) > ttl_ms)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Drop tracking for a location (after invalidation).
    pub fn remove(&self, location: &LocationRef) {
        self.last_touch.remove(location);
    }

    /// Number of tracked locations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.last_touch.len()
    }

    /// Whether nothing is tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.last_touch.is_empty()
    }
}

// =============================================================================
// Location service map
// =============================================================================

/// `Arc<DashMap<LocationRef, Arc<LocationGraph>>>` with the v2
/// build/invalidate/reload semantics:
///
/// - **idleTimeToLive infinity** — only explicit invalidation evicts
///   (the TTL sweeper is the one caller doing time-based eviction);
/// - **boot failure evicts + invalidates** (retry possible) —
///   [`LocationServiceMap::get_or_build`] returns [`BootError`] and never
///   publishes a half-built graph;
/// - **invalidate = remove → shutdown sequence** with old-graph survival for
///   running borrows (see [`LocationGraph::shutdown`]);
/// - **reload = invalidate + build a NEW graph** and swap the entry — the
///   graph is never mutated in place.
pub struct LocationServiceMap {
    map: DashMap<LocationRef, Arc<LocationGraph>>,
    globals: Arc<GlobalServices>,
    generation: AtomicU64,
}

impl LocationServiceMap {
    /// Empty map over shared globals.
    #[must_use]
    pub fn new(globals: Arc<GlobalServices>) -> Self {
        Self { map: DashMap::new(), globals, generation: AtomicU64::new(1) }
    }

    /// The shared globals.
    #[must_use]
    pub fn globals(&self) -> &Arc<GlobalServices> {
        &self.globals
    }

    /// Current build generation (increments per graph build).
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Existing graph for a location, if built.
    #[must_use]
    pub fn get(&self, location: &LocationRef) -> Option<Arc<LocationGraph>> {
        self.map.get(location).map(|entry| Arc::clone(entry.value()))
    }

    /// Get or build the graph for a location. Repeated calls return the
    /// same graph (`Arc::ptr_eq`) until invalidated. Building is fallible
    /// (missing directory / database probe — v2 "boot failure evicts +
    /// invalidates, retry possible"); a failed boot leaves the map
    /// untouched so the next call retries.
    pub async fn get_or_build(
        &self,
        location: LocationRef,
    ) -> Result<Arc<LocationGraph>, BootError> {
        if let Some(existing) = self.get(&location) {
            return Ok(existing);
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst);
        let graph = LocationGraph::build(location.clone(), &self.globals, generation).await?;
        match self.map.entry(location) {
            dashmap::mapref::entry::Entry::Occupied(existing) => {
                // raced with another builder — the existing graph wins
                Ok(Arc::clone(existing.get()))
            }
            dashmap::mapref::entry::Entry::Vacant(slot) => {
                slot.insert(Arc::clone(&graph));
                Ok(graph)
            }
        }
    }

    /// Invalidate a location: remove the entry, then run the shutdown
    /// sequence (permission close → forms close → rpc close → publish
    /// `location.shutdown`). Returns the old graph — its `Arc` survives in
    /// any outstanding borrows, which may keep using it.
    pub async fn invalidate(&self, location: &LocationRef) -> Option<Arc<LocationGraph>> {
        let (_, graph) = self.map.remove(location)?;
        graph.shutdown().await;
        Some(graph)
    }

    /// Invalidate + build a fresh graph and swap it in (config/plugin
    /// hot path). Returns the new graph; never mutates the old graph. A
    /// failed rebuild leaves the location absent (retry on the next
    /// `get_or_build`).
    pub async fn reload(&self, location: LocationRef) -> Result<Arc<LocationGraph>, BootError> {
        self.invalidate(&location).await;
        self.get_or_build(location).await
    }

    /// Reload every live location (`LocationServiceMap.reload` —
    /// invalidate, then rebuild all). Every location is attempted; failed
    /// rebuilds are reported, and those locations stay absent until the
    /// next `get_or_build` (the v2 boot-failure semantics).
    pub async fn reload_all(&self) -> Vec<(LocationRef, BootError)> {
        let locations: Vec<LocationRef> =
            self.map.iter().map(|entry| entry.key().clone()).collect();
        let mut failures = Vec::new();
        for location in locations {
            if let Err(error) = self.reload(location.clone()).await {
                failures.push((location, error));
            }
        }
        failures
    }

    /// Number of live graphs.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether no graphs are live.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Live locations (canonical keys).
    pub fn locations(&self) -> Vec<LocationRef> {
        self.map.iter().map(|entry| entry.key().clone()).collect()
    }
}

// =============================================================================
// Activity TTL sweeper
// =============================================================================

/// Knobs for the TTL sweeper (defaults match v2: 60-minute TTL, 1-minute
/// sweep).
#[derive(Debug, Clone, Copy)]
pub struct SweeperConfig {
    /// Idle time before a location expires.
    pub ttl: Duration,
    /// Time between sweep passes.
    pub sweep_interval: Duration,
}

impl Default for SweeperConfig {
    fn default() -> Self {
        Self { ttl: LOCATION_TTL_DEFAULT, sweep_interval: LOCATION_SWEEP_DEFAULT }
    }
}

/// Spawn the activity TTL sweeper task. Each pass:
///
/// 1. read `now` from the injectable clock,
/// 2. for every location idle longer than the TTL: interrupt active
///    executions (reason `"inactivity"`, awaiting settlement) then
///    invalidate the location (shutdown sequence),
/// 3. drop its activity entry.
///
/// A `ttl` of zero disables expiry (nothing is ever older than `now`).
///
/// The handle is abort-able; the task holds no other resources.
pub fn spawn_activity_sweeper(
    config: SweeperConfig,
    map: Arc<LocationServiceMap>,
    activity: Arc<LocationActivity>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let ttl_ms = i64::try_from(config.ttl.as_millis()).unwrap_or(i64::MAX);
        loop {
            tokio::time::sleep(config.sweep_interval).await;
            let now = map.globals().clock.unix_ms();
            for location in activity.expired(now, ttl_ms) {
                map.globals().interruptor.interrupt(&location, INACTIVITY_REASON).await;
                map.invalidate(&location).await;
                activity.remove(&location);
            }
        }
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    // The `assert!(x.is_empty())` spelling reads better than the lint's
    // suggested `assert_eq!(x, [])` form.
    #![allow(clippy::assert_is_empty)]

    use super::*;
    use crate::hooks::PluginSupervisor;
    use crate::location::LOCATION_SHUTDOWN_EVENT;
    use async_trait::async_trait;
    use futures::StreamExt;
    use oc_schema::prompt::Prompt;
    use oc_util::determinism::Clock;
    use parking_lot::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    // -- fakes ---------------------------------------------------------------

    /// Mutable wall clock for deterministic sweeper tests (oc-util
    /// `FixedClock` cannot advance; gate-2 finding 10 notes the gap — this
    /// fills it for oc-core tests).
    struct MutableClock(Mutex<SystemTime>);

    #[allow(
        clippy::cast_sign_loss,
        clippy::unused_async,
        reason = "test clock ms stays positive; async-trait fakes must stay async"
    )]
    impl MutableClock {
        fn at(unix_ms: i64) -> Arc<Self> {
            Arc::new(Self(Mutex::new(
                UNIX_EPOCH + Duration::from_millis(u64::try_from(unix_ms.max(0)).unwrap_or(0)),
            )))
        }
        fn advance(&self, ms: i64) {
            let mut guard = self.0.lock();
            *guard = if ms >= 0 {
                *guard + Duration::from_millis(ms as u64)
            } else {
                *guard - Duration::from_millis((-ms) as u64)
            };
        }
    }

    impl Clock for MutableClock {
        fn now(&self) -> SystemTime {
            *self.0.lock()
        }
    }

    struct RecordingPermission(Mutex<Vec<&'static str>>);

    #[async_trait]
    impl PermissionPending for RecordingPermission {
        #[allow(clippy::unused_async, reason = "test fake")]
        async fn close(&self) -> Vec<String> {
            self.0.lock().push("permission.close");
            vec!["per_1".into()]
        }
        #[allow(clippy::unused_async, reason = "test fake")]
        async fn pending_count(&self) -> usize {
            0
        }
    }

    struct RecordingForms(Mutex<Vec<&'static str>>);

    #[async_trait]
    impl FormsPort for RecordingForms {
        #[allow(clippy::unused_async, reason = "test fake")]
        async fn close(&self) -> Vec<String> {
            self.0.lock().push("forms.close");
            vec![]
        }
    }

    struct RecordingRpc(Mutex<Vec<&'static str>>);

    #[async_trait]
    impl RpcPort for RecordingRpc {
        #[allow(clippy::unused_async, clippy::unused_async_trait_impl, reason = "test fake")]
        async fn register(
            &self,
            _definition: crate::hooks::RpcDefinition,
            _handler: Arc<dyn crate::hooks::RpcHandler>,
        ) -> Result<crate::hooks::Registration, crate::hooks::RpcError> {
            Err(crate::hooks::RpcError("placeholder".into()))
        }
        #[allow(clippy::unused_async, clippy::unused_async_trait_impl, reason = "test fake")]
        async fn emit(
            &self,
            _rpc_id: &str,
            _event: &str,
            _data: serde_json::Value,
        ) -> Result<(), crate::hooks::RpcError> {
            Ok(())
        }
        #[allow(clippy::unused_async, clippy::unused_async_trait_impl, reason = "test fake")]
        async fn close(&self) {
            self.0.lock().push("rpc.close");
        }
    }

    struct RecordingInterruptor(Mutex<Vec<(String, String)>>);

    #[async_trait]
    impl SessionInterruptor for RecordingInterruptor {
        #[allow(clippy::unused_async, reason = "test fake")]
        async fn interrupt(&self, location: &LocationRef, reason: &str) {
            self.0.lock().push((location.to_string(), reason.to_string()));
        }
    }

    struct FixedConfig(u64);
    impl ConfigCache for FixedConfig {
        fn generation(&self) -> u64 {
            self.0
        }
    }

    struct Harness {
        map: Arc<LocationServiceMap>,
        bus: Arc<Bus>,
        permissions: Arc<RecordingPermission>,
        forms: Arc<RecordingForms>,
        rpc: Arc<RecordingRpc>,
        interruptor: Arc<RecordingInterruptor>,
        dir: tempfile::TempDir,
    }

    async fn harness(clock: &Arc<MutableClock>) -> Harness {
        let permissions = Arc::new(RecordingPermission(Mutex::new(Vec::new())));
        let forms = Arc::new(RecordingForms(Mutex::new(Vec::new())));
        let rpc = Arc::new(RecordingRpc(Mutex::new(Vec::new())));
        let interruptor = Arc::new(RecordingInterruptor(Mutex::new(Vec::new())));
        let db = Arc::new(Database::open_in_memory().await.expect("database"));
        let bus = Arc::new(Bus::new(
            (*db).clone(),
            crate::bus::BusOptions {
                clock: Arc::clone(clock) as Arc<dyn Clock>,
                ..crate::bus::BusOptions::deterministic()
            },
        ));
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("temp dir under /tmp/opencode");
        let globals = GlobalServices::new(
            db,
            bus.clone(),
            Arc::new(crate::project::MemoryProjectStore::new()),
            Arc::new(crate::provider_catalog::Catalog::empty()),
            Arc::clone(clock) as Arc<dyn Clock>,
            dir.path().join("home"),
            Arc::clone(&interruptor) as Arc<dyn SessionInterruptor>,
            Arc::new(FixedConfig(1)),
            Arc::clone(&permissions) as Arc<dyn PermissionPending>,
            Arc::clone(&forms) as Arc<dyn FormsPort>,
            Arc::clone(&rpc) as Arc<dyn RpcPort>,
            PluginSupervisor::new(),
        );
        Harness {
            map: Arc::new(LocationServiceMap::new(Arc::new(globals))),
            bus,
            permissions,
            forms,
            rpc,
            interruptor,
            dir,
        }
    }

    fn loc_under(dir: &std::path::Path, name: &str) -> LocationRef {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).expect("location dir");
        LocationRef::new(path, None)
    }

    /// Reads every buffered event off a subscription created BEFORE the
    /// action under test (mpsc subscriptions see only future events).
    async fn drain(subscription: &mut crate::bus::Subscription) -> Vec<String> {
        let mut types = Vec::new();
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(50), subscription.next()).await
        {
            types.push(event.type_);
        }
        types
    }

    #[tokio::test]
    async fn location_map_build_invalidate_shutdown_sequence() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let mut live = harness.bus.subscribe();
        let location = loc_under(harness.dir.path(), "repo");
        let graph = harness.map.get_or_build(location.clone()).await.expect("boot");
        assert!(Arc::ptr_eq(&graph, &harness.map.get_or_build(location.clone()).await.unwrap()));
        assert_eq!(harness.map.len(), 1);
        assert!(graph.plugin_failures.is_empty());

        let old = harness.map.invalidate(&location).await.expect("graph was live");
        assert!(Arc::ptr_eq(&old, &graph));
        assert!(harness.map.get(&location).is_none());
        assert_eq!(
            *harness.permissions.0.lock(),
            vec!["permission.close"],
            "permission closes first"
        );
        assert_eq!(*harness.forms.0.lock(), vec!["forms.close"], "forms close second");
        assert_eq!(*harness.rpc.0.lock(), vec!["rpc.close"], "rpc closes third");
        assert_eq!(
            drain(&mut live).await,
            vec![LOCATION_SHUTDOWN_EVENT.to_string()],
            "shutdown publishes location.shutdown (data {{}}, via the bus)"
        );
    }

    #[tokio::test]
    async fn location_map_missing_directory_is_typed_boot_error() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let location = LocationRef::new(harness.dir.path().join("missing"), None);
        let error = harness
            .map
            .get_or_build(location.clone())
            .await
            .expect_err("missing directory fails the boot");
        assert!(matches!(error, BootError::MissingDirectory(_)));
        assert!(harness.map.get(&location).is_none(), "failed boot never publishes");
        // Retry possible once the directory appears.
        std::fs::create_dir_all(harness.dir.path().join("missing")).unwrap();
        assert!(harness.map.get_or_build(location).await.is_ok());
    }

    #[tokio::test]
    async fn location_old_graph_survives_invalidation() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let location = loc_under(harness.dir.path(), "repo");
        let graph = harness.map.get_or_build(location.clone()).await.expect("boot");

        // a running step borrows the graph (Arc clone) across invalidation
        let borrow = Arc::clone(&graph);
        let step = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            // still fully usable after the map entry is gone
            borrow.db.migration_ids().await.expect("database usable on old graph");
            let event = crate::hooks::SessionPromptEvent {
                session_id: oc_schema::id::SessionId::new(),
                message_id: oc_schema::id::MessageId::new(),
                prompt: Prompt::default(),
                metadata: None,
                delivery: oc_schema::session_inbox::Delivery::Queue,
            };
            borrow.hooks.dispatch_session_prompt(event).await.is_ok()
        });

        harness.map.invalidate(&location).await;
        assert!(harness.map.get(&location).is_none());
        assert!(step.await.expect("borrowed step finishes"), "old graph stays usable");
    }

    #[tokio::test]
    async fn location_reload_swaps_in_new_graph() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let mut live = harness.bus.subscribe();
        let location = loc_under(harness.dir.path(), "repo");
        let first = harness.map.get_or_build(location.clone()).await.expect("boot");
        let second = harness.map.reload(location.clone()).await.expect("rebuild");
        assert!(!Arc::ptr_eq(&first, &second), "reload builds a NEW graph");
        assert!(Arc::ptr_eq(&second, &harness.map.get(&location).unwrap()));
        assert!(second.built_at_generation > first.built_at_generation);
        // reload published exactly one shutdown for the old graph
        assert_eq!(drain(&mut live).await, vec![LOCATION_SHUTDOWN_EVENT.to_string()]);
    }

    #[tokio::test]
    async fn location_reload_all_rebuilds_every_location() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let mut live = harness.bus.subscribe();
        let a = loc_under(harness.dir.path(), "a");
        let b = loc_under(harness.dir.path(), "b");
        let first_a = harness.map.get_or_build(a.clone()).await.expect("boot");
        let first_b = harness.map.get_or_build(b.clone()).await.expect("boot");
        let failures = harness.map.reload_all().await;
        assert!(failures.is_empty(), "{failures:?}");
        let next_a = harness.map.get(&a).unwrap();
        let next_b = harness.map.get(&b).unwrap();
        assert!(!Arc::ptr_eq(&first_a, &next_a));
        assert!(!Arc::ptr_eq(&first_b, &next_b));
        assert_eq!(drain(&mut live).await, vec![LOCATION_SHUTDOWN_EVENT.to_string(); 2]);
    }

    #[tokio::test]
    async fn location_ttl_sweeper_expires_idle_locations() {
        let clock = MutableClock::at(10_000);
        let harness = harness(&clock).await;
        let mut live = harness.bus.subscribe();
        let location = loc_under(harness.dir.path(), "idle");
        let _ = harness.map.get_or_build(location.clone()).await.expect("boot");
        harness.map.globals().activity.touch(&location);

        let sweeper = spawn_activity_sweeper(
            SweeperConfig {
                ttl: Duration::from_millis(100),
                sweep_interval: Duration::from_millis(5),
            },
            Arc::clone(&harness.map),
            Arc::clone(&harness.map.globals().activity),
        );

        // nothing expired yet
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(harness.map.len(), 1);

        // advance past the TTL — sweeper must interrupt + invalidate
        clock.advance(500);
        let deadline = tokio::time::timeout(Duration::from_secs(2), async {
            while !harness.map.is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(deadline.is_ok(), "location expired after TTL");
        sweeper.abort();

        let interrupts = harness.interruptor.0.lock().clone();
        assert_eq!(
            interrupts,
            vec![(location.directory.to_string_lossy().into_owned(), "inactivity".to_string())],
            "expiry interrupts active executions with reason inactivity"
        );
        assert_eq!(drain(&mut live).await, vec![LOCATION_SHUTDOWN_EVENT.to_string()]);
        assert!(harness.map.globals().activity.is_empty(), "activity entry dropped");
    }

    #[tokio::test]
    async fn location_ttl_sweeper_spares_touched_locations() {
        let clock = MutableClock::at(10_000);
        let harness = harness(&clock).await;
        let location = loc_under(harness.dir.path(), "busy");
        let _ = harness.map.get_or_build(location.clone()).await.expect("boot");

        let activity = Arc::clone(&harness.map.globals().activity);
        let sweeper = spawn_activity_sweeper(
            SweeperConfig {
                ttl: Duration::from_millis(60),
                sweep_interval: Duration::from_millis(5),
            },
            Arc::clone(&harness.map),
            Arc::clone(&activity),
        );

        // keep touching while the clock advances past the TTL
        for _ in 0..6 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            clock.advance(20);
            activity.touch(&location);
        }
        assert_eq!(harness.map.len(), 1, "active location survives");
        sweeper.abort();
    }

    #[tokio::test]
    async fn location_graph_carries_concrete_handles_and_services() {
        let harness = harness(&MutableClock::at(1_000)).await;
        let location = loc_under(harness.dir.path(), "repo");
        let graph = harness.map.get_or_build(location.clone()).await.expect("boot");
        // Concrete global handles (graph v2).
        assert!(Arc::ptr_eq(&graph.db, &harness.map.globals().db));
        assert!(Arc::ptr_eq(&graph.bus, &harness.map.globals().bus));
        assert_eq!(graph.location, location);
        // Per-location service nodes are live and empty at boot.
        assert!(graph.agents.is_empty());
        assert!(graph.tools.is_empty());
        assert_eq!(graph.shell.directory(), location.as_path());
        assert_eq!(graph.files.directory(), location.as_path());
        assert!(graph.instructions.slots().is_empty());
        assert_eq!(graph.hooks.total_hooks(), 0);
    }
}
