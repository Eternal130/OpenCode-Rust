//! Event bus — port of `packages/core/src/bus.ts` (`core-deep-map.md` §2).
//!
//! # Persist-then-publish invariant (binding)
//!
//! **Durable-event visibility order must match commit order.** A durable
//! event becomes visible to *any* observer (projectors, listeners, live
//! streams, durable wakes / `log(follow)`) only after its SQLite transaction
//! committed successfully. This is enforced structurally:
//!
//! 1. Publishers serialize per aggregate on a keyed async mutex
//!    (`DashMap<AggregateId, Arc<tokio::sync::Mutex>>`) **before** opening
//!    the transaction, so sequence allocation, projection and notification
//!    for one aggregate are totally ordered.
//! 2. Projectors and `commit` hooks run **inside** the transaction; their
//!    failure rolls the transaction back (`Database::transaction` rolls
//!    back on `Err`), and every post-commit step — routing-map apply,
//!    durable wake, listener/stream notification — sits on the `Ok` path
//!    *after* the transaction future resolves. A rolled-back publish
//!    notifies nothing and leaves no row.
//! 3. Notification happens while the aggregate lock is still held, so
//!    observers see one aggregate's events in exactly commit (`seq`) order.
//! 4. The `(aggregate_id, seq)` unique index backstops cross-process races.
//!
//! # Shape (v2 parity)
//!
//! - **Ephemeral** events never touch the database: routes are computed and
//!   applied immediately, then listeners are notified **unisolated** — a
//!   failing listener fails the publish (`bus.ts:481`).
//! - **Durable** events: under the aggregate lock, one `BEGIN IMMEDIATE`
//!   transaction reads the latest sequence, allocates `latest + 1`, runs
//!   projectors keyed by `"{type}.{version}"`, runs the `commit(seq)` hook,
//!   upserts `event_sequence` with `max()`, and — when persisting — inserts
//!   the `event` row. After commit: routes apply, the aggregate's durable
//!   wake fires, and listeners are notified **isolated** (errors logged,
//!   never propagated).
//! - `publish_all`: one aggregate, one lock, one transaction,
//!   `latest+1 ..= latest+n`, batch insert, post-commit notify in order.
//! - `persist` defaults to **false** (the server turns it on; core tests
//!   may run memory-only).
//! - `log`: paged (512) ordered read; unknown versioned types are skipped
//!   while the raw cursor still advances; exactly one `log.synced` marker
//!   at the captured watermark; `follow` registers the wake **before** the
//!   historical read and re-arms it before every tail read, so no event can
//!   fall between replay and live.

#![allow(clippy::must_use_candidate)] // stream/handle constructors, no side effects

pub mod registry;
pub mod routing;
pub mod sink;
#[cfg(test)]
mod tests;

pub use registry::{
    by_type, by_type_latest, by_versioned_type, config_updated, definitions, is_session_event,
    location_shutdown, non_session_definitions, permission_asked, permission_replied,
    project_updated, DurableMeta, EventDefinition,
};
pub use sink::PermissionBusSink;

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use oc_schema::event::{versioned_type, DurableInfo};
use oc_schema::events::LogSynced;
use oc_schema::id::EventId;
use oc_schema::location;
use oc_util::determinism::{AscendingIdGen, Clock, IdGen, SequentialIdGen, SystemClock, SystemRng};
use parking_lot::{Mutex, RwLock};
use rusqlite::OptionalExtension;
use tokio::sync::{mpsc, Notify};

use crate::database::error::{DbError, Result as DbResult};
use crate::database::schema::EventRow;
use crate::database::{Database, Tx};

/// Alias for the per-aggregate async mutex.
type AsyncMutex = tokio::sync::Mutex<()>;

/// Bus errors. `Invalid` mirrors v2's `InvalidDurableEventError` defects
/// (aggregate/sequence/replay invariants); the port surfaces them as
/// `Err` instead of aborting.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// A durable-event invariant was violated.
    #[error("invalid durable event {type_:?}: {message}")]
    Invalid { type_: String, message: String },
    /// An ephemeral listener failed (durable publishes log these instead).
    #[error("event listener failed: {0}")]
    Listener(String),
    /// Underlying database failure.
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Error type returned by [`Listener`]s.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ListenerError(pub String);

/// A published event in the wire shape of `Event.Payload` (durable events
/// carry `durable: { aggregateID, seq, version }`; ephemeral ones omit it).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub id: EventId,
    /// Epoch milliseconds.
    pub created: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable: Option<DurableInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<location::Ref>,
    pub data: serde_json::Value,
}

impl Event {
    /// The aggregate id, when this is a committed durable event.
    #[must_use]
    pub fn aggregate_id(&self) -> Option<&str> {
        self.durable.as_ref().map(|info| info.aggregate_id.as_str())
    }

    /// The durable sequence, when present.
    #[must_use]
    pub fn seq(&self) -> Option<i64> {
        self.durable.as_ref().map(|info| i64::try_from(info.seq).unwrap_or(0))
    }
}

/// A committed durable event handed to projectors and commit hooks inside
/// the transaction (`durable` is guaranteed present).
#[derive(Debug, Clone, PartialEq)]
pub struct DurableEvent {
    pub id: EventId,
    pub created: u64,
    pub metadata: Option<serde_json::Value>,
    pub type_: String,
    pub durable: DurableInfo,
    pub location: Option<location::Ref>,
    pub data: serde_json::Value,
}

impl DurableEvent {
    /// The dynamic [`Event`] view of this committed event.
    #[must_use]
    pub fn to_event(&self) -> Event {
        Event {
            id: self.id.clone(),
            created: self.created,
            metadata: self.metadata.clone(),
            type_: self.type_.clone(),
            durable: Some(self.durable.clone()),
            location: self.location.clone(),
            data: self.data.clone(),
        }
    }
}

