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
//! ## The F2 decision: replay dedup by event id (gate-10)
//!
//! Reconnects re-subscribe with the tracked cursor as the SSE
//! `Last-Event-ID` header (below). No v2.0.24 server honors it today,
//! so the header is speculative — but a future server that DOES
//! replays events the client already reduced, and the delta reducers
//! blindly `push_str`, so a replay would double-append text onto the
//! message trees.
//!
//! **Decision (Phase 8): keep sending the header; dedup by event id
//! in the store.** [`crate::store::Store::ingest_all`] remembers the
//! last 4096 bus-event ids and drops exact re-deliveries before any
//! reducer runs. The contract, in order:
//!
//! 1. live events reduce as they arrive (ids recorded);
//! 2. a disconnect triggers reconnect: `Last-Event-ID: <cursor>` +
//!    the resync fetches (session list + open `message.list`) queue
//!    ahead of the new stream;
//! 3. the snapshot rebuilds the open sessions' trees wholesale
//!    (drain-before-reconcile);
//! 4. a replay-honoring server re-sends post-cursor events — the
//!    store drops any whose id is in the window (step 1 recorded
//!    them), so nothing double-applies onto the rebuilt tree;
//! 5. genuinely new events (fresh ids) apply on top.
//!
//! Exact-id matching (not a numeric high-water mark) is the honest
//! option: bus event ids are opaque strings (`evt_*`), there is no
//! monotonic ordering to compare against, and replay re-sends the
//! SAME ids. Events without ids pass untouched (fixture streams).
//! The 4096-id window bounds memory while comfortably covering any
//! plausible offline burst between resubscribes; a server replaying
//! more than 4096 events past the cursor would exceed the resync's
//! own consistency net anyway (the snapshot is the backstop, not the
//! reverse).
//!
//! Wire shape: `/api/event` SSE frames are JSON-in-string
//! (`V2EventEncoded`, double-decoded by [`oc_client::SseStream`]) whose
//! payload is the bus `Event` — `{ id, created, type, data }` with
//! `server.connected` prepended per response by the server.

use std::time::Duration;

use oc_client::types::SessionMessageInfo;
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

/// One session row from a resync fetch.
///
/// `time_idle` / `viewed` come from the `session.list` wire shape
/// (`SessionInfo.time.idle` / `.viewed`, epoch seconds) and drive the
/// **unread derivation** (see [`resync_unread`]); both are `None` when
/// the server omits them. `model` is the session record's durable
/// model selection (`SessionInfo.model`) — the TUI selection chain's
/// per-session restore input (v2 `durableSelection`,
/// context/local.tsx:258-266).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshot {
    pub id: String,
    pub title: Option<String>,
    /// Last activity timestamp (`time.idle`); absent when unknown.
    pub time_idle: Option<f64>,
    /// Last view timestamp (`time.viewed`); absent when unknown.
    pub viewed: Option<f64>,
    /// The session record's model (`{providerID, id, variant?}`).
    pub model: Option<SessionModelRef>,
}

/// The durable model off a session record (`ModelRef` subset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionModelRef {
    pub provider_id: String,
    pub id: String,
    pub variant: Option<String>,
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
    /// A transcript fetch result for one session (the session-open
    /// path — v2's `data.session.message.sync` when the route mounts;
    /// the store MERGES it over live-projected rows, unlike the
    /// reconnect resync's wholesale rebuild).
    MessagesSnapshot {
        session_id: String,
        messages: Vec<crate::components::message::model::Message>,
    },
    /// A post-reconnect resync snapshot (applied only after the channel
    /// drained — see [`crate::store::Store::ingest_all`]): refreshed
    /// sessions, the idle>viewed unread derivation, and the re-fetched
    /// message trees of every open session (`message.list`).
    Resync {
        sessions: Vec<SessionSnapshot>,
        unread: Vec<String>,
        /// `(sessionID, messages)` per OPEN session at resync time.
        messages: Vec<(String, Vec<crate::components::message::model::Message>)>,
    },
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

