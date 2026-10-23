//! `oc-pty` — the PTY daemon binary (Phase 9, design §2.2): a thin
//! process wrapper (Unix-socket server + framing + auth + registration)
//! hosting the ONE `PersistentPty` state machine from `oc-core` (R1).
//!
//! Layout:
//!
//! - [`args`] — the `daemon --runtime-dir ROOT --name ID
//!   --orphan-grace SECS` spawn surface (§8.2, `daemon.ts:235`);
//! - [`daemon`] — claim/registration/lifecycle (§5.1, §5.3), the frame
//!   server with per-frame token auth (§4.3), every §3 op handler, the
//!   owner-connection `bus_event` channel (§3.1) and the orphan-grace
//!   self-exit predicate (§5.1).

// Crate-design decisions (not noise), each with a reason — the oc-ai
// precedent for pinning port-shape lints at the crate boundary.
#![allow(
    // The op dispatcher mirrors the design §3 message table: one arm
    // per op keeps the wire↔machine mapping reviewable in one place.
    clippy::too_many_lines,
    // Subscribe carries the §3 stream contract's parameter set.
    clippy::too_many_arguments,
    // The dispatch table's futures are sized by its largest response
    // arm (WireResponse payloads live inline); boxing every exchange
    // would obscure the protocol's one-in-flight-per-conn shape.
    clippy::large_futures
)]

pub mod args;
pub mod daemon;

pub use args::{parse_daemon_args, DaemonConfig, DEFAULT_ORPHAN_GRACE_SECS};
