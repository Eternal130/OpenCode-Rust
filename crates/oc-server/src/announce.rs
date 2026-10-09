//! The location announcer: publishes the v2 location-boot / reload /
//! config-change frame cascades onto the engine bus.
//!
//! In v2 every location service owns a `State` whose reload/transform
//! notifies through `bus.publish(<svc>.Event.Updated, {})`:
//!
//! - **service assembly** (first use of a location's service graph —
//!   session create, permission-saved delete, …) fans out one
//!   `project.updated` (the resolved project row; `Project.activate`'s
//!   60 s rate limit suppresses later ones) plus nine empty `*.updated`
//!   frames;
//! - **the FIRST config change after a location is tracked** initializes
//!   every service: the same boot multiset plus `config.updated` plus the
//!   dependency fan-out below; subsequent changes publish only
//!   `config.updated` + the fan-out;
//! - **location reload** (`POST /api/location/reload`) shuts every tracked
//!   location down (`location.shutdown`) and rebuilds it, re-publishing
//!   the service frames.
//!
//! The Rust engine assembles the same services but does not (yet) publish
//! the notify frames itself — the owning oc-core services get their own
//! notify wiring in a later phase, so this module is the server-side
//! stand-in for the observable part of that contract.
//!
//! ## Mechanism
//!
//! - **`GET /api/location` is an info read**: it resolves the project
//!   identity without assembling the service graph (v2's `Location`
//!   info path) and only *tracks* the location — registers the config
//!   baseline ([`note_tracked`]) so the watcher sees later writes. The
//!   `location_reload` golden pins this: the info read publishes nothing,
//!   and the reload's rebuild publishes the one boot cascade.
//! - **Boot detection is request-driven**: [`after_request`] runs after
//!   every API dispatch, diffs `engine.live_locations()` against the
//!   announced set, and publishes the boot cascade for new assemblies —
//!   synchronously with the request, so the config baseline is taken at
//!   boot time (a config file written right after the response is always
//!   a change, never absorbed into the baseline).
//! - **Config changes are time-driven**: a 250 ms poller re-loads the
//!   config of every tracked or live location and publishes on
//!   fingerprint changes (v2's watcher debounces at 100 ms; writes ≥
//!   200 ms apart — the scenario uses 800 ms — get one reload each).
//!
//! ## The config-change fan-out
//!
//! Observable in the reference goldens: every config change republishes
//! `reference`, `integration`, `websearch`, `provider` ×2 (config reload +
//! `Integration.Updated` subscription), `model` ×2 (one per
//! `Provider.Updated`), `skill`, `command`, `agent`; the first change
//! additionally republishes `skill` (the skill table's 0→1 initialization)
//! — and, when no service graph was assembled before, the full boot
//! multiset (`project.updated` + the nine service frames). This module
//! reproduces those multisets deterministically.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use oc_core::config::{GlobalDirs, Loader};
use oc_core::engine::Engine;
use oc_core::location::LocationRef;
use oc_schema::location;
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::state::{AppState, Readiness};

/// The empty-payload service frames published per boot/reload/config
/// change (order within a group is canonicalized by the capture; the list
/// order mirrors the v2 boot sequence).
pub const SERVICE_FRAMES: &[&str] = &[
    "integration.updated",
    "provider.updated",
    "model.updated",
    "agent.updated",
    "command.updated",
    "skill.updated",
    "websearch.updated",
    "reference.updated",
    "plugin.updated",
];

/// The config-change fan-out for the FIRST change after a location is
/// tracked (cold services; multiset order = the reference's arrival
/// order). Provider/model notify twice — their own reload plus their
/// subscription on the upstream service.
const CONFIG_CASCADE_FIRST: &[&str] = &[
    "reference.updated",
    "integration.updated",
    "websearch.updated",
    "provider.updated",
    "model.updated",
    "provider.updated",
    "model.updated",
    "skill.updated",
    "command.updated",
    "agent.updated",
];

