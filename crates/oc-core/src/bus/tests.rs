//! Unit + chaos tests for the bus lane (`src/bus.rs` + `src/bus/`).
//!
//! The binding chaos requirement: **persist-then-publish** — durable-event
//! visibility order must match commit order. Verified by concurrent
//! publishers on shared aggregates (contiguous sequences, commit-order
//! notifications), rolled-back transactions emitting nothing, and
//! `log(follow)` reading an exactly-once sequence under concurrent appends.
//! Everything runs against a real sqlite database in a tempdir via the
//! [`crate::database::Database`] layer.

#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // test-only counters

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use oc_schema::event::DurableInfo;
use oc_schema::id::{EventId, PermissionId, SessionId};
use oc_schema::location;
use oc_util::determinism::{Clock, FixedClock, IdGen, SequentialIdGen};

use super::{
    Bus, BusError, BusOptions, Event, EventDefinition, ListenerError, LogInput, LogItem, Projector,
    PublishInput, PublishOptions, ReplayOptions, SerializedEvent,
};
use crate::database::schema::SessionRow;
use crate::database::Tx;
use crate::permission::EventSink as _;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const NOW_MS: u64 = 1_725_000_000_000;

fn fake_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock::new(std::time::UNIX_EPOCH + Duration::from_millis(NOW_MS)))
}

fn fake_ids() -> Arc<dyn IdGen> {
    Arc::new(SequentialIdGen::new())
}

async fn test_bus(persist: bool) -> (Bus, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-bus-")
        .tempdir_in("/tmp/opencode")
        .expect("temp dir under /tmp/opencode");
    let db = crate::database::Database::open_file(dir.path().join("opencode.db"))
        .await
        .expect("open database");
    let options =
        BusOptions { clock: fake_clock(), ids: fake_ids(), persist, log_read_page_size: 512 };
    (Bus::new(db, options), dir)
}

fn definition(type_: &str) -> &'static EventDefinition {
    super::by_type_latest(type_).unwrap_or_else(|| panic!("no definition for {type_}"))
}

fn renamed_def() -> &'static EventDefinition {
    definition("session.renamed")
}

fn created_def() -> &'static EventDefinition {
    definition("session.created")
}

fn moved_def() -> &'static EventDefinition {
    definition("session.moved")
}

fn deleted_def() -> &'static EventDefinition {
    definition("session.deleted")
}

fn usage_def() -> &'static EventDefinition {
    definition("session.usage.updated")
}

fn worktree_def() -> &'static EventDefinition {
    definition("worktree.resolved")
}

fn renamed_data(session: &str, title: &str) -> serde_json::Value {
    serde_json::json!({ "sessionID": session, "title": title })
}

fn created_data(session: &str, directory: &str) -> serde_json::Value {
    serde_json::json!({
        "sessionID": session,
        "projectID": "global",
        "location": { "directory": directory },
        "slug": "test-slug",
        "version": "2.0.24",
    })
}

fn moved_data(session: &str, directory: &str) -> serde_json::Value {
    serde_json::json!({
        "sessionID": session,
        "location": { "directory": directory },
        "projectID": "global",
    })
}

fn loc(directory: &str) -> location::Ref {
    location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(directory),
        workspace_id: None,
    }
}

/// One raw `event` row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: String,
    seq: i64,
    type_: String,
    data: String,
}

