//! D-07 / G11-F1 — the first-connect snapshot/stream race window,
//! **closed by proof** (deterministic simulation, registered residual
//! D-07 in `ROADMAP.md`).
//!
//! The registered theoretical window: on first connect (and every
//! reconnect) the client combines a **snapshot** (the resync fetches:
//! `session.list` + open `message.list`) with the **live event
//! stream** — an event committed between the subscribe and the fetch
//! can land in BOTH, and ordering between the two surfaces is only
//! constrained by the data layer's single ingest channel. The hazard
//! is a drop or a duplicate around that boundary.
//!
//! This suite drives the REAL pair — the oc-tui [`Store`] (drain,
//! reducers, the 4096-id replay-dedup ring, the G11-F1
//! complete-message guard) plus the REAL oc-plugin-host
//! [`BroadcastTap`] (its own 4096-id ring) — through every adversarial
//! ordering, with a scripted server that models the one load-bearing
//! **server-side invariant** the whole construction rides on:
//!
//! > `session.text.delta` / `session.reasoning.delta` are EPHEMERAL
//! > (`oc-core/src/bus/registry.rs` `SESSION_EPHEMERAL`) — never
//! > persisted, never projected: the durable message state's text
//! > part materializes EMPTY at `session.text.started` and is filled
//! > ONLY by `session.text.ended` (`oc-core/src/session/
//! > message_updater.rs` `"session.text.ended" => *target = text`).
//! > Therefore **no fetch ever contains partial in-flight delta
//! > text**, and no snapshot/stream text overlap can exist.
//!
//! With that invariant held by construction, the proofs are:
//!
//! | scenario | test | result |
//! |---|---|---|
//! | snapshot-then-stream, overlap in flight | [`first_connect_snapshot_then_stream_overlap_is_exact`] | no dup, no drop |
//! | stream-then-snapshot (reconnect mid-generation) | [`reconnect_resync_resets_display_and_ended_heals`] | transient display reset (v2 parity — deltas are unrecoverable), healed exactly at `text.ended` |
//! | snapshot after completion, stream replays the deltas | [`complete_message_snapshot_never_appends`] | the complete-guard drops every overlapped delta |
//! | Last-Event-ID resume mid-snapshot | [`last_event_id_resume_mid_snapshot_is_idempotent`] | exact envelope-id re-deliveries dropped in either arrival order |
//! | 4096-ring boundary coverage | [`dedup_ring_boundary_exact_coverage`] | a full-window overlap is 100% covered on BOTH rings; past the window the documented bound applies (no false positives) |
//! | the drain discipline | [`snapshot_queued_mid_stream_serializes_in_channel_order`] | a snapshot queued mid-stream applies in exact channel order |
//!
//! Every test also asserts tap/store parity: the tap observed EXACTLY
//! the store's reduced stream — 1:1, in order. If any assertion here
//! can be violated by a real interleaving, the construction is
//! broken; if not, this suite IS the proof and D-07 closes as proven
//! safe (cite the test names).

use std::sync::Arc;

use oc_plugin_host::{BroadcastTap, TapEvent};
use oc_tui::components::message::model::{Message, Part, PartTime};
use oc_tui::data::{BusEvent, Ingest, SessionSnapshot};
use oc_tui::reactive::Runtime;
use oc_tui::store::{EventTap, Store};

// ---------------------------------------------------------------------------
// The scripted server — commit-ordered frames + the two read surfaces
// ---------------------------------------------------------------------------

/// Whether a frame type is durable (persisted + projected into the
/// fetchable message state). Deltas are ephemeral — THE invariant.
fn is_durable(type_: &str) -> bool {
    !matches!(type_, "session.text.delta" | "session.reasoning.delta" | "session.tool.input.delta")
}

/// One server-side commit, in order.
#[derive(Clone)]
struct Frame {
    /// Envelope id (`evt_…`); deltas get ids too (the live tap
    /// delivers them with envelopes).
    id: &'static str,
    type_: &'static str,
    data: serde_json::Value,
    /// Commit timestamp (ordering only).
    t: u64,
}

impl Frame {
    fn event(&self) -> BusEvent {
        BusEvent {
            id: self.id.to_owned(),
            type_: self.type_.to_owned(),
            data: self.data.clone(),
            created: Some(self.t),
        }
    }
}