/// How the reconnect resync re-fetches state (swappable for tests;
/// production uses [`HttpResync`]): the session list plus the
/// `message.list` of every open session.
pub trait ResyncSource: Send {
    fn fetch_sessions(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<SessionSnapshot>, String>> + Send;
    /// The session IDs whose message trees should re-fetch (the UI's
    /// open tabs — shared with the store side).
    fn open_sessions(&self) -> Vec<String>;
    fn fetch_messages(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<
        Output = Result<Vec<crate::components::message::model::Message>, String>,
    > + Send;
}

/// The open-session registry the store side keeps current (tab
/// open/close); the resync reads it to know which message lists to
/// re-fetch.
pub type SharedOpenSessions = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// Production resync: `session.list` + per-open-session `message.list`
/// over the generated client.
#[derive(Clone)]
pub struct HttpResync {
    client: Client,
    open_sessions: SharedOpenSessions,
}

impl HttpResync {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client, open_sessions: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())) }
    }

    /// The handle the store side updates as tabs open and close.
    #[must_use]
    pub fn open_sessions_handle(&self) -> SharedOpenSessions {
        std::sync::Arc::clone(&self.open_sessions)
    }
}

impl ResyncSource for HttpResync {
    async fn fetch_sessions(&self) -> Result<Vec<SessionSnapshot>, String> {
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
                model: session.model.map(|model| SessionModelRef {
                    provider_id: model.provider_id,
                    id: model.id,
                    variant: model.variant,
                }),
            })
            .collect())
    }

    fn open_sessions(&self) -> Vec<String> {
        self.open_sessions.lock().map(|ids| ids.clone()).unwrap_or_default()
    }

    async fn fetch_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::components::message::model::Message>, String> {
        fetch_session_messages(&self.client, session_id).await
    }
}

/// One session's `message.list` as a CHRONOLOGICAL transcript (the
/// store's bucket order). v2 fetches `order: "desc"` and
/// `toReversed()`s the page (client/solid/data.ts:1633-1637) — the
/// wire default is newest-first, and the transcript renders oldest →
/// newest with the sticky-bottom viewport, so the raw page order would
/// scramble the restored transcript (F19: new turns landing "after"
/// the oldest rows, below the fold).
pub async fn fetch_session_messages(
    client: &oc_client::Client,
    session_id: &str,
) -> Result<Vec<crate::components::message::model::Message>, String> {
    let query = oc_client::types::SessionMessageListQuery {
        cursor: None,
        limit: None,
        order: Some(oc_client::types::SessionMessageListQueryOrder::Desc),
        type_: None,
    };
    let response = client.message().list(session_id, &query).await.map_err(|e| e.to_string())?;
    let mut messages: Vec<crate::components::message::model::Message> =
        response.data.iter().filter_map(message_from_info).collect();
    messages.reverse();
    Ok(messages)
}

/// Convert the wire `files` array (resolved `Prompt.FileAttachment`)
/// into the model's [`UserFile`] set — inline images only matter for
/// display, and duplicates collapse (`deduplicateVisibleImages`).
#[must_use]
pub fn files_from_wire(
    files: Option<&[oc_client::types::PromptFileAttachment]>,
) -> Vec<crate::components::message::model::UserFile> {
    let Some(files) = files else { return Vec::new() };
    let mapped: Vec<crate::components::message::model::UserFile> = files
        .iter()
        .map(|file| crate::components::message::model::UserFile {
            mime: file.mime.clone(),
            data: file.data.clone(),
            name: file.name.clone(),
        })
        .collect();
    crate::components::message::model::deduplicate_visible_files(&mapped)
}

