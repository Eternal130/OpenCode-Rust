//! Attention — the terminal-bell port of v2's attention surface
//! (`packages/tui/src/attention.ts` + the
//! `feature-plugins/system/notifications.ts` plugin).
//!
//! v2 notifies through two channels: OS notifications (blurred only)
//! and sounds (always). A plain-terminal Rust TUI has neither host
//! hook; the equivalent always-on channel is the **terminal bell**
//! (`\x07`) — visible in every terminal, audible over ssh, and
//! surfaced by multiplexers even for background panes. Sounds and OS
//! notifications are C2-deferred (gate-10 §3 ranking: a missed
//! background `permission.asked` is the costliest daily loss).
//!
//! Which bus events ring (mirroring the notifications plugin):
//!
//! * `permission.asked` → "Permission needs input" (dedup by id),
//! * `form.created` → "Input needs response" (dedup by id),
//! * a background session turning **unread** (`message.updated` in a
//!   non-active session — the store's unread transition).
//!
//! The bell is "always" like v2's sound channel (it fires whether or
//! not the terminal is focused); the **attention flag** is the
//! blurred-only complement: set on the same transitions, consumed
//! when focus returns (`FocusGained`), so the UI can glow a pending
//! ask after the bell was heard and the user re-focuses.

use crate::data::BusEvent;

/// The attention state the store carries: a pending (unacknowledged)
/// attention marker + the bell request the run loop drains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttentionState {
    /// A bell was requested; the run loop writes `\x07` once and
    /// clears the request ([`AttentionState::take_bell`]).
    pub bell: bool,
    /// An attention marker is pending; consumed on focus.
    pub pending: bool,
}

impl AttentionState {
    /// `true` when the event is an attention transition per the
    /// notifications plugin (the ring decision).
    #[must_use]
    pub fn rings_on(event: &BusEvent) -> bool {
        matches!(event.type_.as_str(), "permission.asked" | "form.created")
    }

    /// Request the bell + set the pending flag (one transition).
    pub fn ring(&mut self) {
        self.bell = true;
        self.pending = true;
    }

    /// Take the bell request (the run loop's write-`\x07`-once).
    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell)
    }

    /// Focus returned: consume the pending marker.
    pub fn focused(&mut self) {
        self.pending = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(type_: &str, data: serde_json::Value) -> BusEvent {
        BusEvent::new(type_, data)
    }

    /// `permission.asked` / `form.created` ring; ordinary events do not.
    #[test]
    fn rings_on_permission_and_form_events_only() {
        assert!(AttentionState::rings_on(&event("permission.asked", json!({"id": "p1"}))));
        assert!(AttentionState::rings_on(&event("form.created", json!({"id": "f1"}))));
        assert!(!AttentionState::rings_on(&event("message.updated", json!({}))));
        assert!(!AttentionState::rings_on(&event("permission.replied", json!({}))));
        assert!(!AttentionState::rings_on(&event("server.connected", json!({}))));
    }

    /// The flag lifecycle: ring → `take_bell` once → pending survives
    /// until focus consumes it.
    #[test]
    fn bell_and_pending_flag_lifecycle() {
        let mut attention = AttentionState::default();
        attention.ring();
        assert!(attention.bell && attention.pending);
        assert!(attention.take_bell(), "the bell fires exactly once");
        assert!(!attention.bell);
        assert!(attention.pending, "the marker outlives the bell");
        assert!(!attention.take_bell(), "no double bell");
        attention.focused();
        assert!(!attention.pending, "focus consumes the marker");
    }
}
