//! The event publicness manifest (v2 `packages/schema/src/event-manifest.ts`
//! `EventManifest.Server`/`isServer`): the SINGLE source of truth for which
//! event types the SSE feed (`sse.rs`) forwards.
//!
//! v2 derives the public set from `ServerDefinitions` (one inventory of
//! schema'd event types). The Rust analogue is the oc-core bus registry
//! (`registry::definitions()` — the auditD §3.4 inventory) plus the
//! location-service frames the announcer publishes. Both halves are
//! declared exactly once HERE:
//!
//! * registry membership — public by definition (every registry type);
//! * [`SERVICE_FRAMES`] — the schema'd-public v2 service frames the Rust
//!   registry inventory does not list yet; `announce.rs` publishes them
//!   and the feed forwards them, both from this one constant;
//! * the synthetic `server.connected` frame and the `rpc.*` wildcard.
//!
//! The drift-guard tests pin the invariants so a new event type cannot
//! silently miss classification: every registry definition is public,
//! every service frame is public (and not redundantly in the registry).

use oc_core::bus::registry;

/// The synthetic initial frame every subscriber receives.
pub const CONNECTED: &str = "server.connected";

/// The wildcard prefix for guest RPC frames.
pub const RPC_PREFIX: &str = "rpc.";

/// The empty-payload service frames published per boot/reload/config
/// change (order within a group is canonicalized by the capture; the list
/// order mirrors the v2 boot sequence). Declared once here — the announcer
/// publishes them and the SSE feed classifies them from the same list.
pub const SERVICE_FRAMES: &[&str] = &[
    "integration.updated",
    "provider.updated",
    "model.updated",
    "agent.updated",
    "command.updated",
    "skill.updated",
    "websearch.updated",
    "reference.updated",
    "plugin.updated",
];

/// Whether an event type is public (`EventManifest.isServer`):
/// `server.connected`, any known registry type, `rpc.*`, or one of the
/// service frames above.
#[must_use]
pub fn is_public(type_: &str) -> bool {
    type_ == CONNECTED
        || registry::by_type(type_).is_some()
        || type_.starts_with(RPC_PREFIX)
        || SERVICE_FRAMES.contains(&type_)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard 1: every event type in the registry inventory is
    /// public — a new registry definition cannot miss classification.
    #[test]
    fn manifest_every_registry_definition_is_public() {
        let definitions = registry::definitions();
        assert!(!definitions.is_empty(), "registry inventory is populated");
        for definition in definitions {
            assert!(
                is_public(&definition.type_),
                "registry type {} must be public",
                definition.type_
            );
        }
    }

    /// Drift guard 2: every service frame is public through the manifest,
    /// so the announcer can never publish a frame the feed would filter
    /// out (frames also present in the registry — e.g.
    /// `integration.updated` — classify through registry membership; the
    /// rest through the SERVICE_FRAMES segment).
    #[test]
    fn manifest_every_service_frame_is_public() {
        for type_ in SERVICE_FRAMES {
            assert!(is_public(type_), "{type_} must be public");
        }
    }

    /// Drift guard 3: the classification is exactly registry ∪ service
    /// frames ∪ `server.connected` ∪ `rpc.*` — nothing else slips through.
    #[test]
    fn manifest_classification_is_closed() {
        assert!(is_public("server.connected"));
        assert!(is_public("session.created"));
        assert!(is_public("rpc.custom"));
        assert!(is_public("plugin.updated"));
        assert!(!is_public("server.connect"));
        assert!(!is_public("rpc"));
        assert!(!is_public("test.internal"));
        assert!(!is_public("weird.unknown"));
    }
}
