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

/// Event types that exist in the Rust registry inventory but are NOT part
/// of the public wire surface — REF `packages/schema/src/event-manifest.ts`
/// `ServerDefinitions` (event-manifest.ts:71-81) includes only
/// `McpEvent.StatusChanged` and `McpEvent.ResourcesChanged` from the MCP
/// module, and `packages/protocol/test/event.test.ts:28` pins
/// `isOpenCodeEvent({ type: "mcp.tools.changed" }) === false`.
///
/// * `mcp.tools.changed` — private in REF (consumed only by in-process
///   listeners; the Rust tool-bridge debounce subscribes the same way,
///   `oc-core/src/mcp/registry.rs`).
/// * `mcp.prompts.changed` — does not exist in REF at all. The Rust port
///   publishes it locally (oc-core `mcp/registry.rs` prompts watch) for
///   in-process consumers; documented divergence, kept private so it
///   never reaches the public stream.
pub const PRIVATE_TYPES: &[&str] = &["mcp.tools.changed", "mcp.prompts.changed"];

/// Whether an event type is public (`EventManifest.isServer`):
/// `server.connected`, any known registry type, `rpc.*`, or one of the
/// service frames above — minus the REF-private exceptions above.
#[must_use]
pub fn is_public(type_: &str) -> bool {
    if PRIVATE_TYPES.contains(&type_) {
        return false;
    }
    type_ == CONNECTED
        || registry::by_type(type_).is_some()
        || type_.starts_with(RPC_PREFIX)
        || SERVICE_FRAMES.contains(&type_)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard 1: every event type in the registry inventory is
    /// public — a new registry definition cannot miss classification —
    /// except the REF-private exceptions in [`PRIVATE_TYPES`], which must
    /// all be real registry members (documented divergences, not typos).
    #[test]
    fn manifest_every_registry_definition_is_public() {
        let mut definitions: Vec<&registry::EventDefinition> =
            registry::definitions().iter().collect();
        definitions.extend(registry::non_session_definitions().iter());
        assert!(!definitions.is_empty(), "registry inventory is populated");
        for definition in definitions {
            if PRIVATE_TYPES.contains(&definition.type_.as_str()) {
                continue;
            }
            assert!(
                is_public(&definition.type_),
                "registry type {} must be public",
                definition.type_
            );
        }
        for type_ in PRIVATE_TYPES {
            assert!(
                registry::by_type(type_).is_some(),
                "private type {type_} must still be a registry member"
            );
            assert!(!is_public(type_), "private type {type_} must not be public");
        }
    }

    /// Drift guard 1b: the MCP public set is exactly REF's —
    /// `ServerDefinitions` (event-manifest.ts:71-81) carries only
    /// `McpEvent.StatusChanged` + `McpEvent.ResourcesChanged`, and
    /// `protocol/test/event.test.ts:28` pins `mcp.tools.changed` private.
    /// `mcp.prompts.changed` does not exist in REF (documented Rust-side
    /// divergence, kept private).
    #[test]
    fn manifest_mcp_publicness_matches_ref() {
        assert!(is_public("mcp.status.changed"));
        assert!(is_public("mcp.resources.changed"));
        assert!(!is_public("mcp.tools.changed"));
        assert!(!is_public("mcp.prompts.changed"));
    }

    /// Drift guard 4 (F3, announce side ⊆ manifest): every event type the
    /// announcer can publish inline must be classified public by this
    /// manifest, so a future `publish_empty`/typed publish of an
    /// unclassified type is caught here instead of silently being dropped
    /// by the SSE feed.
    #[test]
    fn manifest_announce_publish_side_is_classified() {
        for type_ in crate::announce::inline_published_types() {
            assert!(is_public(type_), "announcer publishes {type_}, which must be public");
        }
    }

    /// Drift guard 2: every service frame is public through the manifest,
    /// so the announcer can never publish a frame the feed would filter
    /// out (frames also present in the registry — e.g.
    /// `integration.updated` — classify through registry membership; the
    /// rest through the `SERVICE_FRAMES` segment).
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
        assert!(!is_public("mcp.tools.changed"));
        assert!(!is_public("mcp.prompts.changed"));
    }

    /// RE-06 drift guard (consumer mirror): the client-side typed event
    /// classification (`oc_client::bus_event` — what oc-tui dispatches
    /// on) stays in lockstep with this manifest. A type added to the
    /// registry or the service frames without a client-side
    /// `EventKind` variant fails here (and vice versa: a stale client
    /// kind whose classification source disappeared fails too).
    #[test]
    fn manifest_client_event_kinds_match_public_surface() {
        // The public surface this manifest admits (minus the `rpc.*`
        // wildcard, which the client models as one `EventKind::Rpc`).
        let mut public: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        public.insert(CONNECTED.to_owned());
        for definition in registry::definitions() {
            public.insert(definition.type_.clone());
        }
        for definition in registry::non_session_definitions() {
            public.insert(definition.type_.clone());
        }
        for type_ in SERVICE_FRAMES {
            public.insert((*type_).to_owned());
        }

        // Wire types that predate the registry inventory and stay on
        // the feed through documented channels: the v1 legacy event
        // surface (schema `v1/session.ts`) and the manifest-private
        // registry members. These are client-known without being
        // classified public here.
        let legacy: std::collections::BTreeSet<&str> = [
            "message.part.delta",
            "message.part.removed",
            "message.part.updated",
            "message.removed",
            "message.updated",
            "session.diff",
            "session.error",
            "session.updated",
            "log.synced",
        ]
        .into_iter()
        .chain([
            // Schema-defined public types the Rust registry inventory
            // does not list yet (oc-schema unions: `ServerEvent`,
            // `SessionCompactionEvent`, `LocationEvent`-family,
            // `LspEvent`, `WorkspaceEvent`, `WorktreeEvent`) — the
            // manifest classifies them only once a publisher registers
            // them; the client knows them because oc-schema defines
            // their wire shape.
            "global.disposed",
            "session.compacted",
            "lsp.updated",
            "workspace.failed",
            "workspace.ready",
            "workspace.status",
            "worktree.failed",
            "worktree.ready",
        ])
        .collect();

        let client_known: std::collections::BTreeSet<&str> =
            oc_client::bus_event::KNOWN_EVENT_TYPES.iter().copied().collect();

        // Forward: every public type is client-known (a new server
        // event type cannot silently fall back to `EventKind::Unknown`
        // in consumers).
        for type_ in &public {
            assert!(
                client_known.contains(type_.as_str()),
                "public event type {type_} has no oc_client EventKind variant — \
                 update oc-client/src/bus_event.rs"
            );
        }
        // Reverse: every client-known type is public, a documented
        // legacy wire type, or a manifest-private member.
        for type_ in &client_known {
            let classified = public.contains(*type_) || legacy.contains(type_);
            assert!(
                classified,
                "oc_client EventKind knows {type_}, but no manifest source \
                 (registry/service frame/{CONNECTED}) or legacy entry covers it — \
                 stale client kind or missing manifest entry"
            );
        }
    }
}
