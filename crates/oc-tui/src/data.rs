//! SSE data layer on `oc-client`: a [`Connection`] with reconnect
//! (exponential backoff), **resync on reconnect** (session re-fetch +
//! event cursor), a connection-state stream for the status line, and a
//! **single ordered ingest channel** into the store.
//!
//! ## Single-consumer ingest (drain-before-reconcile)
//!
//! Everything the UI consumes — live bus events, resync snapshots,
//! connection transitions — is serialized into ONE [`Ingest`] stream
//! (the `tx` side of an mpsc). The store side drains the channel
//! completely before reconciling, so a resync snapshot can never
//! overtake live events queued ahead of it. This is the `run`
//! consumer-drain discipline, promoted to the design rule for the TUI
//! data layer from day one (gate-7 risk 3).
//!
//! Wire shape: `/api/event` SSE frames are JSON-in-string
//! (`V2EventEncoded`, double-decoded by [`oc_client::SseStream`]) whose
//! payload is the bus `Event` — `{ id, created, type, data }` with
//! `server.connected` prepended per response by the server.

use std::time::Duration;

use oc_client::Client;
use serde_json::Value;

/// Connection state for the status line / reconnect overlay.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionState {
    #[default]
    Connecting,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    Failed,
}

/// One bus event off the SSE feed (`oc_core::bus::Event` wire shape).
#[derive(Debug, Clone, PartialEq)]
pub struct BusEvent {
    pub id: String,
    pub type_: String,
    pub data: Value,
    /// Envelope `created` (epoch ms) — the only timestamp SSE reducers
    /// may use (reasoning/tool completion times; `None` when absent).
    pub created: Option<u64>,
}

impl BusEvent {
    #[must_use]
    pub fn new(type_: &str, data: Value) -> Self {
        Self { id: String::new(), type_: type_.to_owned(), data, created: None }
    }

    /// Decode the double-decoded SSE JSON payload.
    #[must_use]
    pub fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            id: object.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
            type_: object.get("type").and_then(Value::as_str)?.to_owned(),
            data: object.get("data").cloned().unwrap_or(Value::Null),
            created: object.get("created").and_then(Value::as_u64),
        })
    }

    /// A string field out of `data`.
    #[must_use]
    pub fn string(&self, field: &str) -> Option<String> {
        self.data.get(field).and_then(Value::as_str).map(str::to_owned)
    }

    /// The part ordinal of a text/reasoning delta/ended event (0 when
    /// the server omits it).
    #[must_use]
    pub fn ordinal(&self) -> u64 {
        self.data.get("ordinal").and_then(Value::as_u64).unwrap_or(0)
    }
}

/// A session row from a resync fetch.
///
/// `time_idle` / `viewed` come from the `session.list` wire shape
/// (`SessionInfo.time.idle` / `.viewed`, epoch seconds) and drive the
/// **unread derivation** (see [`resync_unread`]); both are `None` when
/// the server omits them.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    pub id: String,
    pub title: Option<String>,
    /// Last activity timestamp (`time.idle`); absent when unknown.
    pub time_idle: Option<f64>,
    /// Last view timestamp (`time.viewed`); absent when unknown.
    pub viewed: Option<f64>,
}

/// Sessions a resync marks unread: exactly those whose last activity
/// is newer than their last view — `time.idle > time.viewed`, the same
/// comparison the reference derives via `session.view` round-trips.
///
/// Both timestamps must be **known**: absent data marks nothing, so a
/// reconnect can never fabricate unread state (gate-9 finding 6 — the
/// old code marked every session unread on every reconnect). The
/// full `session.view` refinement for viewed-absent sessions lands
/// with the wave-3 `message.list` resync.
#[must_use]
pub fn resync_unread(sessions: &[SessionSnapshot]) -> Vec<String> {
    sessions
        .iter()
        .filter(|session| match (session.time_idle, session.viewed) {
            (Some(idle), Some(viewed)) => idle > viewed,
            _ => false,
        })
        .map(|session| session.id.clone())
        .collect()
}

/// The single ordered ingest stream into the store (see module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Ingest {
    /// `server.connected` observed (or first successful subscribe).
    Connected,
    /// A connection-state transition (drives the status line).
    Connection(ConnectionState),
    /// A live bus event, in arrival order.
    Event(BusEvent),
    /// A post-reconnect resync snapshot (applied only after the channel
    /// drained — see [`crate::store::Store::ingest_all`]).
    Resync { sessions: Vec<SessionSnapshot>, unread: Vec<String> },
}