/// Local operational projection committed atomically with a durable event;
/// not replayed or serialized (`PublishOptions.commit`, bus.ts:96-97).
///
/// Runs inside the transaction with the allocated sequence, before the
/// `event_sequence` upsert. Must only use the passed transaction — calling
/// back into [`Bus`] database methods from a hook deadlocks the connection.
pub type CommitHook = Box<dyn for<'c> FnOnce(&mut Tx<'c>, i64) -> DbResult<()> + Send>;

/// Synchronous projector executed inside the durable-commit transaction.
/// A projector failure rolls the transaction back (event and projection
/// land together or not at all). Like commit hooks, projectors must only
/// use the passed transaction.
pub type Projector = Arc<dyn for<'c> Fn(&mut Tx<'c>, &DurableEvent) -> DbResult<()> + Send + Sync>;

/// Sync listener tapped on every publish (`Bus.listen`). On durable
/// publishes errors are logged, never propagated; on ephemeral publishes
/// the first listener error fails the publish.
pub type Listener = Arc<dyn Fn(&Event) -> std::result::Result<(), ListenerError> + Send + Sync>;

/// Options for [`Bus::publish`] (v2 `PublishOptions` plus the per-call
/// `persist` override).
#[derive(Default)]
pub struct PublishOptions {
    pub id: Option<EventId>,
    pub metadata: Option<serde_json::Value>,
    pub location: Option<location::Ref>,
    /// Suppress the location entirely (overrides `location`).
    pub global: bool,
    /// Persist the event row for historical reads; defaults to the bus
    /// level [`BusOptions::persist`].
    pub persist: Option<bool>,
    pub commit: Option<CommitHook>,
}

/// One entry of a [`Bus::publish_all`] batch.
pub struct PublishInput {
    pub definition: EventDefinition,
    pub data: serde_json::Value,
    pub id: Option<EventId>,
    pub metadata: Option<serde_json::Value>,
    pub location: Option<location::Ref>,
    pub global: bool,
    pub commit: Option<CommitHook>,
}

impl PublishInput {
    /// Minimal durable input.
    #[must_use]
    pub fn new(definition: EventDefinition, data: serde_json::Value) -> Self {
        Self {
            definition,
            data,
            id: None,
            metadata: None,
            location: None,
            global: false,
            commit: None,
        }
    }
}

/// A serialized durable event, as accepted by [`Bus::replay`] and produced
/// by reading the `event` table. `type_` is the **versioned** type
/// (`"session.deleted.2"`).
#[derive(Debug, Clone, PartialEq)]
pub struct SerializedEvent {
    pub id: EventId,
    pub type_: String,
    pub created: Option<u64>,
    pub seq: i64,
    pub aggregate_id: String,
    pub data: serde_json::Value,
}

/// Options for [`Bus::replay`].
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    /// Notify subscribers when the replay actually commits.
    pub publish: bool,
    /// Owner stamp for the sequence upsert.
    pub owner_id: Option<String>,
    /// Error on owner mismatch instead of skipping.
    pub strict_owner: bool,
}

/// Marker/event union emitted by [`Bus::log`].
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // the union mirrors the wire shapes
pub enum LogItem {
    Event(Event),
    Synced(LogSynced),
}

/// Input for [`Bus::log`].
#[derive(Debug, Clone)]
pub struct LogInput {
    pub aggregate_id: String,
    /// Resume cursor (exclusive); `-1` from the start.
    pub after: Option<i64>,
    /// Replay then transition to live.
    pub follow: bool,
}

/// Construction options.
pub struct BusOptions {
    pub clock: Arc<dyn Clock>,
    /// Event id source (`evt_…` prefix).
    pub ids: Arc<dyn IdGen>,
    /// Retain durable event payloads for historical reads and replay.
    pub persist: bool,
    /// Maximum durable rows read per log page (v2 default 512).
    pub log_read_page_size: i64,
}

impl Default for BusOptions {
    fn default() -> Self {
        let ids: Arc<dyn IdGen> =
            Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SystemRng)));
        Self { clock: Arc::new(SystemClock), ids, persist: false, log_read_page_size: 512 }
    }
}

impl BusOptions {
    /// Deterministic options for tests: a fixed clock plus sequential ids.
    #[must_use]
    pub fn deterministic() -> Self {
        Self {
            clock: Arc::new(oc_util::determinism::FixedClock::epoch()),
            ids: Arc::new(SequentialIdGen::new()),
            persist: false,
            log_read_page_size: 512,
        }
    }
}

/// One item on the internal live/typed channels: the event plus its routing
/// snapshot (`bus.ts` keeps this in a `WeakMap` alongside the payload so slow
/// subscribers still see the route captured at publish time).
#[derive(Debug, Clone)]
pub(crate) struct Routed {
    pub event: Event,
    pub routes: Option<Arc<Vec<location::Ref>>>,
}

/// Live subscription over the unbounded broadcast stream. Implements
/// [`futures::Stream`]; `.local(ref)` restricts delivery to one location
/// using the routing map (`Bus.subscribe` ambient-location filter).
pub struct Subscription {
    rx: mpsc::UnboundedReceiver<Routed>,
    local: Option<location::Ref>,
    types: Option<std::collections::HashSet<String>>,
}

impl Subscription {
    /// Restricts this subscription to events routed at `location` (or
    /// carrying no location at all). Consumes and returns self for chaining.
    #[must_use]
    pub fn local(mut self, location: &location::Ref) -> Self {
        self.local = Some(location.clone());
        self
    }
}

