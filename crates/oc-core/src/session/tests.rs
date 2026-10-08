//! Ported unit tests for the session lane.
//!
//! Sources: `packages/core/test/session-{store,projector,create,execution,
//! run-coordinator}.test.ts` plus the inbox-promotion semantics those files
//! exercise directly. Test names carry the `session_` prefix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::assert_is_empty, // assert!(!x.is_empty()) reads clearer with a message
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::items_after_statements,
    clippy::type_complexity, // coordinator hook types match the lib's
    clippy::redundant_closure_for_method_calls,
)]
#![allow(clippy::too_many_lines)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use oc_schema::id::{EventId, MessageId, SessionId};
use oc_schema::location;
use oc_schema::session_event::InterruptReason;
use oc_schema::session_inbox::{Delivery, Item, SyntheticPayload, UserPayload};
use oc_util::determinism::{Clock, FixedClock, SequentialIdGen};
use rusqlite::OptionalExtension;

use crate::bus::{Bus, BusOptions, PublishOptions, ReplayOptions, SerializedEvent};
use crate::database::schema::{
    InstructionStateRow, ProjectRow, SessionInboxRow, SessionMessageRow, SessionRow,
};
use crate::database::Database;
use crate::session::execution::{
    CancelJobs, CoordinatorOptions, DrainFn, DrainOutcome, InterruptOptions, RunError,
    SessionExecution, SessionRunCoordinator, Terminal,
};
use crate::session::inbox::{InboxError, Promotable, Scope, SessionInbox};
use crate::session::projector::{fork_title, install_projectors};
use crate::session::store::{
    attempts, claims, CreateInput, CreateOutcome, ListAnchor, ListDirection, ListInput, ListOrder,
    MessagesInput, ResolvedProject, SessionStore,
};

const NOW_MS: u64 = 1_725_000_000_000;

fn fixed_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock::new(std::time::UNIX_EPOCH + Duration::from_millis(NOW_MS)))
}

struct Stack {
    db: Database,
    bus: Bus,
    store: SessionStore,
    inbox: SessionInbox,
}

async fn stack() -> Stack {
    let dir = tempfile::Builder::new()
        .prefix("oc-core-session-")
        .tempdir_in("/tmp/opencode")
        .expect("temp dir under /tmp/opencode");
    let db = Database::open_file(dir.path().join("opencode.db")).await.expect("open db");
    // Hold the tempdir for the test's process lifetime.
    std::mem::forget(dir);
    let options = BusOptions {
        clock: fixed_clock(),
        ids: Arc::new(SequentialIdGen::new()),
        persist: true,
        log_read_page_size: 512,
    };
    let bus = Bus::new(db.clone(), options);
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());
    Stack { db, bus, store, inbox }
}

fn def(type_: &str) -> &'static crate::bus::EventDefinition {
    crate::bus::registry::by_type_latest(type_)
        .unwrap_or_else(|| panic!("no definition for {type_}"))
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

async fn seed_session_raw(db: &Database, id: &str) {
    seed_session_with(db, id, &[]).await;
}

async fn seed_session_with(db: &Database, id: &str, extra: &[(&str, String)]) {
    let id = id.to_string();
    let title = id.clone();
    let extra: Vec<(String, String)> =
        extra.iter().map(|(column, value)| ((*column).to_string(), value.clone())).collect();
    db.transaction(move |tx| {
        ProjectRow::insert_or_ignore(
            tx,
            &ProjectRow::new("global", "/project", None).expect("project row"),
        )?;
        let mut row =
            SessionRow::new(id, "global", "/project", &title, "test").expect("session row");
        row.title = Some(title);
        for (column, value) in &extra {
            match column.as_str() {
                "time_suspended" => row.time_suspended = Some(value.parse::<i64>().unwrap()),
                "resume_attempts" => row.resume_attempts = value.parse::<i64>().unwrap(),
                "parent_id" => row.parent_id = Some(value.clone()),
                "agent" => row.agent = Some(value.clone()),
                "model" => row.model = Some(value.clone()),
                _ => {}
            }
        }
        SessionRow::insert_or_ignore(tx, &row)
    })
    .await
    .expect("seed session");
}

#[derive(Clone)]
struct SeedItem {
    type_: &'static str,
    payload: serde_json::Value,
    delivery: &'static str,
}

fn user_item() -> SeedItem {
    SeedItem {
        type_: "user",
        payload: serde_json::json!({ "text": "queued prompt" }),
        delivery: "steer",
    }
}

fn compaction_item(delivery: &'static str) -> SeedItem {
    SeedItem { type_: "compaction", payload: serde_json::json!({}), delivery }
}

/// Direct pending-row seed (`seedInbox`, session-execution.test.ts:1258).
async fn seed_inbox(db: &Database, session_id: &str, items: &[SeedItem]) {
    let rows: Vec<SessionInboxRow> = items
        .iter()
        .enumerate()
        .map(|(index, item)| SessionInboxRow {
            id: format!("msg_seed_{index}_{session_id}"),
            session_id: session_id.to_string(),
            type_: item.type_.to_string(),
            payload: serde_json::to_string(&item.payload).unwrap(),
            delivery: item.delivery.to_string(),
            enqueued_seq: (index + 1) as i64,
            time_created: 0,
        })
        .collect();
    db.transaction(move |tx| {
        for row in rows {
            SessionInboxRow::insert_or_ignore(tx, &row)?;
        }
        Ok(())
    })
    .await
    .expect("seed inbox");
}

async fn session_column(db: &Database, session_id: &str, column: &str) -> String {
    let (session, column_owned) = (session_id.to_string(), column.to_string());
    db.transaction(move |tx| {
        let text: Option<String> = tx
            .conn()
            .query_row(
                &format!("SELECT CAST({column_owned} AS TEXT) FROM session_v2 WHERE id = ?1"),
                rusqlite::params![session],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        Ok(text.unwrap_or_else(|| "NULL".to_string()))
    })
    .await
    .unwrap()
}

async fn message_data(db: &Database, id: &str) -> String {
    let id = id.to_string();
    db.transaction(move |tx| Ok(SessionMessageRow::get(tx, id)?.map(|row| row.data)))
        .await
        .unwrap()
        .unwrap_or_default()
}

fn message_text(row: &SessionMessageRow) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&row.data).ok()?;
    value.get("text").and_then(serde_json::Value::as_str).map(str::to_string)
}

/// Seeds assistant rows (projector.test.ts `assistantRow`).
async fn seed_messages(db: &Database, session_id: &str, rows: &[(&str, i64, bool)]) {
    let rows: Vec<(String, i64, bool)> =
        rows.iter().map(|(id, seq, completed)| ((*id).to_string(), *seq, *completed)).collect();
    let session = session_id.to_string();
    db.transaction(move |tx| {
        for (id, seq, completed) in rows {
            let mut data = serde_json::json!({
                "agent": "build",
                "model": { "id": "model", "providerID": "provider" },
                "content": [],
                "time": { "created": 0 },
            });
            if completed {
                data["time"]["completed"] = serde_json::json!(2);
            }
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id,
                    session_id: session.clone(),
                    type_: "assistant".to_string(),
                    seq,
                    time_created: 0,
                    time_updated: 0,
                    data: data.to_string(),
                },
            )?;
        }
        Ok(())
    })
    .await
    .unwrap();
}

trait InboxInfoExt {
    fn id(&self) -> MessageId;
    fn delivery(&self) -> Delivery;
}

impl InboxInfoExt for oc_schema::session_inbox::Info {
    fn id(&self) -> MessageId {
        match self {
            oc_schema::session_inbox::Info::User { id, .. }
            | oc_schema::session_inbox::Info::Synthetic { id, .. }
            | oc_schema::session_inbox::Info::Compaction { id, .. }
            | oc_schema::session_inbox::Info::Move { id, .. } => id.clone(),
        }
    }

    fn delivery(&self) -> Delivery {
        match self {
            oc_schema::session_inbox::Info::User { delivery, .. }
            | oc_schema::session_inbox::Info::Synthetic { delivery, .. }
            | oc_schema::session_inbox::Info::Compaction { delivery, .. }
            | oc_schema::session_inbox::Info::Move { delivery, .. } => *delivery,
        }
    }
}

// ===========================================================================
// SessionStore (session-store.test.ts)
// ===========================================================================