/// The scripted event log. `stream_from(S)` is what the SSE
/// subscription delivers (every frame committed at/after the
/// subscribe registered); `fetch_messages_at(F)` is what
/// `message.list` returns — the DURABLE-ONLY materialization.
struct ScriptedServer {
    frames: Vec<Frame>,
}

impl ScriptedServer {
    fn new(frames: Vec<Frame>) -> Self {
        Self { frames }
    }

    /// The live stream from a subscribe point (`Connection::run`
    /// reads it only after queueing the resync).
    fn stream_from(&self, subscribe_at: u64) -> Vec<BusEvent> {
        self.frames.iter().filter(|frame| frame.t >= subscribe_at).map(Frame::event).collect()
    }

    /// `message.list` at a fetch point: the durable projection ONLY —
    /// a message exists after its `session.step.started`; the text
    /// part exists (EMPTY) after `session.text.started` and carries
    /// text only once `session.text.ended` committed; completion from
    /// `session.step.ended` (the `message_updater.rs` rules).
    fn fetch_messages_at(&self, fetch_at: u64) -> Vec<Message> {
        let mut messages: Vec<Message> = Vec::new();
        for frame in &self.frames {
            if frame.t > fetch_at || !is_durable(frame.type_) {
                continue;
            }
            let assistant =
                || frame.data.get("assistantMessageID").and_then(serde_json::Value::as_str);
            match frame.type_ {
                "session.step.started" => {
                    let Some(id) = assistant() else { continue };
                    if !messages.iter().any(|message| message.id() == id) {
                        messages.push(Message::Assistant {
                            id: id.to_owned(),
                            agent: "build".to_owned(),
                            model: String::new(),
                            parts: Vec::new(),
                            finish: None,
                            error: None,
                            time: PartTime { created: Some(frame.t), completed: None },
                        });
                    }
                }
                "session.text.started" => {
                    let Some(id) = assistant() else { continue };
                    if let Some(Message::Assistant { parts, .. }) =
                        messages.iter_mut().find(|message| message.id() == id)
                    {
                        parts.push(Part::Text {
                            // Deltas are ephemeral — NEVER in the fetch.
                            text: String::new(),
                            time: PartTime { created: Some(frame.t), completed: None },
                        });
                    }
                }
                "session.text.ended" => {
                    let Some(id) = assistant() else { continue };
                    let text = frame.data.get("text").and_then(serde_json::Value::as_str);
                    if let Some(Message::Assistant { parts, .. }) =
                        messages.iter_mut().find(|message| message.id() == id)
                    {
                        if let Some(Part::Text { text: slot, .. }) = parts.last_mut() {
                            text.unwrap_or_default().clone_into(slot);
                        }
                    }
                }
                "session.step.ended" => {
                    let Some(id) = assistant() else { continue };
                    if let Some(Message::Assistant { finish, time, .. }) =
                        messages.iter_mut().find(|message| message.id() == id)
                    {
                        *finish = Some("stop".to_owned());
                        time.completed = Some(frame.t);
                    }
                }
                _ => {}
            }
        }
        messages
    }
}

// ---------------------------------------------------------------------------
// The harness — real Store + real BroadcastTap, recording both views
// ---------------------------------------------------------------------------

/// Adapts the store's post-ingest seam onto the real plugin-host tap
/// (the production `plugin_boot::HostTap` wiring).
struct RealTap {
    tap: BroadcastTap,
}

impl EventTap for RealTap {
    fn publish(&self, event: &BusEvent) {
        self.tap.publish(TapEvent {
            id: event.id.clone(),
            typ: event.type_.clone(),
            data: event.data.clone(),
            created: event.created,
        });
    }
}