impl futures::Stream for Subscription {
    type Item = Event;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            match self.rx.poll_recv(cx) {
                std::task::Poll::Ready(Some(routed)) => {
                    if let Some(types) = &self.types {
                        if !types.contains(&routed.event.type_) {
                            continue;
                        }
                    }
                    if let Some(local) = &self.local {
                        let matches = |r: &location::Ref| {
                            r.directory == local.directory && r.workspace_id == local.workspace_id
                        };
                        let delivered = match &routed.routes {
                            Some(refs) => refs.iter().any(matches),
                            // v2: no routing entry → fall back to the
                            // envelope location; unlocated events pass.
                            None => routed.event.location.as_ref().is_none_or(matches),
                        };
                        if !delivered {
                            continue;
                        }
                    }
                    return std::task::Poll::Ready(Some(routed.event));
                }
                std::task::Poll::Ready(None) => return std::task::Poll::Ready(None),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
    }
}

/// Stream of [`LogItem`]s from [`Bus::log`].
pub struct LogStream {
    rx: mpsc::Receiver<std::result::Result<LogItem, BusError>>,
}

impl futures::Stream for LogStream {
    type Item = std::result::Result<LogItem, BusError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Guard detaching a [`Bus::listen`] listener on drop.
pub struct ListenGuard {
    inner: Arc<Inner>,
    listener: Option<Listener>,
}

impl ListenGuard {
    /// Keeps the listener registered forever.
    pub fn detach(mut self) {
        self.listener = None;
    }
}

impl Drop for ListenGuard {
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            self.inner.listeners.write().retain(|l| !Arc::ptr_eq(l, &listener));
        }
    }
}

struct Inner {
    db: Database,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    persist: bool,
    page_size: i64,
    /// Per-aggregate publisher serialization (v2 `KeyedMutex`).
    locks: DashMap<String, Arc<AsyncMutex>>,
    live: Mutex<Vec<mpsc::UnboundedSender<Routed>>>,
    typed: DashMap<String, Mutex<Vec<mpsc::UnboundedSender<Routed>>>>,
    /// Per-aggregate durable wakes (v2 `PubSub.sliding(1)`).
    wakes: DashMap<String, Vec<Arc<Notify>>>,
    listeners: RwLock<Vec<Listener>>,
    /// `"{type}.{version}"` (durable) or `"{type}"` (ephemeral) → handlers.
    projectors: RwLock<HashMap<String, Vec<Projector>>>,
    sessions: routing::SessionRoutes,
}

impl Inner {
    fn lock_for(&self, aggregate_id: &str) -> Arc<AsyncMutex> {
        self.locks
            .entry(aggregate_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn wake_register(&self, aggregate_id: &str) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.wakes.entry(aggregate_id.to_string()).or_default().push(Arc::clone(&notify));
        notify
    }

    fn wake_remove(&self, aggregate_id: &str, notify: &Arc<Notify>) {
        if let Some(mut entry) = self.wakes.get_mut(aggregate_id) {
            entry.value_mut().retain(|n| !Arc::ptr_eq(n, notify));
        }
        self.wakes.remove_if(aggregate_id, |_, wakes| wakes.is_empty());
    }

    fn wake_publish(&self, aggregate_id: &str) {
        if let Some(entry) = self.wakes.get(aggregate_id) {
            for notify in entry.value() {
                notify.notify_waiters();
            }
        }
    }

    fn projectors_for(&self, key: &str) -> Vec<Projector> {
        self.projectors.read().get(key).cloned().unwrap_or_default()
    }

    fn next_id(&self) -> EventId {
        let raw = self.ids.new_id("evt");
        EventId::parse(raw).unwrap_or_else(|err| EventId(err.value))
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.clock.unix_ms()).unwrap_or(0)
    }

    /// Listener → typed channel → live channel, in v2 order. `isolate`
    /// mirrors `notify(event, isolateListeners)`: durable publishes log
    /// listener errors, ephemeral ones propagate them.
    fn notify(&self, routed: &Routed, isolate: bool) -> std::result::Result<(), BusError> {
        for listener in self.listeners.read().clone() {
            match listener(&routed.event) {
                Ok(()) => {}
                Err(err) if isolate => {
                    // Logged, never propagated (bus.ts:486-492, 505).
                    eprintln!(
                        "[bus] event listener failed: event={} type={}: {err}",
                        routed.event.id, routed.event.type_
                    );
                }
                Err(err) => return Err(BusError::Listener(err.to_string())),
            }
        }
        if let Some(mut entry) = self.typed.get_mut(&routed.event.type_) {
            entry.value_mut().lock().retain(|tx| tx.send(routed.clone()).is_ok());
        }
        self.live.lock().retain(|tx| tx.send(routed.clone()).is_ok());
        Ok(())
    }
}

fn invalid(type_: &str, message: impl Into<String>) -> BusError {
    BusError::Invalid { type_: type_.to_string(), message: message.into() }
}

/// Invariant violations raised *inside* transaction closures must travel as
/// [`DbError`] (the tx API carries nothing else); they are wrapped with this
/// marker and lifted back into [`BusError::Invalid`] by [`lift_db_error`].
const INVALID_MARKER: &str = "\u{0}bus-invalid\u{1}";

fn tx_invalid(type_: &str, message: impl Into<String>) -> DbError {
    DbError::Pragma(format!("{INVALID_MARKER}{type_}\u{1}{}", message.into()))
}

/// Converts a database error from a bus transaction, restoring tunnelled
/// [`BusError::Invalid`] invariants.
fn lift_db_error(err: DbError) -> BusError {
    match err {
        DbError::Pragma(message) if message.starts_with(INVALID_MARKER) => {
            let mut parts = message[INVALID_MARKER.len()..].splitn(2, '\u{1}');
            let type_ = parts.next().unwrap_or_default().to_string();
            let message = parts.next().unwrap_or_default().to_string();
            BusError::Invalid { type_, message }
        }
        other => other.into(),
    }
}

