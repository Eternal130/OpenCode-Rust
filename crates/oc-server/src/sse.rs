//! The event feed + SSE handlers (`event-feed.ts` + `handlers/event.ts` +
//! `handlers/session.ts` log streaming).
//!
//! One bus tap (the live stream) fans every **public** event (`server.connected`,
//! known registry types, `rpc.*`) into per-subscriber bounded(4096) mpsc
//! queues. Frames are encoded ONCE per event (`data: {json}\n\n`); a queue
//! that fills up loses its subscriber and its stream terminates with
//! [`FeedError::Overflow`] (drop-and-disconnect); an encoding failure fails
//! ALL subscribers. Each response prepends a synthetic `server.connected`
//! frame and emits `: heartbeat` comments every 15s (the heartbeat dies with
//! the stream).

use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::StreamExt;
use oc_core::bus::registry;
use oc_core::bus::{self, Bus, Event, LogItem};
use parking_lot::Mutex;
use serde_json::json;

use crate::state::AppState;

/// Default per-subscriber capacity (`SubscriberCapacity`).
pub const SUBSCRIBER_CAPACITY: usize = 4_096;

/// Default heartbeat cadence.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// The SSE frame for an event (`EventFeed.frame`).
#[must_use]
pub fn frame(event: &Event) -> String {
    format!("data: {}\n\n", serde_json::to_string(event).expect("event serializes"))
}

/// Failures delivered on a subscriber stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeedError {
    /// The subscriber lagged past its queue capacity; it was removed and its
    /// stream terminated.
    #[error("EventFeed.SubscriberOverflow: capacity {capacity}")]
    Overflow { capacity: usize },
    /// Encoding an event failed; every current subscriber was disconnected.
    #[error("EventFeed.EncodingError: {event_type}")]
    Encoding { event_type: String },
}

/// What a poll of a subscriber queue yields.
enum PollOutcome {
    Value(Result<Arc<str>, FeedError>),
    Pending,
    Closed,
}

/// One subscriber: a bounded dropping queue (oldest frames are dropped at
/// capacity like `Queue.dropping`) plus a terminal error slot. A queue that
/// overflows fails immediately — the terminal error replaces the backlog.
struct SubscriberQueue {
    state: parking_lot::Mutex<SubscriberState>,
    notify: Arc<tokio::sync::Notify>,
}

struct SubscriberState {
    queue: std::collections::VecDeque<Result<Arc<str>, FeedError>>,
    capacity: usize,
    failed: Option<FeedError>,
    closed: bool,
}

impl SubscriberQueue {
    fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(SubscriberState {
                queue: std::collections::VecDeque::new(),
                capacity,
                failed: None,
                closed: false,
            }),
            notify: Arc::new(tokio::sync::Notify::new()),
        })
    }

    /// `Queue.offerUnsafe` + drop-on-full: `Ok` pushes; a full queue fails
    /// terminally with [`FeedError::Overflow`] (backlog discarded, exactly
    /// like effect's `failCauseUnsafe` racing a full dropping queue).
    fn offer(self: &Arc<Self>, frame: Result<Arc<str>, FeedError>) -> bool {
        let mut state = self.state.lock();
        if state.closed {
            return false;
        }
        if state.failed.is_some() {
            // Already terminal: the subscriber is being removed.
            return false;
        }
        match frame {
            Err(error) => {
                state.queue.clear();
                state.queue.push_back(Err(error.clone()));
                state.failed = Some(error);
            }
            Ok(frame) => {
                if state.queue.len() >= state.capacity {
                    let error = FeedError::Overflow { capacity: state.capacity };
                    state.queue.clear();
                    state.queue.push_back(Err(error.clone()));
                    state.failed = Some(error);
                } else {
                    state.queue.push_back(Ok(frame));
                }
            }
        }
        drop(state);
        self.notify.notify_waiters();
        true
    }

    /// Marks the receiving end closed (subscriber teardown).
    fn close(&self) {
        self.state.lock().closed = true;
        self.notify.notify_waiters();
    }

    fn poll_recv(&self) -> PollOutcome {
        let mut state = self.state.lock();
        if let Some(item) = state.queue.pop_front() {
            return PollOutcome::Value(item);
        }
        if state.failed.is_some() || state.closed {
            return PollOutcome::Closed;
        }
        PollOutcome::Pending
    }

    /// Async receive: resolves with the next frame, or `None` once the queue
    /// terminated (failure delivered / closed).
    async fn recv(self: Arc<Self>) -> Option<Result<Arc<str>, FeedError>> {
        loop {
            let notified = self.notify.notified();
            futures::pin_mut!(notified);
            notified.as_mut().enable();
            match self.poll_recv() {
                PollOutcome::Value(value) => return Some(value),
                PollOutcome::Closed => return None,
                PollOutcome::Pending => notified.await,
            }
        }
    }
}

