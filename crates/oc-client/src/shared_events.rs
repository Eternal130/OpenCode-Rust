//! Shared event stream multiplexer — faithful port of the v2 client's
//! `packages/client/src/shared-events.ts`.
//!
//! One underlying connection is shared by every active subscriber
//! (refcounted): the first `poll` opens it, the last dropped subscriber
//! closes it. Events fan out to all subscribers; each has its own
//! bounded queue ([`SUBSCRIBER_CAPACITY`] events — a subscriber that
//! stops draining is terminated with [`ClientError::SubscriberCapacity`]
//! instead of stalling the connection). `onActivity` (keepalive chunks
//! that carry no event) fans out to subscribers that asked for it.
//!
//! Reconnection semantics (per G-14 contract facts): the v2 shared layer
//! has **no automatic retry/backoff** — when the source ends or fails,
//! every subscriber finishes (errors keep their identity, see
//! [`SharedSubscription::terminal_error`]) and a *fresh* `subscribe()`
//! opens a replacement connection. Since the server is live-only,
//! resynchronization after reconnect is snapshot-based and the caller's
//! concern (see `oc-tui::data`, which resubscribes with `Last-Event-ID`
//! and resyncs). Late subscribers receive only the latest
//! `server.connected` marker, never a replay of business events.
//!
//! Deviation from the TS original (narrowed by G-125): `ClientError` is
//! not `Clone` (`reqwest::Error`), so the repeatedly-delivered terminal
//! error is a field-wise clone proxy (transport failures degrade to a
//! [`ClientError::MalformedResponse`] description); the exact error
//! stays available via [`SharedSubscription::terminal_error`]. Since
//! G-125 the *delivery* matches the TS iterator: after a failed
//! completion, every `next()` rejects (previously the stream ended
//! after one `Err`, which made TS-semantics loops exit early).

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use futures::{Stream, StreamExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::error::ClientError;

/// Per-subscriber queued-event cap (v2 `capacity = 4_096`).
///
/// Capacity accounting is exactly the TS gate: `push` errors when
/// `queued.length === capacity` (shared-events.ts), i.e. the
/// (capacity+1)-th undelivered event terminates the subscriber; a
/// tokio channel of this size fails `try_send` on precisely that same
/// event, so the counts match one-for-one (G-376⑤ re-verified — no
/// off-by-one).
pub const SUBSCRIBER_CAPACITY: usize = 4096;

/// Events multiplexed by [`SharedEvents`]; knows the connection marker.
pub trait SharedEvent: Clone + Send + 'static {
    /// Whether this event is `server.connected` (the replay marker).
    fn is_server_connected(&self) -> bool;
}

impl SharedEvent for serde_json::Value {
    fn is_server_connected(&self) -> bool {
        self.get("type").and_then(serde_json::Value::as_str) == Some("server.connected")
    }
}

/// The boxed event source returned by a [`SharedEvents`] connect factory.
pub type EventStream<A> = Pin<Box<dyn Stream<Item = Result<A, ClientError>> + Send>>;

/// Per-connection activity callback: the factory wires it into the
/// transport (`SseStream::with_on_activity`) so keepalive chunks fan out
/// to subscribers that asked for them.
#[derive(Clone)]
pub struct Activity(Arc<dyn Fn() + Send + Sync>);

impl Activity {
    /// Signals transport activity (fires the shared fan-out).
    pub fn signal(&self) {
        (self.0)();
    }
}

/// Connect factory: opens one event source. Returning `Err` models the
/// TS synchronous `connect` throw (subscribers reject, no auto-retry).
pub type Connect<A> = Arc<dyn Fn(Activity) -> Result<EventStream<A>, ClientError> + Send + Sync>;

type ActivityCallback = Arc<dyn Fn() + Send + Sync>;

/// Shared connection multiplexer over one connect factory.
pub struct SharedEvents<A: SharedEvent> {
    inner: Arc<Inner<A>>,
}

// The connect factory is a closure (not `Debug`); `Client` embeds the
// cached multiplexer and derives `Debug`, so this manual impl keeps that
// derive working.
impl<A: SharedEvent> std::fmt::Debug for SharedEvents<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedEvents").finish_non_exhaustive()
    }
}

