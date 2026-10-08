//! Permission-engine event sink adapter — wires
//! [`crate::permission::EventSink`] (and its in-memory
//! [`BroadcastEventSink`]) to the bus.
//!
//! `permission.asked` / `permission.replied` are **ephemeral** events: the
//! adapter publishes them through the normal bus path (routing + unisolated
//! listener notification) without any database write.
//!
//! The [`EventSink`] trait is async (oracle gate 4, finding 2): the adapter
//! **awaits** the bus publish, so the engine's reply/assert call sites
//! resolve their deferreds only after the event is on the bus — v2
//! `permission.ts:270-306` ordering. The definitions come from the
//! centralized non-session registry (`bus::registry`).

use oc_schema::location;
use oc_schema::misc_events::PermissionEvent;

use super::registry;
use super::{Bus, EventDefinition, PublishOptions};
use crate::permission::EventSink;
use async_trait::async_trait;

/// [`EventSink`] publishing permission events into a [`Bus`].
#[derive(Clone)]
pub struct PermissionBusSink {
    bus: Bus,
    location: Option<location::Ref>,
}

impl PermissionBusSink {
    /// Sink for a bus, optionally stamping every event with a location.
    #[must_use]
    pub fn new(bus: Bus, location: Option<location::Ref>) -> Self {
        Self { bus, location }
    }
}

impl std::fmt::Debug for PermissionBusSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionBusSink").finish_non_exhaustive()
    }
}

#[async_trait]
impl EventSink for PermissionBusSink {
    async fn publish(&self, event: PermissionEvent) {
        let (definition, data) = match &event {
            PermissionEvent::Asked(envelope) => {
                (registry::permission_asked(), serde_json::to_value(&envelope.data))
            }
            PermissionEvent::Replied(envelope) => {
                (registry::permission_replied(), serde_json::to_value(&envelope.data))
            }
        };
        let data = match data {
            Ok(data) => data,
            Err(err) => {
                eprintln!("[bus] permission event serialization failed: {err}");
                return;
            }
        };
        let definition: &EventDefinition = definition;
        let options =
            PublishOptions { location: self.location.clone(), ..PublishOptions::default() };
        // Awaited by contract: the engine resolves its deferreds only after
        // this future completes (v2 permission.ts:270-306 ordering).
        if let Err(err) = self.bus.publish(definition, data, options).await {
            eprintln!("[bus] permission event publish failed: {err}");
        }
    }
}

/// Drains a [`BroadcastEventSink`] receiver into a [`Bus`]: every
/// `permission.*` event broadcast by the engine is re-published as an
/// ephemeral bus event. Returns the spawned bridge task.
///
/// This is the zero-change wiring for existing
/// [`crate::permission::Permission`] instances already constructed with a
/// `BroadcastEventSink`: subscribe it here instead of rewriting the engine.
pub fn bridge_permission_sink(
    bus: Bus,
    sink: &crate::permission::BroadcastEventSink,
    location: Option<location::Ref>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = sink.subscribe();
    let adapter = PermissionBusSink::new(bus, location);
    tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            adapter.publish(event).await;
        }
    })
}