/// Whether an event type is public (`isOpenCodeEvent`): `server.connected`,
/// any known registry type (`EventManifest.isServer` analog), `rpc.*`, or
/// one of the v2 location-service frames the announcer publishes (they
/// are schema'd public events in v2; the Rust registry inventory does not
/// list them yet — see `announce`).
///
/// The registry's non-session inventory carries every v2
/// `EventManifest.ServerDefinitions` type (auditD §3.4): the form/shell
/// lifecycle frames oc-core already published, plus `session.status`,
/// `session.idle`, `filesystem.changed`, `worktree.updated`,
/// `vcs.branch.updated`, `credential.*`, `models-dev.refreshed`, `tui.*`,
/// and `installation.*`.
#[must_use]
pub fn is_public_event(type_: &str) -> bool {
    type_ == "server.connected"
        || registry::by_type(type_).is_some()
        || type_.starts_with("rpc.")
        || crate::announce::SERVICE_FRAMES.contains(&type_)
}

/// One subscriber's queue handle.
struct Subscriber {
    queue: Arc<SubscriberQueue>,
}

/// The shared feed: bus tap + subscriber fan-out.
pub struct EventFeed {
    capacity: usize,
    subscribers: Mutex<Vec<Subscriber>>,
}

impl EventFeed {
    /// Creates a feed with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(SUBSCRIBER_CAPACITY)
    }

    /// Creates a feed with an explicit per-subscriber capacity (tests).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { capacity, subscribers: Mutex::new(Vec::new()) }
    }

    /// Taps the bus live stream, publishing every public event until the
    /// process ends. The tap drains the (unbounded) bus stream eagerly, so a
    /// slow HTTP consumer only ever backpressures its own bounded queue.
    pub fn spawn_tap(self: &Arc<Self>, bus: &Arc<Bus>) {
        let mut subscription = bus.subscribe();
        let feed = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = subscription.next().await {
                feed.publish(&event);
            }
        });
    }

    /// Publishes one event: encode once, fan out, terminate lagging
    /// subscribers, fail everyone on encoding errors.
    pub fn publish(&self, event: &Event) {
        if !is_public_event(&event.type_) {
            return;
        }
        let mut subscribers = self.subscribers.lock();
        if subscribers.is_empty() {
            return;
        }
        let encoded = if let Ok(json) = serde_json::to_string(event) {
            Arc::from(format!("data: {json}\n\n"))
        } else {
            // EncodingError: fail ALL current subscribers, stay alive for
            // later ones (event-feed.test.ts).
            let current = std::mem::take(&mut *subscribers);
            for subscriber in current {
                subscriber
                    .queue
                    .offer(Err(FeedError::Encoding { event_type: event.type_.clone() }));
            }
            return;
        };
        // `Queue.offerUnsafe`: a closed/failed receiver drops its
        // registration; the failure frame itself was already delivered.
        subscribers.retain(|subscriber| subscriber.queue.offer(Ok(Arc::clone(&encoded))));
    }

    /// Subscribes: a bounded dropping queue registered with the feed. The
    /// stream ends when the feed drops the subscriber (overflow → terminal
    /// error first).
    pub fn subscribe(&self) -> Subscription {
        let queue = SubscriberQueue::new(self.capacity);
        self.subscribers.lock().push(Subscriber { queue: Arc::clone(&queue) });
        Subscription { queue }
    }

    /// Number of live subscribers (tests).
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.lock().len()
    }
}

/// A subscriber's receiving end.
pub struct Subscription {
    queue: Arc<SubscriberQueue>,
}

impl Subscription {
    /// Next frame; `None` once the stream terminated.
    pub async fn recv(&mut self) -> Option<Result<Arc<str>, FeedError>> {
        Arc::clone(&self.queue).recv().await
    }

