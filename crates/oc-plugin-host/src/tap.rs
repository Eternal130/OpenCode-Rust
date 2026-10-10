//! The post-ingest event broadcast tap (oracle-gate11 §4 wave 1:
//! "插件必见与 store 相同的去重有序流" — the plugin sees the SAME
//! deduplicated, ordered stream the store reduced).
//!
//! The TUI store publishes every bus event through
//! [`BroadcastTap::publish`] **after** its reducer ran; the tap is a
//! bounded channel into the host's dispatch pump. Publishing is
//! strictly non-blocking (a `try_send`): a full queue or a disabled
//! subsystem drops the event with a log line — a slow plugin can never
//! stall the ingest loop.
//!
//! Wave 4 (the G11-F1 broadcast contract): the tap deduplicates by
//! envelope id — a publisher bridging BOTH a snapshot replay and the
//! live stream (the first-connect window where an event lands in both)
//! cannot double-deliver to plugins. Id-less events (fixture streams)
//! pass through untouched.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// The seen-id ring's capacity (bounded memory; the window it covers
/// comfortably exceeds any snapshot/live overlap).
const DEDUP_CAPACITY: usize = 4096;

/// One broadcast event (the oc-tui `BusEvent` wire shape: `id`,
/// `type`, `data`, `created`).
#[derive(Debug, Clone, PartialEq)]
pub struct TapEvent {
    /// Envelope id (`evt_…`; empty for fixture streams).
    pub id: String,
    /// Event type (`session.text.delta`, `tui.test`, …).
    pub typ: String,
    pub data: Value,
    /// Envelope `created` (epoch ms) when present.
    pub created: Option<u64>,
}

impl TapEvent {
    #[must_use]
    pub fn new(typ: &str, data: Value) -> Self {
        Self { id: String::new(), typ: typ.to_owned(), data, created: None }
    }
}

/// The synchronous publish handle the store keeps. Cloneable; when the
/// host is disabled (bun absent) the `tx` is `None` and every publish
/// is a logged no-op.
#[derive(Debug, Clone)]
pub struct BroadcastTap {
    tx: Option<tokio::sync::mpsc::Sender<TapEvent>>,
    /// `true` once a publish was dropped (disables further per-event
    /// logging — one line per run, not one per event).
    warned: Arc<AtomicBool>,
    /// The G11-F1 dedup ring: envelope ids already delivered (shared by
    /// every clone — one tap, one stream).
    seen: Arc<Mutex<SeenRing>>,
    /// Diagnostics: events dropped as duplicates.
    deduped: Arc<AtomicU64>,
}

/// Bounded FIFO of delivered envelope ids.
#[derive(Debug)]
struct SeenRing {
    order: VecDeque<String>,
    set: std::collections::HashSet<String>,
}

impl SeenRing {
    fn new() -> Self {
        Self {
            order: VecDeque::with_capacity(DEDUP_CAPACITY),
            set: std::collections::HashSet::new(),
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    /// Record a delivered id, evicting the oldest past capacity.
    fn remember(&mut self, id: &str) {
        if self.set.contains(id) {
            return;
        }
        if self.order.len() == DEDUP_CAPACITY {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        self.order.push_back(id.to_owned());
        self.set.insert(id.to_owned());
    }
}

impl BroadcastTap {
    pub(crate) fn live(tx: tokio::sync::mpsc::Sender<TapEvent>) -> Self {
        Self {
            tx: Some(tx),
            warned: Arc::new(AtomicBool::new(false)),
            seen: Arc::new(Mutex::new(SeenRing::new())),
            deduped: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The disabled tap (bun absent — graceful degradation).
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            tx: None,
            warned: Arc::new(AtomicBool::new(false)),
            seen: Arc::new(Mutex::new(SeenRing::new())),
            deduped: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Whether events flow anywhere.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.tx.is_some()
    }

    /// Events dropped as duplicates so far (the G11-F1 contract's
    /// observability).
    #[must_use]
    pub fn deduped_count(&self) -> u64 {
        self.deduped.load(Ordering::Relaxed)
    }

    /// Publish one event. Returns `true` when queued. NEVER blocks:
    /// full queue or closed channel → drop + (once) log. An envelope id
    /// already delivered is dropped as a duplicate (first-wins — the
    /// replay/live overlap must not reach plugins twice). The return
    /// is advisory — callers on the ingest path ignore it (the drop
    /// is logged), so `must_use` is deliberately suppressed.
    #[allow(clippy::must_use_candidate)]
    pub fn publish(&self, event: TapEvent) -> bool {
        let Some(tx) = &self.tx else { return false };
        if event.id.is_empty() {
            return if let Ok(()) = tx.try_send(event) {
                true
            } else {
                self.warn_full();
                false
            };
        }
        // Check-then-admit under one lock, with the send INSIDE the
        // critical section (try_send is non-blocking): a queue-full
        // drop does not burn the id, so a retry can still deliver.
        let id = event.id.clone();
        let mut seen = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.contains(&id) {
            self.deduped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        if let Ok(()) = tx.try_send(event) {
            seen.remember(&id);
            true
        } else {
            self.warn_full();
            false
        }
    }

    fn warn_full(&self) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            eprintln!("opencode plugin-host: tap queue full or closed; events will be dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap() -> (BroadcastTap, tokio::sync::mpsc::Receiver<TapEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(DEDUP_CAPACITY + 64);
        (BroadcastTap::live(tx), rx)
    }

    #[test]
    fn duplicate_envelope_ids_deliver_once() {
        let (tap, mut rx) = tap();
        let event = TapEvent {
            id: "evt_1".into(),
            typ: "session.text.delta".into(),
            data: Value::Null,
            created: None,
        };
        assert!(tap.publish(event.clone()));
        assert!(!tap.publish(event.clone()), "the G11-F1 double-append is dropped");
        assert_eq!(tap.deduped_count(), 1);
        assert!(tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { rx.recv().await.is_some() }));
    }

    #[test]
    fn idless_fixture_events_pass_through() {
        let (tap, _rx) = tap();
        assert!(tap.publish(TapEvent::new("tui.test", Value::Null)));
        assert!(tap.publish(TapEvent::new("tui.test", Value::Null)));
        assert_eq!(tap.deduped_count(), 0);
    }

    #[test]
    fn the_ring_evicts_without_false_positives_forever() {
        let (tap, _rx) = tap();
        for n in 0..(DEDUP_CAPACITY as u64 + 16) {
            tap.publish(TapEvent {
                id: format!("evt_{n}"),
                typ: "t".into(),
                data: Value::Null,
                created: None,
            });
        }
        // evt_0 evicted; re-publishing it is a fresh delivery.
        assert!(tap.publish(TapEvent {
            id: "evt_0".into(),
            typ: "t".into(),
            data: Value::Null,
            created: None
        }));
    }

    #[test]
    fn the_disabled_tap_dedups_nothing_and_is_silent() {
        let tap = BroadcastTap::disabled();
        assert!(!tap.is_live());
        assert!(!tap.publish(TapEvent::new("x", Value::Null)));
        assert_eq!(tap.deduped_count(), 0);
    }
}