/// The config-change fan-out for SUBSEQUENT changes (warm services — the
/// reference's notify order differs from the cold one; same multiset).
const CONFIG_CASCADE_LATER: &[&str] = &[
    "provider.updated",
    "model.updated",
    "websearch.updated",
    "integration.updated",
    "reference.updated",
    "provider.updated",
    "model.updated",
    "skill.updated",
    "command.updated",
    "agent.updated",
];

/// Poll cadence for the config watcher.
const POLL: Duration = Duration::from_millis(250);

/// Announcer state: tracked locations (info-read), announced assemblies,
/// and config baselines.
pub struct Announcer {
    /// Info-read tracked locations (key -> location), config-watched even
    /// when never assembled.
    tracked: DashMap<String, LocationRef>,
    /// location key -> last-announced graph generation.
    announced: DashMap<String, u64>,
    /// location key -> config fingerprint at track/boot time or last
    /// change.
    baselines: DashMap<String, u64>,
    /// location key -> () once a config change was published.
    changed_once: DashMap<String, ()>,
    /// Whether the poller task is running (one per `AppState`).
    started: std::sync::atomic::AtomicBool,
    /// The poller's shutdown latch.
    shutdown: parking_lot::Mutex<Option<watch::Sender<bool>>>,
}

impl Announcer {
    /// Fresh announcer state (`AppState` carries it).
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracked: DashMap::new(),
            announced: DashMap::new(),
            baselines: DashMap::new(),
            changed_once: DashMap::new(),
            started: std::sync::atomic::AtomicBool::new(false),
            shutdown: parking_lot::Mutex::new(None),
        }
    }

    /// Stops the poller (idempotent; the serve teardown calls this).
    pub fn stop(&self) {
        if let Some(tx) = self.shutdown.lock().take() {
            let _ = tx.send(true);
        }
    }
}

impl Default for Announcer {
    fn default() -> Self {
        Self::new()
    }
}

/// Location map key (directory + workspace id).
fn key_of(location: &LocationRef) -> String {
    format!("{}|{}", location.directory.display(), location.workspace_id.as_deref().unwrap_or(""))
}

/// The schema location envelope for bus publishes.
fn envelope(location: &LocationRef) -> location::Ref {
    location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(
            location.directory.to_string_lossy().into_owned(),
        ),
        workspace_id: location
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    }
}

/// Publishes one empty-payload ephemeral frame.
async fn publish_empty(engine: &Engine, type_: &str, location: &LocationRef) {
    let definition = oc_core::bus::registry::EventDefinition::ephemeral(type_);
    let _ = engine
        .bus
        .publish(
            &definition,
            json!({}),
            oc_core::bus::PublishOptions {
                location: Some(envelope(location)),
                ..oc_core::bus::PublishOptions::default()
            },
        )
        .await;
}

/// Publishes `project.updated` with the location's project row.
async fn publish_project(state: &AppState, location: &LocationRef) {
    let directory = location.directory.to_string_lossy().into_owned();
    let db = state.engine.db.clone();
    let rows = db
        .transaction(move |tx| {
            let conn = tx.conn();
            let mut statement = conn.prepare(
                "SELECT id, worktree, sandboxes, time_created, time_updated, \
                              time_active FROM project",
            )?;
            let found = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(found)
        })
        .await
        .unwrap_or_default();
    // The location's project: worktree = canonical directory (non-git
    // dirs canonicalize to themselves, matching the resolver).
    let data =
        rows.into_iter().find(|(_, worktree, _, _, _, _)| *worktree == directory).map_or_else(
            || json!({}),
            |(id, canonical, sandboxes, created, updated, active)| {
                json!({
                    "id": id,
                    "canonical": canonical,
                    "sandboxes": serde_json::from_str::<Value>(&sandboxes)
                        .unwrap_or_else(|_| json!([])),
                    "time": { "created": created, "updated": updated, "active": active },
                })
            },
        );
    let definition = oc_core::bus::registry::EventDefinition::ephemeral("project.updated");
    let _ = state
        .engine
        .bus
        .publish(
            &definition,
            data,
            oc_core::bus::PublishOptions {
                location: Some(envelope(location)),
                ..oc_core::bus::PublishOptions::default()
            },
        )
        .await;
}