/// Extracts the aggregate id from an event payload (v2 dies when the field
/// is missing or not a string).
fn aggregate_field(data: &serde_json::Value, field: &str) -> Option<String> {
    data.get(field).and_then(serde_json::Value::as_str).map(str::to_string)
}

/// Replay/claim input for [`Bus::commit_durable_event`].
struct ReplayInput {
    seq: i64,
    aggregate_id: String,
    owner_id: Option<String>,
    strict_owner: bool,
}

/// Successful durable commit produced inside the transaction.
struct CommitRecord {
    durable_events: Vec<DurableEvent>,
    routing: routing::Routing,
}

/// Post-commit view of a single-event commit.
struct Committed {
    event: Event,
    routed: Routed,
}

/// The event bus service. Cloning shares all state.
#[derive(Clone)]
pub struct Bus {
    inner: Arc<Inner>,
    /// The location envelope this handle stamps on publishes that carry
    /// none (v2's location-scoped bus layer — `Bus.layer(ref)`). `None`
    /// on the global bus handle; location services hold scoped clones.
    default_location: Option<location::Ref>,
}

impl Bus {
    /// Assembles the bus over an opened database.
    #[must_use]
    pub fn new(db: Database, options: BusOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                clock: options.clock,
                ids: options.ids,
                persist: options.persist,
                page_size: options.log_read_page_size.max(1),
                locks: DashMap::with_capacity(4),
                live: Mutex::new(Vec::new()),
                typed: DashMap::with_capacity(4),
                wakes: DashMap::with_capacity(4),
                listeners: RwLock::new(Vec::new()),
                projectors: RwLock::new(HashMap::new()),
                sessions: RwLock::new(HashMap::new()),
            }),
            default_location: None,
        }
    }

    /// A handle that stamps `location` on every publish carrying none
    /// (v2's location-scoped bus — `Bus.layer(ref)`): location services
    /// publish session-scoped frames through a scoped clone so the SSE
    /// grouping keys them by location. Explicit `location`/`global`
    /// options still win.
    #[must_use]
    pub fn scoped(&self, location: location::Ref) -> Self {
        Self { inner: Arc::clone(&self.inner), default_location: Some(location) }
    }

    /// The underlying database (diagnostics/tests).
    #[must_use]
    pub fn database(&self) -> &Database {
        &self.inner.db
    }

    /// Latest allocated sequence for an aggregate, `-1` when absent
    /// (`Bus.latestSequence`, bus.ts:21-32).
    pub async fn latest_sequence(&self, aggregate_id: &str) -> DbResult<i64> {
        let aggregate_id = aggregate_id.to_string();
        self.inner
            .db
            .transaction(move |tx| {
                let seq: Option<i64> = tx
                    .conn()
                    .query_row(
                        "SELECT seq FROM event_sequence WHERE aggregate_id = ?1",
                        rusqlite::params![aggregate_id],
                        |row| row.get(0),
                    )
                    .optional()?;
                Ok(seq.unwrap_or(-1))
            })
            .await
    }

    /// Pre-advances an aggregate's sequence (`Bus.reserveSequence`,
    /// bus.ts:34-48) — fork-inherited prefixes.
    pub async fn reserve_sequence(&self, aggregate_id: &str, seq: i64) -> DbResult<()> {
        let aggregate_id = aggregate_id.to_string();
        self.inner
            .db
            .transaction(move |tx| tx.event_sequence_upsert(&aggregate_id, seq, None))
            .await
    }

    /// Publishes one event (durable or ephemeral) — `bus.ts:507-529` plus
    /// `publishEvent` (:450-484).
    pub async fn publish(
        &self,
        definition: &EventDefinition,
        data: serde_json::Value,
        options: PublishOptions,
    ) -> std::result::Result<Event, BusError> {
        if !data.is_object() {
            return Err(invalid(&definition.type_, "Expected object event data"));
        }
        let event = Event {
            id: options.id.clone().unwrap_or_else(|| self.inner.next_id()),
            created: self.inner.now_ms(),
            metadata: options.metadata,
            type_: definition.type_.clone(),
            durable: None,
            location: if options.global {
                None
            } else {
                options.location.clone().or_else(|| self.default_location.clone())
            },
            data,
        };

        let Some(meta) = &definition.durable else {
            if options.commit.is_some() {
                return Err(invalid(&event.type_, "Local commit hooks require a durable event"));
            }
            // Ephemeral: routes computed and applied immediately, listeners
            // notified unisolated — no database writes.
            let routed = self.route_ephemeral(&event).await?;
            self.inner.notify(&routed, false)?;
            return Ok(event);
        };

        let aggregate_id = aggregate_field(&event.data, &meta.aggregate).ok_or_else(|| {
            invalid(&event.type_, format!("Expected string aggregate field {}", meta.aggregate))
        })?;
        let lock = self.inner.lock_for(&aggregate_id);
        let _guard = lock.lock().await;
        let committed = self
            .commit_durable_event(definition, event, None, options.commit, options.persist)
            .await?;
        // Persist-then-publish: notify only after the commit succeeded.
        // (Unreachable `None`: publishes carry no replay input.)
        let committed = committed.expect("publish never replays");
        let _ = self.inner.notify(&committed.routed, true);
        Ok(committed.event)
    }

    /// Publishes a batch to a single aggregate under one lock and one
    /// transaction (`bus.ts:531-669`): sequences `latest+1 ..= latest+n`,
    /// batch row insert, post-commit wake + notify each in order.
    #[allow(clippy::too_many_lines)]
    pub async fn publish_all(
        &self,
        events: Vec<PublishInput>,
    ) -> std::result::Result<Vec<Event>, BusError> {
        if events.is_empty() {
            return Err(invalid("", "publishAll requires at least one event"));
        }
        let created = self.inner.now_ms();
        let mut prepared = Vec::with_capacity(events.len());
        for input in events {
            let meta = input.definition.durable.clone().ok_or_else(|| {
                invalid(&input.definition.type_, "publishAll requires durable events")
            })?;
            if !input.data.is_object() {
                return Err(invalid(&input.definition.type_, "Expected object event data"));
            }
            let aggregate_id = aggregate_field(&input.data, &meta.aggregate).ok_or_else(|| {
                invalid(
                    &input.definition.type_,
                    format!("Expected string aggregate field {}", meta.aggregate),
                )
            })?;
            prepared.push(PreparedItem {
                type_: input.definition.type_,
                version: meta.version,
                aggregate_id,
                id: input.id.unwrap_or_else(|| self.inner.next_id()),
                created,
                metadata: input.metadata,
                location: if input.global {
                    None
                } else {
                    input.location.clone().or_else(|| self.default_location.clone())
                },
                data: input.data,
                commit: input.commit,
            });
        }
        let aggregate_id = prepared[0].aggregate_id.clone();
        if prepared.iter().any(|item| item.aggregate_id != aggregate_id) {
            return Err(invalid(
                &prepared[0].type_,
                "Published events must belong to the same aggregate",
            ));
        }

        let lock = self.inner.lock_for(&aggregate_id);
        let _guard = lock.lock().await;

        let tx_inner = Arc::clone(&self.inner);
        let aggregate_for_tx = aggregate_id.clone();
        let persist = self.inner.persist;
        let db = self.inner.db.clone();
        let record: CommitRecord = db
            .transaction(move |tx| {
                let latest: i64 = tx
                    .conn()
                    .query_row(
                        "SELECT seq FROM event_sequence WHERE aggregate_id = ?1",
                        rusqlite::params![aggregate_for_tx],
                        |row| row.get(0),
                    )
                    .optional()?
                    .unwrap_or(-1);
                let count = i64::try_from(prepared.len()).unwrap_or(i64::MAX / 2);
                let first_seq = latest + 1;
                let final_seq = first_seq + count - 1;

                let queued: Vec<DurableEvent> = prepared
                    .iter()
                    .enumerate()
                    .map(|(index, item)| DurableEvent {
                        id: item.id.clone(),
                        created: item.created,
                        metadata: item.metadata.clone(),
                        type_: item.type_.clone(),
                        durable: DurableInfo {
                            aggregate_id: aggregate_for_tx.clone(),
                            seq: u64::try_from(first_seq + i64::try_from(index).unwrap_or(0)).unwrap_or(0),
                            version: item.version,
                        },
                        location: item.location.clone(),
                        data: item.data.clone(),
                    })
                    .collect();
                let routing = routing::prepare(&tx_inner.sessions, tx.conn(), &queued)?;

                let mut rows: Vec<EventRow> = Vec::with_capacity(queued.len());
                let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
                for (index, item) in prepared.into_iter().enumerate() {
                    let seq = first_seq + i64::try_from(index).unwrap_or(0);
                    let key = versioned_type(&item.type_, item.version);
                    if persist {
                        if !ids.insert(item.id.as_str().to_string()) {
                            return Err(tx_invalid(
                                &item.type_,
                                format!("Event {} appears more than once in the batch", item.id),
                            ));
                        }
                        let stored: Option<(String, i64)> = tx
                            .conn()
                            .query_row(
                                "SELECT aggregate_id, seq FROM event WHERE id = ?1",
                                rusqlite::params![item.id.as_str()],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .optional()?;
                        if let Some((stored_aggregate, stored_seq)) = stored {
                            return Err(tx_invalid(
                                &item.type_,
                                format!(
                                    "Event {} already exists at aggregate {stored_aggregate} sequence {stored_seq}",
                                    item.id
                                ),
                            ));
                        }
                    }
                    let event = &queued[index];
                    for projector in tx_inner.projectors_for(&key) {
                        projector(tx, event)?;
                    }
                    if let Some(commit) = item.commit {
                        commit(tx, seq)?;
                    }
                    if persist {
                        rows.push(EventRow {
                            id: event.id.as_str().to_string(),
                            aggregate_id: aggregate_for_tx.clone(),
                            seq,
                            created: i64::try_from(event.created).unwrap_or(0),
                            type_: key,
                            data: event.data.to_string(),
                        });
                    }
                }
                tx.event_sequence_upsert(&aggregate_for_tx, final_seq, None)?;
                for row in &rows {
                    EventRow::insert(tx, row)?;
                }
                Ok(CommitRecord { durable_events: queued, routing })
            })
            .await
            .map_err(lift_db_error)?;
        // Post-commit: apply routing, wake, notify each in order.
        let resolved = record.routing.apply(&self.inner.sessions);
        self.inner.wake_publish(&aggregate_id);
        let mut out = Vec::with_capacity(record.durable_events.len());
        for (committed, refs) in record.durable_events.iter().zip(resolved) {
            let routed = Routed { event: committed.to_event(), routes: refs };
            let _ = self.inner.notify(&routed, true);
            out.push(routed.event);
        }
        Ok(out)
    }

    /// Ephemeral routing: computed and applied immediately (no
    /// transaction); the `session_v2` fallback reads outside any tx,
    /// mirroring `bus.ts:479`.
    async fn route_ephemeral(&self, event: &Event) -> std::result::Result<Routed, BusError> {
        // Ephemeral events have no durable envelope; routing only looks at
        // type/data/location, so a zero envelope placeholder is safe.
        let durable = DurableEvent {
            id: event.id.clone(),
            created: event.created,
            metadata: event.metadata.clone(),
            type_: event.type_.clone(),
            durable: DurableInfo { aggregate_id: String::new(), seq: 0, version: 0 },
            location: event.location.clone(),
            data: event.data.clone(),
        };
        let inner = Arc::clone(&self.inner);
        let routing_inner = Arc::clone(&self.inner);
        let routes = inner
            .db
            .with_conn(move |conn| {
                let routing = routing::prepare(&routing_inner.sessions, conn, &[durable])?;
                Ok(routing.apply(&routing_inner.sessions).into_iter().next().flatten())
            })
            .await?;
        Ok(Routed { event: event.clone(), routes })
    }

    /// The `commitDurableEvent` port (bus.ts:284-448), lock held by the
    /// caller. Returns `Ok(None)` for idempotent replay skips and
    /// foreign-owner skips (nothing committed, nothing notified).
    #[allow(clippy::too_many_lines)]
    async fn commit_durable_event(
        &self,
        definition: &EventDefinition,
        event: Event,
        input: Option<ReplayInput>,
        commit: Option<CommitHook>,
        persist_override: Option<bool>,
    ) -> std::result::Result<Option<Committed>, BusError> {
        let meta = definition.durable.clone().expect("caller verified durable definition");
        let aggregate_id = aggregate_field(&event.data, &meta.aggregate).ok_or_else(|| {
            invalid(&event.type_, format!("Expected string aggregate field {}", meta.aggregate))
        })?;
        if let Some(input) = &input {
            if input.aggregate_id != aggregate_id {
                return Err(invalid(
                    &event.type_,
                    format!(
                        "Aggregate mismatch: expected {}, got {aggregate_id}",
                        input.aggregate_id
                    ),
                ));
            }
        }

        let inner = Arc::clone(&self.inner);
        let persist = persist_override.unwrap_or(self.inner.persist);
        let type_version = versioned_type(&event.type_, meta.version);
        let projectors = inner.projectors_for(&type_version);
        let aggregate_for_tx = aggregate_id.clone();
        let owner_id = input.as_ref().and_then(|input| input.owner_id.clone());
        let event_type = event.type_.clone();

        let record: Option<CommitRecord> = self
            .inner
            .db
            .transaction(move |tx| {
                let row: Option<(i64, Option<String>)> = tx
                    .conn()
                    .query_row(
                        "SELECT seq, owner_id FROM event_sequence WHERE aggregate_id = ?1",
                        rusqlite::params![aggregate_for_tx],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let latest = row.as_ref().map_or(-1, |r| r.0);
                let current_owner = row.as_ref().and_then(|r| r.1.clone());

                if let Some(input) = &input {
                    if input.strict_owner {
                        if let Some(owner) = &current_owner {
                            if Some(owner.as_str()) != input.owner_id.as_deref() {
                                return Err(tx_invalid(
                                    &event_type,
                                    format!(
                                        "Replay owner mismatch for aggregate {}: expected {owner}, got {}",
                                        aggregate_for_tx,
                                        input.owner_id.as_deref().unwrap_or("none")
                                    ),
                                ));
                            }
                        }
                    }
                    if input.seq <= latest {
                        // Already applied: idempotent backfill or divergence.
                        if !persist {
                            return Ok(None);
                        }
                        let stored: Option<(String, String, i64, String)> = tx
                            .conn()
                            .query_row(
                                "SELECT id, type, created, data FROM event \
                                 WHERE aggregate_id = ?1 AND seq = ?2",
                                rusqlite::params![aggregate_for_tx, input.seq],
                                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                            )
                            .optional()?;
                        let created_i64 = i64::try_from(event.created).unwrap_or(0);
                        let identical = stored.is_some_and(|(id, type_, created, data)| {
                            id == event.id.as_str()
                                && type_ == type_version
                                && created == created_i64
                                // Deep-equal (v2 replay identity): string equality is only stable without key reordering.
                                && serde_json::from_str::<serde_json::Value>(&data).ok()
                                    == Some(event.data.clone())
                        });
                        if identical {
                            if let Some(owner) = &input.owner_id {
                                if current_owner.is_none() {
                                    tx.execute(
                                        "UPDATE event_sequence SET owner_id = ?1 \
                                         WHERE aggregate_id = ?2",
                                        rusqlite::params![owner, aggregate_for_tx],
                                    )?;
                                }
                            }
                            return Ok(None);
                        }
                        return Err(tx_invalid(
                            &event_type,
                            format!(
                                "Replay diverged at aggregate {} sequence {}",
                                aggregate_for_tx, input.seq
                            ),
                        ));
                    }
                    if let Some(owner) = &current_owner {
                        if Some(owner.as_str()) != input.owner_id.as_deref() {
                            // Foreign owner: skip silently.
                            return Ok(None);
                        }
                    }
                }

                let seq = input.as_ref().map_or(latest + 1, |input| input.seq);
                if input.is_some() && seq != latest + 1 {
                    return Err(tx_invalid(
                        &event_type,
                        format!(
                            "Sequence mismatch for aggregate {}: expected {}, got {seq}",
                            aggregate_for_tx,
                            latest + 1
                        ),
                    ));
                }

                if persist {
                    let stored: Option<(String, i64)> = tx
                        .conn()
                        .query_row(
                            "SELECT aggregate_id, seq FROM event WHERE id = ?1",
                            rusqlite::params![event.id.as_str()],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )
                        .optional()?;
                    if let Some((stored_aggregate, stored_seq)) = stored {
                        return Err(tx_invalid(
                            &event_type,
                            format!(
                                "Event {} already exists at aggregate {stored_aggregate} sequence {stored_seq}",
                                event.id
                            ),
                        ));
                    }
                }

                let durable_event = DurableEvent {
                    id: event.id.clone(),
                    created: event.created,
                    metadata: event.metadata.clone(),
                    type_: event.type_.clone(),
                    durable: DurableInfo {
                        aggregate_id: aggregate_for_tx.clone(),
                        seq: u64::try_from(seq).unwrap_or(0),
                        version: meta.version,
                    },
                    location: event.location.clone(),
                    data: event.data.clone(),
                };
                // Routing is computed inside the transaction (its
                // session_v2 fallback must see this commit's projections)
                // but applied only after commit.
                let routing = routing::prepare(&inner.sessions, tx.conn(), std::slice::from_ref(&durable_event))?;
                for projector in &projectors {
                    projector(tx, &durable_event)?;
                }
                if let Some(commit) = commit {
                    commit(tx, seq)?;
                }
                tx.event_sequence_upsert(&aggregate_for_tx, seq, owner_id.as_deref())?;
                if persist {
                    EventRow::insert(
                        tx,
                        &EventRow {
                            id: event.id.as_str().to_string(),
                            aggregate_id: aggregate_for_tx.clone(),
                            seq,
                            created: i64::try_from(event.created).unwrap_or(0),
                            type_: type_version,
                            data: event.data.to_string(),
                        },
                    )?;
                }
                Ok(Some(CommitRecord { durable_events: vec![durable_event], routing }))
            })
            .await
            .map_err(lift_db_error)?;
        let Some(record) = record else {
            return Ok(None);
        };
        // Persist-then-publish: routing apply and wake happen only on this
        // post-commit path; notification is triggered by the caller while
        // it still holds the aggregate lock.
        let routes = record.routing.apply(&self.inner.sessions).into_iter().next().flatten();
        self.inner.wake_publish(&aggregate_id);
        let event = record.durable_events[0].to_event();
        Ok(Some(Committed { event: event.clone(), routed: Routed { event, routes } }))
    }

    /// Volatile live channel: every event published from now on. Combine
    /// with [`Bus::log`] for reliability.
    pub fn subscribe(&self) -> Subscription {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner.live.lock().push(tx);
        Subscription { rx, local: None, types: None }
    }

    /// Lazily created per-type stream (`subscribe(def)`).
    pub fn subscribe_type(&self, type_: &str) -> Subscription {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner
            .typed
            .entry(type_.to_string())
            .or_insert_with(|| Mutex::new(Vec::new()))
            .lock()
            .push(tx);
        Subscription { rx, local: None, types: None }
    }

    /// Live stream filtered to a set of types (`subscribe([defs])`).
    pub fn subscribe_types(&self, types: &[&str]) -> Subscription {
        let mut subscription = self.subscribe();
        subscription.types = Some(types.iter().map(|t| (*t).to_string()).collect());
        subscription
    }

    /// Registers a sync listener tapped on every publish. Returns a guard
    /// that detaches on drop.
    pub fn listen(&self, listener: Listener) -> ListenGuard {
        self.inner.listeners.write().push(Arc::clone(&listener));
        ListenGuard { inner: Arc::clone(&self.inner), listener: Some(listener) }
    }

    /// Registers a projector for a definition. Durable definitions key by
    /// `"{type}.{version}"`; handlers run synchronously inside the
    /// durable-commit transaction.
    pub fn project(&self, definition: &EventDefinition, projector: Projector) {
        let key = match &definition.durable {
            Some(meta) => versioned_type(&definition.type_, meta.version),
            None => definition.type_.clone(),
        };
        self.inner.projectors.write().entry(key).or_default().push(projector);
    }

    /// Durable, ordered per-aggregate log read; `follow: false` completes
    /// after one `log.synced` marker at the captured watermark, `follow:
    /// true` replays then transitions to live with no gap (wake registered
    /// before the historical read, re-armed before every tail read).
    #[must_use]
    pub fn log(&self, input: LogInput) -> LogStream {
        let (tx, rx) = mpsc::channel(64);
        let bus = self.clone();
        let page_size = self.inner.page_size;
        tokio::spawn(async move {
            let aggregate_id = input.aggregate_id;
            let mut cursor = input.after.unwrap_or(-1);
            // Subscribe the wake BEFORE the historical read so events
            // committed during replay either appear in the read or arrive
            // through a post-marker wake (bus.ts:853-855).
            let wake = input.follow.then(|| bus.inner.wake_register(&aggregate_id));
            let mut armed = wake.as_ref().map(|notify| Box::pin(notify.notified()));
            if let Some(notified) = armed.as_mut() {
                let _ = notified.as_mut().enable();
            }

            let target = match bus.latest_sequence(&aggregate_id).await {
                Ok(target) => target,
                Err(err) => {
                    let _ = tx.send(Err(err.into())).await;
                    return;
                }
            };
            match bus.read_through(&aggregate_id, &mut cursor, target, page_size).await {
                Ok(events) => {
                    for event in events {
                        if tx.send(Ok(LogItem::Event(event))).await.is_err() {
                            return;
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(err.into())).await;
                    return;
                }
            }
            let marker = LogSynced {
                log_type: (),
                aggregate_id: aggregate_id.clone(),
                seq: (target >= 0).then(|| u64::try_from(target).unwrap_or(0)),
            };
            if tx.send(Ok(LogItem::Synced(marker))).await.is_err() {
                return;
            }
            if !input.follow {
                return;
            }
            let notify = wake.as_ref().expect("follow registers a wake");

            loop {
                // Await the armed wake or the consumer dropping the stream.
                {
                    let notified = armed.as_mut().expect("armed in follow mode");
                    tokio::select! {
                        () = notified.as_mut() => {}
                        () = tx.closed() => break,
                    }
                }
                // Re-arm BEFORE the tail read: a wake landing during the
                // read fires immediately after it (no gap).
                armed = Some(Box::pin(notify.notified()));
                if let Some(notified) = armed.as_mut() {
                    let _ = notified.as_mut().enable();
                }
                let Ok(target) = bus.latest_sequence(&aggregate_id).await else {
                    break;
                };
                if target <= cursor {
                    continue;
                }
                match bus.read_through(&aggregate_id, &mut cursor, target, page_size).await {
                    Ok(events) => {
                        for event in events {
                            if tx.send(Ok(LogItem::Event(event))).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err.into())).await;
                        return;
                    }
                }
            }
            if let Some(notify) = &wake {
                bus.inner.wake_remove(&aggregate_id, notify);
            }
        });
        LogStream { rx }
    }

    /// Paged read `after < seq <= through`, ordered, decoding via the
    /// durable manifest; unknown versioned types are skipped while the raw
    /// cursor still advances past them (bus.ts:787-806).
    async fn read_through(
        &self,
        aggregate_id: &str,
        cursor: &mut i64,
        through: i64,
        limit: i64,
    ) -> DbResult<Vec<Event>> {
        let mut events = Vec::new();
        loop {
            let aggregate = aggregate_id.to_string();
            let after = *cursor;
            let rows: Vec<(String, i64, i64, String, String)> = self
                .inner
                .db
                .transaction(move |tx| {
                    let mut statement = tx.conn().prepare(
                        "SELECT id, seq, created, type, data FROM event \
                         WHERE aggregate_id = ?1 AND seq > ?2 AND seq <= ?3 \
                         ORDER BY seq LIMIT ?4",
                    )?;
                    let rows = statement
                        .query_map(rusqlite::params![aggregate, after, through, limit], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    Ok(rows)
                })
                .await?;
            let Some(tail) = rows.last().map(|row| row.1) else {
                break;
            };
            for (id, seq, created, type_, data) in rows {
                // Unknown versioned type: skip the row entirely; the raw
                // cursor (tail below) still advances past it.
                if let Some(definition) = registry::by_versioned_type(&type_) {
                    if let Some(meta) = &definition.durable {
                        let data: serde_json::Value = serde_json::from_str(&data)?;
                        events.push(Event {
                            id: EventId(id),
                            created: u64::try_from(created).unwrap_or(0),
                            metadata: None,
                            type_: definition.type_.clone(),
                            durable: Some(DurableInfo {
                                aggregate_id: aggregate_id.to_string(),
                                seq: u64::try_from(seq).unwrap_or(0),
                                version: meta.version,
                            }),
                            location: None,
                            data,
                        });
                    }
                }
            }
            *cursor = tail;
            if tail >= through {
                break;
            }
        }
        Ok(events)
    }

    /// Re-commits a serialized durable event idempotently under the
    /// aggregate lock (`bus.ts:671-701`).
    pub async fn replay(
        &self,
        event: SerializedEvent,
        options: ReplayOptions,
    ) -> std::result::Result<(), BusError> {
        let definition = registry::by_versioned_type(&event.type_).ok_or_else(|| {
            invalid(&event.type_, format!("Unknown durable event type {}", event.type_))
        })?;
        let aggregate_id = event.aggregate_id.clone();
        let payload = Event {
            id: event.id,
            created: event.created.unwrap_or(0),
            metadata: None,
            type_: definition.type_.clone(),
            durable: None,
            location: None,
            data: event.data,
        };
        let input = ReplayInput {
            seq: event.seq,
            aggregate_id: aggregate_id.clone(),
            owner_id: options.owner_id,
            strict_owner: options.strict_owner,
        };
        let lock = self.inner.lock_for(&aggregate_id);
        let _guard = lock.lock().await;
        let committed =
            self.commit_durable_event(definition, payload, Some(input), None, None).await?;
        if let Some(committed) = committed {
            if options.publish {
                let _ = self.inner.notify(&committed.routed, true);
            }
        }
        Ok(())
    }

    /// Deletes an aggregate's event rows, sequence row and routing state
    /// (`bus.ts:703-715`).
    pub async fn remove(&self, aggregate_id: &str) -> std::result::Result<(), BusError> {
        let aggregate = aggregate_id.to_string();
        self.inner
            .db
            .transaction(move |tx| {
                tx.execute(
                    "DELETE FROM event_sequence WHERE aggregate_id = ?1",
                    rusqlite::params![aggregate],
                )?;
                tx.execute(
                    "DELETE FROM event WHERE aggregate_id = ?1",
                    rusqlite::params![aggregate],
                )?;
                Ok(())
            })
            .await?;
        self.inner.sessions.write().remove(aggregate_id);
        Ok(())
    }

    /// Stamps an aggregate's owner (`bus.ts:717-724`).
    pub async fn claim(
        &self,
        aggregate_id: &str,
        owner_id: &str,
    ) -> std::result::Result<(), BusError> {
        let aggregate = aggregate_id.to_string();
        let owner = owner_id.to_string();
        self.inner
            .db
            .transaction(move |tx| {
                tx.execute(
                    "UPDATE event_sequence SET owner_id = ?1 WHERE aggregate_id = ?2",
                    rusqlite::params![owner, aggregate],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

/// One validated entry of a `publish_all` batch.
struct PreparedItem {
    type_: String,
    version: u64,
    aggregate_id: String,
    id: EventId,
    created: u64,
    metadata: Option<serde_json::Value>,
    location: Option<location::Ref>,
    data: serde_json::Value,
    commit: Option<CommitHook>,
}