struct Inner<A: SharedEvent> {
    connect: Connect<A>,
    state: Mutex<State<A>>,
}

struct State<A: SharedEvent> {
    current: Option<Conn<A>>,
    next_sub_id: u64,
    next_conn_id: u64,
}

struct Conn<A: SharedEvent> {
    id: u64,
    subscribers: HashMap<u64, Sub<A>>,
    /// Latest `server.connected` marker (replayed to late subscribers).
    connected: Option<A>,
    task: JoinHandle<()>,
}

struct Sub<A: SharedEvent> {
    tx: mpsc::Sender<A>,
    on_activity: Option<ActivityCallback>,
    shared: Arc<SubShared>,
}

/// Completion state of a subscription: live, cleanly ended, failed, or
/// terminated for overflowing its queue.
enum Terminal {
    Live,
    Ended,
    /// The source failed. Buffered events stay readable first — the TS
    /// subscriber's `finish` overrides the discard default to `false`
    /// (shared-events.ts `finish(result, false)`).
    Failed(Arc<ClientError>),
    /// The subscriber overflowed its queue. Buffered events are
    /// DISCARDED — the TS `push` path calls the local `finish` closure
    /// whose discard default is `true`.
    Overflow(Arc<ClientError>),
}

/// Per-subscription terminal slot.
struct SubShared {
    terminal: Mutex<Terminal>,
}

impl SubShared {
    fn finish(&self, error: Option<Arc<ClientError>>) {
        *self.terminal.lock().expect("terminal lock") = match error {
            Some(error) => Terminal::Failed(error),
            None => Terminal::Ended,
        };
    }

    /// Overflow termination: the queue is discarded (the TS `push`
    /// path's local `finish` closure defaults `discard` to `true`).
    fn finish_overflow(&self, error: Arc<ClientError>) {
        *self.terminal.lock().expect("terminal lock") = Terminal::Overflow(error);
    }
}

impl<A: SharedEvent> SharedEvents<A> {
    /// Builds the shared multiplexer over `connect`.
    pub fn new(connect: Connect<A>) -> Self {
        Self {
            inner: Arc::new(Inner {
                connect,
                state: Mutex::new(State { current: None, next_sub_id: 0, next_conn_id: 0 }),
            }),
        }
    }

    /// Subscribes to the shared stream. Lazy: no connection is opened
    /// until the returned stream is first polled. Dropping the stream
    /// unsubscribes; the last dropped subscriber closes the connection.
    #[must_use]
    pub fn subscribe(&self) -> SharedSubscription<A> {
        self.subscribe_inner(None)
    }

    /// [`SharedEvents::subscribe`] plus an activity callback that fires
    /// for keepalive frames (v2 `SubscribeOptions.onActivity`).
    pub fn subscribe_with_activity(&self, on_activity: ActivityCallback) -> SharedSubscription<A> {
        self.subscribe_inner(Some(on_activity))
    }

    fn subscribe_inner(&self, on_activity: Option<ActivityCallback>) -> SharedSubscription<A> {
        let (tx, rx) = mpsc::channel(SUBSCRIBER_CAPACITY);
        let mut state = self.inner.state.lock().expect("state lock");
        let id = state.next_sub_id;
        state.next_sub_id += 1;
        SharedSubscription {
            inner: Arc::clone(&self.inner),
            on_activity,
            id,
            tx: Some(tx),
            rx: Some(rx),
            conn_id: None,
            shared: Arc::new(SubShared { terminal: Mutex::new(Terminal::Live) }),
        }
    }