async fn event_rows(bus: &Bus, aggregate: &str) -> Vec<Row> {
    let aggregate = aggregate.to_string();
    bus.database()
        .transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT id, seq, type, data FROM event WHERE aggregate_id = ?1 ORDER BY seq",
            )?;
            let rows = statement
                .query_map(rusqlite::params![aggregate], |row| {
                    Ok(Row {
                        id: row.get(0)?,
                        seq: row.get(1)?,
                        type_: row.get(2)?,
                        data: row.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("read event rows")
}

async fn kv_get(bus: &Bus, key: &str) -> Option<serde_json::Value> {
    bus.database().kv_get(key).await.expect("kv read")
}

/// Seeds a `project` row (`session_v2.project_id` has a foreign key).
async fn seed_project(bus: &Bus, id: &str, directory: &str) {
    let id = id.to_string();
    let directory = directory.to_string();
    bus.database()
        .transaction(move |tx| {
            let row = crate::database::schema::ProjectRow::new(id, &directory, None)?;
            crate::database::schema::ProjectRow::insert_or_ignore(tx, &row)
        })
        .await
        .expect("seed project");
}

/// One recorded notification: (aggregate, seq).
type Recorded = (String, Option<i64>);

/// Recorded listener: (aggregate-or-none, seq) per event, optionally failing
/// on one event type.
#[derive(Clone)]
struct Recorder {
    seen: Arc<parking_lot::Mutex<Vec<Recorded>>>,
    fail_on: Option<String>,
}

impl Recorder {
    fn new() -> Self {
        Self { seen: Arc::new(parking_lot::Mutex::new(Vec::new())), fail_on: None }
    }

    fn failing(fail_on: &str) -> Self {
        Self {
            seen: Arc::new(parking_lot::Mutex::new(Vec::new())),
            fail_on: Some(fail_on.to_string()),
        }
    }

    fn listener(&self) -> super::Listener {
        let seen = Arc::clone(&self.seen);
        let fail_on = self.fail_on.clone();
        Arc::new(move |event: &Event| {
            if fail_on.as_deref() == Some(event.type_.as_str()) {
                return Err(ListenerError("listener exploded".into()));
            }
            seen.lock().push((event.aggregate_id().unwrap_or("-").to_string(), event.seq()));
            Ok(())
        })
    }

    fn take(&self) -> Vec<(String, Option<i64>)> {
        self.seen.lock().clone()
    }

    fn for_aggregate(&self, aggregate: &str) -> Vec<Option<i64>> {
        self.take().into_iter().filter(|(id, _)| id == aggregate).map(|(_, seq)| seq).collect()
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

#[test]
fn bus_registry_manifest_complete() {
    let defs = super::definitions();
    // 51 session events + worktree.resolved.
    assert_eq!(defs.len(), 52, "inventory size");
    let durable = defs.iter().filter(|def| def.durable.is_some()).count();
    // 45 durable session events + worktree.resolved.
    assert_eq!(durable, 46, "durable manifest size");

    // Encoded types resolve and round-trip their versions.
    for def in defs.iter().filter(|def| def.durable.is_some()) {
        let meta = def.durable.as_ref().expect("durable");
        let encoded = oc_schema::event::versioned_type(&def.type_, meta.version);
        let resolved = super::by_versioned_type(&encoded).expect("durable type resolves");
        assert_eq!(resolved.type_, def.type_, "{encoded}");
        assert_eq!(resolved.durable, def.durable, "{encoded}");
    }
    // v2 events encode at .2, the rest at .1.
    for type_ in [
        "session.deleted",
        "session.forked",
        "session.instructions.updated",
        "session.tool.success",
        "session.tool.failed",
    ] {
        let def = super::by_type_latest(type_).unwrap();
        assert_eq!(def.durable.as_ref().unwrap().version, 2, "{type_}");
    }
    let renamed = super::by_versioned_type("session.renamed.1").unwrap();
    assert_eq!(renamed.durable.as_ref().unwrap().aggregate, "sessionID");
    let resolved = super::by_versioned_type("worktree.resolved.1").unwrap();
    assert_eq!(resolved.durable.as_ref().unwrap().aggregate, "projectID");

    // Ephemeral types and unversioned keys do not resolve in the durable
    // manifest.
    assert!(super::by_versioned_type("session.text.delta").is_none());
    assert!(super::by_versioned_type("session.renamed").is_none(), "unversioned key");
    assert!(super::by_versioned_type("session.nope.1").is_none());
    // Latest picks the highest version.
    assert_eq!(
        super::by_type_latest("session.deleted").unwrap().durable.as_ref().unwrap().version,
        2
    );
    // Session-event membership gates routing.
    assert!(super::is_session_event("session.renamed"));
    assert!(super::is_session_event("session.usage.updated"));
    assert!(!super::is_session_event("worktree.resolved"));
    assert!(!super::is_session_event("permission.asked"));
}

// ---------------------------------------------------------------------------
// Publish basics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_ephemeral_publish_skips_database() {
    let (bus, _dir) = test_bus(false).await;
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());

    let mut live = bus.subscribe();
    let mut typed = bus.subscribe_type("session.usage.updated");
    let mut other = bus.subscribe_type("session.renamed");

    let event = bus
        .publish(
            usage_def(),
            serde_json::json!({ "sessionID": "ses_1", "cost": 0.5, "tokens": {} }),
            PublishOptions { location: Some(loc("/repo")), ..Default::default() },
        )
        .await
        .expect("ephemeral publish");

    assert_eq!(event.type_, "session.usage.updated");
    assert!(event.durable.is_none(), "ephemeral carries no envelope");
    assert_eq!(event.created, NOW_MS, "clock injected");
    assert!(event.location.is_some());

    // Nothing durable in the database.
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), -1);
    let counts = bus.database().table_counts().await.unwrap();
    let event_count = counts.iter().find(|(name, _)| name == "event").unwrap().1;
    assert_eq!(event_count, 0, "no event rows");

    // Live + typed streams saw it.
    let received = timeout_next(&mut live).await;
    assert_eq!(received.id, event.id);
    let typed_received = timeout_next(&mut typed).await;
    assert_eq!(typed_received.type_, "session.usage.updated");
    assert!(timeout_none(&mut other).await, "other typed stream empty");
    assert_eq!(recorder.take().len(), 1);
}

#[tokio::test]
async fn bus_durable_publish_allocates_seq_and_persists() {
    let (bus, _dir) = test_bus(true).await;
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());

    let first = bus
        .publish(renamed_def(), renamed_data("ses_1", "one"), PublishOptions::default())
        .await
        .expect("first publish");
    let second = bus
        .publish(renamed_def(), renamed_data("ses_1", "two"), PublishOptions::default())
        .await
        .expect("second publish");

    assert_eq!(
        first.durable,
        Some(DurableInfo { aggregate_id: "ses_1".into(), seq: 0, version: 1 })
    );
    assert_eq!(second.durable.as_ref().unwrap().seq, 1);
    assert_eq!(first.id.as_str(), "evt_1", "injected id generator");
    assert_eq!(second.id.as_str(), "evt_2");

    let rows = event_rows(&bus, "ses_1").await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].type_, "session.renamed.1", "encoded versioned type");
    assert_eq!(rows[0].seq, 0);
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 1);

    let notifications = recorder.take();
    assert_eq!(notifications.len(), 2, "notify per commit");
    assert_eq!(notifications[0].1, Some(0));
    assert_eq!(notifications[1].1, Some(1));
}

#[tokio::test]
async fn bus_persist_defaults_to_false() {
    let (bus, _dir) = test_bus(false).await;
    bus.publish(renamed_def(), renamed_data("ses_1", "one"), PublishOptions::default())
        .await
        .expect("publish");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 0, "sequence still allocated");
    assert_eq!(event_rows(&bus, "ses_1").await, Vec::new(), "no rows by default");

    // Per-call override persists.
    bus.publish(
        renamed_def(),
        renamed_data("ses_1", "two"),
        PublishOptions { persist: Some(true), ..Default::default() },
    )
    .await
    .expect("persisted publish");
    let rows = event_rows(&bus, "ses_1").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].seq, 1);
}

