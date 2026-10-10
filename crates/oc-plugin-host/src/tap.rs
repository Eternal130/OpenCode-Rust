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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

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
}

impl BroadcastTap {
    pub(crate) fn live(tx: tokio::sync::mpsc::Sender<TapEvent>) -> Self {
        Self { tx: Some(tx), warned: Arc::new(AtomicBool::new(false)) }
    }

    /// The disabled tap (bun absent — graceful degradation).
    #[must_use]
    pub fn disabled() -> Self {
        Self { tx: None, warned: Arc::new(AtomicBool::new(false)) }
    }

    /// Whether events flow anywhere.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.tx.is_some()
    }

    /// Publish one event. Returns `true` when queued. NEVER blocks:
    /// full queue or closed channel → drop + (once) log.
    /// Publish one event. Returns `true` when queued. NEVER blocks:
    /// full queue or closed channel → drop + (once) log. The return
    /// is advisory — callers on the ingest path ignore it (the drop
    /// is logged), so `must_use` is deliberately suppressed.
    #[allow(clippy::must_use_candidate)]
    pub fn publish(&self, event: TapEvent) -> bool {
        let Some(tx) = &self.tx else { return false };
        if let Ok(()) = tx.try_send(event) {
            true
        } else {
            if !self.warned.swap(true, Ordering::Relaxed) {
                eprintln!("opencode plugin-host: tap queue full or closed; events will be dropped");
            }
            false
        }
    }
}