/// Reconnect/backoff configuration.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub backoff_base: Duration,
    pub backoff_max: Duration,
    /// Give up (report [`ConnectionState::Failed`]) after this many
    /// attempts; `None` retries forever.
    pub max_attempts: Option<u32>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            backoff_base: Duration::from_millis(500),
            backoff_max: Duration::from_secs(8),
            max_attempts: None,
        }
    }
}

impl ConnectionConfig {
    /// Backoff before attempt `n` (1-based): base · 2^(n-1), capped.
    /// Millisecond magnitudes stay far below 2^23; the u128→u64 casts
    /// cannot truncate real inputs.
    #[must_use]
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 2u64.saturating_pow(attempt.saturating_sub(1).min(16));
        let base = u64::try_from(self.backoff_base.as_millis()).unwrap_or(u64::MAX / 2);
        let max = u64::try_from(self.backoff_max.as_millis()).unwrap_or(u64::MAX);
        Duration::from_millis((base * factor).min(max))
    }
}

/// How sessions are re-fetched during resync (swappable for tests;
/// production uses [`HttpResync`]).
pub trait SessionListSource {
    fn fetch(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<SessionSnapshot>, String>> + Send;
}

/// Production resync: `session.list` over the generated client.
#[derive(Clone)]
pub struct HttpResync {
    client: Client,
}

impl HttpResync {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl SessionListSource for HttpResync {
    async fn fetch(&self) -> Result<Vec<SessionSnapshot>, String> {
        let query = oc_client::types::SessionListQuery {
            cursor: None,
            directory: None,
            limit: None,
            order: None,
            parent_id: None,
            project: None,
            search: None,
            subpath: None,
        };
        let response = self.client.session().list(&query).await.map_err(|e| e.to_string())?;
        Ok(response
            .data
            .into_iter()
            .map(|session| SessionSnapshot {
                id: session.id,
                title: session.title,
                time_idle: session.time.idle,
                viewed: session.time.viewed,
            })
            .collect())
    }
}

/// The SSE connection state machine: subscribe, relay, reconnect with
/// backoff, resync on reconnect, feed one ordered ingest channel.
///
/// Gate-9 finding 6 wiring:
///
/// * **Resubscription cursor**: every reconnect re-subscribes with the
///   tracked last event id as the SSE `Last-Event-ID` header
///   ([`oc_client::event_ext`] — the server resumes from it or ignores
///   it; the resync snapshot below remains the consistency net either
///   way).
/// * **No false unread on resync**: the snapshot marks unread only
///   sessions whose `time.idle > time.viewed` with both timestamps
///   known ([`resync_unread`]) — previously every session was marked,
///   so every reconnect fabricated unread state.
pub struct Connection {
    client: Client,
    config: ConnectionConfig,
    attempts: u32,
    cursor: Option<String>,
}

impl Connection {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self::with_config(client, ConnectionConfig::default())
    }

    #[must_use]
    pub fn with_config(client: Client, config: ConnectionConfig) -> Self {
        Self { client, config, attempts: 0, cursor: None }
    }

    #[must_use]
    pub fn state(&self) -> ConnectionState {
        if self.attempts == 0 {
            ConnectionState::Connecting
        } else {
            ConnectionState::Reconnecting { attempt: self.attempts }
        }
    }