#[tokio::test]
async fn bus_non_string_aggregate_rejected() {
    let (bus, _dir) = test_bus(true).await;
    let err = bus
        .publish(
            renamed_def(),
            serde_json::json!({ "sessionID": 42, "title": "x" }),
            PublishOptions::default(),
        )
        .await
        .expect_err("non-string aggregate");
    match err {
        BusError::Invalid { message, .. } => {
            assert!(message.contains("Expected string aggregate field sessionID"));
        }
        other => panic!("wrong error: {other:?}"),
    }
    bus.publish(renamed_def(), serde_json::json!([1]), PublishOptions::default())
        .await
        .expect_err("non-object data");
}

#[tokio::test]
async fn bus_commit_hook_atomic_with_event() {
    let (bus, _dir) = test_bus(true).await;
    let event = bus
        .publish(
            renamed_def(),
            renamed_data("ses_1", "hooked"),
            PublishOptions {
                commit: Some(Box::new(|tx: &mut Tx<'_>, seq: i64| {
                    tx.kv_set("hook:ses_1", &serde_json::json!({ "seq": seq }))
                })),
                ..Default::default()
            },
        )
        .await
        .expect("publish with hook");

    assert_eq!(event.seq(), Some(0));
    assert_eq!(kv_get(&bus, "hook:ses_1").await, Some(serde_json::json!({ "seq": 0 })));
    assert_eq!(event_rows(&bus, "ses_1").await.len(), 1, "event + hook in one tx");
}

#[tokio::test]
async fn bus_commit_hook_requires_durable() {
    let (bus, _dir) = test_bus(false).await;
    let err = bus
        .publish(
            usage_def(),
            serde_json::json!({ "sessionID": "ses_1" }),
            PublishOptions {
                commit: Some(Box::new(|_: &mut Tx<'_>, _: i64| Ok(()))),
                ..Default::default()
            },
        )
        .await
        .expect_err("ephemeral commit hook");
    assert!(err.to_string().contains("Local commit hooks require a durable event"));
}

// ---------------------------------------------------------------------------
// publishAll
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_publish_all_single_tx_contiguous_seq() {
    let (bus, _dir) = test_bus(true).await;
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());

    let events = bus
        .publish_all(vec![
            PublishInput::new(created_def().clone(), created_data("ses_1", "/repo")),
            PublishInput::new(renamed_def().clone(), renamed_data("ses_1", "batch")),
            PublishInput::new(deleted_def().clone(), serde_json::json!({ "sessionID": "ses_1" })),
        ])
        .await
        .expect("publish_all");

    assert_eq!(events.len(), 3);
    let seqs: Vec<i64> = events.iter().map(|event| event.seq().unwrap()).collect();
    assert_eq!(seqs, vec![0, 1, 2], "latest+1..+n");
    let rows = event_rows(&bus, "ses_1").await;
    assert_eq!(rows.len(), 3);
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 2);

    // Notification order matches publish order.
    let notified = recorder.for_aggregate("ses_1");
    assert_eq!(notified, vec![Some(0), Some(1), Some(2)]);
}

#[tokio::test]
async fn bus_publish_all_mixed_aggregate_rejected() {
    let (bus, _dir) = test_bus(true).await;
    let err = bus
        .publish_all(vec![
            PublishInput::new(renamed_def().clone(), renamed_data("ses_1", "a")),
            PublishInput::new(renamed_def().clone(), renamed_data("ses_2", "b")),
        ])
        .await
        .expect_err("mixed aggregates");
    assert!(err.to_string().contains("must belong to the same aggregate"));
    assert_eq!(event_rows(&bus, "ses_1").await, Vec::new(), "nothing committed");
    assert_eq!(event_rows(&bus, "ses_2").await, Vec::new());
}

#[tokio::test]
async fn bus_publish_all_duplicate_ids_rejected() {
    let (bus, _dir) = test_bus(true).await;
    let id = EventId::parse("evt_dup").unwrap();
    let mut first = PublishInput::new(renamed_def().clone(), renamed_data("ses_1", "a"));
    first.id = Some(id.clone());
    let mut second = PublishInput::new(renamed_def().clone(), renamed_data("ses_1", "b"));
    second.id = Some(id);
    let err = bus.publish_all(vec![first, second]).await.expect_err("duplicate id");
    assert!(err.to_string().contains("appears more than once"));
    assert_eq!(event_rows(&bus, "ses_1").await, Vec::new());
}

// ---------------------------------------------------------------------------
// Projectors (atomicity)
// ---------------------------------------------------------------------------

/// Test projector A: writes a kv projection per aggregate.
fn kv_projector() -> Projector {
    Arc::new(|tx: &mut Tx<'_>, event: &super::DurableEvent| {
        let key = format!("proj:kv:{}", event.durable.aggregate_id);
        let count = tx
            .kv_get(&key)?
            .and_then(|value| value.get("count").and_then(serde_json::Value::as_u64))
            .unwrap_or(0);
        tx.kv_set(&key, &serde_json::json!({ "count": count + 1, "lastSeq": event.durable.seq }))?;
        Ok(())
    })
}

/// Test projector B: inserts `session_v2` rows for `session.created`.
fn session_projector() -> Projector {
    Arc::new(|tx: &mut Tx<'_>, event: &super::DurableEvent| {
        if event.type_ != "session.created" {
            return Ok(());
        }
        let directory = event
            .data
            .pointer("/location/directory")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("/");
        let row = SessionRow::new(
            event.data.get("sessionID").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
            event.data.get("projectID").and_then(serde_json::Value::as_str).unwrap_or("global"),
            directory,
            event.data.get("slug").and_then(serde_json::Value::as_str).unwrap_or("slug"),
            event.data.get("version").and_then(serde_json::Value::as_str).unwrap_or("2"),
        )?;
        SessionRow::insert_or_ignore(tx, &row)?;
        Ok(())
    })
}