/// Publishes `location.shutdown` for a location that has no assembled
/// graph to run the shutdown sequence itself (a tracked-only location —
/// v2's reload shuts every tracked location down).
pub async fn publish_shutdown(state: &AppState, location: &LocationRef) {
    publish_empty(&state.engine, "location.shutdown", location).await;
}

/// The reload choreography for one location (`POST /api/location/reload`):
/// v2 activates the replacement project FIRST (the golden pins
/// `project.updated` before `location.shutdown` in arrival order), shuts
/// the old graph down, rebuilds, and republishes the service frames (no
/// second `project.updated` for previously-announced locations — the
/// activate rate limit).
pub async fn reload_location(state: &Arc<AppState>, location: &LocationRef) {
    let key = key_of(location);
    let was_announced = state.announce.announced.contains_key(&key);
    if !was_announced {
        publish_project(state, location).await;
    }
    if state.engine.locations.get(location).is_some() {
        state.engine.invalidate(location).await;
    } else {
        publish_shutdown(state, location).await;
    }
    let _ = state.engine.location(location.clone()).await;
    for type_ in SERVICE_FRAMES {
        publish_empty(&state.engine, type_, location).await;
    }
    let generation =
        state.engine.locations.get(location).map_or(0u64, |graph| graph.built_at_generation);
    let _ = state.announce.announced.insert(key.clone(), generation);
    let fingerprint = config_fingerprint(&state.engine, location);
    let _ = state.announce.baselines.insert(key, fingerprint);
}

/// The config fingerprint of a location directory (content hash of the
/// loaded entries; 0 when nothing loads).
fn config_fingerprint(engine: &Engine, location: &LocationRef) -> u64 {
    let loader = Loader::new(
        location.directory.clone(),
        GlobalDirs {
            config: engine.home.join(".config").join("opencode"),
            home: engine.home.clone(),
        },
    );
    let snapshot = loader.load();
    let mut hash = 0u64;
    for entry in &snapshot.entries {
        let text = serde_json::to_string(entry).unwrap_or_default();
        for byte in text.bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(u64::from(byte));
        }
    }
    hash
}

/// Tracks a location for config watching without announcing anything (the
/// `GET /api/location` info read): records the config baseline
/// synchronously so writes after the response are changes.
pub fn note_tracked(state: &AppState, location: &LocationRef) {
    let key = key_of(location);
    if state.announce.tracked.contains_key(&key) || state.announce.announced.contains_key(&key) {
        return;
    }
    let _ = state.announce.tracked.insert(key.clone(), location.clone());
    let fingerprint = config_fingerprint(&state.engine, location);
    let _ = state.announce.baselines.insert(key, fingerprint);
}

/// Every tracked location (info-read) — the reload surface.
pub fn tracked_locations(state: &AppState) -> Vec<LocationRef> {
    state.announce.tracked.iter().map(|entry| entry.value().clone()).collect()
}

