//! Port of `session/inbox.ts` — the pending-only session inbox.
//!
//! The `session_inbox` table holds admitted-but-unprocessed input; promotion
//! DELETES the row inside the `session.inbox.delivered` durable-event
//! transaction (the deleted row's content feeds the projected message insert
//! in the same transaction). Semantics (runner-map §1.2):
//!
//! - **Steer first**: `next_promotable` returns the first steer row (with
//!   control reordering); only an `"input"` scope falls through to the
//!   oldest queued row.
//! - **Control reordering**: a move bounds the steer batch — compaction is
//!   never pulled across a move; a compaction not at index 0 is spliced to
//!   the FRONT so earlier steers stay verbatim after the checkpoint.
//! - **`promote(db, bus, id, scope)`**: steers exist or scope is steer →
//!   control at index 0 returns `None` (the runner handles the control);
//!   otherwise deliver steers up to the first control. No steers → deliver
//!   one oldest queued (control ⇒ `None`), then re-read steers and deliver
//!   those that arrived, again up to the first control.
//! - **Idempotent admission**: first admission wins per
//!   (session, type); a message id already projected as `user`/`synthetic`
//!   reconciles through `promoted_from_message`.

#![allow(clippy::must_use_candidate, clippy::too_many_lines, clippy::needless_pass_by_value)]

use std::future::Future;
use std::sync::{Arc, LazyLock};

use dashmap::DashMap;
use oc_schema::id::{MessageId, SessionId};
use oc_schema::session_inbox::{
    self, CompactionPayload, Delivery, Info, Item, MovePayload, SyntheticPayload, UserPayload,
};
use oc_schema::session_message;
use rusqlite::OptionalExtension;

use crate::bus::{Bus, BusError, EventDefinition, PublishOptions};
use crate::database::error::{DbError, Result};
use crate::database::schema::{decode_json, encode_json, SessionInboxRow, SessionMessageRow};
use crate::database::{Database, Tx};

/// Marker tunneling `SessionInbox.LifecycleConflict` defects through the
/// bus's transaction error channel (`DbError::Pragma`), mirroring how the
/// bus itself tunnels `InvalidDurableEventError`.
pub const CONFLICT_MARKER: &str = "\u{0}session-inbox-conflict\u{1}";

/// Dies inside a projector transaction with a lifecycle conflict.
pub(crate) fn tx_conflict(id: &str) -> DbError {
    DbError::Pragma(format!("{CONFLICT_MARKER}{id}"))
}

/// Extracts the conflicting id when an error (or bus-wrapped error) carries
/// a lifecycle-conflict defect.
pub fn conflict_id(err: &BusError) -> Option<String> {
    match err {
        BusError::Db(DbError::Pragma(message)) => {
            message.strip_prefix(CONFLICT_MARKER).map(str::to_string)
        }
        _ => None,
    }
}

/// Which pending input `promote` may consume (`inbox.ts` `Promotable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotable {
    /// Steers only (a step boundary mid-work).
    Steer,
    /// Steers, or one queued input when no steers wait (the idle boundary).
    Input,
}

impl Promotable {
    /// The wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Promotable::Steer => "steer",
            Promotable::Input => "input",
        }
    }
}

/// Which pending rows count (`inbox.ts` `Scope`): `"input"` means any item
/// in either delivery mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Input,
    Steer,
    Queue,
}