    /// Wraps this subscription into the SSE body stream: the frames merged
    /// with the heartbeat cadence (`Stream.merge` + `haltStrategy: "left"`),
    /// ending when `shutdown` fires (gate-6 F2: stream bodies must END on
    /// shutdown so the graceful drain is not held open by infinite feeds).
    pub fn into_body_stream(
        self,
        heartbeat: Duration,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send {
        let queue = Arc::clone(&self.queue);
        // The stream owns the lifecycle now; the Subscription's own Drop
        // would immediately close the queue.
        std::mem::forget(self);
        // Dropping the body (client disconnect) closes the queue so the feed
        // reaps the subscriber and the heartbeat loop ends.
        SubscriptionStream { queue, inner: None, heartbeat, shutdown: Some(shutdown) }
    }
}

/// The body stream; closing the queue on drop.
struct SubscriptionStream {
    queue: Arc<SubscriberQueue>,
    inner: Option<FeedStream>,
    heartbeat: Duration,
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
}

impl Drop for SubscriptionStream {
    fn drop(&mut self) {
        self.queue.close();
    }
}

impl futures::Stream for SubscriptionStream {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.inner.is_none() {
            let mut interval = tokio::time::interval(this.heartbeat);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.reset();
            this.inner = Some(FeedStream {
                queue: Arc::clone(&this.queue),
                recv: None,
                heartbeat: interval,
                done: false,
                shutdown_wait: None,
                shutdown: this.shutdown.take(),
            });
        }
        std::pin::Pin::new(this.inner.as_mut().expect("armed")).poll_next(cx)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.queue.close();
    }
}