#[tokio::test]
async fn bus_projectors_run_inside_tx_and_project() {
    let (bus, _dir) = test_bus(true).await;
    seed_project(&bus, "global", "/repo").await;
    bus.project(created_def(), session_projector());
    bus.project(created_def(), kv_projector());
    bus.project(renamed_def(), kv_projector());

    bus.publish(created_def(), created_data("ses_1", "/repo"), PublishOptions::default())
        .await
        .expect("created");
    bus.publish(renamed_def(), renamed_data("ses_1", "hello"), PublishOptions::default())
        .await
        .expect("renamed");

    assert_eq!(
        kv_get(&bus, "proj:kv:ses_1").await,
        Some(serde_json::json!({ "count": 2, "lastSeq": 1 })),
        "kv projection tracks both events"
    );
    let sessions =
        bus.database().transaction(|tx| SessionRow::list(tx)).await.expect("list sessions");
    assert_eq!(sessions.len(), 1, "session_v2 projection written");
    assert_eq!(sessions[0].id, "ses_1");
    assert_eq!(sessions[0].directory, "/repo");
}

/// Atomicity: a failing projector rolls back the event row AND every
/// projection written earlier in the same transaction ("crash between →
/// neither").
#[tokio::test]
async fn bus_projector_failure_rolls_back_everything() {
    let (bus, _dir) = test_bus(true).await;
    bus.project(renamed_def(), kv_projector());
    // Fails only for a specific payload marker.
    bus.project(
        renamed_def(),
        Arc::new(|_tx: &mut Tx<'_>, event: &super::DurableEvent| {
            if event.data.get("title").and_then(serde_json::Value::as_str) == Some("explode") {
                Err(crate::database::error::DbError::Pragma("projector exploded".into()))
            } else {
                Ok(())
            }
        }),
    );

    bus.publish(renamed_def(), renamed_data("ses_1", "ok"), PublishOptions::default())
        .await
        .expect("good publish");
    let bad = bus
        .publish(renamed_def(), renamed_data("ses_1", "explode"), PublishOptions::default())
        .await
        .expect_err("failing projector");

    assert!(bad.to_string().contains("projector exploded"));
    // Neither the event nor the kv projection from the failed tx survived.
    assert_eq!(
        kv_get(&bus, "proj:kv:ses_1").await,
        Some(serde_json::json!({ "count": 1, "lastSeq": 0 })),
        "projection count unchanged by rolled-back event"
    );
    let rows = event_rows(&bus, "ses_1").await;
    assert_eq!(rows.len(), 1, "rolled-back event left no row");
    assert_eq!(rows[0].seq, 0);
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 0, "sequence not advanced");
}

// ---------------------------------------------------------------------------
// Location routing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_location_routing_created_moved_deleted() {
    let (bus, _dir) = test_bus(true).await;
    let mut repo = bus.subscribe().local(&loc("/repo"));
    let mut other = bus.subscribe().local(&loc("/other"));

    // created → its own location.
    bus.publish(created_def(), created_data("ses_1", "/repo"), PublishOptions::default())
        .await
        .expect("created");
    let received = timeout_next(&mut repo).await;
    assert_eq!(received.type_, "session.created");
    assert!(timeout_none(&mut other).await, "not routed at /other");

    // unlocated renamed → routed via the in-memory map (/repo).
    bus.publish(renamed_def(), renamed_data("ses_1", "title"), PublishOptions::default())
        .await
        .expect("renamed");
    let received = timeout_next(&mut repo).await;
    assert_eq!(received.type_, "session.renamed");

    // moved → BOTH old and new locations.
    bus.publish(moved_def(), moved_data("ses_1", "/other"), PublishOptions::default())
        .await
        .expect("moved");
    let received = timeout_next(&mut repo).await;
    assert_eq!(received.type_, "session.moved");
    let received = timeout_next(&mut other).await;
    assert_eq!(received.type_, "session.moved");

    // later unlocated events only reach the destination.
    bus.publish(renamed_def(), renamed_data("ses_1", "after-move"), PublishOptions::default())
        .await
        .expect("renamed after move");
    assert!(timeout_none(&mut repo).await, "old location no longer routed");
    let received = timeout_next(&mut other).await;
    assert_eq!(received.type_, "session.renamed");

    // deleted → clears routing.
    bus.publish(
        deleted_def(),
        serde_json::json!({ "sessionID": "ses_1" }),
        PublishOptions::default(),
    )
    .await
    .expect("deleted");
    let received = timeout_next(&mut other).await;
    assert_eq!(received.type_, "session.deleted");
    bus.publish(renamed_def(), renamed_data("ses_1", "ghost"), PublishOptions::default())
        .await
        .expect("renamed after delete");
    assert!(timeout_none(&mut other).await, "no location after delete");
}

#[tokio::test]
async fn bus_location_routing_db_fallback_and_unknown() {
    let (bus, _dir) = test_bus(true).await;

    // A session_v2 row exists but the in-memory map never saw it.
    seed_project(&bus, "proj_1", "/fromdb").await;
    bus.database()
        .transaction(|tx| {
            let row = SessionRow::new("ses_db", "proj_1", "/fromdb", "slug", "2.0.24")?;
            SessionRow::insert(tx, &row)
        })
        .await
        .expect("seed session_v2");

    let mut fromdb = bus.subscribe().local(&loc("/fromdb"));
    bus.publish(renamed_def(), renamed_data("ses_db", "fallback"), PublishOptions::default())
        .await
        .expect("renamed");
    let received = timeout_next(&mut fromdb).await;
    assert_eq!(received.aggregate_id(), Some("ses_db"), "session_v2 fallback routed");

    // Unknown session, unlocated → routed to no location, but the
    // unfiltered stream still receives it (routing only filters views).
    let mut anywhere = bus.subscribe();
    bus.publish(renamed_def(), renamed_data("ses_ghost", "void"), PublishOptions::default())
        .await
        .expect("renamed");
    let received = timeout_next(&mut anywhere).await;
    assert_eq!(received.aggregate_id(), Some("ses_ghost"));
}