    /// Opens (or joins) the current connection, registering the subscriber.
    fn register(
        inner: &Arc<Inner<A>>,
        sub_id: u64,
        tx: mpsc::Sender<A>,
        on_activity: Option<ActivityCallback>,
        shared: &Arc<SubShared>,
    ) -> u64 {
        let mut state = inner.state.lock().expect("state lock");
        if state.current.is_none() {
            let conn_id = state.next_conn_id;
            state.next_conn_id += 1;
            let activity = {
                let inner = Arc::clone(inner);
                Arc::new(move || {
                    let state = inner.state.lock().expect("state lock");
                    if let Some(conn) = state.current.as_ref() {
                        for sub in conn.subscribers.values() {
                            if let Some(cb) = &sub.on_activity {
                                cb();
                            }
                        }
                    }
                })
            };
            let task = tokio::spawn(run(Arc::clone(inner), conn_id, Activity(activity)));
            state.current =
                Some(Conn { id: conn_id, subscribers: HashMap::new(), connected: None, task });
        }
        let conn = state.current.as_mut().expect("current connection");
        let marker = conn.connected.clone();
        let conn_id = conn.id;
        conn.subscribers.insert(sub_id, Sub { tx, on_activity, shared: Arc::clone(shared) });
        // Late subscribers get the latest connection marker only —
        // never a replay of business events.
        if let Some(marker) = marker {
            if let Some(sub) = conn.subscribers.get_mut(&sub_id) {
                let _ = sub.tx.try_send(marker);
            }
        }
        conn_id
    }

    /// Stops a connection: aborts its task (dropping the event source,
    /// which closes the transport) and detaches it as `current`.
    fn stop(state: &mut State<A>, conn_id: u64) {
        if let Some(conn) = state.current.take() {
            if conn.id == conn_id {
                conn.task.abort();
            } else {
                state.current = Some(conn);
            }
        }
    }

    /// Finishes a connection: every subscriber is terminated with the
    /// completion result (already-buffered events stay readable first).
    fn finish(inner: &Arc<Inner<A>>, conn_id: u64, error: Option<&Arc<ClientError>>) {
        let conn = {
            let mut state = inner.state.lock().expect("state lock");
            match state.current.take() {
                Some(conn) if conn.id == conn_id => conn,
                other => {
                    state.current = other;
                    return;
                }
            }
        };
        for sub in conn.subscribers.values() {
            sub.shared.finish(error.cloned());
            // Dropping `conn` (and with it the tx senders) wakes pending
            // readers once their queues drain.
        }
    }

    /// Terminates an overflowing subscriber with the capacity error.
    fn overflow(inner: &Arc<Inner<A>>, conn_id: u64, sub_id: u64) {
        let mut state = inner.state.lock().expect("state lock");
        if let Some(conn) = state.current.as_mut() {
            if conn.id == conn_id {
                if let Some(sub) = conn.subscribers.remove(&sub_id) {
                    sub.shared.finish_overflow(Arc::new(ClientError::SubscriberCapacity(
                        SUBSCRIBER_CAPACITY,
                    )));
                }            }
        }
    }
}

/// The connection pump: reads the source, updates the marker, fans out.
async fn run<A: SharedEvent>(inner: Arc<Inner<A>>, conn_id: u64, activity: Activity) {
    let stream = match (inner.connect)(activity) {
        Ok(stream) => stream,
        Err(error) => {
            SharedEvents::finish(&inner, conn_id, Some(&Arc::new(error)));
            return;
        }
    };
    let mut stream = stream;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                let marker = event.is_server_connected();
                let overflowed = {
                    let mut state = inner.state.lock().expect("state lock");
                    let Some(conn) = state.current.as_mut() else { return };
                    if conn.id != conn_id {
                        return;
                    }
                    if marker {
                        conn.connected = Some(event.clone());
                    }
                    fan_out(conn, &event)
                };
                for sub_id in overflowed {
                    SharedEvents::overflow(&inner, conn_id, sub_id);
                }
            }
            Err(error) => {
                SharedEvents::finish(&inner, conn_id, Some(&Arc::new(error)));
                return;
            }
        }
    }
    SharedEvents::finish(&inner, conn_id, None);
}

/// Pushes `event` to every subscriber. Returns ids whose queue was full.
fn fan_out<A: SharedEvent>(conn: &mut Conn<A>, event: &A) -> Vec<u64> {
    let mut overflowed = Vec::new();
    conn.subscribers.retain(|id, sub| match sub.tx.try_send(event.clone()) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            overflowed.push(*id);
            true // terminated separately via `overflow`
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    });
    overflowed
}