/// Inbox errors: lifecycle conflicts (typed, recoverable) vs infrastructure.
#[derive(Debug, thiserror::Error)]
pub enum InboxError {
    /// `SessionInbox.LifecycleConflict`.
    #[error("session inbox lifecycle conflict: {0}")]
    Conflict(String),
    /// Underlying database failure.
    #[error(transparent)]
    Db(#[from] DbError),
    /// Bus publish failure.
    #[error(transparent)]
    Bus(#[from] BusError),
    /// Malformed inbox row payload.
    #[error("inbox payload decode failed: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Per-session inbox mutex (`inboxLocks`, KeyedMutex.makeUnsafe).
static LOCKS: LazyLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> =
    LazyLock::new(|| DashMap::with_capacity(4));

fn lock_for(session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    LOCKS
        .entry(session_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// Decodes a delivery column value.
fn delivery_from(value: &str) -> Result<Delivery> {
    match value {
        "steer" => Ok(Delivery::Steer),
        "queue" => Ok(Delivery::Queue),
        other => Err(DbError::Pragma(format!("unknown inbox delivery {other}"))),
    }
}

/// `Delivery` wire string.
fn delivery_str(delivery: Delivery) -> &'static str {
    match delivery {
        Delivery::Steer => "steer",
        Delivery::Queue => "queue",
    }
}

/// Tolerant message-id constructor (invalid prefixes pass through; the
/// projector treats them as opaque row keys like v2's unbranded fallback).
fn mid(value: impl Into<String>) -> MessageId {
    MessageId::parse(value.into()).unwrap_or_else(|err| MessageId(err.value))
}

/// Tolerant session-id constructor.
fn sid(value: impl Into<String>) -> SessionId {
    SessionId::parse(value.into()).unwrap_or_else(|err| SessionId(err.value))
}

fn is_control(row: &SessionInboxRow) -> bool {
    row.type_ == "compaction" || row.type_ == "move"
}

/// Row → [`Info`] (`fromRow`, inbox.ts:76-106).
pub fn info_from_row(row: &SessionInboxRow) -> Result<Info> {
    let id = mid(row.id.clone());
    let session_id = sid(row.session_id.clone());
    let time =
        session_inbox::EnqueuedTime { created: u64::try_from(row.time_created).unwrap_or(0) };
    let delivery = delivery_from(&row.delivery)?;
    match row.type_.as_str() {
        "user" => Ok(Info::User {
            id,
            session_id,
            time,
            payload: serde_json::from_str::<UserPayload>(&row.payload)?,
            delivery,
        }),
        "synthetic" => Ok(Info::Synthetic {
            id,
            session_id,
            time,
            payload: serde_json::from_str::<SyntheticPayload>(&row.payload)?,
            delivery,
        }),
        "compaction" => Ok(Info::Compaction {
            id,
            session_id,
            time,
            payload: serde_json::from_str::<CompactionPayload>(&row.payload)?,
            delivery,
        }),
        "move" => Ok(Info::Move {
            id,
            session_id,
            time,
            payload: serde_json::from_str::<MovePayload>(&row.payload)?,
            delivery,
        }),
        _ => Err(tx_conflict(&row.id)),
    }
}

/// Encodes an inbox item's payload for the `payload` column
/// (`projectAdmitted`'s per-type encode).
fn payload_json(item: &Item) -> String {
    match item {
        Item::User { payload, .. } => {
            encode_json(&serde_json::to_value(payload).unwrap_or_default())
        }
        Item::Synthetic { payload, .. } => {
            encode_json(&serde_json::to_value(payload).unwrap_or_default())
        }
        Item::Compaction { payload, .. } => {
            encode_json(&serde_json::to_value(payload).unwrap_or_default())
        }
        Item::Move_ { payload, .. } => {
            encode_json(&serde_json::to_value(payload).unwrap_or_default())
        }
    }
}

/// The inbox service (`SessionInbox.make`): DB + bus.
#[derive(Clone)]
pub struct SessionInbox {
    db: Database,
    bus: Bus,
}

impl SessionInbox {
    /// Assembles the service.
    #[must_use]
    pub fn new(db: Database, bus: Bus) -> Self {
        Self { db, bus }
    }

    /// The underlying bus.
    #[must_use]
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// Runs `f` under the per-session inbox mutex (`SessionInbox.serialized`).
    pub async fn serialized<R>(&self, session_id: &str, f: impl Future<Output = R>) -> R {
        let lock = lock_for(session_id);
        let _guard = lock.lock().await;
        f.await
    }

    fn definition(type_: &str) -> &'static EventDefinition {
        crate::bus::registry::by_type_latest(type_)
            .unwrap_or_else(|| panic!("no durable definition for {type_}"))
    }

    /// Reads one pending row (`SessionInbox.find`).
    pub async fn find(&self, id: &str) -> Result<Option<Info>> {
        let id = id.to_string();
        self.db.transaction(move |tx| find_tx(tx, &id)).await
    }

    /// Lists a session's pending rows in enqueue order (`SessionInbox.list`).
    pub async fn list(&self, session_id: &str) -> Result<Vec<Info>> {
        let session_id = session_id.to_string();
        self.db.transaction(move |tx| list_tx(tx, &session_id)).await
    }

    /// Pending move ids in enqueue order (`SessionInbox.moveIDs`).
    pub async fn move_ids(&self, session_id: &str) -> Result<Vec<String>> {
        let session_id = session_id.to_string();
        self.db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id FROM session_inbox WHERE session_id = ?1 AND type = 'move' \
                     ORDER BY enqueued_seq",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![session_id], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
    }

    /// Whether pending rows exist in `scope` (`SessionInbox.has`).
    pub async fn has(&self, session_id: &str, scope: Scope) -> Result<bool> {
        let session_id = session_id.to_string();
        self.db
            .transaction(move |tx| {
                let predicate = match scope {
                    Scope::Input => "delivery IN ('steer', 'queue')",
                    Scope::Steer => "delivery = 'steer'",
                    Scope::Queue => "delivery = 'queue'",
                };
                let sql = format!(
                    "SELECT id FROM session_inbox WHERE session_id = ?1 AND {predicate} LIMIT 1"
                );
                let row: Option<String> = tx
                    .conn()
                    .query_row(&sql, rusqlite::params![session_id], |row| row.get(0))
                    .optional()?;
                Ok(row.is_some())
            })
            .await
    }

    /// The next promotable pending row (`SessionInbox.nextPromotable`):
    /// first steer (with control reordering) wins; only `"input"` falls
    /// through to the oldest queued row.
    pub async fn next_promotable(
        &self,
        session_id: &str,
        promotable: Promotable,
    ) -> Result<Option<Info>> {
        let session_id = session_id.to_string();
        self.db.transaction(move |tx| next_promotable_tx(tx, &session_id, promotable)).await
    }

    /// First-admission-wins reconciliation (`SessionInbox.reconcile`):
    /// returns the existing pending item or a reconciled
    /// promoted-from-message row; conflicting (session, type) admits fail
    /// with [`InboxError::Conflict`].
    pub async fn reconcile(
        &self,
        id: &str,
        session_id: &str,
        type_: &str,
        delivery: Delivery,
    ) -> std::result::Result<Option<Info>, InboxError> {
        let existing = match self.find(id).await? {
            Some(existing) => Some(existing),
            None => self.promoted_from_message(session_id, id, delivery).await?,
        };
        let Some(existing) = existing else { return Ok(None) };
        if matches!(existing, Info::Compaction { .. })
            || existing_session(&existing).as_str() != session_id
            || existing_type(&existing) != type_
        {
            return Err(InboxError::Conflict(id.to_string()));
        }
        Ok(Some(existing))
    }

    /// Admits an item durably (`SessionInbox.admit`): idempotent per
    /// (session, type); publishes `session.inbox.enqueued`.
    pub async fn admit(
        &self,
        id: &str,
        session_id: &str,
        item: Item,
    ) -> std::result::Result<Info, InboxError> {
        let delivery = item_delivery(&item);
        if let Some(existing) = self.reconcile(id, session_id, item_type(&item), delivery).await? {
            return Ok(existing);
        }
        let data = serde_json::json!({
            "sessionID": session_id,
            "inboxID": id,
            "item": serde_json::to_value(&item)?,
        });
        let publish = self
            .bus
            .publish(Self::definition("session.inbox.enqueued"), data, PublishOptions::default())
            .await;
        let created = match publish {
            Ok(event) => event.created,
            Err(err) => match conflict_id(&err) {
                // Concurrent admission lost the race: the stored row wins.
                Some(_) => {
                    let stored = self.find(id).await.ok().flatten();
                    let stored = stored.ok_or(err)?;
                    if existing_session(&stored).as_str() != session_id
                        || existing_type(&stored) != item_type(&item)
                    {
                        return Err(InboxError::Conflict(id.to_string()));
                    }
                    return Ok(stored);
                }
                None => return Err(err.into()),
            },
        };
        Ok(info_from_item(mid(id), sid(session_id), created, item))
    }

    /// Admits (or reuses) a pending compaction (`SessionInbox.admitCompaction`).
    pub async fn admit_compaction(
        &self,
        id: &str,
        session_id: &str,
        delivery: Delivery,
    ) -> std::result::Result<Info, InboxError> {
        self.serialized(session_id, async {
            if let Some(exact) = self.find(id).await? {
                if matches!(exact, Info::Compaction { .. })
                    && existing_session(&exact).as_str() == session_id
                {
                    return Ok(exact);
                }
                return Err(InboxError::Conflict(id.to_string()));
            }
            if self.promoted_from_message(session_id, id, delivery).await?.is_some() {
                return Err(InboxError::Conflict(id.to_string()));
            }
            let pending = self.list(session_id).await?;
            if let Some(item) = pending.iter().find(|item| matches!(item, Info::Compaction { .. }))
            {
                return Ok(item.clone());
            }
            self.admit(id, session_id, Item::Compaction { payload: CompactionPayload {}, delivery })
                .await
        })
        .await
    }

    /// Cancels a pending row (`SessionInbox.cancel`): publishes
    /// `session.inbox.cancelled` under the session mutex; conflicts surface
    /// as [`InboxError::Conflict`].
    pub async fn cancel(&self, session_id: &str, id: &str) -> std::result::Result<(), InboxError> {
        self.publish_mutation(session_id, "session.inbox.cancelled", id, None).await
    }

    /// Switches a pending row to steer delivery (`SessionInbox.steer`).
    pub async fn steer(&self, session_id: &str, id: &str) -> std::result::Result<(), InboxError> {
        self.publish_mutation(session_id, "session.inbox.delivery.changed", id, Some("steer")).await
    }

    /// Switches a pending row to queued delivery (`SessionInbox.queue`).
    pub async fn queue(&self, session_id: &str, id: &str) -> std::result::Result<(), InboxError> {
        self.publish_mutation(session_id, "session.inbox.delivery.changed", id, Some("queue")).await
    }

    async fn publish_mutation(
        &self,
        session_id: &str,
        type_: &'static str,
        id: &str,
        delivery: Option<&str>,
    ) -> std::result::Result<(), InboxError> {
        self.serialized(session_id, async {
            let mut data = serde_json::json!({ "sessionID": session_id, "inboxID": id });
            if let Some(delivery) = delivery {
                data["delivery"] = serde_json::json!(delivery);
            }
            match self.bus.publish(Self::definition(type_), data, PublishOptions::default()).await {
                Ok(_) => Ok(()),
                Err(err) => match conflict_id(&err) {
                    Some(_) => Err(InboxError::Conflict(id.to_string())),
                    None => Err(err.into()),
                },
            }
        })
        .await
    }

    /// Reconciles an id already projected as a user/synthetic message
    /// (`promotedFromMessage`, inbox.ts:113-143).
    pub async fn promoted_from_message(
        &self,
        session_id: &str,
        id: &str,
        delivery: Delivery,
    ) -> std::result::Result<Option<Info>, InboxError> {
        let (session_id, lookup) = (session_id.to_string(), id.to_string());
        let row = self.db.transaction(move |tx| SessionMessageRow::get(tx, lookup)).await?;
        let Some(row) = row else { return Ok(None) };
        if row.session_id != session_id || (row.type_ != "user" && row.type_ != "synthetic") {
            return Err(InboxError::Conflict(id.to_string()));
        }
        let message = decode_message_row(&row)?;
        Ok(message_promoted_info(&message, session_id, delivery))
    }

    /// Promotes pending input into visible messages
    /// (`SessionInbox.promote`, inbox.ts:496-531). Returns the promoted
    /// count, or `None` when the runner must first handle a pending control.
    pub async fn promote(
        &self,
        session_id: &str,
        scope: Promotable,
    ) -> std::result::Result<Option<usize>, InboxError> {
        self.serialized(session_id, async {
            let steers = self.pending_steers(session_id).await?;
            if !steers.is_empty() || scope == Promotable::Steer {
                let control = steers.iter().position(is_control);
                if control == Some(0) {
                    return Ok(None);
                }
                let batch = match control {
                    Some(index) => &steers[..index],
                    None => &steers[..],
                };
                return Ok(Some(self.publish_deliveries(session_id, batch).await?));
            }

            let queued_session = session_id.to_string();
            let queued: Option<SessionInboxRow> = self
                .db
                .transaction(move |tx| {
                    Ok(tx.conn()
                        .query_row(
                            "SELECT * FROM session_inbox WHERE session_id = ?1 AND delivery = 'queue' \
                             ORDER BY enqueued_seq LIMIT 1",
                            rusqlite::params![queued_session],
                            SessionInboxRow::decode_row,
                        )
                        .optional()?)
                })
                .await?;
            let Some(queued) = queued else { return Ok(Some(0)) };
            if is_control(&queued) {
                return Ok(None);
            }
            let promoted = self.publish_deliveries(session_id, &[queued]).await?;
            let arrived = self.pending_steers(session_id).await?;
            let control = arrived.iter().position(is_control);
            let batch = match control {
                Some(index) => &arrived[..index],
                None => &arrived[..],
            };
            Ok(Some(promoted + self.publish_deliveries(session_id, batch).await?))
        })
        .await
    }

    /// Publishes `session.inbox.delivered` for each row
    /// (`publish`, inbox.ts:458-488): compaction rows die as conflicts;
    /// a conflict for an id already promoted-from-message is swallowed.
    async fn publish_deliveries(
        &self,
        session_id: &str,
        rows: &[SessionInboxRow],
    ) -> std::result::Result<usize, InboxError> {
        for row in rows {
            if row.type_ == "compaction" {
                return Err(InboxError::Conflict(row.id.clone()));
            }
            let data = serde_json::json!({ "sessionID": session_id, "inboxID": row.id });
            if let Err(err) = self
                .bus
                .publish(
                    Self::definition("session.inbox.delivered"),
                    data,
                    PublishOptions::default(),
                )
                .await
            {
                match conflict_id(&err) {
                    Some(_) => {
                        let delivery = delivery_from(&row.delivery)?;
                        let stored =
                            self.promoted_from_message(session_id, &row.id, delivery).await?;
                        if stored.is_none() {
                            return Err(err.into());
                        }
                    }
                    None => return Err(err.into()),
                }
            }
        }
        Ok(rows.len())
    }

    /// Steer rows in enqueue order with control reordering
    /// (`pendingSteers`, inbox.ts:533-550).
    async fn pending_steers(&self, session_id: &str) -> Result<Vec<SessionInboxRow>> {
        let session_id = session_id.to_string();
        let mut rows = self
            .db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT * FROM session_inbox WHERE session_id = ?1 AND delivery = 'steer' \
                     ORDER BY enqueued_seq",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![session_id], SessionInboxRow::decode_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        reorder_controls(&mut rows);
        Ok(rows)
    }
}

/// A move changes the context's Location: never pull compaction across it.
/// Within that boundary, compact before promoting even earlier steers so
/// their text stays verbatim after the checkpoint, not inside its summary.
pub(crate) fn reorder_controls(rows: &mut [SessionInboxRow]) {
    let Some(control) = rows.iter().position(is_control) else { return };
    if control > 0 && rows[control].type_ == "compaction" {
        let row = rows[control].clone();
        for index in (0..control).rev() {
            rows[index + 1] = rows[index].clone();
        }
        rows[0] = row;
    }
}

fn existing_session(info: &Info) -> SessionId {
    match info {
        Info::User { session_id, .. }
        | Info::Synthetic { session_id, .. }
        | Info::Compaction { session_id, .. }
        | Info::Move { session_id, .. } => session_id.clone(),
    }
}

fn existing_type(info: &Info) -> &'static str {
    match info {
        Info::User { .. } => "user",
        Info::Synthetic { .. } => "synthetic",
        Info::Compaction { .. } => "compaction",
        Info::Move { .. } => "move",
    }
}

fn item_delivery(item: &Item) -> Delivery {
    match item {
        Item::User { delivery, .. }
        | Item::Synthetic { delivery, .. }
        | Item::Compaction { delivery, .. }
        | Item::Move_ { delivery, .. } => *delivery,
    }
}

fn item_type(item: &Item) -> &'static str {
    match item {
        Item::User { .. } => "user",
        Item::Synthetic { .. } => "synthetic",
        Item::Compaction { .. } => "compaction",
        Item::Move_ { .. } => "move",
    }
}

/// Builds the admitted [`Info`] from the publish result (`admit`'s
/// event.created stamping).
fn info_from_item(id: MessageId, session_id: SessionId, created: u64, item: Item) -> Info {
    let time = session_inbox::EnqueuedTime { created };
    match item {
        Item::User { payload, delivery } => Info::User { id, session_id, time, payload, delivery },
        Item::Synthetic { payload, delivery } => {
            Info::Synthetic { id, session_id, time, payload, delivery }
        }
        Item::Compaction { payload, delivery } => {
            Info::Compaction { id, session_id, time, payload, delivery }
        }
        Item::Move_ { payload, delivery } => Info::Move { id, session_id, time, payload, delivery },
    }
}

/// Decodes a `session_message` row into a message (`decodeMessageRow`).
pub(crate) fn decode_message_row(
    row: &SessionMessageRow,
) -> std::result::Result<session_message::Info, InboxError> {
    let mut data = decode_json(&row.data)?;
    let object =
        data.as_object_mut().ok_or_else(|| DbError::Pragma("message data not an object".into()))?;
    object.insert("id".into(), serde_json::json!(row.id));
    object.insert("type".into(), serde_json::json!(row.type_));
    Ok(serde_json::from_value(data)?)
}

/// The promoted-from-message inbox view of a projected user/synthetic
/// message (`promotedFromMessage`).
fn message_promoted_info(
    message: &session_message::Info,
    session_id: String,
    delivery: Delivery,
) -> Option<Info> {
    let session_id = sid(session_id);
    match message {
        session_message::Info::User { id, time, text, files, agents, skills, metadata, .. } => {
            Some(Info::User {
                id: id.clone(),
                session_id: session_id.clone(),
                time: session_inbox::EnqueuedTime { created: time.created },
                payload: UserPayload {
                    text: text.clone(),
                    files: files.clone(),
                    agents: agents.clone(),
                    skills: skills.clone(),
                    metadata: metadata.clone(),
                },
                delivery,
            })
        }
        session_message::Info::Synthetic { id, time, text, description, metadata, .. } => {
            Some(Info::Synthetic {
                id: id.clone(),
                session_id,
                time: session_inbox::EnqueuedTime { created: time.created },
                payload: SyntheticPayload {
                    text: text.clone(),
                    description: description.clone(),
                    metadata: metadata.clone(),
                },
                delivery,
            })
        }
        _ => None,
    }
}

/// `find` over a transaction.
pub(crate) fn find_tx(tx: &Tx<'_>, id: &str) -> Result<Option<Info>> {
    match SessionInboxRow::get(tx, id.to_string())? {
        Some(row) => Ok(Some(info_from_row(&row)?)),
        None => Ok(None),
    }
}

/// `list` over a transaction.
pub(crate) fn list_tx(tx: &Tx<'_>, session_id: &str) -> Result<Vec<Info>> {
    let mut statement = tx
        .conn()
        .prepare("SELECT * FROM session_inbox WHERE session_id = ?1 ORDER BY enqueued_seq")?;
    let rows = statement
        .query_map(rusqlite::params![session_id], SessionInboxRow::decode_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.iter().map(info_from_row).collect()
}

/// `nextPromotable` over a transaction.
pub(crate) fn next_promotable_tx(
    tx: &Tx<'_>,
    session_id: &str,
    promotable: Promotable,
) -> Result<Option<Info>> {
    let mut statement = tx.conn().prepare(
        "SELECT * FROM session_inbox WHERE session_id = ?1 AND delivery = 'steer' ORDER BY enqueued_seq",
    )?;
    let mut steers = statement
        .query_map(rusqlite::params![session_id], SessionInboxRow::decode_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    reorder_controls(&mut steers);
    if let Some(first) = steers.first() {
        return Ok(Some(info_from_row(first)?));
    }
    if promotable != Promotable::Input {
        return Ok(None);
    }
    let queued = tx
        .conn()
        .query_row(
            "SELECT * FROM session_inbox WHERE session_id = ?1 AND delivery = 'queue' \
             ORDER BY enqueued_seq LIMIT 1",
            rusqlite::params![session_id],
            SessionInboxRow::decode_row,
        )
        .optional()?;
    queued.as_ref().map(info_from_row).transpose()
}

// ---------------------------------------------------------------------------
// In-transaction projection (bus projector callbacks)
// ---------------------------------------------------------------------------

/// `SessionInbox.projectAdmitted`: INSERT the pending row (on conflict the
/// admission loses the race ⇒ lifecycle conflict); a message id already
/// projected is likewise a conflict.
pub fn project_admitted(
    tx: &mut Tx<'_>,
    enqueued_seq: i64,
    id: &str,
    session_id: &str,
    item: &Item,
    time_created: u64,
) -> Result<()> {
    if SessionMessageRow::get(tx, id.to_string())?.is_some() {
        return Err(tx_conflict(id));
    }
    let row = SessionInboxRow {
        id: id.to_string(),
        session_id: session_id.to_string(),
        type_: item_type(item).to_string(),
        payload: payload_json(item),
        delivery: delivery_str(item_delivery(item)).to_string(),
        enqueued_seq,
        time_created: i64::try_from(time_created).unwrap_or(0),
    };
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO session_inbox (id, session_id, type, payload, delivery, enqueued_seq, time_created) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            row.id,
            row.session_id,
            row.type_,
            row.payload,
            row.delivery,
            row.enqueued_seq,
            row.time_created
        ],
    )?;
    if inserted == 0 {
        return Err(tx_conflict(id));
    }
    Ok(())
}

/// `SessionInbox.projectDelivered`: DELETE the pending row (pending-only
/// table) and return its decoded content.
pub fn project_delivered(tx: &mut Tx<'_>, session_id: &str, id: &str) -> Result<Info> {
    let deleted = tx
        .conn()
        .query_row(
            "DELETE FROM session_inbox WHERE id = ?1 AND session_id = ?2 RETURNING *",
            rusqlite::params![id, session_id],
            SessionInboxRow::decode_row,
        )
        .optional()?;
    match deleted {
        Some(row) => info_from_row(&row),
        None => Err(tx_conflict(id)),
    }
}

/// `SessionInbox.projectCancelled`: delete queue|steer rows only.
pub fn project_cancelled(tx: &mut Tx<'_>, session_id: &str, id: &str) -> Result<()> {
    let deleted = tx.execute(
        "DELETE FROM session_inbox WHERE id = ?1 AND session_id = ?2 \
         AND delivery IN ('queue', 'steer')",
        rusqlite::params![id, session_id],
    )?;
    if deleted == 0 {
        return Err(tx_conflict(id));
    }
    Ok(())
}

/// `SessionInbox.projectDeliveryChanged`: guarded flip — the row must
/// currently hold the opposite delivery.
pub fn project_delivery_changed(
    tx: &mut Tx<'_>,
    session_id: &str,
    id: &str,
    delivery: &str,
) -> Result<()> {
    let from = if delivery == "steer" { "queue" } else { "steer" };
    let updated = tx.execute(
        "UPDATE session_inbox SET delivery = ?1 WHERE id = ?2 AND session_id = ?3 AND delivery = ?4",
        rusqlite::params![delivery, id, session_id, from],
    )?;
    if updated == 0 {
        return Err(tx_conflict(id));
    }
    Ok(())
}