#[tokio::test]
async fn bus_envelope_location_routes_and_global_suppresses() {
    let (bus, _dir) = test_bus(true).await;
    let mut local = bus.subscribe().local(&loc("/envelope"));

    // Envelope location routes the event even without routing-map state.
    bus.publish(
        worktree_def(),
        serde_json::json!({
            "projectID": "proj_x",
            "directory": "/repo",
            "previous": "global",
        }),
        PublishOptions { location: Some(loc("/envelope")), ..Default::default() },
    )
    .await
    .expect("worktree resolved");
    let received = timeout_next(&mut local).await;
    assert_eq!(received.type_, "worktree.resolved");

    // global:true drops the envelope location → unlocated events pass
    // every location filter.
    bus.publish(
        worktree_def(),
        serde_json::json!({
            "projectID": "proj_x",
            "directory": "/repo",
            "previous": "global",
        }),
        PublishOptions { global: true, ..Default::default() },
    )
    .await
    .expect("global publish");
    let received = timeout_next(&mut local).await;
    assert!(received.location.is_none(), "global suppresses location");
    assert_eq!(received.seq(), Some(1), "worktree aggregate advances independently");
}

// ---------------------------------------------------------------------------
// Replay / remove / claim
// ---------------------------------------------------------------------------

fn serialized(bus_event: &Event) -> SerializedEvent {
    let durable = bus_event.durable.as_ref().unwrap();
    SerializedEvent {
        id: bus_event.id.clone(),
        type_: oc_schema::event::versioned_type(&bus_event.type_, durable.version),
        created: Some(bus_event.created),
        seq: i64::try_from(durable.seq).unwrap(),
        aggregate_id: durable.aggregate_id.clone(),
        data: bus_event.data.clone(),
    }
}

#[tokio::test]
async fn bus_replay_idempotent_and_divergent() {
    let (bus, _dir) = test_bus(true).await;
    let first = bus
        .publish(renamed_def(), renamed_data("ses_1", "one"), PublishOptions::default())
        .await
        .unwrap();
    let second = bus
        .publish(renamed_def(), renamed_data("ses_1", "two"), PublishOptions::default())
        .await
        .unwrap();

    // Replaying committed events is idempotent.
    bus.replay(serialized(&first), ReplayOptions::default()).await.expect("replay first");
    bus.replay(serialized(&second), ReplayOptions::default()).await.expect("replay second");
    assert_eq!(event_rows(&bus, "ses_1").await.len(), 2, "no duplicates");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 1);

    // Same seq, different payload → divergence error.
    let mut divergent = serialized(&second);
    divergent.data = renamed_data("ses_1", "tampered");
    let err = bus.replay(divergent, ReplayOptions::default()).await.expect_err("divergent");
    assert!(err.to_string().contains("Replay diverged"), "{err}");

    // Skipping ahead → sequence mismatch.
    let mut skip = serialized(&second);
    skip.seq = 5;
    skip.data = renamed_data("ses_1", "future");
    skip.id = EventId::parse("evt_future").unwrap();
    let err = bus.replay(skip, ReplayOptions::default()).await.expect_err("sequence mismatch");
    assert!(err.to_string().contains("Sequence mismatch"), "{err}");

    // Unknown versioned type.
    let mut unknown = serialized(&second);
    unknown.type_ = "session.renamed.9".into();
    let err = bus.replay(unknown, ReplayOptions::default()).await.expect_err("unknown type");
    assert!(err.to_string().contains("Unknown durable event type"));

    // Aggregate mismatch between envelope and payload.
    let mut mismatch = serialized(&second);
    mismatch.aggregate_id = "ses_9".into();
    let err = bus.replay(mismatch, ReplayOptions::default()).await.expect_err("aggregate mismatch");
    assert!(err.to_string().contains("Aggregate mismatch"));

    // A fresh event replays at its declared seq and may notify.
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());
    let fresh = SerializedEvent {
        id: EventId::parse("evt_new").unwrap(),
        type_: "session.renamed.1".into(),
        created: Some(7),
        seq: 2,
        aggregate_id: "ses_1".into(),
        data: renamed_data("ses_1", "replayed"),
    };
    bus.replay(fresh, ReplayOptions { publish: true, ..Default::default() })
        .await
        .expect("fresh replay commits");
    assert_eq!(event_rows(&bus, "ses_1").await.len(), 3);
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 2);
    assert_eq!(recorder.for_aggregate("ses_1"), vec![Some(2)], "replay publish notifies once");
}

#[tokio::test]
async fn bus_replay_owner_semantics() {
    let (bus, _dir) = test_bus(true).await;
    let event = bus
        .publish(renamed_def(), renamed_data("ses_1", "one"), PublishOptions::default())
        .await
        .unwrap();

    // Foreign owner: replay of a new seq is skipped without error.
    bus.claim("ses_1", "owner-a").await.expect("claim");
    let mut foreign = serialized(&event);
    foreign.seq = 1;
    foreign.id = EventId::parse("evt_foreign").unwrap();
    foreign.data = renamed_data("ses_1", "next");
    bus.replay(
        foreign.clone(),
        ReplayOptions { owner_id: Some("owner-b".into()), ..Default::default() },
    )
    .await
    .expect("foreign owner skip");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 0, "skip commits nothing");
    assert_eq!(event_rows(&bus, "ses_1").await.len(), 1);

    // strict owner mismatch → error.
    let mut strict = serialized(&event);
    strict.seq = 1;
    strict.id = EventId::parse("evt_strict").unwrap();
    strict.data = renamed_data("ses_1", "next");
    let err = bus
        .replay(
            strict,
            ReplayOptions {
                owner_id: Some("owner-b".into()),
                strict_owner: true,
                ..Default::default()
            },
        )
        .await
        .expect_err("strict owner");
    assert!(err.to_string().contains("Replay owner mismatch"), "{err}");

    // Matching owner commits.
    let mut owned = serialized(&event);
    owned.seq = 1;
    owned.id = EventId::parse("evt_owned").unwrap();
    owned.data = renamed_data("ses_1", "owned");
    bus.replay(owned, ReplayOptions { owner_id: Some("owner-a".into()), ..Default::default() })
        .await
        .expect("owned replay");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 1);
}