    #[must_use]
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    /// The reconnect loop. Pushes every observation into `tx` in order;
    /// the receiving side owns the store. Cancelling the task (or
    /// dropping `tx`) ends the loop.
    pub async fn run(
        mut self,
        tx: tokio::sync::mpsc::Sender<Ingest>,
        fetcher: impl SessionListSource,
    ) {
        loop {
            self.attempts += 1;
            if let Some(max) = self.config.max_attempts {
                if self.attempts > max {
                    let _ = tx.send(Ingest::Connection(ConnectionState::Failed)).await;
                    return;
                }
            }
            if self.attempts > 1 {
                if tx
                    .send(Ingest::Connection(ConnectionState::Reconnecting {
                        attempt: self.attempts,
                    }))
                    .await
                    .is_err()
                {
                    return;
                }
                tokio::time::sleep(self.config.backoff(self.attempts - 1)).await;
            }
            // Resubscription carries the tracked cursor as the SSE
            // `Last-Event-ID` header: a cursor-aware server resumes the
            // event stream from it; any other server ignores the header.
            let stream = match self.cursor.as_deref() {
                Some(cursor) => self.client.event().subscribe_from(cursor).await,
                None => self.client.event().subscribe().await,
            };
            let Ok(mut stream) = stream else {
                continue;
            };
            if self.attempts > 1 {
                // Resync on reconnect BEFORE relaying further live
                // events: the fetch result queues behind everything
                // already sent and ahead of the new stream's events.
                // Unread marks derive from `idle > viewed`, known-only.
                if let Ok(sessions) = fetcher.fetch().await {
                    let unread = resync_unread(&sessions);
                    if tx.send(Ingest::Resync { sessions, unread }).await.is_err() {
                        return;
                    }
                }
            }
            while let Ok(Some(value)) = stream.next_json().await {
                if let Some(event) = BusEvent::from_json(&value) {
                    if event.type_ == "server.connected"
                        && tx.send(Ingest::Connected).await.is_err()
                    {
                        return;
                    }
                    if !event.id.is_empty() {
                        self.cursor = Some(event.id.clone());
                    }
                    if tx.send(Ingest::Event(event)).await.is_err() {
                        return;
                    }
                }
            }
            // Stream end or decode error falls through — reconnect.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_events_decode_from_wire_json() {
        let value = serde_json::json!({
            "id": "evt_1",
            "created": 0,
            "type": "message.updated",
            "data": { "sessionID": "ses_1", "messageID": "msg_1" }
        });
        let event = BusEvent::from_json(&value).expect("decodes");
        assert_eq!(event.type_, "message.updated");
        assert_eq!(event.string("sessionID").as_deref(), Some("ses_1"));
        assert_eq!(event.id, "evt_1");
        assert!(BusEvent::from_json(&serde_json::json!({ "created": 0 })).is_none());
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let config = ConnectionConfig {
            backoff_base: Duration::from_millis(500),
            backoff_max: Duration::from_secs(8),
            max_attempts: None,
        };
        assert_eq!(config.backoff(1), Duration::from_millis(500));
        assert_eq!(config.backoff(2), Duration::from_millis(1000));
        assert_eq!(config.backoff(3), Duration::from_millis(2000));
        assert_eq!(config.backoff(5), Duration::from_millis(8000), "capped at max");
        assert_eq!(config.backoff(50), Duration::from_millis(8000));
    }

    #[test]
    fn resync_unread_requires_known_idle_newer_than_viewed() {
        // Gate-9 finding 6: only `idle > viewed` with BOTH timestamps
        // known marks unread; absent data marks nothing (no false
        // unread on reconnect).
        let session = |id: &str, idle: Option<f64>, viewed: Option<f64>| SessionSnapshot {
            id: id.to_owned(),
            title: None,
            time_idle: idle,
            viewed,
        };
        let sessions = vec![
            session("fresh", Some(100.0), Some(50.0)), // idle > viewed → unread
            session("caught-up", Some(30.0), Some(30.0)), // idle ≤ viewed → clean
            session("stale-view", Some(10.0), Some(20.0)),
            session("no-idle", None, Some(50.0)), // idle unknown → clean
            session("no-viewed", Some(100.0), None), // viewed unknown → clean
        ];
        assert_eq!(resync_unread(&sessions), vec!["fresh".to_owned()]);
    }

    /// One SSE frame for the scripted server: /api/event payloads are
    /// JSON-in-string (double-decoded).
    fn sse_frame(id: &str, type_: &str, data_json: &str) -> String {
        let event = format!("{{\"id\":\"{id}\",\"type\":\"{type_}\",\"data\":{data_json}}}");
        let escaped = event.replace('\\', "\\\\").replace('"', "\\\"");
        format!("data: \"{escaped}\"\n\n")
    }

    /// `/api/session` body (full `SessionInfo` wire shape): s1 has
    /// activity newer than its last view (unread), s2 is caught up, s3
    /// omits idle/viewed entirely (no false unread).
    fn session_list_body() -> String {
        concat!(
            r#"{"cursor":{},"data":["#,
            r#"{"id":"s1","title":"Fresh","time":{"created":0,"updated":0,"idle":100,"viewed":50},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}},"#,
            r#"{"id":"s2","title":"Caught up","time":{"created":0,"updated":0,"idle":30,"viewed":30},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}},"#,
            r#"{"id":"s3","title":"Unknown times","time":{"created":0,"updated":0},"location":{"directory":"/tmp"},"projectID":"p","cost":0,"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}"#,
            r#"]}"#,
        )
        .to_owned()
    }

    /// Scripted local server (FakeLlm-style): `/api/event` serves SSE
    /// frames then closes the stream; `/api/session` serves a session
    /// list with `time.idle`/`time.viewed`. Drives the full reconnect →
    /// resync state machine through oc-client AND the production
    /// [`HttpResync`] fetcher, asserting (a) the ordered ingest stream,
    /// (b) the idle>viewed unread derivation over the wire, and (c) the
    /// reconnect carrying the tracked cursor as `Last-Event-ID`.
    #[tokio::test]
    async fn reconnect_resyncs_and_orders_ingest() {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        // Per-connection `Last-Event-ID` headers seen on /api/event.
        let seen_cursors: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let cursors = Arc::clone(&seen_cursors);
        let server = tokio::spawn(async move {
            let mut connections = 0u32;
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                connections += 1;
                let first = connections <= 1;
                let cursors = Arc::clone(&cursors);
                let (mut reader, _) = socket.split();
                let mut request = vec![0u8; 8192];
                let _ = reader.read(&mut request).await;
                let head = String::from_utf8_lossy(&request);
                let path = head.split_whitespace().nth(1).unwrap_or_default().to_owned();
                if path.starts_with("/api/event") {
                    let cursor = head.lines().find_map(|line| {
                        let lower = line.to_lowercase();
                        lower.strip_prefix("last-event-id:").map(|value| value.trim().to_owned())
                    });
                    cursors.lock().expect("cursor log").push(cursor);
                    // First connection: connected + one live event, then
                    // EOF. Second connection: one live event, then EOF.
                    let body = if first {
                        sse_frame("e1", "server.connected", "{}")
                            + &sse_frame("e2", "message.updated", "{\"sessionID\":\"s1\"}")
                    } else {
                        sse_frame("e3", "message.updated", "{\"sessionID\":\"s1\"}")
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if path.starts_with("/api/session") {
                    let body = session_list_body();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else {
                    let _ = socket
                        .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                        .await;
                }
            }
        });

        let client = Client::new(&format!("http://{addr}")).expect("client");
        let connection = Connection::with_config(
            client.clone(),
            ConnectionConfig {
                backoff_base: Duration::from_millis(10),
                backoff_max: Duration::from_millis(10),
                max_attempts: Some(2),
            },
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        // Production fetcher: session.list through the wire types.
        let runner = tokio::spawn(connection.run(tx, HttpResync::new(client)));

        let mut observed = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while observed.len() < 6 {
            let Some(message) =
                tokio::time::timeout_at(deadline, rx.recv()).await.expect("deadline")
            else {
                break;
            };
            observed.push(message);
        }
        let _ = runner.await;
        server.abort();
        // ONE ordered stream: connected marker + the raw server.connected
        // event + live events from connection 1, the reconnect
        // transition, the resync snapshot, then live events from
        // connection 2.
        assert_eq!(observed.len(), 6, "observed: {observed:?}");
        assert_eq!(observed[0], Ingest::Connected);
        assert!(
            matches!(&observed[1], Ingest::Event(event) if event.id == "e1" && event.type_ == "server.connected")
        );
        assert!(matches!(&observed[2], Ingest::Event(event) if event.id == "e2"));
        assert!(matches!(
            &observed[3],
            Ingest::Connection(ConnectionState::Reconnecting { attempt: 2 })
        ));
        let Ingest::Resync { sessions, unread } = &observed[4] else {
            panic!("resync arrives after the reconnect transition: {observed:?}");
        };
        // The snapshot carries the fetched sessions (titles through the
        // wire types) and marks ONLY the idle>viewed session unread.
        assert_eq!(sessions[0].title.as_deref(), Some("Fresh"));
        assert_eq!(sessions[0].time_idle, Some(100.0));
        assert_eq!(sessions[0].viewed, Some(50.0));
        assert_eq!(sessions[2].time_idle, None, "s3 omits idle");
        assert_eq!(unread, &vec!["s1".to_owned()], "no false unread for s2/s3");
        assert!(matches!(&observed[5], Ingest::Event(event) if event.id == "e3"));
        // The reconnect carried the tracked cursor (e2) as Last-Event-ID.
        let cursors = seen_cursors.lock().expect("cursor log").clone();
        assert_eq!(cursors.len(), 2, "two /api/event connections");
        assert_eq!(cursors[0], None, "first subscribe has no cursor");
        assert_eq!(cursors[1].as_deref(), Some("e2"), "resubscribe sent Last-Event-ID: e2");
    }

    /// The ingest stream drives the store end-to-end (drain → reduce →
    /// connection state).
    #[test]
    fn ingest_stream_feeds_the_store() {
        use crate::reactive::Runtime;
        use crate::store::{Route, Store, Unread};
        let runtime = Runtime::new();
        let store = Store::new(runtime);
        store.open_tab("s1", None);
        store.navigate(Route::Home);
        store.ingest_all([
            Ingest::Connected,
            Ingest::Event(BusEvent::new(
                "message.updated",
                serde_json::json!({ "sessionID": "s1" }),
            )),
        ]);
        assert_eq!(store.connection(), ConnectionState::Connected);
        assert_eq!(store.tabs().unread.get("s1"), Some(&Unread::Activity));
    }
}