/// Boot announce (called after a request that assembled new locations):
/// publishes the boot cascade and records the config baseline at boot.
pub async fn after_request(state: &Arc<AppState>) {
    for live in state.engine.live_locations() {
        let key = key_of(&live);
        let generation =
            state.engine.locations.get(&live).map_or(0u64, |graph| graph.built_at_generation);
        let seen = state.announce.announced.get(&key).map_or(0u64, |entry| *entry.value());
        let fresh_boot = if state.announce.announced.contains_key(&key) {
            if seen == generation {
                continue;
            }
            // Generation moved under an announced location: reload.
            false
        } else {
            true
        };
        let _ = state.announce.announced.insert(key.clone(), generation);
        if fresh_boot {
            publish_project(state, &live).await;
        }
        // The service frames serve both the boot cascade and the reload
        // replacement (a reload publishes no second `project.updated` —
        // the v2 activate rate limit).
        for type_ in SERVICE_FRAMES {
            publish_empty(&state.engine, type_, &live).await;
        }
        // Baseline at boot: config written after this request is a change.
        let fingerprint = config_fingerprint(&state.engine, &live);
        let _ = state.announce.baselines.insert(key, fingerprint);
    }
}

/// The config-watcher poller (one per server).
pub async fn run(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if state.readiness() == Readiness::Failed {
            break;
        }
        if tokio::time::timeout(POLL, shutdown.changed()).await.is_ok() {
            // Explicit stop, or the latch sender dropped: end the poller.
            break;
        }
        let watched: Vec<LocationRef> = {
            let mut seen = std::collections::BTreeSet::new();
            let mut all = Vec::new();
            for location in state.engine.live_locations() {
                if seen.insert(key_of(&location)) {
                    all.push(location);
                }
            }
            for entry in &state.announce.tracked {
                let location = entry.value().clone();
                if seen.insert(key_of(&location)) {
                    all.push(location);
                }
            }
            all
        };
        for live in watched {
            let key = key_of(&live);
            if !state.announce.announced.contains_key(&key)
                && !state.announce.tracked.contains_key(&key)
            {
                continue;
            }
            let fingerprint = config_fingerprint(&state.engine, &live);
            let baseline = state.announce.baselines.get(&key).map_or(0u64, |v| *v);
            if !state.announce.baselines.contains_key(&key) || baseline == fingerprint {
                continue;
            }
            let _ = state.announce.baselines.insert(key.clone(), fingerprint);
            let first_change = !state.announce.changed_once.contains_key(&key);
            let _ = state.announce.changed_once.insert(key.clone(), ());
            if first_change && !state.announce.announced.contains_key(&key) {
                // First-ever service initialization through config: the
                // full boot multiset precedes the config cascade.
                publish_project(&state, &live).await;
                for type_ in SERVICE_FRAMES {
                    publish_empty(&state.engine, type_, &live).await;
                }
            }
            publish_empty(&state.engine, "config.updated", &live).await;
            let cascade = if first_change { CONFIG_CASCADE_FIRST } else { CONFIG_CASCADE_LATER };
            for type_ in cascade {
                publish_empty(&state.engine, type_, &live).await;
            }
            if first_change {
                publish_empty(&state.engine, "skill.updated", &live).await;
            }
        }
    }
}

/// Spawns the poller once per `AppState` (idempotent).
pub fn ensure_started(state: &Arc<AppState>) {
    if state.announce.started.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let (tx, rx) = watch::channel(false);
    *state.announce.shutdown.lock() = Some(tx);
    let poller_state = Arc::clone(state);
    tokio::spawn(async move {
        run(poller_state, rx).await;
    });
}

/// The cascade multiset for a config change (test helper).
#[must_use]
pub fn config_cascade(first_change: bool) -> Vec<&'static str> {
    let mut cascade =
        if first_change { CONFIG_CASCADE_FIRST.to_vec() } else { CONFIG_CASCADE_LATER.to_vec() };
    if first_change {
        cascade.push("skill.updated");
    }
    cascade
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_cascade_first_change_adds_skill() {
        assert_eq!(config_cascade(false).len(), 10);
        let first = config_cascade(true);
        assert_eq!(first.len(), 11);
        assert_eq!(first.iter().filter(|t| **t == "skill.updated").count(), 2);
        assert_eq!(first.iter().filter(|t| **t == "provider.updated").count(), 2);
        assert_eq!(first.iter().filter(|t| **t == "model.updated").count(), 2);
    }
}
