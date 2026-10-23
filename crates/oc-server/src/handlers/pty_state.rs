//! PTY + persistent-PTY service access.
//!
//! The machines live on [`AppState`] (`state.pty` / `state.persistent_pty`),
//! bus-backed and constructed once per server — the Phase 5 integration
//! moved them off the process singleton so concurrently-booted servers
//! (tests, embedded mounts) keep independent PTY tables while sharing the
//! engine bus wiring with v2's route-builder DI graph.

use std::sync::Arc;

use oc_core::persistent_pty::frontend::Frontend;
use oc_core::pty::PtyService;

use super::common::SharedState;

/// The PTY machines for a server (AppState-backed, bus-wired).
pub struct PtyShared {
    pub pty: PtyService,
    pub persistent: Frontend,
}

/// The server's PTY machines (a borrowed view over the state fields).
#[must_use]
pub fn shared(state: &SharedState) -> Arc<PtyShared> {
    // The machines are internally `Arc`-shared handles; the wrapper exists
    // so the handler call sites keep one accessor.
    Arc::new(PtyShared { pty: state.pty.clone(), persistent: state.persistent_pty.clone() })
}