struct Harness {
    store: Store,
    tap: BroadcastTap,
    /// The receiver keeps the real tap's channel open.
    _rx: tokio::sync::mpsc::Receiver<TapEvent>,
    /// The store-side reduced stream (ids in reduce order) — the
    /// parity reference for the tap's observations.
    applied: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Harness {
    fn new() -> Self {
        // A recorder in FRONT of the real tap: the store publishes to
        // both — the recorder proves the store's reduced stream, the
        // real ring proves the tap's own dedup/coverage semantics.
        struct Pair {
            real: RealTap,
            applied: Arc<std::sync::Mutex<Vec<String>>>,
        }
        impl EventTap for Pair {
            fn publish(&self, event: &BusEvent) {
                self.applied.lock().expect("applied").push(event.id.clone());
                self.real.publish(event);
            }
        }
        let (tx, rx) = tokio::sync::mpsc::channel(16_384);
        let tap = BroadcastTap::live(tx);
        let store = Store::new(Runtime::new());
        let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
        store.set_event_tap(Arc::new(Pair {
            real: RealTap { tap: tap.clone() },
            applied: Arc::clone(&applied),
        }));
        Self { store, tap, _rx: rx, applied }
    }

    /// One connection epoch, in the exact `Connection::run` channel
    /// order: the resync snapshot (queued BEFORE the stream is read),
    /// then the live frames from the subscribe point.
    fn connect_epoch(&self, server: &ScriptedServer, subscribe_at: u64, fetch_at: u64) {
        let snapshot = server.fetch_messages_at(fetch_at);
        let batch: Vec<Ingest> = std::iter::once(Ingest::Resync {
            sessions: vec![SessionSnapshot {
                model: None,
                id: SESSION.to_owned(),
                title: Some(SESSION.to_owned()),
                time_idle: None,
                viewed: None,
            }],
            unread: Vec::new(),
            messages: vec![(SESSION.to_owned(), snapshot)],
        })
        .chain(server.stream_from(subscribe_at).into_iter().map(Ingest::Event))
        .collect();
        self.store.ingest_all(batch);
    }

    /// Feeds raw frames (e.g. a Last-Event-ID replay from a
    /// cursor-aware server) straight through the store's ingest.
    fn feed(&self, events: Vec<BusEvent>) {
        self.store.ingest_all(events.into_iter().map(Ingest::Event));
    }

    /// The concatenated text of `message`'s parts.
    fn text_of(&self, message: &str) -> String {
        let messages = self.store.messages(SESSION);
        let Some(found) = messages.iter().find(|m| m.id() == message) else {
            panic!("message {message} not found in {messages:?}")
        };
        let Message::Assistant { parts, .. } = found else { panic!("not assistant") };
        parts.iter().map(Part::text).collect::<Vec<_>>().join("")
    }

    fn is_complete(&self, message: &str) -> bool {
        self.store.messages(SESSION).iter().find(|m| m.id() == message).is_some_and(|m| match m {
            Message::Assistant { time, .. } => time.completed.is_some(),
            _ => false,
        })
    }

    /// Tap/store parity: the store reduced EXACTLY the expected
    /// envelopes, in order, with no duplicates — and the real tap ring
    /// dropped nothing the store reduced (`deduped_count` only counts
    /// re-publishes, which the store never makes).
    fn assert_parity(&self, expected: &[&str]) {
        let applied = self.applied.lock().expect("applied").clone();
        assert_eq!(applied, expected, "the store reduced exactly the expected stream");
        assert_eq!(
            self.tap.deduped_count(),
            0,
            "the store never re-publishes: the real tap ring never fires on the store's stream"
        );
        let unique: std::collections::HashSet<&String> = applied.iter().collect();
        assert_eq!(unique.len(), applied.len(), "no duplicate reductions");
    }
}

// ---------------------------------------------------------------------------
// The script — one streaming message everyone shares
// ---------------------------------------------------------------------------

const SESSION: &str = "ses_g11";
const MESSAGE: &str = "msg_g11";

/// The canonical streaming lifecycle: the step starts, text streams in
/// three deltas, ends with the authoritative full text, the step ends.
/// Deltas at t=30/40/50 straddle every subscribe/fetch split below.
fn streaming_script() -> Vec<Frame> {
    let part_event = |extra: &[(&'static str, serde_json::Value)]| {
        let mut data = serde_json::Map::new();
        data.insert("sessionID".to_owned(), serde_json::json!(SESSION));
        data.insert("assistantMessageID".to_owned(), serde_json::json!(MESSAGE));
        data.insert("ordinal".to_owned(), serde_json::json!(0));
        for (key, value) in extra {
            data.insert((*key).to_owned(), value.clone());
        }
        serde_json::Value::Object(data)
    };
    vec![
        Frame { id: "evt_step", type_: "session.step.started", data: part_event(&[]), t: 10 },
        Frame { id: "evt_ts", type_: "session.text.started", data: part_event(&[]), t: 20 },
        Frame {
            id: "evt_d1",
            type_: "session.text.delta",
            data: part_event(&[("delta", serde_json::json!("Hello "))]),
            t: 30,
        },
        Frame {
            id: "evt_d2",
            type_: "session.text.delta",
            data: part_event(&[("delta", serde_json::json!("proven "))]),
            t: 40,
        },
        Frame {
            id: "evt_d3",
            type_: "session.text.delta",
            data: part_event(&[("delta", serde_json::json!("safe"))]),
            t: 50,
        },
        Frame {
            id: "evt_te",
            type_: "session.text.ended",
            data: part_event(&[("text", serde_json::json!("Hello proven safe"))]),
            t: 60,
        },
        Frame {
            id: "evt_se",
            type_: "session.step.ended",
            data: part_event(&[("finish", serde_json::json!("stop"))]),
            t: 70,
        },
    ]
}

// ---------------------------------------------------------------------------
// S1 — snapshot-then-stream with the overlap in flight
// ---------------------------------------------------------------------------

/// First connect while the message streams: subscribe at t=25 (after
/// `text.started`), fetch at t=45. The overlap window (subscribe,
/// fetch) contains d1 (t=30) and d2 (t=40) — committed BEFORE the
/// fetch AND delivered on the stream: the exact G11-F1 double-append
/// shape. The fetch contains NO partial text (deltas are ephemeral —
/// the server-side invariant), so the stream deltas are the only text
/// source: the final state is exact — nothing dropped, nothing
/// duplicated.
#[test]
fn first_connect_snapshot_then_stream_overlap_is_exact() {
    let server = ScriptedServer::new(streaming_script());
    let harness = Harness::new();

    harness.connect_epoch(&server, 25, 45);

    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe");
    assert!(harness.is_complete(MESSAGE));
    harness.assert_parity(&["evt_d1", "evt_d2", "evt_d3", "evt_te", "evt_se"]);
}

// ---------------------------------------------------------------------------
// S2 — stream-then-snapshot (reconnect mid-generation)
// ---------------------------------------------------------------------------

/// Epoch 1 streams step/text-start/d1/d2, the connection drops, epoch
/// 2 reconnects (subscribe t=55, fetch t=56): the durable-only
/// snapshot rebuilds the in-flight message with an EMPTY text part —
/// the pre-subscribe delta text is unrecoverable in ANY implementation
/// (v2 parity: deltas are ephemeral server-side there too) — and the
/// authoritative `text.ended` heals the state exactly. No duplicates
/// across epochs, no double-append of d1/d2.
#[test]
fn reconnect_resync_resets_display_and_ended_heals() {
    let server = ScriptedServer::new(streaming_script());
    let harness = Harness::new();

    // Epoch 1: subscribed before everything; the stream cuts after d2.
    harness.feed(server.stream_from(5).into_iter().take(4).collect());
    assert_eq!(harness.text_of(MESSAGE), "Hello proven ", "epoch 1 partial display");

    // Epoch 2: resync first (durable-only), then the stream from t=55
    // (d3 at t=50 predates the subscribe — an ephemeral frame missed
    // by definition; ended + step.ended are durable and delivered).
    harness.connect_epoch(&server, 55, 56);

    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe", "text.ended healed authoritatively");
    assert!(harness.is_complete(MESSAGE));
    // Both epochs reduced every delivered envelope at most once.
    harness.assert_parity(&["evt_step", "evt_ts", "evt_d1", "evt_d2", "evt_te", "evt_se"]);
}

// ---------------------------------------------------------------------------
// S3 — the snapshot completed first, the stream replays the deltas
// ---------------------------------------------------------------------------

/// Subscribe at t=15, fetch at t=100 — the message is COMPLETE in the
/// snapshot with its final text — while the stream delivers every
/// delta of the overlap window. Each delta hits the G11-F1
/// complete-message guard (dropped); `text.ended` replaces with the
/// identical authoritative text. This is the belt-AND-suspenders case:
/// the fetch genuinely contains final text (unlike S1/S2), and the
/// predicted double-append is still impossible.
#[test]
fn complete_message_snapshot_never_appends() {
    let server = ScriptedServer::new(streaming_script());
    let harness = Harness::new();

    harness.connect_epoch(&server, 15, 100);

    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe");
    assert!(harness.is_complete(MESSAGE));
    // text.started and all three deltas passed the envelope filter
    // and hit the complete-guard, each reduced exactly once.
    harness.assert_parity(&["evt_ts", "evt_d1", "evt_d2", "evt_d3", "evt_te", "evt_se"]);
}

// ---------------------------------------------------------------------------
// S4 — Last-Event-ID resume mid-snapshot
// ---------------------------------------------------------------------------

/// A cursor-aware server (our port's client-side addition — v2.0.24
/// servers ignore the header) replays the durable frames after the
/// cursor with the SAME envelope ids while the resync snapshot also
/// lands. Exact-id re-deliveries are dropped by the store's replay
/// ring.
///
/// Reachability note (part of the proof): "replay lands before the
/// snapshot" is NOT a real ordering — `Connection::run` queues the
/// resync fetch results into the single ingest channel BEFORE reading
/// one frame of the new stream (data.rs), and the resume cursor is
/// always the id of the LAST APPLIED event, so a replay is
/// prefix-consistent with everything already reduced (it can only
/// re-send durable frames the store saw applied, never their
/// predecessors). The scenario below drives the reachable extremes:
/// the replay arriving after the epoch's stream (mid-snapshot), and a
/// full re-subscribe from scratch (the boot-hydration epoch: empty
/// snapshot, whole stream, then the durable-tail replay).
#[test]
fn last_event_id_resume_mid_snapshot_is_idempotent() {
    let server = ScriptedServer::new(streaming_script());
    let harness = Harness::new();

    // Epoch 1 with the cursor landing mid-message (after d2, t=40):
    // normal overlap epoch, everything applied once.
    harness.connect_epoch(&server, 15, 45);
    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe");

    // The reconnect's Last-Event-ID replay: the durable tail after
    // the cursor, re-sent with the ORIGINAL envelope ids.
    let replay: Vec<BusEvent> = server
        .frames
        .iter()
        .filter(|frame| is_durable(frame.type_) && frame.t > 40)
        .map(Frame::event)
        .collect();
    assert_eq!(
        replay.iter().map(|event| event.id.as_str()).collect::<Vec<_>>(),
        vec!["evt_te", "evt_se"],
        "the replay covers the durable tail only (deltas are ephemeral: never replayed)"
    );

    // Real order: the replay follows the epoch (the new stream), then
    // the epoch-2 resync snapshot lands — every re-delivery dropped.
    harness.feed(replay.clone());
    harness.connect_epoch(&server, 65, 100);
    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe", "the replay changed nothing");
    harness.assert_parity(&["evt_ts", "evt_d1", "evt_d2", "evt_d3", "evt_te", "evt_se"]);

    // The other reachable extreme on a FRESH store: the
    // boot-hydration epoch (fetch@5: the durable state is EMPTY —
    // nothing had committed — so the snapshot carries no messages and
    // the stream builds everything), followed by the durable-tail
    // replay after the cursor. Same idempotence.
    let boot = Harness::new();
    boot.connect_epoch(&server, 5, 5);
    assert_eq!(boot.text_of(MESSAGE), "Hello proven safe");
    boot.feed(replay);
    assert_eq!(boot.text_of(MESSAGE), "Hello proven safe", "the replay changed nothing");
    boot.assert_parity(&["evt_step", "evt_ts", "evt_d1", "evt_d2", "evt_d3", "evt_te", "evt_se"]);
}

// ---------------------------------------------------------------------------
// S5 — the 4096-id ring boundary (store ring + tap ring)
// ---------------------------------------------------------------------------

/// One durable ring-window event.
fn ring_event(id: String) -> BusEvent {
    BusEvent {
        id,
        type_: "session.updated".to_owned(),
        data: serde_json::json!({ "sessionID": "ses_ring" }),
        created: Some(1),
    }
}

/// The replay-dedup window (`SEEN_EVENT_WINDOW = 4096`) covers a
/// FULL-WINDOW overlap exactly: re-delivering 4096 seen ids drops all
/// of them; one event past the window evicts the oldest, whose
/// re-delivery is then a FRESH application (the documented bound: an
/// overlap deeper than the window falls back to the resync snapshot
/// as the consistency net — the ring never false-positives on fresh
/// ids). A SECOND real tap gets the same overlap fed directly and
/// dedups it independently (the tap-side hardening, exercised without
/// the store in front of it).
#[test]
fn dedup_ring_boundary_exact_coverage() {
    let harness = Harness::new();

    // 4096 distinct durable frames reduce through the store.
    let ids: Vec<String> = (1..=4096).map(|n| format!("evt_{n}")).collect();
    harness.feed(ids.iter().cloned().map(ring_event).collect());

    // A full-window Last-Event-ID replay: every id still retained →
    // every re-delivery dropped.
    harness.feed(ids.iter().cloned().map(ring_event).collect());
    assert_eq!(harness.applied.lock().expect("applied").len(), 4096, "full-window replay dropped");

    // One more fresh event evicts evt_1 from the ring; evt_1's
    // re-delivery past the window is then a FRESH application — the
    // bound, not a hole in the construction (an overlap this deep is
    // the resync snapshot's job).
    harness.feed(vec![ring_event("evt_4097".to_owned())]);
    harness.feed(vec![ring_event("evt_1".to_owned())]);
    let applied = harness.applied.lock().expect("applied").clone();
    assert_eq!(applied.len(), 4098, "evt_1 re-applied past the window");
    assert_eq!(applied.last().map(String::as_str), Some("evt_1"));

    // The tap-side ring, fed the full-window overlap directly (the
    // naive-bridge hazard: a publisher bridging replay + live
    // straight into the tap): it drops all 4096 on its own.
    let (tx, rx) = tokio::sync::mpsc::channel(8192);
    let bare_tap = BroadcastTap::live(tx);
    for id in &ids {
        bare_tap.publish(TapEvent {
            id: id.clone(),
            typ: "session.updated".to_owned(),
            data: serde_json::json!({}),
            created: None,
        });
    }
    assert_eq!(bare_tap.deduped_count(), 0, "first delivery of the full window");
    for id in &ids {
        bare_tap.publish(TapEvent {
            id: id.clone(),
            typ: "session.updated".to_owned(),
            data: serde_json::json!({}),
            created: None,
        });
    }
    assert_eq!(
        bare_tap.deduped_count(),
        4096,
        "the tap's own ring covered the full-window overlap independently"
    );
    drop(rx);
}

// ---------------------------------------------------------------------------
// S6 — the drain discipline: a snapshot queued mid-stream serializes
// ---------------------------------------------------------------------------

/// The single ingest channel's drain-before-reconcile: live events
/// already queued ahead of a snapshot apply first, the snapshot
/// rebuilds wholesale, and events after it continue on top —
/// `ingest_all` applies the batch in EXACT channel order, so no
/// interleaving of the two surfaces inside one epoch can reorder.
#[test]
fn snapshot_queued_mid_stream_serializes_in_channel_order() {
    let server = ScriptedServer::new(streaming_script());
    let harness = Harness::new();

    // The channel holds d1, then the resync (fetch at t=45: durable
    // state has the EMPTY text part), then d2..step.ended.
    let batch: Vec<Ingest> = vec![
        Ingest::Event(server.frames[2].event()), // d1 (t=30)
        Ingest::Resync {
            sessions: Vec::new(),
            unread: Vec::new(),
            messages: vec![(SESSION.to_owned(), server.fetch_messages_at(45))],
        },
        Ingest::Event(server.frames[3].event()), // d2 (t=40)
        Ingest::Event(server.frames[4].event()), // d3 (t=50)
        Ingest::Event(server.frames[5].event()), // ended (t=60)
        Ingest::Event(server.frames[6].event()), // step.ended (t=70)
    ];
    harness.store.ingest_all(batch);

    // d1's text was discarded by the wholesale rebuild (the S2
    // transient — the durable state never contained it), d2/d3
    // applied on top, and ended healed authoritatively. Nothing
    // applied twice.
    assert_eq!(harness.text_of(MESSAGE), "Hello proven safe");
    harness.assert_parity(&["evt_d1", "evt_d2", "evt_d3", "evt_te", "evt_se"]);
}