/// One subscriber's view of the shared stream. Lazily joins the shared
/// connection on first poll; `Drop` unsubscribes (the last one out
/// closes the connection).
pub struct SharedSubscription<A: SharedEvent> {
    inner: Arc<Inner<A>>,
    on_activity: Option<ActivityCallback>,
    id: u64,
    tx: Option<mpsc::Sender<A>>,
    rx: Option<mpsc::Receiver<A>>,
    conn_id: Option<u64>,
    shared: Arc<SubShared>,
}

impl<A: SharedEvent> SharedSubscription<A> {
    /// The exact terminal error, if the shared source failed (identity-
    /// preserving `Arc`; survives the single `Err` the stream yields).
    #[must_use]
    pub fn terminal_error(&self) -> Option<Arc<ClientError>> {
        match &*self.shared.terminal.lock().expect("terminal lock") {
            Terminal::Failed(error) | Terminal::Overflow(error) => Some(Arc::clone(error)),
            _ => None,
        }
    }

    fn terminal_poll(&mut self) -> Poll<Option<Result<A, ClientError>>> {
        let terminal = self.shared.terminal.lock().expect("terminal lock");
        match &*terminal {
            Terminal::Live | Terminal::Ended => Poll::Ready(None),
            // G-125: the TS iterator rejects on EVERY next() after a
            // failed completion (shared-events.ts:76,123) — deliver the
            // terminal error repeatedly instead of ending the stream.
            // The clone is a field-wise proxy (`ClientError: !Clone`);
            // the identity-preserving original stays in terminal_error().
            Terminal::Failed(error) | Terminal::Overflow(error) => {
                Poll::Ready(Some(Err(clone_error(error))))
            }
        }
    }
}

/// Field-wise clone proxy (`ClientError: !Clone` because of
/// `reqwest::Error`; a transport failure degrades to a
/// [`ClientError::MalformedResponse`] description).
fn clone_error(error: &ClientError) -> ClientError {
    match error {
        ClientError::Api { status, name, message, fields } => ClientError::Api {
            status: *status,
            name: name.clone(),
            message: message.clone(),
            fields: fields.clone(),
        },
        ClientError::UnexpectedStatus { status, body } => {
            ClientError::UnexpectedStatus { status: *status, body: body.clone() }
        }
        ClientError::UnsupportedContentType(c) => ClientError::UnsupportedContentType(c.clone()),
        ClientError::MalformedResponse(m) => ClientError::MalformedResponse(m.clone()),
        ClientError::SseEventTooLarge(n) => ClientError::SseEventTooLarge(*n),
        ClientError::Transport(e) => ClientError::MalformedResponse(format!("transport: {e}")),
        ClientError::Cancelled => ClientError::Cancelled,
        ClientError::InvalidBaseUrl(u) => ClientError::InvalidBaseUrl(u.clone()),
        ClientError::Encoding(e) => ClientError::Encoding(e.clone()),
        ClientError::SubscriberCapacity(n) => ClientError::SubscriberCapacity(*n),
    }
}