#[tokio::test]
async fn bus_remove_and_reserve() {
    let (bus, _dir) = test_bus(true).await;
    bus.publish(renamed_def(), renamed_data("ses_1", "one"), PublishOptions::default())
        .await
        .expect("publish");
    bus.reserve_sequence("ses_1", 10).await.expect("reserve advances");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), 10);

    bus.remove("ses_1").await.expect("remove");
    assert_eq!(bus.latest_sequence("ses_1").await.unwrap(), -1);
    assert_eq!(event_rows(&bus, "ses_1").await, Vec::new());
}

// ---------------------------------------------------------------------------
// Log (paged read + follow)
// ---------------------------------------------------------------------------

async fn timeout_next<S>(stream: &mut S) -> Event
where
    S: futures::Stream<Item = Event> + Unpin,
{
    tokio::time::timeout(Duration::from_millis(500), stream.next())
        .await
        .expect("item within timeout")
        .expect("stream open")
}

async fn timeout_none<S>(stream: &mut S) -> bool
where
    S: futures::Stream<Item = Event> + Unpin,
{
    tokio::time::timeout(Duration::from_millis(100), stream.next()).await.is_err()
}

#[tokio::test]
async fn bus_log_paging_unknown_types_and_marker() {
    let (bus, _dir) = test_bus(true).await;
    // 300 known events, then an unknown versioned type at seq 300 (which
    // also advances the allocator), then 300 more known at 301..=600 —
    // forcing a second 512-page and a decode gap.
    for index in 0..300 {
        bus.publish(
            renamed_def(),
            renamed_data("ses_1", &format!("t{index}")),
            PublishOptions::default(),
        )
        .await
        .expect("publish");
    }
    bus.database()
        .transaction(|tx| {
            tx.event_sequence_upsert("ses_1", 300, None)?;
            tx.execute(
                "INSERT INTO event (id, aggregate_id, seq, created, type, data) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["evt_future_1", "ses_1", 300, 0, "session.renamed.9", "{}"],
            )?;
            Ok(())
        })
        .await
        .expect("inject unknown row");
    for index in 300..600 {
        bus.publish(
            renamed_def(),
            renamed_data("ses_1", &format!("t{index}")),
            PublishOptions::default(),
        )
        .await
        .expect("publish after gap");
    }

    let stream = bus.log(LogInput { aggregate_id: "ses_1".into(), after: None, follow: false });
    let items: Vec<LogItem> = stream.map(Result::unwrap).collect().await;

    let events: Vec<&Event> = items
        .iter()
        .filter_map(|item| match item {
            LogItem::Event(event) => Some(event),
            LogItem::Synced(_) => None,
        })
        .collect();
    assert_eq!(events.len(), 600, "unknown type skipped");
    let seqs: Vec<i64> = events.iter().map(|event| event.seq().unwrap()).collect();
    let expected: Vec<i64> = (0..300).chain(301..601).collect();
    assert_eq!(seqs, expected, "cursor advances past the unknown row");
    assert_eq!(events[299].data["title"], "t299");
    assert_eq!(events[300].data["title"], "t300", "first known event after the gap");

    let markers: Vec<&LogItem> =
        items.iter().filter(|item| matches!(item, LogItem::Synced(_))).collect();
    assert_eq!(markers.len(), 1, "exactly one synced marker");
    match markers[0] {
        LogItem::Synced(marker) => {
            assert_eq!(marker.aggregate_id, "ses_1");
            assert_eq!(marker.seq, Some(600), "marker at watermark");
        }
        LogItem::Event(_) => unreachable!(),
    }

    // Resume cursor skips the prefix; the resume read carries exactly one
    // marker of its own at the same watermark.
    let mut resumed = Vec::new();
    let mut resumed_markers = 0;
    let stream =
        bus.log(LogInput { aggregate_id: "ses_1".into(), after: Some(598), follow: false });
    let items: Vec<LogItem> = stream.map(Result::unwrap).collect().await;
    for item in items {
        match item {
            LogItem::Event(event) => resumed.push(event),
            LogItem::Synced(marker) => {
                resumed_markers += 1;
                assert_eq!(marker.seq, Some(600));
            }
        }
    }
    assert_eq!(resumed_markers, 1);
    assert_eq!(resumed.len(), 2);
    assert_eq!(resumed[0].seq(), Some(599));
}

#[tokio::test]
async fn bus_log_follow_exactly_once_under_concurrent_appends() {
    let (bus, _dir) = test_bus(true).await;
    // Historical prefix.
    for index in 0..5 {
        bus.publish(
            renamed_def(),
            renamed_data("ses_1", &format!("pre{index}")),
            PublishOptions::default(),
        )
        .await
        .expect("prefix publish");
    }

    let mut stream = bus.log(LogInput { aggregate_id: "ses_1".into(), after: None, follow: true });

    // Concurrent appends while the follower replays.
    let appender = {
        let bus = bus.clone();
        tokio::spawn(async move {
            let mut batch = Vec::new();
            for index in 0..25 {
                batch.push(PublishInput::new(
                    renamed_def().clone(),
                    renamed_data("ses_1", &format!("live{index}")),
                ));
                if batch.len() == 5 {
                    bus.publish_all(std::mem::take(&mut batch)).await.expect("append batch");
                }
            }
            if !batch.is_empty() {
                bus.publish_all(batch).await.expect("append tail");
            }
        })
    };

    // Collect until quiescence (no item for 300ms).
    let mut events = Vec::new();
    let mut markers = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        assert!(tokio::time::Instant::now() < deadline, "follower never went quiescent");
        match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
            Err(_elapsed) => break,
            Ok(Some(Ok(LogItem::Event(event)))) => events.push(event),
            Ok(Some(Ok(LogItem::Synced(_)))) => markers += 1,
            Ok(Some(Err(err))) => panic!("log error: {err}"),
            Ok(None) => break,
        }
    }
    appender.await.expect("appender");

    assert_eq!(markers, 1, "one synced marker");
    assert_eq!(events.len(), 30, "prefix + concurrent events exactly once");
    let seqs: Vec<i64> = events.iter().map(|event| event.seq().unwrap()).collect();
    let expected: Vec<i64> = (0..30).collect();
    assert_eq!(seqs, expected, "exactly-once ordered sequence 0..=29");
}