/// The snapshot's terminal-step projection (lane/chrome #13): the
/// `finish` label, the tok/s numerator (`tokens.output+reasoning`),
/// and the context-usage total — the wire snapshot REPLACES the
/// live-streamed message, so these must survive the swap.
#[must_use]
fn assistant_snapshot_stats(
    assistant: &oc_client::types::SessionMessageAssistant,
) -> (Option<String>, Option<u64>, Option<u64>) {
    let finish = assistant.finish.map(|finish| {
        match finish {
            oc_client::types::SessionMessageAssistantFinish::Stop => "stop",
            oc_client::types::SessionMessageAssistantFinish::Length => "length",
            oc_client::types::SessionMessageAssistantFinish::ToolCalls => "tool-calls",
            oc_client::types::SessionMessageAssistantFinish::ContentFilter => "content-filter",
            oc_client::types::SessionMessageAssistantFinish::Error => "error",
            oc_client::types::SessionMessageAssistantFinish::Unknown => "unknown",
        }
        .to_owned()
    });
    let Some(tokens) = assistant.tokens.as_ref() else {
        return (finish, None, None);
    };
    let positive = |value: f64| value.max(0.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let (output_reasoning, total) = (
        (positive(tokens.output) + positive(tokens.reasoning)) as u64,
        (positive(tokens.input)
            + positive(tokens.output)
            + positive(tokens.reasoning)
            + positive(tokens.cache.read)
            + positive(tokens.cache.write)) as u64,
    );
    (finish, (output_reasoning > 0).then_some(output_reasoning), (total > 0).then_some(total))
}
/// Convert one `message.list` wire entry into the transcript model
/// (`SessionMessageInfo` subset the store reducers build — user /
/// assistant-with-parts / idle; other kinds are skipped, matching the
/// SSE reducers).
#[must_use]
pub fn message_from_info(
    info: &SessionMessageInfo,
) -> Option<crate::components::message::model::Message> {
    use crate::components::message::model::{Message, Part, PartTime};
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // epoch ms: bounded by real time
    let epoch = |value: Option<f64>| value.map(|ms| ms.round() as u64);
    match info {
        SessionMessageInfo::User(user) => Some(Message::User {
            id: user.id.clone(),
            text: user.text.clone(),
            created: epoch(Some(user.time.created)).unwrap_or(0),
            files: files_from_wire(user.files.as_deref()),
        }),
        SessionMessageInfo::Assistant(assistant) => {
            let mut parts = Vec::new();
            for item in &assistant.content {
                match item {
                    oc_client::types::SessionMessageAssistantContentItem::Text(text) => {
                        parts.push(Part::Text {
                            text: text.text.clone(),
                            time: PartTime { created: None, completed: None },
                        });
                    }
                    oc_client::types::SessionMessageAssistantContentItem::Reasoning(reasoning) => {
                        parts.push(Part::Reasoning {
                            text: reasoning.text.clone(),
                            time: PartTime {
                                created: reasoning
                                    .time
                                    .as_ref()
                                    .and_then(|time| epoch(Some(time.created))),
                                completed: reasoning
                                    .time
                                    .as_ref()
                                    .and_then(|time| epoch(time.completed)),
                            },
                        });
                    }
                    oc_client::types::SessionMessageAssistantContentItem::Tool(tool) => {
                        parts.push(Part::Tool {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            time: PartTime {
                                created: epoch(Some(tool.time.created)),
                                completed: epoch(tool.time.completed),
                            },
                        });
                    }
                }
            }
            // The snapshot's terminal step finish + token stats
            // (lane/chrome #13 — see `assistant_snapshot_stats`).
            let (finish, tokens, usage_total) = assistant_snapshot_stats(assistant);
            Some(Message::Assistant {
                id: assistant.id.clone(),
                agent: assistant.agent.clone(),
                model: format!("{}/{}", assistant.model.provider_id, assistant.model.id),
                parts,
                error: assistant.error.as_ref().map(|error| error.message.clone()),
                time: PartTime {
                    created: epoch(Some(assistant.time.created)),
                    completed: epoch(assistant.time.completed),
                },
                finish,
                tokens,
                usage_total,
            })
        }
        SessionMessageInfo::Idle(idle) => Some(Message::Idle {
            id: idle.id.clone(),
            created: epoch(Some(idle.time.created)).unwrap_or(0),
        }),
        _ => None,
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
    pub async fn run(mut self, tx: tokio::sync::mpsc::Sender<Ingest>, fetcher: impl ResyncSource) {
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
            // Resync after EVERY successful subscribe — reconnects (the
            // consistency net for events missed while offline) AND the
            // FIRST connect (boot hydration: a TUI that restores tabs
            // from tabs.json never reconnects, so without this arm the
            // restored transcripts would stay empty forever — the M2 D4
            // defect). The fetch results queue behind everything already
            // sent and ahead of the new stream's events. Unread marks
            // derive from `idle > viewed`, known-only; the message trees
            // of every OPEN session re-fetch through `message.list` and
            // rebuild through the store reducers.
            if let Ok(sessions) = fetcher.fetch_sessions().await {
                let unread = resync_unread(&sessions);
                let mut messages = Vec::new();
                for session_id in fetcher.open_sessions() {
                    if let Ok(list) = fetcher.fetch_messages(&session_id).await {
                        messages.push((session_id, list));
                    }
                }
                if tx.send(Ingest::Resync { sessions, unread, messages }).await.is_err() {
                    return;
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
            model: None,
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
    #[allow(clippy::too_many_lines)] // scripted end-to-end listener
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
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
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
        while observed.len() < 7 {
            let Some(message) =
                tokio::time::timeout_at(deadline, rx.recv()).await.expect("deadline")
            else {
                break;
            };
            observed.push(message);
        }
        let _ = runner.await;
        server.abort();
        // ONE ordered stream: the FIRST-connect boot resync (session
        // hydration — the M2 D4 fix), the connected marker + the raw
        // server.connected event + live events from connection 1, the
        // reconnect transition, the second resync snapshot, then live
        // events from connection 2.
        assert_eq!(observed.len(), 7, "observed: {observed:?}");
        let Ingest::Resync { .. } = &observed[0] else {
            panic!("boot resync leads the first connect: {observed:?}");
        };
        assert_eq!(observed[1], Ingest::Connected);
        assert!(
            matches!(&observed[2], Ingest::Event(event) if event.id == "e1" && event.type_ == "server.connected")
        );
        assert!(matches!(&observed[3], Ingest::Event(event) if event.id == "e2"));
        assert!(matches!(
            &observed[4],
            Ingest::Connection(ConnectionState::Reconnecting { attempt: 2 })
        ));
        let Ingest::Resync { sessions, unread, .. } = &observed[5] else {
            panic!("reconnect resync arrives after the transition: {observed:?}");
        };
        // The snapshot carries the fetched sessions (titles through the
        // wire types) and marks ONLY the idle>viewed session unread.
        assert_eq!(sessions[0].title.as_deref(), Some("Fresh"));
        assert_eq!(sessions[0].time_idle, Some(100.0));
        assert_eq!(sessions[0].viewed, Some(50.0));
        assert_eq!(sessions[2].time_idle, None, "s3 omits idle");
        assert_eq!(unread, &vec!["s1".to_owned()], "no false unread for s2/s3");
        assert!(matches!(&observed[6], Ingest::Event(event) if event.id == "e3"));
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

    /// The FULL wave-3 message.list resync, scripted end-to-end through
    /// the production [`HttpResync`] over a local server:
    ///
    /// live events → **disconnect** (events missed while offline: the
    /// server's message list advances) → **reconnect** (the resync
    /// re-fetches `session.list` + the OPEN session's `message.list`,
    /// carrying the missed turn) → the store **reconciles** (the
    /// message tree rebuilt through the resync batch, unread derived
    /// from `idle > viewed`) → **live events continue ordered** on top
    /// of the reconciled tree.
    #[tokio::test]
    #[allow(clippy::too_many_lines)] // scripted end-to-end listener
    async fn reconnect_resync_rebuilds_message_trees_and_live_continues() {
        use crate::reactive::Runtime;
        use crate::store::{Route, Store};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        // The server-side message list — advances while the client is
        // offline (the missed events).
        // DESC order (the wire default): the assistant turn precedes
        // its user prompt; the fetch reverses into chronological.
        let messages = r#"{"cursor":{},"data":[
            {"type":"assistant","id":"msg_a1","agent":"build",
             "model":{"providerID":"fixture","id":"fixture"},"finish":"stop",
             "time":{"created":2,"completed":3},
             "content":[
               {"type":"tool","id":"t1","name":"read","time":{"created":2,"completed":2},
                "state":{"status":"completed","input":{},"content":[]}},
               {"type":"text","text":"It reads the configuration."}
             ]},
            {"type":"user","id":"msg_u1","text":"What does the parser do?","time":{"created":1}}
        ]}"#;
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let (mut reader, _) = socket.split();
                let mut request = vec![0u8; 8192];
                let _ = reader.read(&mut request).await;
                let head = String::from_utf8_lossy(&request);
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let respond = |body: String, kind: &'static str| {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                if path.starts_with("/api/event") {
                    // Both connections: one live event then EOF (the
                    // second connection's event arrives AFTER the
                    // resync — proving live continues on top).
                    let resumed = head.to_lowercase().contains("last-event-id");
                    let body = if resumed {
                        sse_frame(
                            "e3",
                            "session.text.delta",
                            r#"{"sessionID":"s1","assistantMessageID":"msg_a2","delta":"Live after resync."}"#,
                        )
                    } else {
                        sse_frame("e1", "server.connected", "{}")
                            + &sse_frame("e2", "message.updated", r#"{"sessionID":"s1"}"#)
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if path == "/api/session" {
                    // idle > viewed → unread derivation input.
                    let body = concat!(
                        r#"{"cursor":{},"data":[{"#,
                        r#""id":"s1","title":"Resynced","#,
                        r#""time":{"created":0,"updated":0,"idle":100,"viewed":50},"#,
                        r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
                        r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}]}"#,
                    )
                    .to_owned();
                    let _ = socket.write_all(respond(body, "application/json").as_bytes()).await;
                } else if path == "/api/session/s1/message" {
                    let _ = socket
                        .write_all(respond(messages.to_owned(), "application/json").as_bytes())
                        .await;
                } else {
                    let _ = socket
                        .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                        .await;
                }
            }
        });

        let client = Client::new(&format!("http://{addr}")).expect("client");
        let resync = HttpResync::new(client.clone());
        let open_sessions = resync.open_sessions_handle();
        // The run loop keeps this current from boot (tab open/close) —
        // seed it BEFORE the connection so the first reconnect's
        // fetch already sees the open session.
        if let Ok(mut ids) = open_sessions.lock() {
            *ids = vec!["s1".to_owned()];
        }
        let connection = Connection::with_config(
            client,
            ConnectionConfig {
                backoff_base: Duration::from_millis(10),
                backoff_max: Duration::from_millis(10),
                max_attempts: Some(2),
            },
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let runner = tokio::spawn(connection.run(tx, resync));

        // The store side: an OPEN session tab feeds the open-session
        // registry (the run loop's sync).
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Session one".to_owned()));
        store.navigate(Route::Home);

        let mut observed = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while observed.len() < 7 {
            let Some(message) =
                tokio::time::timeout_at(deadline, rx.recv()).await.expect("deadline")
            else {
                break;
            };
            // The run loop keeps the registry current between drains.
            if let Ok(mut ids) = open_sessions.lock() {
                *ids = store.open_session_ids();
            }
            store.ingest_all([message.clone()]);
            observed.push(message);
        }
        let _ = runner.await;
        server.abort();

        // ONE ordered stream: the boot resync (the missed turn fetched
        // on the FIRST connect — session hydration) → live (connected
        // marker + the raw server.connected + the live event) →
        // reconnect transition → resync (sessions + unread + messages)
        // → live.
        assert_eq!(observed.len(), 7, "observed: {observed:?}");
        assert!(
            matches!(&observed[0], Ingest::Resync { messages, .. } if matches!(
                messages.as_slice(),
                [(_, list)] if list.len() == 2
            )),
            "the first-connect resync already hydrated the open session: {observed:?}"
        );
        assert!(matches!(observed[1], Ingest::Connected));
        assert!(matches!(&observed[2], Ingest::Event(event) if event.id == "e1"));
        assert!(matches!(&observed[3], Ingest::Event(event) if event.id == "e2"));
        assert!(matches!(
            &observed[4],
            Ingest::Connection(ConnectionState::Reconnecting { attempt: 2 })
        ));
        let Ingest::Resync { sessions, unread, messages } = &observed[5] else {
            panic!("resync snapshot: {observed:?}");
        };
        assert_eq!(sessions[0].title.as_deref(), Some("Resynced"));
        assert_eq!(unread, &vec!["s1".to_owned()], "idle 100 > viewed 50");
        // The OPEN session's message.list re-fetch carried the missed
        // turn through the wire types.
        assert_eq!(messages.len(), 1, "one open session refetched");
        let (session_id, list) = &messages[0];
        assert_eq!(session_id, "s1");
        assert_eq!(list.len(), 2, "user + assistant");
        assert!(
            matches!(&list[0], crate::components::message::model::Message::User { text, .. } if text == "What does the parser do?")
        );
        assert!(
            matches!(&list[1], crate::components::message::model::Message::Assistant { parts, finish, .. }
                if parts.len() == 2 && finish.as_deref() == Some("stop"))
        );
        // The store reconciled and live continued ON TOP: the tree is
        // the resync's rebuilt pair, then the post-resync delta
        // appended in order (the ingest loop applied all seven).
        assert!(matches!(&observed[6], Ingest::Event(event) if event.id == "e3"), "{observed:?}");
        let after = store.messages("s1");
        assert_eq!(after.len(), 3, "rebuilt pair + live delta, in order");
        assert!(matches!(&after[0], crate::components::message::model::Message::User { text, .. }
                if text == "What does the parser do?"));
        assert!(
            matches!(&after[1], crate::components::message::model::Message::Assistant { parts, .. }
                if parts.len() == 2 && parts[1].text() == "It reads the configuration.")
        );
        assert!(
            matches!(&after[2], crate::components::message::model::Message::Assistant { parts, .. }
                if parts[0].text() == "Live after resync.")
        );
        // Unread derived (offline event + idle>viewed) and the title
        // refreshed in place.
        assert_eq!(store.tabs().tabs[0].title.as_deref(), Some("Resynced"));
        assert!(store.tabs().unread.contains_key("s1"));
    }

    /// **The F2 decision test**: a scripted server that HONORS
    /// `Last-Event-ID` on resubscribe REPLAYS the post-cursor events
    /// (same ids) — the store's replay-dedup window must drop them so
    /// nothing double-appends onto the resync-rebuilt tree.
    ///
    /// Timeline: live delta e2 ("Hello") → disconnect (the server's
    /// message list already contains the full "Hello") → reconnect
    /// (Last-Event-ID: e2) → the resync rebuilds the tree from
    /// `message.list` ("Hello") → the server replays e2 ("Hello"!)
    /// and then sends e3 (" World"). Without dedup the tree would end
    /// `"HelloHello World"`; the contract says "Hello World".
    #[tokio::test]
    #[allow(clippy::too_many_lines)] // scripted end-to-end listener
    async fn replaying_last_event_id_server_does_not_double_append() {
        use crate::reactive::Runtime;
        use crate::store::{Route, Store};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        // The server-side message list at resync time: the turn already
        // completed (text "Hello").
        let messages = r#"{"cursor":{},"data":[
            {"type":"assistant","id":"msg_a1","agent":"build",
             "model":{"providerID":"fixture","id":"fixture"},
             "time":{"created":1},
             "content":[{"type":"text","text":"Hello"}]}
        ]}"#;
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.expect("accept");
                let (mut reader, _) = socket.split();
                let mut request = vec![0u8; 8192];
                let _ = reader.read(&mut request).await;
                let head = String::from_utf8_lossy(&request);
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .split('?')
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let respond = |body: String, kind: &'static str| {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                if path.starts_with("/api/event") {
                    // The replay-honoring arm: on resubscription re-send
                    // the post-cursor events (SAME ids) plus one fresh.
                    let resumed = head.to_lowercase().contains("last-event-id: e2");
                    let body = if resumed {
                        sse_frame(
                            "e2",
                            "session.text.delta",
                            r#"{"sessionID":"s1","assistantMessageID":"msg_a1","delta":"Hello"}"#,
                        ) + &sse_frame(
                            "e3",
                            "session.text.delta",
                            r#"{"sessionID":"s1","assistantMessageID":"msg_a1","delta":" World"}"#,
                        )
                    } else {
                        sse_frame("e1", "server.connected", "{}")
                            + &sse_frame(
                                "e2",
                                "session.text.delta",
                                r#"{"sessionID":"s1","assistantMessageID":"msg_a1","delta":"Hello"}"#,
                            )
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                } else if path == "/api/session" {
                    let body = concat!(
                        r#"{"cursor":{},"data":[{"#,
                        r#""id":"s1","title":"Replay","#,
                        r#""time":{"created":0,"updated":0,"idle":1,"viewed":1},"#,
                        r#""location":{"directory":"/tmp"},"projectID":"p","cost":0,"#,
                        r#""tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}]}"#,
                    )
                    .to_owned();
                    let _ = socket.write_all(respond(body, "application/json").as_bytes()).await;
                } else if path == "/api/session/s1/message" {
                    let _ = socket
                        .write_all(respond(messages.to_owned(), "application/json").as_bytes())
                        .await;
                } else {
                    let _ = socket
                        .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                        .await;
                }
            }
        });

        let client = Client::new(&format!("http://{addr}")).expect("client");
        let resync = HttpResync::new(client.clone());
        let open_sessions = resync.open_sessions_handle();
        if let Ok(mut ids) = open_sessions.lock() {
            *ids = vec!["s1".to_owned()];
        }
        let connection = Connection::with_config(
            client,
            ConnectionConfig {
                backoff_base: Duration::from_millis(10),
                backoff_max: Duration::from_millis(10),
                max_attempts: Some(2),
            },
        );
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let runner = tokio::spawn(connection.run(tx, resync));

        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Session one".to_owned()));
        store.navigate(Route::Home);

        let mut observed = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while observed.len() < 8 {
            let Some(message) =
                tokio::time::timeout_at(deadline, rx.recv()).await.expect("deadline")
            else {
                break;
            };
            if let Ok(mut ids) = open_sessions.lock() {
                *ids = store.open_session_ids();
            }
            store.ingest_all([message.clone()]);
            observed.push(message);
        }
        let _ = runner.await;
        server.abort();

        // boot resync ("Hello") + connected + live delta → reconnect →
        // resync → replayed e2 (delivered, but dropped by the dedup window) + e3.
        assert_eq!(observed.len(), 8, "observed: {observed:?}");
        assert!(
            matches!(&observed[0], Ingest::Resync { messages, .. } if matches!(
                messages.as_slice(),
                [(_, list)] if list.len() == 1
            )),
            "the first-connect boot resync fetched the open session: {observed:?}"
        );
        assert!(matches!(observed[1], Ingest::Connected));
        assert!(matches!(&observed[2], Ingest::Event(event) if event.id == "e1"));
        assert!(matches!(&observed[3], Ingest::Event(event) if event.id == "e2"));
        assert!(
            matches!(
                &observed[4],
                Ingest::Connection(ConnectionState::Reconnecting { attempt: 2 })
            ),
            "{observed:?}"
        );
        assert!(matches!(
            &observed[5],
            Ingest::Resync { messages, .. } if matches!(
                messages.as_slice(),
                [(_, list)] if list.len() == 1
            )
        ));
        assert!(
            matches!(&observed[6], Ingest::Event(event) if event.id == "e2"),
            "the replayed frame DID arrive (same id)"
        );
        assert!(matches!(&observed[7], Ingest::Event(event) if event.id == "e3"));
        // THE CONTRACT: the replayed e2 did not double-append onto the
        // resync-rebuilt tree ("HelloHello World" without dedup).
        let messages = store.messages("s1");
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].parts()[0].text(),
            "Hello World",
            "replay deduped; fresh delta applied"
        );
    }

    /// F19 — `fetch_session_messages` asks `order: "desc"` and REVERSES
    /// the page into a chronological transcript (v2 data.ts:1633-1637
    /// `order: "desc"` + `toReversed()`). The store's bucket renders
    /// oldest → newest with the sticky-bottom viewport; the raw
    /// newest-first page scrambled restored transcripts (new turns
    /// landing "after" the oldest rows, below the fold).
    #[tokio::test]
    async fn fetch_session_messages_reverses_the_desc_page() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            // DESC wire order: newest (msg_a2) first.
            let body = r#"{"cursor":{},"data":[
                {"type":"assistant","id":"msg_a2","agent":"build",
                 "model":{"providerID":"p","id":"m"},"time":{"created":4,"completed":5},
                 "content":[{"type":"text","text":"second reply"}]},
                {"type":"user","id":"msg_u2","text":"second ask","time":{"created":3}},
                {"type":"assistant","id":"msg_a1","agent":"build",
                 "model":{"providerID":"p","id":"m"},"time":{"created":2,"completed":2},
                 "content":[{"type":"text","text":"first reply"}]},
                {"type":"user","id":"msg_u1","text":"first ask","time":{"created":1}}
            ]}"#;
            let (mut socket, _) = listener.accept().await.expect("accept");
            let (mut reader, _) = socket.split();
            let mut request = vec![0u8; 8192];
            let _ = reader.read(&mut request).await;
            let head = String::from_utf8_lossy(&request);
            let raw_path = head.split_whitespace().nth(1).unwrap_or_default();
            // The query carries the explicit desc order (the path is
            // the transcript endpoint).
            assert!(raw_path.contains("order=desc"), "the fetch asks desc: {raw_path}");
            assert!(raw_path.starts_with("/api/session/s1/message"), "the endpoint: {raw_path}");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        let client = Client::new(&format!("http://{addr}")).expect("client");
        let messages = fetch_session_messages(&client, "s1").await.expect("fetch");
        server.abort();
        // Chronological: the store's bucket order.
        let ids: Vec<&str> =
            messages.iter().map(crate::components::message::model::Message::id).collect();
        assert_eq!(ids, vec!["msg_u1", "msg_a1", "msg_u2", "msg_a2"], "reversed to oldest-first");
        assert!(
            matches!(&messages[0], crate::components::message::model::Message::User { text, .. }
                if text == "first ask")
        );
    }
}