impl<A: SharedEvent> Stream for SharedSubscription<A> {
    type Item = Result<A, ClientError>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(tx) = this.tx.take() {
            this.conn_id = Some(SharedEvents::register(
                &this.inner,
                this.id,
                tx,
                this.on_activity.clone(),
                &this.shared,
            ));
        }
        // G-376⑤: a FAILED source completion still yields buffered
        // events first — the v2 subscriber's `finish` overrides the
        // discard default to `false` (shared-events.ts
        // `finish(result, false)`), so events already queued when the
        // source errored stay readable and the terminal error applies
        // only after the queue drains (and then, per G-125, rejects on
        // every later `next()`). Only OVERFLOW discards its buffer (the
        // TS `push` path's local `finish` closure defaults discard to
        // `true`).
        if matches!(&*this.shared.terminal.lock().expect("terminal lock"), Terminal::Overflow(_)) {
            this.rx = None;
            return this.terminal_poll();
        }
        let Some(rx) = this.rx.as_mut() else {
            return this.terminal_poll();
        };
        match rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => Poll::Ready(Some(Ok(event))),
            // All senders gone: buffered events were drained, now the
            // terminal result applies.
            Poll::Ready(None) => {
                this.rx = None;
                this.terminal_poll()
            }
            // Overflow terminates immediately (buffered discarded),
            // matching the TS `push` behavior after completion. This
            // branch only trips once the queue is EMPTY (a non-empty
            // queue returns Ready above) — an empty queue plus a
            // terminal state means the completion result applies now.
            Poll::Pending => {
                if matches!(
                    &*this.shared.terminal.lock().expect("terminal lock"),
                    Terminal::Ended | Terminal::Failed(_)
                ) {
                    this.rx = None;
                    this.terminal_poll()
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

impl<A: SharedEvent> Drop for SharedSubscription<A> {
    fn drop(&mut self) {
        let Some(conn_id) = self.conn_id else { return };
        let mut state = self.inner.state.lock().expect("state lock");
        let should_stop = match state.current.as_mut() {
            Some(conn) if conn.id == conn_id => {
                conn.subscribers.remove(&self.id);
                conn.subscribers.is_empty()
            }
            _ => false,
        };
        if should_stop {
            SharedEvents::stop(&mut state, conn_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use futures::channel::mpsc as fmpsc;
    use futures::StreamExt;
    use serde_json::{json, Value};

    use super::*;

    type SourceSender = fmpsc::Sender<Result<Value, ClientError>>;

    /// Fake source harness mirroring the v2 `shared-events.test.ts`
    /// `source()`: scripted connections we can push into, close, or fail.
    #[derive(Clone, Default)]
    struct Harness {
        attempts: Arc<AtomicUsize>,
        senders: Arc<Mutex<Vec<SourceSender>>>,
        sync_error: Arc<Mutex<Option<ClientError>>>,
        activity: Arc<Mutex<Option<Activity>>>,
    }

    impl Harness {
        fn connect(&self, activity: Activity) -> Result<EventStream<Value>, ClientError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            *self.activity.lock().unwrap() = Some(activity);
            if let Some(error) = self.sync_error.lock().unwrap().take() {
                return Err(error);
            }
            let (tx, rx) = fmpsc::channel(16);
            self.senders.lock().unwrap().push(tx);
            Ok(Box::pin(rx))
        }

        fn shared(&self) -> SharedEvents<Value> {
            let harness = self.clone();
            SharedEvents::new(Arc::new(move |activity| harness.connect(activity)))
        }

        fn sender(&self, index: usize) -> SourceSender {
            self.senders.lock().unwrap()[index].clone()
        }

        /// Awaits connection `index` opening (mirrors the v2 harness's
        /// `opened` promises).
        async fn at(&self, index: usize) -> SourceSender {
            for _ in 0..2000 {
                {
                    let senders = self.senders.lock().unwrap();
                    if let Some(sender) = senders.get(index) {
                        return sender.clone();
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            panic!("connection {index} never opened");
        }

        fn push(&self, index: usize, event: Value) {
            let _ = self.sender(index).try_send(Ok(event));
        }

        fn close(&self, index: usize) {
            self.senders.lock().unwrap().truncate(index);
        }

        fn fail(&self, index: usize, error: ClientError) {
            let _ = self.sender(index).try_send(Err(error));
        }

        async fn source_closed(&self, index: usize) -> bool {
            for _ in 0..200 {
                if self.sender(index).is_closed() {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            false
        }
    }

    fn event(kind: &str, value: i64) -> Value {
        json!({ "type": kind, "value": value })
    }

    /// Polls once (triggering lazy registration) without consuming.
    macro_rules! poke {
        ($stream:expr) => {
            let _ = futures::poll!($stream.as_mut().next());
        };
    }

    #[tokio::test]
    async fn lazy_until_first_poll() {
        let harness = Harness::default();
        let shared = harness.shared();
        let idle = shared.subscribe();
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 0);
        drop(idle);
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fan_out_to_many_subscribers_over_one_connection() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut first = Box::pin(shared.subscribe());
        let mut second = Box::pin(shared.subscribe());
        let mut third = Box::pin(shared.subscribe());

        poke!(first);
        harness.at(0).await;
        for value in [1, 2, 3] {
            harness.push(0, event("session.updated", value));
            let events =
                futures::future::join_all([first.next(), second.next(), third.next()]).await;
            for result in events {
                assert_eq!(result.expect("event").expect("ok"), event("session.updated", value));
            }
        }
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn late_subscriber_gets_marker_but_no_replay() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut first = Box::pin(shared.subscribe());
        poke!(first);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        assert_eq!(first.next().await.unwrap().unwrap(), event("server.connected", 1));
        harness.push(0, event("server.connected", 2));
        assert_eq!(first.next().await.unwrap().unwrap(), event("server.connected", 2));
        harness.push(0, event("session.updated", 3));
        assert_eq!(first.next().await.unwrap().unwrap(), event("session.updated", 3));

        let mut late = Box::pin(shared.subscribe());
        assert_eq!(late.next().await.unwrap().unwrap(), event("server.connected", 2));
        harness.push(0, event("session.updated", 4));
        assert_eq!(late.next().await.unwrap().unwrap(), event("session.updated", 4));
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn last_subscriber_drop_closes_the_source() {
        let harness = Harness::default();
        let shared = harness.shared();
        let first = Box::pin(shared.subscribe());
        let mut second = Box::pin(shared.subscribe());
        poke!(second);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        let _ = second.next().await;

        drop(first);
        assert!(!harness.sender(0).is_closed(), "connection survives while a subscriber remains");
        drop(second);
        assert!(harness.source_closed(0).await, "last dropped subscriber closes the connection");
    }

    #[tokio::test]
    async fn eof_finishes_all_and_fresh_subscribe_reconnects_without_duplication() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut first = Box::pin(shared.subscribe());
        let mut second = Box::pin(shared.subscribe());
        poke!(second);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        harness.push(0, event("session.updated", 2));
        assert_eq!(first.next().await.unwrap().unwrap(), event("server.connected", 1));
        assert_eq!(second.next().await.unwrap().unwrap(), event("server.connected", 1));
        harness.close(0); // server closes the stream
                          // Buffered events are preserved, then the stream ends.
        assert_eq!(second.next().await.unwrap().unwrap(), event("session.updated", 2));
        assert!(second.next().await.is_none());
        assert_eq!(first.next().await.unwrap().unwrap(), event("session.updated", 2));
        assert!(first.next().await.is_none());
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 1);

        // Reconnect: a fresh subscribe opens a replacement connection.
        let mut fresh = Box::pin(shared.subscribe());
        poke!(fresh);
        // close(0) truncated the harness, so the replacement connection
        // is index 0 again.
        harness.at(0).await;
        harness.push(0, event("server.connected", 3));
        harness.push(0, event("session.updated", 4));
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
        assert_eq!(fresh.next().await.unwrap().unwrap(), event("server.connected", 3));
        assert_eq!(fresh.next().await.unwrap().unwrap(), event("session.updated", 4));
        // No duplication across the reconnect: the old stream's events
        // never reappear on the replacement connection.
        harness.close(0);
        assert!(fresh.next().await.is_none());
    }

    #[tokio::test]
    async fn source_error_finishes_every_subscriber_with_identity() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut first = Box::pin(shared.subscribe());
        let mut second = Box::pin(shared.subscribe());
        poke!(second);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        assert!(first.next().await.is_some());
        assert!(second.next().await.is_some());
        // G-376⑤: events queued when the source errors stay readable —
        // the error applies only after the buffer drains.
        harness.push(0, event("session.updated", 42));
        harness.fail(0, ClientError::MalformedResponse("boom".into()));

        for subscription in [&mut first, &mut second] {
            assert_eq!(
                subscription.next().await.unwrap().expect("buffered event"),
                event("session.updated", 42)
            );
            match subscription.next().await {
                Some(Err(ClientError::MalformedResponse(message))) => assert_eq!(message, "boom"),
                other => panic!("expected the source error, got {other:?}"),
            }
            // G-125: the terminal error is delivered on EVERY subsequent
            // next(), matching the TS iterator (see the test below).
            assert!(
                subscription.next().await.is_some_and(|item| item.is_err()),
                "post-failure next() must reject again"
            );
        }
        let left = first.terminal_error().expect("terminal error");
        let right = second.terminal_error().expect("terminal error");
        assert!(Arc::ptr_eq(&left, &right), "error identity preserved across subscribers");

        // A fresh subscription works again.
        let mut fresh = Box::pin(shared.subscribe());
        poke!(fresh);
        harness.at(1).await;
        harness.push(1, event("server.connected", 2));
        assert_eq!(fresh.next().await.unwrap().unwrap(), event("server.connected", 2));
    }

    #[allow(clippy::cast_possible_wrap)] // SUBSCRIBER_CAPACITY is a tiny constant
    #[tokio::test]
    async fn subscriber_overflow_terminates_with_capacity_error() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut idle = Box::pin(shared.subscribe());
        poke!(idle);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        assert_eq!(idle.next().await.unwrap().unwrap(), event("server.connected", 1));
        for value in 0..=(SUBSCRIBER_CAPACITY as i64) {
            harness.push(0, event("session.updated", value));
        }
        match idle.next().await {
            Some(Err(ClientError::SubscriberCapacity(capacity))) => {
                assert_eq!(capacity, SUBSCRIBER_CAPACITY);
            }
            other => panic!("expected capacity error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn sync_connect_failure_rejects_without_retry() {
        let harness = Harness::default();
        *harness.sync_error.lock().unwrap() =
            Some(ClientError::MalformedResponse("connect failed".into()));
        let shared = harness.shared();
        let mut subscriber = Box::pin(shared.subscribe());
        match subscriber.next().await {
            Some(Err(ClientError::MalformedResponse(message))) => {
                assert_eq!(message, "connect failed");
            }
            other => panic!("expected connect failure, got {other:?}"),
        }
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 1, "no automatic retry");

        // A fresh subscribe attempts again (and succeeds this time).
        let mut fresh = Box::pin(shared.subscribe());
        poke!(fresh);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        assert_eq!(fresh.next().await.unwrap().unwrap(), event("server.connected", 1));
        assert_eq!(harness.attempts.load(Ordering::SeqCst), 2);
    }

    /// G-125: after a failed completion, the second AND third `next()`
    /// also reject (TS shared-events.ts:76,123 — completion rejects all
    /// pending and every later `next()`); the stream no longer ends after
    /// a single Err.
    #[tokio::test]
    async fn terminal_error_is_delivered_repeatedly() {
        let harness = Harness::default();
        let shared = harness.shared();
        let mut subscriber = Box::pin(shared.subscribe());
        poke!(subscriber);
        harness.at(0).await;
        harness.fail(0, ClientError::MalformedResponse("boom".into()));
        for round in 1..=3 {
            match subscriber.next().await {
                Some(Err(ClientError::MalformedResponse(message))) => {
                    assert_eq!(message, "boom", "round {round} carries the error");
                }
                other => panic!("round {round}: expected Err, got {other:?}"),
            }
        }
        // Clean (EOF) completion still ENDS the stream — only the failed
        // completion repeats.
        let mut clean = Box::pin(shared.subscribe());
        poke!(clean);
        harness.at(1).await;
        harness.close(1);
        assert!(clean.next().await.is_none());
        assert!(clean.next().await.is_none());
    }

    #[tokio::test]
    async fn activity_fans_out_only_to_subscribers_that_asked() {
        let harness = Harness::default();
        let shared = harness.shared();
        let first_count = Arc::new(AtomicUsize::new(0));
        let second_count = Arc::new(AtomicUsize::new(0));
        let first_cb = {
            let count = Arc::clone(&first_count);
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }) as ActivityCallback
        };
        let second_cb = {
            let count = Arc::clone(&second_count);
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            }) as ActivityCallback
        };
        let mut first = Box::pin(shared.subscribe_with_activity(first_cb));
        let mut second = Box::pin(shared.subscribe_with_activity(second_cb));
        let mut plain = Box::pin(shared.subscribe());
        poke!(first);
        harness.at(0).await;
        harness.push(0, event("server.connected", 1));
        let _ = first.next().await;
        let _ = second.next().await;
        let _ = plain.next().await;

        let activity = harness.activity.lock().unwrap().clone().expect("activity captured");
        activity.signal();
        activity.signal();
        assert_eq!(first_count.load(Ordering::SeqCst), 2);
        assert_eq!(second_count.load(Ordering::SeqCst), 2);
    }
}