// ---------------------------------------------------------------------------
// Chaos (binding requirement)
// ---------------------------------------------------------------------------

/// N=8 concurrent publishers × M events on ONE aggregate: contiguous
/// sequences, commit-order notifications, every event exactly once.
#[tokio::test]
async fn bus_chaos_concurrent_publishers_same_aggregate() {
    const PUBLISHERS: usize = 8;
    const PER_PUBLISHER: usize = 5;

    let (bus, _dir) = test_bus(true).await;
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());

    let mut handles = Vec::new();
    for worker in 0..PUBLISHERS {
        let bus = bus.clone();
        handles.push(tokio::spawn(async move {
            for index in 0..PER_PUBLISHER {
                bus.publish(
                    renamed_def(),
                    renamed_data("ses_chaos", &format!("w{worker}-e{index}")),
                    PublishOptions::default(),
                )
                .await
                .expect("chaos publish");
            }
        }));
    }
    for handle in handles {
        handle.await.expect("publisher finished");
    }

    let total: i64 = (PUBLISHERS * PER_PUBLISHER) as i64;
    let rows = event_rows(&bus, "ses_chaos").await;
    assert_eq!(rows.len(), usize::try_from(total).unwrap(), "every event persisted");
    let mut seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
    seqs.sort_unstable();
    let expected: Vec<i64> = (0..total).collect();
    assert_eq!(seqs, expected, "per-aggregate sequences contiguous 0..{total}");

    // Notification order matches commit order (== seq order) per aggregate.
    let notified = recorder.for_aggregate("ses_chaos");
    assert_eq!(notified.len(), usize::try_from(total).unwrap());
    assert_eq!(notified, expected.iter().map(|seq| Some(*seq)).collect::<Vec<_>>());
    assert_eq!(bus.latest_sequence("ses_chaos").await.unwrap(), total - 1);
}

/// Cross-aggregate interleaving preserves per-aggregate ordering.
#[tokio::test]
async fn bus_chaos_cross_aggregate_interleaving() {
    const AGGREGATES: usize = 4;
    const PER_AGGREGATE: usize = 6;

    let (bus, _dir) = test_bus(true).await;
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());

    let mut handles = Vec::new();
    for aggregate in 0..AGGREGATES {
        let bus = bus.clone();
        handles.push(tokio::spawn(async move {
            for index in 0..PER_AGGREGATE {
                bus.publish(
                    renamed_def(),
                    renamed_data(&format!("ses_x{aggregate}"), &format!("e{index}")),
                    PublishOptions::default(),
                )
                .await
                .expect("cross-aggregate publish");
            }
        }));
    }
    for handle in handles {
        handle.await.expect("publisher finished");
    }

    // Global notifications interleave across aggregates, but every
    // aggregate's subsequence is exactly its commit order 0,1,2,....
    let notifications = recorder.take();
    assert_eq!(notifications.len(), AGGREGATES * PER_AGGREGATE);
    let mut per_aggregate: HashMap<String, Vec<i64>> = HashMap::new();
    for (aggregate, seq) in notifications {
        per_aggregate.entry(aggregate).or_default().push(seq.expect("durable seq"));
    }
    let expected: Vec<i64> = (0..i64::try_from(PER_AGGREGATE).unwrap()).collect();
    for aggregate in 0..AGGREGATES {
        let id = format!("ses_x{aggregate}");
        assert_eq!(per_aggregate[&id], expected, "{id} notification order == commit order");
        let rows = event_rows(&bus, &id).await;
        let seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, expected, "{id} contiguous");
    }
}

/// A projector failing mid-chaos: rolled-back publishes emit NOTHING
/// (no notification, no row, no sequence hole for committed events).
#[tokio::test]
async fn bus_chaos_rollback_emits_nothing() {
    const WORKERS: usize = 18; // even → good, odd → exploding payload

    let (bus, _dir) = test_bus(true).await;
    bus.project(
        renamed_def(),
        Arc::new(|_tx: &mut Tx<'_>, event: &super::DurableEvent| {
            if event.data.get("title").and_then(serde_json::Value::as_str) == Some("explode") {
                Err(crate::database::error::DbError::Pragma("chaos rollback".into()))
            } else {
                Ok(())
            }
        }),
    );
    let recorder = Recorder::new();
    let _guard = bus.listen(recorder.listener());
    let mut live = bus.subscribe();

    let mut handles = Vec::new();
    for worker in 0..WORKERS {
        let bus = bus.clone();
        let exploding = worker % 2 == 1;
        handles.push(tokio::spawn(async move {
            let title = if exploding { "explode".to_string() } else { format!("ok{worker}") };
            bus.publish(
                renamed_def(),
                renamed_data("ses_rollback", &title),
                PublishOptions::default(),
            )
            .await
        }));
    }
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for handle in handles {
        match handle.await.expect("join") {
            Ok(_) => succeeded += 1,
            Err(_) => failed += 1,
        }
    }
    let good = WORKERS / 2;
    assert_eq!(succeeded, good);
    assert_eq!(failed, WORKERS - good);

    // Committed events: contiguous, no holes from the interleaved rollbacks.
    let rows = event_rows(&bus, "ses_rollback").await;
    assert_eq!(rows.len(), good);
    let seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
    let expected: Vec<i64> = (0..good as i64).collect();
    assert_eq!(seqs, expected);

    // Exactly one notification per committed event, none for rolled back.
    let notified = recorder.for_aggregate("ses_rollback");
    assert_eq!(notified.len(), good);
    assert_eq!(notified, expected.iter().map(|seq| Some(*seq)).collect::<Vec<_>>());

    // The live stream saw only committed events.
    let mut streamed = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(10), live.next()).await {
        streamed.push(event);
    }
    assert_eq!(streamed.len(), good, "no stream item for rolled-back events");
    let streamed_seqs: Vec<i64> = streamed.iter().map(|event| event.seq().unwrap()).collect();
    assert_eq!(streamed_seqs, expected);

    // And the log sees exactly the committed prefix.
    let mut events = Vec::new();
    let mut log_markers = 0;
    let mut log =
        bus.log(LogInput { aggregate_id: "ses_rollback".into(), after: None, follow: false });
    while let Some(item) = log.next().await {
        match item {
            Ok(LogItem::Event(event)) => events.push(event),
            Ok(LogItem::Synced(_)) => log_markers += 1,
            Err(err) => panic!("log error: {err}"),
        }
    }
    assert_eq!(log_markers, 1);
    assert_eq!(events.len(), good);
}