impl Default for EventFeed {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GET /api/event
// ---------------------------------------------------------------------------

/// SSE headers shared by the event feed.
fn sse_response(body: axum::body::Body) -> axum::response::Response {
    let mut response = axum::response::Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-cache, no-transform"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("x-accel-buffering"),
        axum::http::HeaderValue::from_static("no"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("x-content-type-options"),
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}

/// The synthesized initial frame (`server.connected`), id from the app's
/// `IdGen` seam.
fn connected_frame(state: &AppState) -> Arc<str> {
    let id = state.ids.new_id("evt");
    Arc::from(format!("data: {}\n\n", json!({ "id": id, "type": "server.connected", "data": {} })))
}

/// The in-flight receive future of a subscriber stream.
type RecvFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Option<Result<Arc<str>, FeedError>>> + Send>,
>;

/// A subscriber stream merged with the 15s heartbeat (`Stream.merge` with
/// `haltStrategy: "left"` — the heartbeat dies with the frames stream).
/// The shutdown watch ends the body when the server begins stopping.
struct FeedStream {
    queue: Arc<SubscriberQueue>,
    recv: Option<RecvFuture>,
    heartbeat: tokio::time::Interval,
    shutdown: Option<tokio::sync::watch::Receiver<bool>>,
    /// The armed `wait_for(|flag| flag)` future — single-shot like `recv`,
    /// armed on first poll so its waker keeps this stream responsive while
    /// no frames or ticks are due.
    shutdown_wait: Option<ShutdownWait>,
    done: bool,
}

/// The in-flight shutdown watch future.
type ShutdownWait = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

fn shutdown_wait(mut receiver: tokio::sync::watch::Receiver<bool>) -> ShutdownWait {
    Box::pin(async move {
        let _ = receiver.wait_for(|flag| *flag).await;
    })
}

impl FeedStream {
    /// Polls the in-flight receive future, (re)arming it as needed: async-fn
    /// futures are single-shot, so each delivered frame needs a fresh one.
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Arc<str>, FeedError>>> {
        if self.recv.is_none() {
            self.recv = Some(Box::pin(Arc::clone(&self.queue).recv()));
        }
        match self.recv.as_mut().expect("armed").as_mut().poll(cx) {
            Poll::Ready(Some(item)) => {
                self.recv = None;
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => {
                self.recv = None;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl futures::Stream for FeedStream {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        // Shutdown ends the body even mid-frame / mid-heartbeat. `wait_for`
        // resolves immediately for a receiver created latched true.
        if self.shutdown_wait.is_none() {
            self.shutdown_wait = self.shutdown.take().map(shutdown_wait);
        }
        let shutdown_fired = match self.shutdown_wait.as_mut() {
            Some(wait) => wait.as_mut().poll(cx).is_ready(),
            None => true,
        };
        if shutdown_fired {
            self.done = true;
            return Poll::Ready(None);
        }
        match self.poll_recv(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(
                item.map(|frame| bytes::Bytes::from(frame.as_bytes().to_vec())).map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string())
                }),
            )),
            Poll::Ready(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => {
                // Heartbeat cadence; ticks flow only while frames stay pending.
                match self.heartbeat.poll_tick(cx) {
                    Poll::Ready(_) => {
                        Poll::Ready(Some(Ok(bytes::Bytes::from_static(b": heartbeat\n\n"))))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

/// `GET /api/event` handler (auth upstream).
pub fn event_response(
    state: &AppState,
    feed: &Arc<EventFeed>,
    heartbeat: Duration,
) -> axum::response::Response {
    let initial = connected_frame(state);
    let stream =
        futures::stream::once(async move { Ok(bytes::Bytes::from(initial.as_bytes().to_vec())) })
            .chain(feed.subscribe().into_body_stream(heartbeat, state.shutdown.subscribe()));
    sse_response(axum::body::Body::from_stream(stream))
}

// ---------------------------------------------------------------------------
// GET /api/experimental/session/{id}/log
// ---------------------------------------------------------------------------

/// One session-log item rendered as an SSE frame.
fn log_item_frame(item: &LogItem) -> Result<String, FeedError> {
    let json = match item {
        LogItem::Event(event) => serde_json::to_string(event),
        LogItem::Synced(synced) => serde_json::to_string(synced),
    };
    json.map(|payload| format!("data: {payload}\n\n"))
        .map_err(|_| FeedError::Encoding { event_type: "log".to_owned() })
}

/// `GET /api/experimental/session/:sessionID/log` handler (auth upstream).
/// Validates the session id + cursor/follow query, checks the session row,
/// then streams `bus.log` (`data:` frames, one `log.synced` marker).
pub async fn session_log_response(
    state: &AppState,
    uri: &axum::http::Uri,
    headers: &axum::http::HeaderMap,
    session_id: &str,
) -> axum::response::Response {
    use crate::errors;

    // Session id validation (Schema.decodeUnknownEffect(Session.ID)).
    let Ok(session) = oc_schema::id::SessionId::parse(session_id) else {
        return errors::invalid_request_field(session_id.to_owned(), "ParseError", "sessionID")
            .into_response();
    };

    // after: NumberFromString; follow: BooleanFromString.
    let after = match crate::auth::query_param(uri.query(), "after") {
        None => None,
        Some(raw) => match raw.parse::<i64>() {
            Ok(value) => Some(value),
            Err(_) => {
                return errors::invalid_request_kind(
                    format!("Expected after to be a number, received \"{raw}\""),
                    "ParseError",
                )
                .into_response();
            }
        },
    };
    let follow = match crate::auth::query_param(uri.query(), "follow") {
        None => false,
        Some(raw) => match raw.as_str() {
            "true" => true,
            "false" => false,
            other => {
                return errors::invalid_request_kind(
                    format!("Expected follow to be a boolean, received \"{other}\""),
                    "ParseError",
                )
                .into_response();
            }
        },
    };

    // Session existence (session.get → 404) at the request's location.
    let location: oc_core::location::LocationRef =
        crate::middleware::request_location(uri, headers, state.boot_location.as_deref()).into();
    match state.engine.session(&location, &session).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return errors::session_not_found(session.as_str()).into_response();
        }
        Err(error) => {
            eprintln!("[session.log] session lookup failed: {error}");
            return errors::unknown_with_default_message().into_response();
        }
    }

    let input = bus::LogInput { aggregate_id: session.as_str().to_owned(), after, follow };
    let mut shutdown = state.shutdown.subscribe();
    let stream = state
        .engine
        .bus
        .log(input)
        .take_until(async move {
            // Gate-6 F2: a following log stream is infinite; end the body
            // when the server begins stopping so the drain completes.
            let _ = shutdown.wait_for(|flag| *flag).await;
        })
        .filter_map(|item| async move {
            match item {
                Ok(log_item) => match log_item_frame(&log_item) {
                    Ok(frame) => Some(Ok(bytes::Bytes::from(frame.into_bytes()))),
                    Err(error) => Some(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        error.to_string(),
                    ))),
                },
                Err(error) => Some(Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    error.to_string(),
                ))),
            }
        });
    sse_response(axum::body::Body::from_stream(stream))
}

// ---------------------------------------------------------------------------
// Tests (event-feed.test.ts port)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oc_schema::id::EventId;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn event(id: &str, type_: &str) -> Event {
        Event {
            id: EventId(format!("evt_{id}")),
            created: 1,
            metadata: None,
            type_: type_.to_owned(),
            durable: None,
            location: None,
            data: json!({}),
        }
    }

    /// A controllable source standing in for the bus tap.
    struct CountingFeed {
        feed: Arc<EventFeed>,
        encodes: Arc<AtomicUsize>,
    }

    impl CountingFeed {
        fn new(capacity: usize) -> Self {
            Self {
                feed: Arc::new(EventFeed::with_capacity(capacity)),
                encodes: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn publish(&self, event: &Event) {
            if !is_public_event(&event.type_) {
                return;
            }
            self.encodes.fetch_add(1, Ordering::SeqCst);
            self.feed.publish(event);
        }
    }

    async fn recv(stream: &mut Subscription) -> Option<Result<Arc<str>, FeedError>> {
        stream.recv().await
    }

    #[test]
    fn server_feed_frame_encoding() {
        let payload = event("wire", "config.updated");
        assert_eq!(
            frame(&payload),
            format!("data: {}\n\n", serde_json::to_string(&payload).unwrap())
        );
    }

    #[tokio::test]
    async fn server_feed_encodes_once_delivers_everywhere() {
        let source = CountingFeed::new(4096);
        let mut first = source.feed.subscribe();
        let mut second = source.feed.subscribe();

        source.publish(&event("example", "config.updated"));

        let left = recv(&mut first).await.expect("first frame");
        let right = recv(&mut second).await.expect("second frame");
        assert_eq!(left.expect("ok"), right.expect("ok"));
        assert_eq!(source.encodes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn server_feed_overflow_fails_only_the_laggard() {
        let source = CountingFeed::new(1);
        let mut slow = source.feed.subscribe();
        let mut fast = source.feed.subscribe();

        // Fill + overflow the slow queue while the fast one drains.
        source.publish(&event("one", "config.updated"));
        let first = recv(&mut fast).await;
        source.publish(&event("two", "config.updated"));
        let second = recv(&mut fast).await;
        source.publish(&event("three", "config.updated"));
        let third = recv(&mut fast).await;

        assert!(first.is_some_and(|f| f.unwrap().contains("config.updated")), "first frame");
        assert!(second.is_some() && third.is_some());

        let overflow = recv(&mut slow).await;
        assert_eq!(overflow, Some(Err(FeedError::Overflow { capacity: 1 })));
        assert!(recv(&mut slow).await.is_none(), "stream terminated after overflow");
    }

    #[tokio::test]
    async fn server_feed_filters_internal_events() {
        let source = CountingFeed::new(1);
        let mut stream = source.feed.subscribe();

        source.publish(&event("a", "test.internal"));
        source.publish(&event("b", "test.internal"));
        source.publish(&event("c", "config.updated"));

        let frame = recv(&mut stream).await.expect("public frame").expect("ok");
        assert!(frame.contains("config.updated"), "frame was {frame}");
        assert_eq!(source.encodes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn server_feed_public_type_filter() {
        assert!(is_public_event("server.connected"));
        assert!(is_public_event("session.created"));
        assert!(is_public_event("config.updated"));
        assert!(is_public_event("rpc.custom"));
        assert!(!is_public_event("test.internal"));
        assert!(!is_public_event("weird.unknown"));
    }

    /// The 20 auditD §3.4 types: the 6 published-but-filtered form/shell
    /// frames plus the 14 newly published ones must all pass the public
    /// filter (v2 `EventManifest.ServerDefinitions` + `session-status`,
    /// `tui`, `installation`, `vcs` — `event-manifest.ts:71-81`).
    #[tokio::test]
    async fn server_feed_public_type_filter_covers_audit_d_types() {
        // (a) published-but-filtered.
        for type_ in [
            "form.created",
            "form.replied",
            "form.cancelled",
            "shell.created",
            "shell.exited",
            "shell.deleted",
        ] {
            assert!(is_public_event(type_), "{type_} must be public");
        }
        // (b) the 14 wired in this lane.
        for type_ in [
            "session.status",
            "session.idle",
            "filesystem.changed",
            "worktree.updated",
            "vcs.branch.updated",
            "credential.updated",
            "credential.switched",
            "models-dev.refreshed",
            "tui.prompt.append",
            "tui.command.execute",
            "tui.toast.show",
            "tui.session.select",
            "installation.updated",
            "installation.update-available",
        ] {
            assert!(is_public_event(type_), "{type_} must be public");
        }
        // Registry membership backs the filter (by_type resolves each).
        for type_ in ["form.created", "shell.exited", "session.status", "installation.updated"] {
            assert!(oc_core::bus::registry::by_type(type_).is_some(), "{type_} in registry");
        }
    }

    #[tokio::test]
    async fn server_feed_closed_subscriber_is_reaped() {
        let source = CountingFeed::new(4);
        let gone = source.feed.subscribe();
        drop(gone);
        let mut alive = source.feed.subscribe();
        source.publish(&event("x", "config.updated"));
        assert!(recv(&mut alive).await.is_some());
        assert_eq!(source.feed.subscriber_count(), 1);
    }

    #[tokio::test]
    async fn server_feed_durable_events_pass_the_filter() {
        let source = CountingFeed::new(4);
        let mut stream = source.feed.subscribe();
        source.publish(&event("d", "session.deleted"));
        let frame = recv(&mut stream).await.expect("frame").expect("ok");
        assert!(frame.contains("session.deleted"));
    }
}