/// `lists by updated time and ID with exclusive two-item pages in either direction`.
#[tokio::test]
async fn session_store_lists_by_updated_and_id_with_exclusive_pages() {
    let stack = stack().await;
    let seed = [
        ("ses_d", 20i64),
        ("ses_z", 10),
        ("ses_a", 30),
        ("ses_c", 20),
        ("ses_y", 10),
        ("ses_e", 30),
        ("ses_b", 20),
    ];
    for (id, updated) in seed {
        seed_session_raw(&stack.db, id).await;
        stack
            .bus
            .replay(
                SerializedEvent {
                    id: EventId::parse(format!("evt_store_{id}")).unwrap(),
                    type_: "session.renamed.1".to_string(),
                    created: Some(updated as u64),
                    seq: 0,
                    aggregate_id: id.to_string(),
                    data: serde_json::json!({ "sessionID": id, "title": id }),
                },
                ReplayOptions::default(),
            )
            .await
            .expect("replay renamed");
    }

    let ids = |rows: &[SessionRow]| rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();

    let all = stack.store.list(ListInput::default()).await.unwrap();
    assert_eq!(ids(&all), vec!["ses_e", "ses_a", "ses_d", "ses_c", "ses_b", "ses_z", "ses_y"]);
    let ascending = stack
        .store
        .list(ListInput { order: Some(ListOrder::Asc), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(
        ids(&ascending),
        vec!["ses_y", "ses_z", "ses_b", "ses_c", "ses_d", "ses_a", "ses_e"]
    );

    let pages: [(ListOrder, ListDirection, Vec<&str>); 4] = [
        (ListOrder::Asc, ListDirection::Next, vec!["ses_d", "ses_a"]),
        (ListOrder::Asc, ListDirection::Previous, vec!["ses_z", "ses_b"]),
        (ListOrder::Desc, ListDirection::Next, vec!["ses_b", "ses_z"]),
        (ListOrder::Desc, ListDirection::Previous, vec!["ses_a", "ses_d"]),
    ];
    for (order, direction, expected) in pages {
        let page = stack
            .store
            .list(ListInput {
                limit: Some(2),
                order: Some(order),
                anchor: Some(ListAnchor { id: "ses_c".to_string(), time: 20, direction }),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(ids(&page), expected, "{order:?} {direction:?}");
    }
}

/// `pages messages by durable sequence, not timestamp or ID, and scopes
/// cursor lookup`.
#[tokio::test]
async fn session_store_pages_messages_by_durable_sequence() {
    let stack = stack().await;
    let session_id = "ses_messages";
    let foreign_id = "ses_foreign";
    seed_session_raw(&stack.db, session_id).await;
    seed_session_raw(&stack.db, foreign_id).await;

    let order = [
        ("evt_z", 300u64),
        ("evt_b", 700),
        ("evt_x", 100),
        ("evt_c", 400),
        ("evt_w", 200),
        ("evt_a", 600),
        ("evt_y", 500),
    ];
    for (index, (id, created)) in order.iter().enumerate() {
        stack
            .bus
            .replay(
                SerializedEvent {
                    id: EventId::parse(*id).unwrap(),
                    type_: "session.synthetic.1".to_string(),
                    created: Some(*created),
                    seq: index as i64,
                    aggregate_id: session_id.to_string(),
                    data: serde_json::json!({ "sessionID": session_id, "text": id }),
                },
                ReplayOptions::default(),
            )
            .await
            .expect("replay synthetic");
    }
    stack
        .bus
        .publish(
            def("session.synthetic"),
            serde_json::json!({ "sessionID": foreign_id, "text": "foreign" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    let ids =
        |rows: &[SessionMessageRow]| rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();

    let all = stack
        .store
        .messages(MessagesInput { session_id: session_id.into(), ..Default::default() })
        .await
        .unwrap();
    assert_eq!(ids(&all), vec!["msg_y", "msg_a", "msg_w", "msg_c", "msg_x", "msg_b", "msg_z"]);
    let ascending = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            order: Some(ListOrder::Asc),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        ids(&ascending),
        vec!["msg_z", "msg_b", "msg_x", "msg_c", "msg_w", "msg_a", "msg_y"]
    );

    let pages: [(ListOrder, ListDirection, Vec<&str>); 4] = [
        (ListOrder::Asc, ListDirection::Next, vec!["msg_w", "msg_a"]),
        (ListOrder::Asc, ListDirection::Previous, vec!["msg_b", "msg_x"]),
        (ListOrder::Desc, ListDirection::Next, vec!["msg_x", "msg_b"]),
        (ListOrder::Desc, ListDirection::Previous, vec!["msg_a", "msg_w"]),
    ];
    for (order, direction, expected) in pages {
        let page = stack
            .store
            .messages(MessagesInput {
                session_id: session_id.into(),
                order: Some(order),
                limit: Some(2),
                cursor: Some(("msg_c".to_string(), direction)),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(ids(&page), expected, "{order:?} {direction:?}");
    }

    let missing = stack
        .store
        .messages(MessagesInput { session_id: "ses_missing".into(), ..Default::default() })
        .await
        .unwrap();
    assert!(missing.is_empty());
    let bad_cursor = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            cursor: Some(("msg_missing".to_string(), ListDirection::Next)),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(bad_cursor.is_empty());
    let foreign_cursor = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            order: Some(ListOrder::Asc),
            cursor: Some(("msg_foreign".to_string(), ListDirection::Next)),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(foreign_cursor.is_empty(), "cursor lookup is session-scoped");
}

// ===========================================================================
// SessionInbox promotion semantics (runner-map §1.2)
// ===========================================================================

#[tokio::test]
async fn session_inbox_promotes_steers_before_queued_input() {
    let stack = stack().await;
    let session_id = "ses_promote_steers";
    seed_session_raw(&stack.db, session_id).await;
    seed_inbox(
        &stack.db,
        session_id,
        &[user_item(), SeedItem { delivery: "queue", ..user_item() }],
    )
    .await;

    let next = stack.inbox.next_promotable(session_id, Promotable::Input).await.unwrap().unwrap();
    assert!(matches!(next, oc_schema::session_inbox::Info::User { .. }));

    let promoted = stack.inbox.promote(session_id, Promotable::Input).await.unwrap().unwrap();
    assert_eq!(promoted, 1, "only the steer row promotes");
    let remaining = stack.inbox.list(session_id).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].delivery(), Delivery::Queue);
}

#[tokio::test]
async fn session_inbox_promote_returns_none_when_control_owns_boundary() {
    let stack = stack().await;
    let session_id = "ses_promote_control";
    seed_session_raw(&stack.db, session_id).await;
    seed_inbox(&stack.db, session_id, &[compaction_item("steer")]).await;
    assert_eq!(stack.inbox.promote(session_id, Promotable::Input).await.unwrap(), None);

    let queued = "ses_promote_control_q";
    seed_session_raw(&stack.db, queued).await;
    seed_inbox(&stack.db, queued, &[compaction_item("queue")]).await;
    assert_eq!(stack.inbox.promote(queued, Promotable::Input).await.unwrap(), None);
}

#[tokio::test]
async fn session_inbox_splices_front_compaction_onto_the_steer_batch() {
    let stack = stack().await;
    let session_id = "ses_promote_splice";
    seed_session_raw(&stack.db, session_id).await;
    // [user steer, compaction steer]: pendingSteers splices the compaction
    // to the front, so the control owns the boundary.
    seed_inbox(&stack.db, session_id, &[user_item(), compaction_item("steer")]).await;
    let next = stack.inbox.next_promotable(session_id, Promotable::Input).await.unwrap().unwrap();
    assert!(matches!(next, oc_schema::session_inbox::Info::Compaction { .. }));
    assert_eq!(stack.inbox.promote(session_id, Promotable::Input).await.unwrap(), None);
}

#[tokio::test]
async fn session_inbox_never_pulls_compaction_across_a_move() {
    let stack = stack().await;
    let session_id = "ses_promote_move";
    seed_session_raw(&stack.db, session_id).await;
    seed_inbox(
        &stack.db,
        session_id,
        &[
            user_item(),
            SeedItem {
                type_: "move",
                payload: serde_json::json!({
                    "location": { "directory": "/other" },
                    "projectID": "global",
                }),
                delivery: "steer",
            },
        ],
    )
    .await;
    let promoted = stack.inbox.promote(session_id, Promotable::Input).await.unwrap().unwrap();
    assert_eq!(promoted, 1);
    let remaining = stack.inbox.list(session_id).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert!(matches!(remaining[0], oc_schema::session_inbox::Info::Move { .. }));
}

#[tokio::test]
async fn session_inbox_promotes_queued_then_arrived_steers() {
    let stack = stack().await;
    let session_id = "ses_promote_mixed";
    seed_session_raw(&stack.db, session_id).await;
    // Queued input alone: the input scope delivers it, and the arrival
    // re-read finds nothing more.
    seed_inbox(&stack.db, session_id, &[SeedItem { delivery: "queue", ..user_item() }]).await;
    let promoted = stack.inbox.promote(session_id, Promotable::Input).await.unwrap().unwrap();
    assert_eq!(promoted, 1);
    assert!(stack.inbox.list(session_id).await.unwrap().is_empty());

    // Steers present ⇒ the steer branch wins; the queued row parks even at
    // the idle boundary (promote only delivers one queued row when NO
    // steers wait).
    let mixed = "ses_promote_mixed2";
    seed_session_raw(&stack.db, mixed).await;
    seed_inbox(
        &stack.db,
        mixed,
        &[
            SeedItem { delivery: "queue", ..user_item() },
            SeedItem {
                type_: "synthetic",
                payload: serde_json::json!({ "text": "arrived" }),
                delivery: "steer",
            },
        ],
    )
    .await;
    let promoted = stack.inbox.promote(mixed, Promotable::Input).await.unwrap().unwrap();
    assert_eq!(promoted, 1, "steers deliver first; queued stays parked");
    let remaining = stack.inbox.list(mixed).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].delivery(), Delivery::Queue);
}

#[tokio::test]
async fn session_inbox_steer_scope_parks_queued_input() {
    let stack = stack().await;
    let session_id = "ses_promote_parked";
    seed_session_raw(&stack.db, session_id).await;
    seed_inbox(&stack.db, session_id, &[SeedItem { delivery: "queue", ..user_item() }]).await;
    let promoted = stack.inbox.promote(session_id, Promotable::Steer).await.unwrap().unwrap();
    assert_eq!(promoted, 0);
    assert_eq!(stack.inbox.list(session_id).await.unwrap().len(), 1);
    assert!(stack.inbox.has(session_id, Scope::Input).await.unwrap());
    assert!(!stack.inbox.has(session_id, Scope::Steer).await.unwrap());
}

#[tokio::test]
async fn session_inbox_cancel_and_delivery_change_are_guarded() {
    let stack = stack().await;
    let session_id = "ses_inbox_mutations";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .inbox
        .admit(
            "msg_mutate",
            session_id,
            Item::User {
                payload: UserPayload { text: "hello".into(), ..Default::default() },
                delivery: Delivery::Queue,
            },
        )
        .await
        .expect("admit");

    stack.inbox.steer(session_id, "msg_mutate").await.expect("steer");
    let steered = stack.inbox.find("msg_mutate").await.unwrap().unwrap();
    assert_eq!(steered.delivery(), Delivery::Steer);
    assert!(matches!(
        stack.inbox.steer(session_id, "msg_mutate").await,
        Err(InboxError::Conflict(_))
    ));

    stack.inbox.cancel(session_id, "msg_mutate").await.expect("cancel");
    assert!(stack.inbox.find("msg_mutate").await.unwrap().is_none());
    assert!(matches!(
        stack.inbox.cancel(session_id, "msg_mutate").await,
        Err(InboxError::Conflict(_))
    ));
}

#[tokio::test]
async fn session_inbox_admit_is_idempotent_per_session_and_type() {
    let stack = stack().await;
    let session_id = "ses_admit_idem";
    seed_session_raw(&stack.db, session_id).await;
    let item = Item::User {
        payload: UserPayload { text: "first".into(), ..Default::default() },
        delivery: Delivery::Steer,
    };
    let first = stack.inbox.admit("msg_idem", session_id, item.clone()).await.expect("admit");
    let second = stack.inbox.admit("msg_idem", session_id, item).await.expect("re-admit");
    assert_eq!(first, second);

    let mismatched = stack
        .inbox
        .admit(
            "msg_idem",
            session_id,
            Item::Synthetic { payload: SyntheticPayload::default(), delivery: Delivery::Steer },
        )
        .await;
    assert!(matches!(mismatched, Err(InboxError::Conflict(_))));

    let other = "ses_admit_other";
    seed_session_raw(&stack.db, other).await;
    let foreign = stack
        .inbox
        .admit(
            "msg_idem",
            other,
            Item::User { payload: UserPayload::default(), delivery: Delivery::Steer },
        )
        .await;
    assert!(matches!(foreign, Err(InboxError::Conflict(_))));
}

#[tokio::test]
async fn session_inbox_reconciles_promoted_from_message() {
    let stack = stack().await;
    let session_id = "ses_promoted_from_message";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .inbox
        .admit(
            "msg_promoted",
            session_id,
            Item::User {
                payload: UserPayload { text: "delivered".into(), ..Default::default() },
                delivery: Delivery::Steer,
            },
        )
        .await
        .expect("admit");
    stack
        .bus
        .publish(
            def("session.inbox.delivered"),
            serde_json::json!({ "sessionID": session_id, "inboxID": "msg_promoted" }),
            PublishOptions::default(),
        )
        .await
        .expect("deliver");
    assert!(stack.inbox.find("msg_promoted").await.unwrap().is_none());

    // Re-admitting the same id reconciles through the projected message.
    let reconciled = stack
        .inbox
        .admit(
            "msg_promoted",
            session_id,
            Item::User { payload: UserPayload::default(), delivery: Delivery::Queue },
        )
        .await
        .expect("reconcile");
    assert_eq!(reconciled.id().as_str(), "msg_promoted");
    assert_eq!(reconciled.delivery(), Delivery::Queue);
}

#[tokio::test]
async fn session_inbox_admit_compaction_reuses_pending() {
    let stack = stack().await;
    let session_id = "ses_compaction_reuse";
    seed_session_raw(&stack.db, session_id).await;
    let first = stack
        .inbox
        .admit_compaction("msg_compact", session_id, Delivery::Queue)
        .await
        .expect("admit compaction");
    assert!(matches!(first, oc_schema::session_inbox::Info::Compaction { .. }));
    let reused = stack
        .inbox
        .admit_compaction("msg_compact_other", session_id, Delivery::Queue)
        .await
        .expect("reuse");
    assert_eq!(reused.id().as_str(), "msg_compact");
}

// ===========================================================================
// SessionProjector (session-projector.test.ts)
// ===========================================================================

/// `consumes the pending row and projects the message at promotion`.
#[tokio::test]
async fn session_projector_consumes_pending_row_at_promotion() {
    let stack = stack().await;
    let session_id = "ses_projector_test";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .inbox
        .admit(
            "msg_admitted",
            session_id,
            Item::User {
                payload: UserPayload { text: "promote me".into(), ..Default::default() },
                delivery: Delivery::Steer,
            },
        )
        .await
        .expect("admit");

    let event = stack
        .bus
        .publish(
            def("session.inbox.delivered"),
            serde_json::json!({ "sessionID": session_id, "inboxID": "msg_admitted" }),
            PublishOptions::default(),
        )
        .await
        .expect("deliver");

    assert!(stack.inbox.find("msg_admitted").await.unwrap().is_none());
    let row = stack
        .db
        .transaction(|tx| SessionMessageRow::get(tx, "msg_admitted".to_string()))
        .await
        .unwrap()
        .expect("message projected");
    assert_eq!(row.session_id, session_id);
    assert_eq!(row.type_, "user");
    assert_eq!(row.seq, event.seq().unwrap());
}

/// `does not settle a pending manual compaction on an auto failure`.
#[tokio::test]
async fn session_projector_no_settle_pending_manual_compaction_on_auto_failure() {
    let stack = stack().await;
    let session_id = "ses_projector_auto_fail";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .inbox
        .admit_compaction("msg_manual_compaction", session_id, Delivery::Queue)
        .await
        .expect("admit compaction");

    stack
        .bus
        .publish(
            def("session.compaction.failed"),
            serde_json::json!({
                "sessionID": session_id,
                "reason": "auto",
                "error": { "type": "compaction.failed", "message": "Auto compaction failed" },
            }),
            PublishOptions::default(),
        )
        .await
        .expect("publish failure");

    let pending = stack.inbox.find("msg_manual_compaction").await.unwrap().expect("still pending");
    assert_eq!(pending.id().as_str(), "msg_manual_compaction");
}

/// `projects staged, cleared, and committed reverts`.
#[tokio::test]
async fn session_projector_projects_reverts() {
    let stack = stack().await;
    let session_id = "ses_projector_revert";
    seed_session_with(&stack.db, session_id, &[]).await;
    seed_messages(
        &stack.db,
        session_id,
        &[("msg_earlier", 0, false), ("msg_boundary", 1, true), ("msg_later", 2, true)],
    )
    .await;
    stack
        .db
        .transaction(|tx| {
            InstructionStateRow::insert(
                tx,
                &InstructionStateRow {
                    session_id: session_id.to_string(),
                    epoch_start: 0,
                    through_seq: 0,
                    initial_values: "{}".into(),
                    current_values: "{}".into(),
                },
            )
        })
        .await
        .unwrap();

    stack
        .bus
        .publish(
            def("session.revert.staged"),
            serde_json::json!({
                "sessionID": session_id,
                "revert": { "messageID": "msg_boundary", "snapshot": "tree", "files": [] },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    let revert = session_column(&stack.db, session_id, "revert").await;
    let revert_json: serde_json::Value = serde_json::from_str(&revert).unwrap();
    assert_eq!(revert_json["messageID"], "msg_boundary");
    assert_eq!(revert_json["files"], serde_json::json!([]));

    stack
        .bus
        .publish(
            def("session.revert.cleared"),
            serde_json::json!({ "sessionID": session_id }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(session_column(&stack.db, session_id, "revert").await, "NULL");

    stack
        .bus
        .publish(
            def("session.revert.staged"),
            serde_json::json!({
                "sessionID": session_id,
                "revert": { "messageID": "msg_boundary", "files": [] },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.revert.committed"),
            serde_json::json!({ "sessionID": session_id, "to": "msg_boundary" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    let remaining = stack
        .db
        .transaction(move |tx| {
            let mut statement = tx.conn().prepare("SELECT id FROM session_message ORDER BY seq")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    assert_eq!(remaining, vec!["msg_earlier"]);
    let state = stack
        .db
        .transaction(move |tx| InstructionStateRow::get(tx, session_id.to_string()))
        .await
        .unwrap();
    assert!(state.is_none(), "committed revert resets the instruction epoch");
}

/// `orders projected messages and context by durable aggregate sequence`.
#[tokio::test]
async fn session_projector_orders_messages_by_durable_sequence() {
    let stack = stack().await;
    let session_id = "ses_projector_order";
    seed_session_raw(&stack.db, session_id).await;
    for (id, text) in [("msg_first", "first"), ("msg_second", "second")] {
        stack
            .bus
            .publish(
                def("session.inbox.enqueued"),
                serde_json::json!({
                    "sessionID": session_id,
                    "inboxID": id,
                    "item": { "type": "user", "payload": { "text": text }, "delivery": "steer" },
                }),
                PublishOptions::default(),
            )
            .await
            .unwrap();
        stack
            .bus
            .publish(
                def("session.inbox.delivered"),
                serde_json::json!({ "sessionID": session_id, "inboxID": id }),
                PublishOptions::default(),
            )
            .await
            .unwrap();
    }

    let first_page = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            limit: Some(1),
            order: Some(ListOrder::Asc),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first_page.len(), 1);
    assert_eq!(message_text(&first_page[0]), Some("first".into()));
    let second_page = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            limit: Some(1),
            order: Some(ListOrder::Asc),
            cursor: Some((first_page[0].id.clone(), ListDirection::Next)),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(message_text(&second_page[0]), Some("second".into()));
    let back = stack
        .store
        .messages(MessagesInput {
            session_id: session_id.into(),
            limit: Some(1),
            order: Some(ListOrder::Asc),
            cursor: Some((second_page[0].id.clone(), ListDirection::Previous)),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(message_text(&back[0]), Some("first".into()));
}

/// `projects durable context messages supported by the updater`.
#[tokio::test]
async fn session_projector_projects_durable_context_messages() {
    let stack = stack().await;
    let session_id = "ses_projector_context";
    seed_session_with(
        &stack.db,
        session_id,
        &[
            ("agent", "plan".to_string()),
            (
                "model",
                serde_json::json!({ "id": "model", "providerID": "provider", "variant": "medium" })
                    .to_string(),
            ),
        ],
    )
    .await;

    stack
        .bus
        .publish(
            def("session.agent.selected"),
            serde_json::json!({ "sessionID": session_id, "agent": "build" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.model.selected"),
            serde_json::json!({
                "sessionID": session_id,
                "model": { "id": "model", "providerID": "provider" },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.synthetic"),
            serde_json::json!({
                "sessionID": session_id,
                "text": "synthetic context",
                "metadata": { "source": "projector-test" },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .replay(
            SerializedEvent {
                id: EventId::parse("evt_shell_replay").unwrap(),
                type_: "session.shell.started.1".to_string(),
                created: Some(0),
                seq: 3,
                aggregate_id: session_id.to_string(),
                data: serde_json::json!({
                    "sessionID": session_id,
                    "shell": {
                        "id": "sh_projector",
                        "status": "running",
                        "command": "pwd",
                        "cwd": "/project",
                        "shell": "/bin/sh",
                        "file": "/tmp/sh_projector.out",
                        "metadata": { "background": true },
                        "time": { "started": 0 },
                    },
                }),
            },
            ReplayOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.shell.ended"),
            serde_json::json!({
                "sessionID": session_id,
                "shell": {
                    "id": "sh_projector",
                    "status": "exited",
                    "command": "pwd",
                    "cwd": "/project",
                    "shell": "/bin/sh",
                    "file": "/tmp/sh_projector.out",
                    "exit": 0,
                    "metadata": {},
                    "time": { "started": 0, "completed": 1 },
                },
                "output": { "output": "/project", "cursor": 8, "size": 8, "truncated": false },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.compaction.started"),
            serde_json::json!({
                "sessionID": session_id,
                "reason": "manual",
                "recent": "recent context",
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.compaction.ended"),
            serde_json::json!({
                "sessionID": session_id,
                "reason": "manual",
                "text": "summary",
                "recent": "recent context",
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    let rows =
        stack.db.transaction(move |tx| tx.session_messages(session_id, None, 100)).await.unwrap();
    let types: Vec<&str> = rows.iter().map(|row| row.type_.as_str()).collect();
    assert_eq!(types, vec!["agent-switched", "model-switched", "synthetic", "shell", "compaction"]);
    assert!(rows[0].data.contains("\"build\""));
    assert!(rows[0].data.contains("\"plan\""), "previous agent from the session row");
    assert!(rows[1].data.contains("medium"), "previous model variant from the session row");
    assert!(rows[3].data.contains("\"exited\""));
    assert!(rows[3].data.contains("\"background\":true"));
    assert!(rows[4].data.contains("\"summary\":\"summary\""));
    assert_eq!(session_column(&stack.db, session_id, "agent").await, "build");
}

/// `rejects distinct creator events that reuse one projected message ID`.
#[tokio::test]
async fn session_projector_rejects_message_id_reuse() {
    let stack = stack().await;
    let session_id = "ses_projector_collision";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .db
        .transaction(|tx| {
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_creator_collision".to_string(),
                    session_id: session_id.to_string(),
                    type_: "synthetic".to_string(),
                    seq: 0,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"text":"existing","time":{"created":0}}"#.to_string(),
                },
            )
        })
        .await
        .unwrap();

    let result = stack
        .bus
        .publish(
            def("session.step.started"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_creator_collision",
                "agent": "build",
                "model": { "id": "model", "providerID": "provider" },
                "started": 0,
            }),
            PublishOptions::default(),
        )
        .await;
    assert!(result.is_err(), "duplicate creator ids must fail the publish");
    let row = stack
        .db
        .transaction(|tx| SessionMessageRow::get(tx, "msg_creator_collision".to_string()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.type_, "synthetic", "rolled back");
}

/// `projects retry state and clears it at the next step or execution terminal`.
#[tokio::test]
async fn session_projector_projects_and_clears_retry_state() {
    let stack = stack().await;
    let session_id = "ses_projector_retry";
    seed_session_raw(&stack.db, session_id).await;
    let model = serde_json::json!({ "id": "model", "providerID": "provider" });
    let start = |id: &'static str| {
        let model = model.clone();
        move |stack: &Stack| {
            let data = serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": id,
                "agent": "build",
                "model": model,
                "started": 0,
            });
            let bus = stack.bus.clone();
            async move {
                bus.publish(def("session.step.started"), data, PublishOptions::default())
                    .await
                    .unwrap();
            }
        }
    };
    let retry = |id: &'static str, attempt: u64, at: u64| {
        let data = serde_json::json!({
            "sessionID": session_id,
            "assistantMessageID": id,
            "attempt": attempt,
            "at": at,
            "error": { "type": "provider.internal", "message": "Unavailable" },
        });
        let bus = stack.bus.clone();
        async move {
            bus.publish(def("session.retry.scheduled"), data, PublishOptions::default())
                .await
                .unwrap();
        }
    };
    // TS order: start first → retry first → start second → retry second.
    start("msg_retry_first")(&stack).await;
    retry("msg_retry_first", 2, 2_000).await;
    assert!(message_data(&stack.db, "msg_retry_first").await.contains("\"retry\""));
    start("msg_retry_second")(&stack).await;
    retry("msg_retry_second", 3, 6_000).await;
    stack
        .bus
        .publish(
            def("session.execution.interrupted"),
            serde_json::json!({ "sessionID": session_id, "reason": "shutdown" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    assert!(!message_data(&stack.db, "msg_retry_first").await.contains("\"retry\""));
    assert!(!message_data(&stack.db, "msg_retry_second").await.contains("\"retry\""));
}

/// `updates only the newest incomplete assistant projection` + usage totals
/// + the `session.usage.updated` fan-out.
#[tokio::test]
async fn session_projector_updates_only_newest_incomplete_assistant() {
    let stack = stack().await;
    let session_id = "ses_projector_newest";
    seed_session_raw(&stack.db, session_id).await;
    seed_messages(
        &stack.db,
        session_id,
        &[("msg_assistant_1", 0, false), ("msg_assistant_2", 1, false)],
    )
    .await;

    let mut usage_stream = stack.bus.subscribe_type("session.usage.updated");
    stack
        .bus
        .publish(
            def("session.step.ended"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_assistant_2",
                "finish": "stop",
                "cost": 1.25,
                "tokens": { "input": 10, "output": 4, "reasoning": 2, "cache": { "read": 3, "write": 1 } },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    assert!(
        !message_data(&stack.db, "msg_assistant_1").await.contains("completed"),
        "older row untouched"
    );
    let second = message_data(&stack.db, "msg_assistant_2").await;
    assert!(second.contains("\"finish\":\"stop\""));
    assert!(second.contains("completed"));
    assert_eq!(session_column(&stack.db, session_id, "cost").await, "1.25");
    assert_eq!(session_column(&stack.db, session_id, "tokens_input").await, "10");
    assert_eq!(session_column(&stack.db, session_id, "tokens_cache_write").await, "1");

    let usage = tokio::time::timeout(Duration::from_secs(2), usage_stream.next())
        .await
        .expect("usage fan-out within 2s")
        .expect("stream open");
    assert_eq!(usage.data["sessionID"], session_id);
    assert_eq!(usage.data["cost"].as_f64(), Some(1.25));
}

/// `projects ended and failed step terminal state`.
#[tokio::test]
async fn session_projector_ended_and_failed_step_terminal_state() {
    let stack = stack().await;
    let session_id = "ses_projector_terminals";
    seed_session_raw(&stack.db, session_id).await;
    seed_messages(&stack.db, session_id, &[("msg_ended", 0, false), ("msg_failed", 1, false)])
        .await;

    stack
        .bus
        .publish(
            def("session.step.streamed"),
            serde_json::json!({ "sessionID": session_id, "assistantMessageID": "msg_ended" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.step.ended"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_ended",
                "finish": "stop",
                "rawFinish": "stop_sequence",
                "providerState": { "response": "ended" },
                "cost": 1,
                "tokens": { "input": 2, "output": 3, "reasoning": 4, "cache": { "read": 5, "write": 6 } },
                "snapshot": "snap_ended",
                "files": ["src/ended.ts"],
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.step.failed"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_failed",
                "error": { "type": "provider.invalid-request", "message": "Failed" },
                "snapshot": "snap_failed",
                "files": ["src/failed.ts"],
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();

    let ended = message_data(&stack.db, "msg_ended").await;
    assert!(ended.contains("\"rawFinish\":\"stop_sequence\""));
    assert!(ended.contains("\"end\":\"snap_ended\""));
    assert!(ended.contains("src/ended.ts"));
    assert!(ended.contains("\"streamed\""));
    let failed = message_data(&stack.db, "msg_failed").await;
    assert!(failed.contains("provider.invalid-request"));
    assert!(failed.contains("\"end\":\"snap_failed\""));
}

/// `does not revive a stale incomplete assistant projection`: step.started
/// with an existing assistant id RESETS that row instead of appending.
#[tokio::test]
async fn session_projector_step_started_reuses_existing_assistant() {
    let stack = stack().await;
    let session_id = "ses_projector_reuse";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .bus
        .publish(
            def("session.step.started"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_reused",
                "agent": "build",
                "model": { "id": "model", "providerID": "provider" },
                "started": 7,
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.text.started"),
            serde_json::json!({ "sessionID": session_id, "assistantMessageID": "msg_reused", "ordinal": 0 }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    // A retry mark, then a fresh step on the same id clears it and resets times.
    stack
        .bus
        .publish(
            def("session.retry.scheduled"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_reused",
                "attempt": 2,
                "at": 10,
                "error": { "type": "provider.transport", "message": "x" },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    stack
        .bus
        .publish(
            def("session.step.started"),
            serde_json::json!({
                "sessionID": session_id,
                "assistantMessageID": "msg_reused",
                "agent": "build",
                "model": { "id": "model", "providerID": "provider" },
                "started": 20,
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    let data = message_data(&stack.db, "msg_reused").await;
    assert!(!data.contains("\"retry\""));
    assert!(data.contains("\"created\":20"));
    assert!(data.contains("\"text\""), "content preserved across the reset");
    let count = stack.db.transaction(|tx| SessionMessageRow::count(tx)).await.unwrap();
    assert_eq!(count, 1, "no duplicate projection");
}

/// Shutdown interruption skips the idle marker; failure records one.
#[tokio::test]
async fn session_projector_shutdown_interrupt_marks_no_idle() {
    let stack = stack().await;
    let session_id = "ses_projector_shutdown";
    seed_session_raw(&stack.db, session_id).await;
    stack
        .bus
        .publish(
            def("session.execution.interrupted"),
            serde_json::json!({ "sessionID": session_id, "reason": "shutdown" }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(session_column(&stack.db, session_id, "time_idle").await, "NULL");
    assert_eq!(session_column(&stack.db, session_id, "idle_outcome").await, "NULL");

    stack
        .bus
        .publish(
            def("session.execution.failed"),
            serde_json::json!({
                "sessionID": session_id,
                "error": { "type": "unknown", "message": "boom" },
            }),
            PublishOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(session_column(&stack.db, session_id, "idle_outcome").await, "failed");
    assert_ne!(session_column(&stack.db, session_id, "time_idle").await, "NULL");
}

/// `Viewed` monotone watermark.
#[tokio::test]
async fn session_projector_viewed_is_monotone() {
    let stack = stack().await;
    let session_id = "ses_projector_viewed";
    seed_session_raw(&stack.db, session_id).await;
    for idle in [100, 50, 100] {
        stack
            .bus
            .publish(
                def("session.viewed"),
                serde_json::json!({ "sessionID": session_id, "idle": idle }),
                PublishOptions::default(),
            )
            .await
            .unwrap();
    }
    assert_eq!(session_column(&stack.db, session_id, "time_viewed").await, "100");
}

/// `forkTitle` pattern.
#[test]
fn session_projector_fork_title_pattern() {
    assert_eq!(fork_title(None), None);
    assert_eq!(fork_title(Some("plan")), Some("plan (fork #1)".to_string()));
    assert_eq!(fork_title(Some("plan (fork #2)")), Some("plan (fork #3)".to_string()));
}

/// `projectFork` copies settled history below the boundary and reserves the
/// sequence prefix.
#[tokio::test]
async fn session_projector_fork_copies_settled_history() {
    let stack = stack().await;
    let parent = "ses_fork_parent";
    seed_session_with(&stack.db, parent, &[]).await;
    stack
        .db
        .transaction(|tx| {
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_user_1".to_string(),
                    session_id: parent.to_string(),
                    type_: "user".to_string(),
                    seq: 0,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"text":"hi","time":{"created":0}}"#.to_string(),
                },
            )?;
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_incomplete".to_string(),
                    session_id: parent.to_string(),
                    type_: "assistant".to_string(),
                    seq: 1,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"agent":"build","model":{"id":"m","providerID":"p"},"content":[],"time":{"created":0}}"#.to_string(),
                },
            )?;
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_settled".to_string(),
                    session_id: parent.to_string(),
                    type_: "assistant".to_string(),
                    seq: 2,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"agent":"build","model":{"id":"m","providerID":"p"},"content":[],"time":{"created":0,"completed":9}}"#.to_string(),
                },
            )
        })
        .await
        .unwrap();

    let event = stack
        .bus
        .publish(
            def("session.forked"),
            serde_json::json!({
                "sessionID": "ses_fork_child",
                "parentID": parent,
                "boundary": { "type": "through", "messageID": "msg_settled" },
            }),
            PublishOptions::default(),
        )
        .await
        .expect("fork");

    let fork = stack.store.get("ses_fork_child").await.unwrap().expect("fork row");
    assert_eq!(fork.fork_session_id.as_deref(), Some(parent));
    assert_eq!(fork.title.as_deref(), Some("ses_fork_parent (fork #1)"));
    let copied =
        stack.db.transaction(|tx| tx.session_messages("ses_fork_child", None, 100)).await.unwrap();
    // Only settled rows: the user message and the completed assistant.
    assert_eq!(copied.len(), 2, "incomplete assistant stays on the parent");
    let prefix = format!("msg_{}_", event.id.as_str().trim_start_matches("evt_"));
    assert!(copied[0].id.starts_with(&prefix), "{}", copied[0].id);
    let latest = stack.bus.latest_sequence("ses_fork_child").await.unwrap();
    assert_eq!(latest, 2, "reserveSequence covered the copied prefix");
}

// ===========================================================================
// Session.create (session.ts:262-318)
// ===========================================================================

fn create_input(id: Option<SessionId>, title: Option<&str>) -> CreateInput {
    CreateInput {
        id,
        location: Some(location::Ref {
            directory: oc_schema::common::AbsolutePath::parse("/project"),
            workspace_id: None,
        }),
        project: Some(ResolvedProject { id: "global".into(), directory: "/project".into() }),
        title: title.map(str::to_string),
        version: "test".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn session_create_is_idempotent_by_id() {
    let stack = stack().await;
    let id = SessionId::parse("ses_create_idem").unwrap();
    let first = stack
        .store
        .create(&stack.bus, create_input(Some(id.clone()), Some("first")))
        .await
        .unwrap();
    assert!(matches!(first, CreateOutcome::Created(_)));
    let second = stack.store.create(&stack.bus, create_input(Some(id), None)).await.unwrap();
    assert!(matches!(second, CreateOutcome::Existing(_)));
    let count = stack.db.transaction(|tx| SessionRow::count(tx)).await.unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn session_create_recovers_from_projection_race() {
    let stack = stack().await;
    let id = "ses_create_race";
    seed_session_raw(&stack.db, id).await;
    let event = stack
        .bus
        .publish(
            def("session.created"),
            serde_json::json!({
                "sessionID": id,
                "projectID": "global",
                "location": { "directory": "/project" },
                "slug": "race",
                "version": "test",
            }),
            PublishOptions::default(),
        )
        .await;
    assert!(event.is_err(), "duplicate projection dies");
    let outcome = stack
        .store
        .create(&stack.bus, create_input(Some(SessionId::parse(id).unwrap()), None))
        .await
        .unwrap();
    assert!(matches!(outcome, CreateOutcome::Existing(_)));
}

#[tokio::test]
async fn session_create_children_inherit_parent_state() {
    let stack = stack().await;
    let parent = stack
        .store
        .create(
            &stack.bus,
            CreateInput {
                metadata: Some(
                    [("origin".to_string(), serde_json::json!("test"))].into_iter().collect(),
                ),
                ..create_input(None, Some("parent"))
            },
        )
        .await
        .unwrap();
    let CreateOutcome::Created(parent_row) = parent else { panic!("parent created") };

    let child = stack
        .store
        .create(
            &stack.bus,
            CreateInput {
                parent_id: Some(SessionId::parse(parent_row.id.clone()).unwrap()),
                project: Some(ResolvedProject {
                    id: "global".into(),
                    directory: "/project".into(),
                }),
                version: "test".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let CreateOutcome::Created(child_row) = child else { panic!("child created") };
    assert_eq!(child_row.parent_id.as_deref(), Some(parent_row.id.as_str()));
    assert_eq!(child_row.directory, "/project", "child inherits location");
    assert!(child_row.metadata.is_some(), "child inherits metadata");
}

#[tokio::test]
async fn session_create_requires_location_or_parent() {
    let stack = stack().await;
    let result = stack
        .store
        .create(&stack.bus, CreateInput { version: "test".into(), ..Default::default() })
        .await;
    assert!(result.is_err());
    let missing_parent = stack
        .store
        .create(
            &stack.bus,
            CreateInput {
                parent_id: Some(SessionId::parse("ses_missing_parent").unwrap()),
                version: "test".into(),
                ..Default::default()
            },
        )
        .await;
    assert!(missing_parent.is_err());
}

#[tokio::test]
async fn session_remove_publishes_deleted() {
    let stack = stack().await;
    seed_session_raw(&stack.db, "ses_removed").await;
    stack.store.remove(&stack.bus, "ses_removed").await.unwrap();
    assert!(stack.store.get("ses_removed").await.unwrap().is_none());
}

// ===========================================================================
// SessionExecution claim lifecycle (session-execution.test.ts)
// ===========================================================================

#[derive(Default)]
struct CancelCounter {
    cancelled: std::sync::Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl CancelJobs for CancelCounter {
    async fn cancel(&self, session_id: &str) {
        self.cancelled.lock().unwrap().push(session_id.to_string());
    }
}

async fn execution_stack(
    drain: DrainFn,
    jobs: Option<Arc<dyn CancelJobs>>,
) -> (Database, Bus, Arc<SessionExecution>) {
    let stack = stack().await;
    let execution = Arc::new(SessionExecution::new(
        stack.store.clone(),
        stack.bus.clone(),
        stack.inbox.clone(),
        jobs,
        drain,
    ));
    (stack.db, stack.bus, execution)
}

fn hanging_drain() -> DrainFn {
    Arc::new(|_, _, _, _| {
        Box::pin(async {
            std::future::pending::<()>().await;
            unreachable!()
        })
    })
}

/// `classifies success and typed failure terminals` +
/// `defaults owner-scope interruption to shutdown`.
#[test]
fn session_execution_terminal_classification() {
    use crate::session::execution::terminal;
    use oc_schema::common::SessionError;

    assert_eq!(terminal(&Ok(()), None), Terminal::Succeeded);
    let failure = RunError::Failed(SessionError {
        error_type: "provider.transport".into(),
        message: "Disconnected".into(),
        status: None,
        response: None,
    });
    assert_eq!(
        terminal(&Err(failure), None),
        Terminal::Failed {
            error: SessionError {
                error_type: "provider.transport".into(),
                message: "Disconnected".into(),
                status: None,
                response: None,
            }
        }
    );
    assert_eq!(
        terminal(&Err(RunError::Interrupted), None),
        Terminal::Interrupted { reason: InterruptReason::Shutdown }
    );
    assert_eq!(
        terminal(&Err(RunError::Interrupted), Some(InterruptReason::User)),
        Terminal::Interrupted { reason: InterruptReason::User }
    );
}

/// `claims at execution start, releases on completion, and preserves
/// through teardown`.
#[tokio::test]
async fn session_execution_claim_lifecycle() {
    let interrupted = "ses_claim_interrupted";
    let completed = "ses_claim_completed";
    let completed_key = completed.to_string();
    let drain: DrainFn = Arc::new(move |session, _, _, _| {
        let completed_key = completed_key.clone();
        Box::pin(async move {
            if session == completed_key {
                Ok(DrainOutcome::Complete)
            } else {
                std::future::pending::<()>().await;
                unreachable!()
            }
        })
    });
    let (db, _bus, execution) = execution_stack(drain, None).await;
    seed_session_raw(&db, interrupted).await;
    seed_session_raw(&db, completed).await;

    tokio::spawn({
        let execution = Arc::clone(&execution);
        let id = interrupted.to_string();
        async move {
            let _ = execution.resume(&id).await;
        }
    });
    execution.resume(completed).await.expect("completed exit ok");
    execution.await_idle(completed).await;

    let map = claims(&db).await.unwrap();
    let claimed = |map: &[(String, bool)], id: &str| {
        map.iter().find(|(row, _)| row == id).is_some_and(|(_, c)| *c)
    };
    assert!(!claimed(&map, completed), "natural completion releases the claim");
    assert!(execution.is_active(interrupted));
    assert!(claimed(&claims(&db).await.unwrap(), interrupted), "claim held while draining");

    execution.shutdown();
    execution.await_idle(interrupted).await;
    assert!(!execution.is_active(interrupted));
    assert!(
        claimed(&claims(&db).await.unwrap(), interrupted),
        "teardown keeps the claim for restart resume"
    );
}

/// `a user interrupt releases the claim so the turn never resurrects`.
#[tokio::test]
async fn session_execution_user_interrupt_releases_claim() {
    let session_id = "ses_claim_user_cancel";
    let (db, _bus, execution) = execution_stack(hanging_drain(), None).await;
    seed_session_raw(&db, session_id).await;

    tokio::spawn({
        let execution = Arc::clone(&execution);
        let id = session_id.to_string();
        async move {
            let _ = execution.resume(&id).await;
        }
    });
    while !execution.is_active(session_id) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(claims(&db).await.unwrap().iter().any(|(id, c)| id == session_id && *c));

    assert!(
        execution
            .interrupt(
                session_id,
                InterruptOptions { reason: Some(InterruptReason::User), ..Default::default() }
            )
            .await
    );
    execution.await_idle(session_id).await;
    assert!(claims(&db).await.unwrap().iter().any(|(id, c)| id == session_id && !c));
}

/// `reports an idle interrupt as a no-op`.
#[tokio::test]
async fn session_execution_idle_interrupt_is_noop() {
    let drain: DrainFn = Arc::new(|_, _, _, _| Box::pin(async { Ok(DrainOutcome::Complete) }));
    let (_db, _bus, execution) = execution_stack(drain, None).await;
    assert!(!execution.interrupt("ses_idle_cancel", InterruptOptions::default()).await);
    assert!(!execution.is_active("ses_idle_cancel"));
    assert!(execution.active().is_empty());
}

/// `the sweep only lists claimed top-level sessions` + child-claim cleanup.
#[tokio::test]
async fn session_store_suspended_sweep_and_child_claims() {
    let stack = stack().await;
    seed_session_with(&stack.db, "ses_recover_parent", &[("time_suspended", "1".to_string())])
        .await;
    seed_session_raw(&stack.db, "ses_recover_idle").await;
    seed_session_with(
        &stack.db,
        "ses_recover_child",
        &[("time_suspended", "1".to_string()), ("parent_id", "ses_recover_parent".to_string())],
    )
    .await;

    let suspended = stack.store.list_suspended().await.unwrap();
    assert_eq!(suspended, vec!["ses_recover_parent".to_string()]);

    stack.store.release_child_claims(&[]).await.unwrap();
    let map = claims(&stack.db).await.unwrap();
    let claimed = |id: &str| map.iter().find(|(row, _)| row == id).is_some_and(|(_, c)| *c);
    assert!(claimed("ses_recover_parent"));
    assert!(!claimed("ses_recover_child"));
    assert!(!claimed("ses_recover_idle"));
}

/// `countResume` is durable.
#[tokio::test]
async fn session_store_counts_resumes() {
    let stack = stack().await;
    seed_session_raw(&stack.db, "ses_resume_count").await;
    assert_eq!(attempts(&stack.db, "ses_resume_count").await.unwrap(), Some(0));
    assert_eq!(stack.store.count_resume("ses_resume_count").await.unwrap(), Some(1));
    assert_eq!(attempts(&stack.db, "ses_resume_count").await.unwrap(), Some(1));
}

/// `resumes only steering input after an interrupt with continue`.
#[tokio::test]
async fn session_execution_resume_steers_only_after_interrupt() {
    let session_id = "ses_continue_steer";
    let calls: Arc<std::sync::Mutex<Vec<(String, bool, Promotable)>>> = Arc::default();
    let started = Arc::new(Gate::default());
    let drain: DrainFn = {
        let calls = Arc::clone(&calls);
        let started = Arc::clone(&started);
        Arc::new(move |session, force, _, promotable| {
            let calls = Arc::clone(&calls);
            let started = Arc::clone(&started);
            Box::pin(async move {
                let first = calls.lock().unwrap().is_empty();
                if first {
                    calls.lock().unwrap().push((session, force, promotable));
                    started.fire();
                    std::future::pending::<()>().await;
                    unreachable!()
                }
                calls.lock().unwrap().push((session, force, promotable));
                Ok(DrainOutcome::Complete)
            })
        })
    };
    let (db, _bus, execution) = execution_stack(drain, None).await;
    seed_session_raw(&db, session_id).await;
    seed_inbox(&db, session_id, &[user_item(), SeedItem { delivery: "queue", ..user_item() }])
        .await;

    tokio::spawn({
        let execution = Arc::clone(&execution);
        let id = session_id.to_string();
        async move {
            let _ = execution.resume(&id).await;
        }
    });
    started.wait().await;
    execution.interrupt(session_id, InterruptOptions { resume: true, ..Default::default() }).await;
    execution.await_idle(session_id).await;

    let calls = calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            (session_id.to_string(), true, Promotable::Input),
            (session_id.to_string(), false, Promotable::Steer),
        ],
        "the successor drain is steer-scoped; queued next-turn work stays parked"
    );
}

/// `an idle interrupt with continue resumes pending steers`.
#[tokio::test]
async fn session_execution_idle_interrupt_resume_wakes_pending_steers() {
    let session_id = "ses_continue_idle";
    let calls: Arc<std::sync::Mutex<Vec<(String, bool, Promotable)>>> = Arc::default();
    let drain: DrainFn = {
        let calls = Arc::clone(&calls);
        Arc::new(move |session, force, _, promotable| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.lock().unwrap().push((session, force, promotable));
                Ok(DrainOutcome::Complete)
            })
        })
    };
    let (db, _bus, execution) = execution_stack(drain, None).await;
    seed_session_raw(&db, session_id).await;
    seed_inbox(&db, session_id, &[user_item()]).await;

    execution.interrupt(session_id, InterruptOptions { resume: true, ..Default::default() }).await;
    execution.await_idle(session_id).await;
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls, vec![(session_id.to_string(), false, Promotable::Steer)]);
}

/// `an interrupt with continue resumes a queued compaction next in line` +
/// `keeps a control item parked behind a queued prompt`.
#[tokio::test]
async fn session_execution_interrupt_continue_compaction_rules() {
    let session_id = "ses_continue_compaction";
    let calls: Arc<std::sync::Mutex<Vec<(String, bool, Promotable)>>> = Arc::default();
    let started = Arc::new(Gate::default());
    let drain: DrainFn = {
        let calls = Arc::clone(&calls);
        let started = Arc::clone(&started);
        Arc::new(move |session, force, _, promotable| {
            let calls = Arc::clone(&calls);
            let started = Arc::clone(&started);
            Box::pin(async move {
                let first = calls.lock().unwrap().is_empty();
                if first {
                    calls.lock().unwrap().push((session, force, promotable));
                    started.fire();
                    std::future::pending::<()>().await;
                    unreachable!()
                }
                calls.lock().unwrap().push((session, force, promotable));
                Ok(DrainOutcome::Complete)
            })
        })
    };
    let (db, _bus, execution) = execution_stack(drain, None).await;
    seed_session_raw(&db, session_id).await;
    seed_inbox(&db, session_id, &[compaction_item("queue")]).await;
    tokio::spawn({
        let execution = Arc::clone(&execution);
        let id = session_id.to_string();
        async move {
            let _ = execution.resume(&id).await;
        }
    });
    started.wait().await;
    execution.interrupt(session_id, InterruptOptions { resume: true, ..Default::default() }).await;
    execution.await_idle(session_id).await;
    let calls = calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        vec![
            (session_id.to_string(), true, Promotable::Input),
            (session_id.to_string(), false, Promotable::Steer),
        ],
        "control work is housekeeping: continue runs it"
    );

    let parked = "ses_continue_control_behind";
    let parked_calls: Arc<std::sync::Mutex<Vec<(String, bool, Promotable)>>> = Arc::default();
    let parked_drain: DrainFn = {
        let calls = Arc::clone(&parked_calls);
        Arc::new(move |session, force, _, promotable| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                calls.lock().unwrap().push((session, force, promotable));
                Ok(DrainOutcome::Complete)
            })
        })
    };
    let (db2, _bus2, execution2) = execution_stack(parked_drain, None).await;
    seed_session_raw(&db2, parked).await;
    seed_inbox(
        &db2,
        parked,
        &[SeedItem { delivery: "queue", ..user_item() }, compaction_item("queue")],
    )
    .await;
    execution2.interrupt(parked, InterruptOptions { resume: true, ..Default::default() }).await;
    execution2.await_idle(parked).await;
    assert!(parked_calls.lock().unwrap().is_empty(), "queued prompt is next in line");
}

/// Non-shutdown interrupts cancel session jobs (typed Wave D hook).
#[tokio::test]
async fn session_execution_user_interrupt_cancels_jobs() {
    let session_id = "ses_interrupt_jobs";
    let jobs = Arc::new(CancelCounter::default());
    let (db, _bus, execution) =
        execution_stack(hanging_drain(), Some(Arc::clone(&jobs) as Arc<dyn CancelJobs>)).await;
    seed_session_raw(&db, session_id).await;
    tokio::spawn({
        let execution = Arc::clone(&execution);
        let id = session_id.to_string();
        async move {
            let _ = execution.resume(&id).await;
        }
    });
    while !execution.is_active(session_id) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    execution
        .interrupt(
            session_id,
            InterruptOptions { reason: Some(InterruptReason::User), ..Default::default() },
        )
        .await;
    execution.await_idle(session_id).await;
    assert_eq!(*jobs.cancelled.lock().unwrap(), vec![session_id.to_string()]);
}

// ===========================================================================
// SessionRunCoordinator (session-run-coordinator.test.ts)
// ===========================================================================

/// Single-shot gate with waiters (test `Deferred`).
#[derive(Default)]
struct Gate {
    notify: tokio::sync::Notify,
    fired: std::sync::atomic::AtomicBool,
}

impl Gate {
    fn fire(&self) {
        self.fired.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        if self.fired.load(Ordering::SeqCst) {
            return;
        }
        self.notify.notified().await;
    }
}

type TestDrain = Arc<
    dyn Fn(String, bool, Promotable) -> futures::future::BoxFuture<'static, Result<(), RunError>>
        + Send
        + Sync,
>;
type TestSettled = Arc<
    dyn Fn(
            String,
            Result<(), RunError>,
            Option<InterruptReason>,
        ) -> futures::future::BoxFuture<'static, Result<(), RunError>>
        + Send
        + Sync,
>;

fn coordinator_from(drain: TestDrain) -> SessionRunCoordinator {
    SessionRunCoordinator::new(CoordinatorOptions {
        drain: Some(Arc::new(move |key, force, _continuation, scope| {
            let drain = Arc::clone(&drain);
            Box::pin(async move { drain(key, force, scope).await.map(|()| DrainOutcome::Complete) })
        })),
        started: None,
        settled: None,
    })
}

fn coordinator_with(
    drain: TestDrain,
    settled: Option<TestSettled>,
    started: Option<TestDrain>,
) -> SessionRunCoordinator {
    SessionRunCoordinator::new(CoordinatorOptions {
        drain: Some(Arc::new(move |key, force, _continuation, scope| {
            let drain = Arc::clone(&drain);
            Box::pin(async move { drain(key, force, scope).await.map(|()| DrainOutcome::Complete) })
        })),
        started: started.map(|hook| {
            Arc::new(move |key: String| {
                let hook = Arc::clone(&hook);
                hook(key, false, Promotable::Input)
            }) as crate::session::execution::StartedFn
        }),
        settled: settled.map(|hook| {
            Arc::new(move |key: String, exit: Result<(), RunError>, reason| hook(key, exit, reason))
                as crate::session::execution::SettledFn
        }),
    })
}

use futures::future::BoxFuture;
use futures::FutureExt;

fn drain_fn(
    f: impl Fn(String, bool, Promotable) -> BoxFuture<'static, Result<(), RunError>>
        + Send
        + Sync
        + 'static,
) -> TestDrain {
    Arc::new(f)
}

fn settled_fn(
    f: impl Fn(
            String,
            Result<(), RunError>,
            Option<InterruptReason>,
        ) -> BoxFuture<'static, Result<(), RunError>>
        + Send
        + Sync
        + 'static,
) -> TestSettled {
    Arc::new(f)
}

#[tokio::test]
async fn session_coordinator_joins_concurrent_resumes_for_one_key() {
    let runs = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Gate::default());
    let coordinator = coordinator_from(drain_fn({
        let runs = Arc::clone(&runs);
        let gate = Arc::clone(&gate);
        move |_, _, _| {
            let runs = Arc::clone(&runs);
            let gate = Arc::clone(&gate);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                gate.wait().await;
                Ok(())
            }
            .boxed()
        }
    }));

    let first = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let second = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    gate.fire();
    let _ = first.await.unwrap();
    let _ = second.await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn session_coordinator_joins_wake_started_execution_without_forcing() {
    let started = Arc::new(Gate::default());
    let gate = Arc::new(Gate::default());
    let forces: Arc<std::sync::Mutex<Vec<bool>>> = Arc::default();
    let coordinator = coordinator_from(drain_fn({
        let started = Arc::clone(&started);
        let gate = Arc::clone(&gate);
        let forces = Arc::clone(&forces);
        move |_, force, _| {
            let started = Arc::clone(&started);
            let gate = Arc::clone(&gate);
            let forces = Arc::clone(&forces);
            async move {
                forces.lock().unwrap().push(force);
                started.fire();
                gate.wait().await;
                Ok(())
            }
            .boxed()
        }
    }));

    coordinator.wake("session", Promotable::Input);
    started.wait().await;
    let resumed = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    gate.fire();
    let _ = resumed.await.unwrap();
    assert_eq!(*forces.lock().unwrap(), vec![false]);
}

#[tokio::test]
async fn session_coordinator_starts_execution_when_woken_while_idle() {
    let drained = Arc::new(Gate::default());
    let coordinator = coordinator_from(drain_fn({
        let drained = Arc::clone(&drained);
        move |_, _, _| {
            let drained = Arc::clone(&drained);
            async move {
                drained.fire();
                Ok(())
            }
            .boxed()
        }
    }));
    coordinator.wake("session", Promotable::Input);
    drained.wait().await;
    coordinator.await_idle("session").await;
}

#[tokio::test]
async fn session_coordinator_snapshots_only_active_executions() {
    let first_started = Arc::new(Gate::default());
    let second_started = Arc::new(Gate::default());
    let first_gate = Arc::new(Gate::default());
    let second_gate = Arc::new(Gate::default());
    let coordinator = coordinator_from(drain_fn({
        let first_started = Arc::clone(&first_started);
        let second_started = Arc::clone(&second_started);
        let first_gate = Arc::clone(&first_gate);
        let second_gate = Arc::clone(&second_gate);
        move |key, _, _| {
            let first_started = Arc::clone(&first_started);
            let second_started = Arc::clone(&second_started);
            let first_gate = Arc::clone(&first_gate);
            let second_gate = Arc::clone(&second_gate);
            async move {
                if key == "first" {
                    first_started.fire();
                    first_gate.wait().await;
                } else {
                    second_started.fire();
                    second_gate.wait().await;
                }
                Ok(())
            }
            .boxed()
        }
    }));

    assert!(coordinator.active().is_empty());
    assert!(!coordinator.is_active("first"));
    let first = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("first").await }
    });
    first_started.wait().await;
    assert_eq!(coordinator.active(), vec!["first".to_string()]);
    assert!(coordinator.is_active("first"));
    assert!(!coordinator.is_active("second"));

    let second = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("second").await }
    });
    second_started.wait().await;
    let mut active = coordinator.active();
    active.sort();
    assert_eq!(active, vec!["first".to_string(), "second".to_string()]);

    first_gate.fire();
    let _ = first.await.unwrap();
    assert_eq!(coordinator.active(), vec!["second".to_string()]);
    assert!(!coordinator.is_active("first"));
    second_gate.fire();
    let _ = second.await.unwrap();
    assert!(coordinator.active().is_empty());
}

#[tokio::test]
async fn session_coordinator_cleans_active_after_failure_and_defect() {
    let settled_count = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_with(
        drain_fn(|key, _, _| {
            async move {
                if key == "failure" {
                    Err(RunError::Failed(oc_schema::common::SessionError {
                        error_type: "test".into(),
                        message: "failed".into(),
                        status: None,
                        response: None,
                    }))
                } else {
                    Err(RunError::Defect("defect".into()))
                }
            }
            .boxed()
        }),
        Some(settled_fn({
            let settled_count = Arc::clone(&settled_count);
            move |_, _, _| {
                let settled_count = Arc::clone(&settled_count);
                async move {
                    settled_count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    assert!(coordinator.run("failure").await.is_err());
    assert!(coordinator.active().is_empty());
    assert!(!coordinator.is_active("failure"));

    assert!(coordinator.run("defect").await.is_err());
    assert!(coordinator.active().is_empty());
    assert_eq!(settled_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn session_coordinator_preserves_settlement_defects_while_releasing_ownership() {
    let coordinator = coordinator_with(
        drain_fn(|_, _, _| async { Ok(()) }.boxed()),
        Some(settled_fn(|_, _, _| {
            async { Err(RunError::Defect("terminal publication failed".into())) }.boxed()
        })),
        None,
    );
    let exit = coordinator.run("session").await;
    assert!(
        matches!(exit, Err(RunError::Defect(message)) if message == "terminal publication failed")
    );
    assert!(coordinator.active().is_empty());
}

#[tokio::test]
async fn session_coordinator_coalesces_wakes_during_execution() {
    let first_started = Arc::new(Gate::default());
    let first_gate = Arc::new(Gate::default());
    let second_started = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_from(drain_fn({
        let first_started = Arc::clone(&first_started);
        let first_gate = Arc::clone(&first_gate);
        let second_started = Arc::clone(&second_started);
        let runs = Arc::clone(&runs);
        move |_, _, _| {
            let first_started = Arc::clone(&first_started);
            let first_gate = Arc::clone(&first_gate);
            let second_started = Arc::clone(&second_started);
            let runs = Arc::clone(&runs);
            async move {
                let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                if run == 1 {
                    first_started.fire();
                    first_gate.wait().await;
                } else {
                    second_started.fire();
                }
                Ok(())
            }
            .boxed()
        }
    }));

    let resumed = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    first_started.wait().await;
    coordinator.wake("session", Promotable::Input);
    coordinator.wake("session", Promotable::Input);
    coordinator.wake("session", Promotable::Input);
    first_gate.fire();
    second_started.wait().await;
    let _ = resumed.await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn session_coordinator_runs_again_when_woken_during_follow_up() {
    let first_gate = Arc::new(Gate::default());
    let second_started = Arc::new(Gate::default());
    let second_gate = Arc::new(Gate::default());
    let third_started = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_from(drain_fn({
        let first_gate = Arc::clone(&first_gate);
        let second_started = Arc::clone(&second_started);
        let second_gate = Arc::clone(&second_gate);
        let third_started = Arc::clone(&third_started);
        let runs = Arc::clone(&runs);
        move |_, _, _| {
            let first_gate = Arc::clone(&first_gate);
            let second_started = Arc::clone(&second_started);
            let second_gate = Arc::clone(&second_gate);
            let third_started = Arc::clone(&third_started);
            let runs = Arc::clone(&runs);
            async move {
                let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                match run {
                    1 => first_gate.wait().await,
                    2 => {
                        second_started.fire();
                        second_gate.wait().await;
                    }
                    _ => third_started.fire(),
                }
                Ok(())
            }
            .boxed()
        }
    }));

    let resumed = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    coordinator.wake("session", Promotable::Input);
    first_gate.fire();
    second_started.wait().await;
    coordinator.wake("session", Promotable::Input);
    second_gate.fire();
    third_started.wait().await;
    let _ = resumed.await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn session_coordinator_interrupt_idle_is_noop() {
    let reasons: Arc<std::sync::Mutex<Vec<Option<InterruptReason>>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn(|_, _, _| async { Ok(()) }.boxed()),
        Some(settled_fn({
            let reasons = Arc::clone(&reasons);
            move |_, _, reason| {
                let reasons = Arc::clone(&reasons);
                async move {
                    reasons.lock().unwrap().push(reason);
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );
    let (interrupted, _) =
        coordinator.interrupt("session", Some(InterruptReason::User), false).await;
    assert!(!interrupted);
    coordinator.run("session").await.unwrap();
    let reasons = reasons.lock().unwrap().clone();
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].is_none());
}

#[tokio::test]
async fn session_coordinator_settlement_window_wake_starts_fresh_execution() {
    let settling = Arc::new(Gate::default());
    let release = Arc::new(Gate::default());
    let scopes: Arc<std::sync::Mutex<Vec<Promotable>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let scopes = Arc::clone(&scopes);
            move |_, _, scope| {
                let scopes = Arc::clone(&scopes);
                async move {
                    scopes.lock().unwrap().push(scope);
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let settling = Arc::clone(&settling);
            let release = Arc::clone(&release);
            move |_, _, _| {
                let settling = Arc::clone(&settling);
                let release = Arc::clone(&release);
                async move {
                    settling.fire();
                    release.wait().await;
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    coordinator.wake("session", Promotable::Steer);
    settling.wait().await;
    coordinator.wake("session", Promotable::Input);
    release.fire();
    coordinator.await_idle("session").await;
    assert_eq!(*scopes.lock().unwrap(), vec![Promotable::Steer, Promotable::Input]);
}

#[tokio::test]
async fn session_coordinator_interrupt_clears_pending_wake() {
    let started = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let reasons: Arc<std::sync::Mutex<Vec<Option<InterruptReason>>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let started = Arc::clone(&started);
            let runs = Arc::clone(&runs);
            move |_, _, _| {
                let started = Arc::clone(&started);
                let runs = Arc::clone(&runs);
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    started.fire();
                    std::future::pending::<()>().await;
                    #[allow(unreachable_code)]
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let reasons = Arc::clone(&reasons);
            move |_, _, reason| {
                let reasons = Arc::clone(&reasons);
                async move {
                    reasons.lock().unwrap().push(reason);
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    let first = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    started.wait().await;
    let second = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    let idle = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.await_idle("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    coordinator.wake("session", Promotable::Input);
    let (interrupted, _) =
        coordinator.interrupt("session", Some(InterruptReason::User), false).await;
    assert!(interrupted);

    let exit_a = first.await.unwrap();
    let exit_b = second.await.unwrap();
    assert!(matches!(exit_a, Err(RunError::Interrupted)));
    assert!(matches!(exit_b, Err(RunError::Interrupted)));
    idle.await.unwrap();
    assert!(coordinator.active().is_empty());
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(*reasons.lock().unwrap(), vec![Some(InterruptReason::User)]);
}

#[tokio::test]
async fn session_coordinator_runs_wake_registered_during_cleanup() {
    let first_started = Arc::new(Gate::default());
    let cleanup_started = Arc::new(Gate::default());
    let cleanup_gate = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_with(
        drain_fn({
            let first_started = Arc::clone(&first_started);
            let runs = Arc::clone(&runs);
            move |_, _, _| {
                let first_started = Arc::clone(&first_started);
                let runs = Arc::clone(&runs);
                async move {
                    let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                    if run == 1 {
                        first_started.fire();
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let cleanup_started = Arc::clone(&cleanup_started);
            let cleanup_gate = Arc::clone(&cleanup_gate);
            move |_, _, _| {
                let cleanup_started = Arc::clone(&cleanup_started);
                let cleanup_gate = Arc::clone(&cleanup_gate);
                async move {
                    cleanup_started.fire();
                    cleanup_gate.wait().await;
                    Ok(())
                }
                .boxed()
            }
        })),
        Some(drain_fn({
            let starts = Arc::clone(&starts);
            move |_, _, _| {
                let starts = Arc::clone(&starts);
                async move {
                    starts.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }
        })),
    );

    coordinator.wake("session", Promotable::Input);
    first_started.wait().await;
    let (interrupted, _) = coordinator.interrupt("session", None, false).await;
    assert!(interrupted);
    cleanup_started.wait().await;
    coordinator.wake("session", Promotable::Input);
    cleanup_gate.fire();
    coordinator.await_idle("session").await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn session_coordinator_coalesces_scopes_input_first() {
    let first_started = Arc::new(Gate::default());
    let release = Arc::new(Gate::default());
    let scopes: Arc<std::sync::Mutex<Vec<Promotable>>> = Arc::default();
    let coordinator = coordinator_from(drain_fn({
        let first_started = Arc::clone(&first_started);
        let release = Arc::clone(&release);
        let scopes = Arc::clone(&scopes);
        move |_, _, scope| {
            let first_started = Arc::clone(&first_started);
            let release = Arc::clone(&release);
            let scopes = Arc::clone(&scopes);
            async move {
                let first = scopes.lock().unwrap().is_empty();
                if first {
                    scopes.lock().unwrap().push(scope);
                    first_started.fire();
                    release.wait().await;
                    return Ok(());
                }
                scopes.lock().unwrap().push(scope);
                Ok(())
            }
            .boxed()
        }
    }));

    coordinator.wake("session", Promotable::Steer);
    first_started.wait().await;
    coordinator.wake("session", Promotable::Steer);
    coordinator.wake("session", Promotable::Input);
    release.fire();
    coordinator.await_idle("session").await;
    assert_eq!(*scopes.lock().unwrap(), vec![Promotable::Steer, Promotable::Input]);
}

#[tokio::test]
async fn session_coordinator_no_completed_input_scope_into_steer() {
    let first_started = Arc::new(Gate::default());
    let release = Arc::new(Gate::default());
    let scopes: Arc<std::sync::Mutex<Vec<Promotable>>> = Arc::default();
    let coordinator = coordinator_from(drain_fn({
        let first_started = Arc::clone(&first_started);
        let release = Arc::clone(&release);
        let scopes = Arc::clone(&scopes);
        move |_, _, scope| {
            let first_started = Arc::clone(&first_started);
            let release = Arc::clone(&release);
            let scopes = Arc::clone(&scopes);
            async move {
                let first = scopes.lock().unwrap().is_empty();
                if first {
                    scopes.lock().unwrap().push(scope);
                    first_started.fire();
                    release.wait().await;
                    return Ok(());
                }
                scopes.lock().unwrap().push(scope);
                Ok(())
            }
            .boxed()
        }
    }));

    coordinator.wake("session", Promotable::Input);
    first_started.wait().await;
    coordinator.wake("session", Promotable::Steer);
    release.fire();
    coordinator.await_idle("session").await;
    assert_eq!(*scopes.lock().unwrap(), vec![Promotable::Input, Promotable::Steer]);
}

#[tokio::test]
async fn session_coordinator_cleanup_era_wake_keeps_own_scope() {
    let first_started = Arc::new(Gate::default());
    let cleanup_started = Arc::new(Gate::default());
    let cleanup_gate = Arc::new(Gate::default());
    let scopes: Arc<std::sync::Mutex<Vec<Promotable>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let first_started = Arc::clone(&first_started);
            let scopes = Arc::clone(&scopes);
            move |_, _, scope| {
                let first_started = Arc::clone(&first_started);
                let scopes = Arc::clone(&scopes);
                async move {
                    let first = scopes.lock().unwrap().is_empty();
                    scopes.lock().unwrap().push(scope);
                    if first {
                        first_started.fire();
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let cleanup_started = Arc::clone(&cleanup_started);
            let cleanup_gate = Arc::clone(&cleanup_gate);
            move |_, _, _| {
                let cleanup_started = Arc::clone(&cleanup_started);
                let cleanup_gate = Arc::clone(&cleanup_gate);
                async move {
                    cleanup_started.fire();
                    cleanup_gate.wait().await;
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    coordinator.wake("session", Promotable::Input);
    first_started.wait().await;
    let (interrupted, _) = coordinator.interrupt("session", None, false).await;
    assert!(interrupted);
    cleanup_started.wait().await;
    coordinator.wake("session", Promotable::Input);
    cleanup_gate.fire();
    coordinator.await_idle("session").await;
    assert_eq!(*scopes.lock().unwrap(), vec![Promotable::Input, Promotable::Input]);
}

#[tokio::test]
async fn session_coordinator_one_follow_up_when_wake_races_failure() {
    let gate = Arc::new(Gate::default());
    let second_started = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_from(drain_fn({
        let gate = Arc::clone(&gate);
        let second_started = Arc::clone(&second_started);
        let runs = Arc::clone(&runs);
        move |_, _, _| {
            let gate = Arc::clone(&gate);
            let second_started = Arc::clone(&second_started);
            let runs = Arc::clone(&runs);
            async move {
                let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                if run == 1 {
                    gate.wait().await;
                    return Err(RunError::Failed(oc_schema::common::SessionError {
                        error_type: "test".into(),
                        message: "failed".into(),
                        status: None,
                        response: None,
                    }));
                }
                second_started.fire();
                Ok(())
            }
            .boxed()
        }
    }));

    let resumed = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    coordinator.wake("session", Promotable::Input);
    gate.fire();
    let exit = resumed.await.unwrap();
    assert!(matches!(exit, Err(RunError::Failed(_))));
    second_started.wait().await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn session_coordinator_does_not_cancel_when_a_joined_waiter_is_interrupted() {
    let gate = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_from(drain_fn({
        let gate = Arc::clone(&gate);
        let runs = Arc::clone(&runs);
        move |_, _, _| {
            let gate = Arc::clone(&gate);
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                gate.wait().await;
                Ok(())
            }
            .boxed()
        }
    }));

    let first = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    let second = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    second.abort(); // interrupting a waiter never cancels the execution
    gate.fire();
    let _ = first.await.unwrap();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn session_coordinator_runs_different_keys_concurrently() {
    let both_started = Arc::new(Gate::default());
    let gate = Arc::new(Gate::default());
    let active_count = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_from(drain_fn({
        let both_started = Arc::clone(&both_started);
        let gate = Arc::clone(&gate);
        let active_count = Arc::clone(&active_count);
        move |_, _, _| {
            let both_started = Arc::clone(&both_started);
            let gate = Arc::clone(&gate);
            let active_count = Arc::clone(&active_count);
            async move {
                if active_count.fetch_add(1, Ordering::SeqCst) + 1 == 2 {
                    both_started.fire();
                }
                gate.wait().await;
                Ok(())
            }
            .boxed()
        }
    }));

    let first = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("first").await }
    });
    let second = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("second").await }
    });
    both_started.wait().await;
    gate.fire();
    let (a, b) = tokio::join!(first, second);
    let _ = a.unwrap();
    let _ = b.unwrap();
}

#[tokio::test]
async fn session_coordinator_settles_once_per_execution() {
    let started = Arc::new(Gate::default());
    let gate = Arc::new(Gate::default());
    let idle = Arc::new(Gate::default());
    let drains = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let settled: Arc<std::sync::Mutex<Vec<Result<(), RunError>>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let started = Arc::clone(&started);
            let gate = Arc::clone(&gate);
            let drains = Arc::clone(&drains);
            move |_, _, _| {
                let started = Arc::clone(&started);
                let gate = Arc::clone(&gate);
                let drains = Arc::clone(&drains);
                async move {
                    let run = drains.fetch_add(1, Ordering::SeqCst) + 1;
                    if run == 1 {
                        started.fire();
                        gate.wait().await;
                    }
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let settled = Arc::clone(&settled);
            let idle = Arc::clone(&idle);
            move |_, exit, _| {
                let settled = Arc::clone(&settled);
                let idle = Arc::clone(&idle);
                async move {
                    settled.lock().unwrap().push(exit);
                    idle.fire();
                    Ok(())
                }
                .boxed()
            }
        })),
        Some(drain_fn({
            let starts = Arc::clone(&starts);
            move |_, _, _| {
                let starts = Arc::clone(&starts);
                async move {
                    starts.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }
        })),
    );

    coordinator.wake("session", Promotable::Input);
    started.wait().await;
    coordinator.wake("session", Promotable::Input);
    gate.fire();
    idle.wait().await;
    coordinator.await_idle("session").await;
    assert_eq!(drains.load(Ordering::SeqCst), 2);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let settled = settled.lock().unwrap().clone();
    assert_eq!(settled.len(), 1);
    assert!(settled[0].is_ok());
}

#[tokio::test]
async fn session_coordinator_settles_interrupted_before_waiters() {
    let started = Arc::new(Gate::default());
    let settled: Arc<std::sync::Mutex<Vec<Result<(), RunError>>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let started = Arc::clone(&started);
            move |_, _, _| {
                let started = Arc::clone(&started);
                async move {
                    started.fire();
                    std::future::pending::<()>().await;
                    #[allow(unreachable_code)]
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let settled = Arc::clone(&settled);
            move |_, exit, _| {
                let settled = Arc::clone(&settled);
                async move {
                    settled.lock().unwrap().push(exit);
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    coordinator.wake("session", Promotable::Input);
    started.wait().await;
    let (_, receiver) = coordinator.interrupt("session", None, false).await;
    if let Some(mut receiver) = receiver {
        loop {
            if let Some(exit) = receiver.borrow().clone() {
                assert!(matches!(exit, Err(RunError::Interrupted)));
                break;
            }
            receiver.changed().await.expect("watch alive");
        }
    }
    coordinator.await_idle("session").await;
    let settled = settled.lock().unwrap().clone();
    assert_eq!(settled.len(), 1);
    assert!(matches!(settled[0], Err(RunError::Interrupted)));
    assert!(coordinator.active().is_empty());
}

#[tokio::test]
async fn session_coordinator_acknowledges_interruption_before_cleanup_settles() {
    let started = Arc::new(Gate::default());
    let cleanup_started = Arc::new(Gate::default());
    let cleanup_gate = Arc::new(Gate::default());
    let settled: Arc<std::sync::Mutex<Vec<Option<InterruptReason>>>> = Arc::default();
    let coordinator = coordinator_with(
        drain_fn({
            let started = Arc::clone(&started);
            move |_, _, _| {
                let started = Arc::clone(&started);
                async move {
                    started.fire();
                    std::future::pending::<()>().await;
                    #[allow(unreachable_code)]
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let cleanup_started = Arc::clone(&cleanup_started);
            let cleanup_gate = Arc::clone(&cleanup_gate);
            let settled = Arc::clone(&settled);
            move |_, _, reason| {
                let cleanup_started = Arc::clone(&cleanup_started);
                let cleanup_gate = Arc::clone(&cleanup_gate);
                let settled = Arc::clone(&settled);
                async move {
                    cleanup_started.fire();
                    cleanup_gate.wait().await;
                    settled.lock().unwrap().push(reason);
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    coordinator.wake("session", Promotable::Input);
    started.wait().await;
    let (interrupted, _) =
        coordinator.interrupt("session", Some(InterruptReason::User), false).await;
    assert!(interrupted, "interruption acknowledges before cleanup settles");
    assert!(settled.lock().unwrap().is_empty());
    assert!(coordinator.is_active("session"));
    // Repeating the interrupt during cleanup stays an immediate no-op.
    let (again, _) = coordinator.interrupt("session", Some(InterruptReason::User), false).await;
    assert!(!again);

    cleanup_started.wait().await;
    cleanup_gate.fire();
    coordinator.await_idle("session").await;
    assert_eq!(*settled.lock().unwrap(), vec![Some(InterruptReason::User)]);
    assert!(coordinator.active().is_empty());
    assert!(!coordinator.is_active("session"));
}

#[tokio::test]
async fn session_coordinator_settlement_window_interrupt_claims_wake() {
    let settling = Arc::new(Gate::default());
    let release = Arc::new(Gate::default());
    let drains = Arc::new(AtomicUsize::new(0));
    let coordinator = coordinator_with(
        drain_fn({
            let drains = Arc::clone(&drains);
            move |_, _, _| {
                let drains = Arc::clone(&drains);
                async move {
                    drains.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                .boxed()
            }
        }),
        Some(settled_fn({
            let settling = Arc::clone(&settling);
            let release = Arc::clone(&release);
            move |_, _, _| {
                let settling = Arc::clone(&settling);
                let release = Arc::clone(&release);
                async move {
                    settling.fire();
                    release.wait().await;
                    Ok(())
                }
                .boxed()
            }
        })),
        None,
    );

    coordinator.wake("session", Promotable::Input);
    settling.wait().await;
    assert!(coordinator.is_active("session"));
    coordinator.wake("session", Promotable::Input);
    let (interrupted, _) = coordinator.interrupt("session", None, false).await;
    assert!(!interrupted, "the terminal outcome is already decided");
    release.fire();
    coordinator.await_idle("session").await;
    assert_eq!(drains.load(Ordering::SeqCst), 1, "settle starts no dead-intent successor");
    assert!(coordinator.active().is_empty());
    assert!(!coordinator.is_active("session"));
}

#[tokio::test]
async fn session_coordinator_starts_resume_registered_during_cleanup() {
    let first_started = Arc::new(Gate::default());
    let cleanup_started = Arc::new(Gate::default());
    let cleanup_gate = Arc::new(Gate::default());
    let second_started = Arc::new(Gate::default());
    let forces: Arc<std::sync::Mutex<Vec<bool>>> = Arc::default();
    let coordinator = coordinator_from(drain_fn({
        let first_started = Arc::clone(&first_started);
        let second_started = Arc::clone(&second_started);
        let forces = Arc::clone(&forces);
        move |_, force, _| {
            let first_started = Arc::clone(&first_started);
            let second_started = Arc::clone(&second_started);
            let forces = Arc::clone(&forces);
            async move {
                let first = forces.lock().unwrap().is_empty();
                if first {
                    forces.lock().unwrap().push(force);
                    first_started.fire();
                    std::future::pending::<()>().await;
                }
                forces.lock().unwrap().push(force);
                second_started.fire();
                Ok(())
            }
            .boxed()
        }
    }));

    coordinator.wake("session", Promotable::Input);
    first_started.wait().await;
    // The interrupt's cleanup gate: hold settlement until the resume lands.
    let interrupt = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.interrupt("session", None, true).await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    // A stopping execution refuses joiners; the resume waits out cleanup.
    let resumed = tokio::spawn({
        let coordinator = coordinator.clone();
        async move { coordinator.run("session").await }
    });
    tokio::time::sleep(Duration::from_millis(10)).await;
    cleanup_started_notified(&cleanup_started).await;
    cleanup_gate.fire();
    let _ = interrupt.await.unwrap();
    let _ = resumed.await.unwrap();
    second_started.wait().await;
    assert_eq!(*forces.lock().unwrap(), vec![false, true]);

    async fn cleanup_started_notified(_gate: &Arc<Gate>) {
        // cleanup gate is not armed in this variant; just yield once to
        // let the interrupt be accepted.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn session_coordinator_trampolines_self_waking_execution() {
    const LIMIT: usize = 20_000;
    let completed = Arc::new(Gate::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let coordinator = Arc::new(coordinator_from(drain_fn({
        let completed = Arc::clone(&completed);
        let runs = Arc::clone(&runs);
        move |key, _, _| {
            let completed = Arc::clone(&completed);
            let runs = Arc::clone(&runs);
            async move {
                let run = runs.fetch_add(1, Ordering::SeqCst) + 1;
                if run < LIMIT {
                    WAKER.with(|waker| {
                        if let Some(wake) = waker.borrow().as_ref() {
                            wake(key.clone(), Promotable::Input);
                        }
                    });
                } else {
                    completed.fire();
                }
                Ok(())
            }
            .boxed()
        }
    })));
    WAKER.with(|slot| {
        let coordinator = Arc::clone(&coordinator);
        *slot.borrow_mut() = Some(Box::new(move |key: String, scope: Promotable| {
            coordinator.wake(&key, scope);
        }));
    });

    coordinator.wake("session", Promotable::Input);
    completed.wait().await;
    coordinator.await_idle("session").await;
    assert_eq!(runs.load(Ordering::SeqCst), LIMIT);
}

thread_local! {
    static WAKER: std::cell::RefCell<Option<Box<dyn Fn(String, Promotable) + Send>>> =
        const { std::cell::RefCell::new(None) };
}