// ---------------------------------------------------------------------------
// Listeners
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_listener_error_semantics() {
    let (bus, _dir) = test_bus(false).await;
    let failing = Recorder::failing("session.usage.updated");
    let _guard = bus.listen(failing.listener());

    // Ephemeral publish: listener errors propagate (v2 unisolated notify).
    let err = bus
        .publish(
            usage_def(),
            serde_json::json!({ "sessionID": "ses_1", "cost": 0, "tokens": {} }),
            PublishOptions::default(),
        )
        .await
        .expect_err("ephemeral listener failure propagates");
    assert!(matches!(err, BusError::Listener(_)));

    // Durable publish: errors are logged and swallowed; streams still run.
    let failing = Recorder::failing("session.renamed");
    let _guard = bus.listen(failing.listener());
    let mut live = bus.subscribe();
    bus.publish(renamed_def(), renamed_data("ses_1", "isolated"), PublishOptions::default())
        .await
        .expect("durable publish survives listener failure");
    let received = timeout_next(&mut live).await;
    assert_eq!(received.aggregate_id(), Some("ses_1"));
}

#[tokio::test]
async fn bus_listen_guard_detaches_on_drop() {
    let (bus, _dir) = test_bus(false).await;
    let recorder = Recorder::new();
    let guard = bus.listen(recorder.listener());
    drop(guard);
    bus.publish(
        usage_def(),
        serde_json::json!({ "sessionID": "ses_1", "cost": 0, "tokens": {} }),
        PublishOptions::default(),
    )
    .await
    .expect("publish");
    assert_eq!(recorder.take(), Vec::new(), "detached listener not invoked");
}

// ---------------------------------------------------------------------------
// Permission sink wiring
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_permission_sink_adapter_publishes() {
    let (bus, _dir) = test_bus(false).await;
    let sink = super::PermissionBusSink::new(bus.clone(), Some(loc("/repo")));
    let mut local = bus.subscribe().local(&loc("/repo"));
    let mut elsewhere = bus.subscribe().local(&loc("/elsewhere"));

    let asked =
        oc_schema::misc_events::PermissionEvent::Asked(oc_schema::event::EphemeralEnvelope {
            id: EventId::parse("evt_ask").unwrap(),
            created: 1,
            metadata: None,
            location: None,
            data: oc_schema::permission::Request {
                id: PermissionId::parse("per_1").unwrap(),
                session_id: SessionId::parse("ses_1").unwrap(),
                action: "bash".into(),
                resources: vec!["rm -rf /tmp/x".into()],
                save: None,
                metadata: None,
                source: None,
                message: None,
            },
        });
    sink.publish(asked).await;

    let received = timeout_next(&mut local).await;
    assert_eq!(received.type_, "permission.asked");
    assert_eq!(received.data["sessionID"], "ses_1");
    assert_eq!(received.data["id"], "per_1");
    assert!(timeout_none(&mut elsewhere).await, "location-scoped");
}

#[tokio::test]
async fn bus_permission_sink_bridge_from_broadcast() {
    let (bus, _dir) = test_bus(false).await;
    let broadcast = crate::permission::BroadcastEventSink::new(16);
    let _bridge = super::sink::bridge_permission_sink(bus.clone(), &broadcast, None);
    let mut live = bus.subscribe();

    broadcast
        .publish(oc_schema::misc_events::PermissionEvent::Replied(
            oc_schema::event::EphemeralEnvelope {
                id: EventId::parse("evt_reply").unwrap(),
                created: 2,
                metadata: None,
                location: None,
                data: oc_schema::misc_events::PermissionReplied {
                    session_id: SessionId::parse("ses_1").unwrap(),
                    request_id: PermissionId::parse("per_1").unwrap(),
                    reply: oc_schema::permission::Reply::Once,
                },
            },
        ))
        .await;

    let received = timeout_next(&mut live).await;
    assert_eq!(received.type_, "permission.replied");
    assert_eq!(received.data["reply"], "once");
}

// ---------------------------------------------------------------------------
// Wire shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bus_event_wire_shape() {
    let (bus, _dir) = test_bus(true).await;
    let event = bus
        .publish(
            renamed_def(),
            renamed_data("ses_1", "wire"),
            PublishOptions { location: Some(loc("/repo")), ..Default::default() },
        )
        .await
        .expect("publish");
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": event.id.as_str(),
            "created": NOW_MS,
            "type": "session.renamed",
            "durable": { "aggregateID": "ses_1", "seq": 0, "version": 1 },
            "location": { "directory": "/repo" },
            "data": { "sessionID": "ses_1", "title": "wire" },
        }),
        "Event.Payload wire shape"
    );
}
