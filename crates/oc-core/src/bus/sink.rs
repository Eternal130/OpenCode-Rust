//! Permission-engine event sink adapter — wires
//! [`crate::permission::EventSink`] (and its in-memory
//! [`BroadcastEventSink`]) to the bus without touching `permission.rs`.
//!
//! `permission.asked` / `permission.replied` are **ephemeral** events: the
//! adapter publishes them through the normal bus path (routing + unisolated
//! listener notification) without any database write. Because
//! [`crate::permission::EventSink::publish`] is synchronous, the async
//! publish is spawned onto the current runtime; outside a runtime the event
//! is logged and dropped (the permission engine never blocks on delivery).

use oc_schema::location;
use oc_schema::misc_events::PermissionEvent;

use super::{Bus, EventDefinition, PublishOptions};
use crate::permission::EventSink;

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

impl EventSink for PermissionBusSink {
    fn publish(&self, event: PermissionEvent) {
        let (type_, data) = match &event {
            PermissionEvent::Asked(envelope) => {
                ("permission.asked", serde_json::to_value(&envelope.data))
            }
            PermissionEvent::Replied(envelope) => {
                ("permission.replied", serde_json::to_value(&envelope.data))
            }
        };
        let data = match data {
            Ok(data) => data,
            Err(err) => {
                eprintln!("[bus] permission event serialization failed: {err}");
                return;
            }
        };
        let definition = EventDefinition::ephemeral(type_);
        let options =
            PublishOptions { location: self.location.clone(), ..PublishOptions::default() };
        let bus = self.bus.clone();
        let spawned = tokio::runtime::Handle::try_current().map(|handle| {
            handle.spawn(async move {
                if let Err(err) = bus.publish(&definition, data, options).await {
                    eprintln!("[bus] permission event publish failed: {err}");
                }
            });
        });
        if let Err(err) = spawned {
            eprintln!("[bus] permission event dropped (no runtime): {type_} ({err})");
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
            adapter.publish(event);
        }
    })
}
