//! G-548's pty-level regression (wave va21h): a NEVER-CONNECTED app —
//! a stub whose `/api/event` SSE omits the `server.connected` boot
//! frame (the slowG reproducer shape,
//! [`vt_harness::FakeLlm::omit_server_connected_boot_frame`]) — must
//! stay in its never-connected presentation after the sessions-list
//! dialog refresh. The dialog's `session.list` refetch lands through
//! the loop fetcher's `LoopFetch::SessionList(Ok(..))` arm
//! (app.rs — the arm ingests `Ingest::Resync` DIRECTLY, bypassing the
//! data loop's subscribe-gated `resync_after_subscribe`); pre-fix the
//! Resync arm flipped the store `Connected` unconditionally, latching
//! `connection_ever_connected` and defeating the W6 never-connected
//! overlay gate (store.rs) — the very next reconnect-loop failure
//! observation then surfaced the "Connection lost…" scrim over the
//! dialog. Post-fix a Resync never touches the connection status
//! (ref: the status is driven by the `server.connected` SSE event
//! alone, server-sync/connection.ts:14-20 — a session-list fetch
//! cannot touch it).
//!
//! Choreography: boot against the frame-less stub (the reconnect loop
//! churns — Connecting forever), open the Sessions dialog
//! (`ctrl+x l`, the `session.list` keybind), wait for the fixture row
//! (proof the dialog refetch actually ingested a Resync), settle past
//! the G-300 connect window (2 s) + the reconnect cadence, then
//! assert: NO "Connection lost" / reconnecting overlay EVER painted
//! (the raw mirror carries every frame the app ever emitted), and the
//! app is still alive in its normal presentation.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::similar_names)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    exit_chord, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

/// The reconnecting overlay's headline (overlays.rs —
/// "Connection lost…" / "Reconnecting to the server automatically.",
/// reconnecting.tsx:8-39).
const LOST_OVERLAY_NEEDLE: &str = "Connection lost";

/// Past the G-300 connect window (2 s) + the reconnect cadence (1 s
/// delay per attempt): long enough that a pre-fix latch (flip to
/// Connected → the next `Reconnecting{n}` observation arms the 1 s
/// scrim threshold) would certainly have painted.
const SETTLE: Duration = Duration::from_secs(6);

#[test]
fn never_connected_sessions_dialog_stays_connecting() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("g548-nc");
    let fake = FakeLlm::spawn(standard_sessions());
    // G-548's rig switch: the SSE never writes the `server.connected`
    // boot frame — the client's G-300 first-frame check keeps the app
    // never-connected for the whole session.
    fake.omit_server_connected_boot_frame();
    let harness = LiveHarness::attach(120, 30);
    let result_slot: std::sync::Arc<std::sync::Mutex<Option<Result<(), String>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&result_slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = (|| {
            // Boot: the home wordmark renders while never connected
            // (the presentation is data-independent; only the wire
            // data rides the never-arriving resync).
            if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered (never-connected boot)".to_owned());
            }
            // Pre-dialog sanity: never-connected alone must NOT surface
            // the overlay (the W6 gate — this held pre-fix too).
            if raw_contains(&driver, LOST_OVERLAY_NEEDLE) {
                return Err(
                    "the overlay painted BEFORE the sessions dialog (W6 gate broken)".to_owned()
                );
            }
            // Open the Sessions dialog (`ctrl+x l`): the mount sync
            // dispatches `session.list` through the loop fetcher —
            // the G-548 path (a Resync ingested while never
            // connected).
            let from = driver.raw_len();
            driver.write(b"\x18l");
            let dialog_listed = driver.wait_for_after(b"Alpha", from, Duration::from_secs(5));
            if !dialog_listed {
                return Err(
                    "the sessions dialog never listed the fixture row (the refetch never landed)"
                        .to_owned(),
                );
            }
            // Settle past the G-300 window: the reconnect loop keeps
            // cycling (each attempt parks 2 s on the frame-less
            // stream, then re-observes Reconnecting{n}).
            std::thread::sleep(SETTLE);
            // THE assertion — the overlay never painted at ANY point
            // (the raw mirror carries every emitted frame).
            if raw_contains(&driver, LOST_OVERLAY_NEEDLE) {
                return Err(
                    "G-548 REGRESSION: the never-connected app surfaced the Connection lost \
                     overlay after the sessions-list refresh (the Resync flipped the connection \
                     state / latched the W6 gate)"
                        .to_owned(),
                );
            }
            // Alive + still its normal presentation: the dialog rows
            // are still on the grid (the app kept rendering).
            let grid = driver.grid_now();
            let listed = (0..grid.rows).any(|row| {
                grid.row_text(row).contains("Alpha") || grid.row_text(row).contains("Sessions")
            });
            if !listed {
                return Err("the sessions dialog vanished after the settle (app dead?)".to_owned());
            }
            Ok(())
        })();
        exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, 120, 30, Duration::from_secs(90));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    match result_slot.lock().expect("slot").take() {
        Some(Ok(())) => {}
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!("the driver thread never reported (deadlock?); raw TAIL: {tail:?}");
        }
    }
    let _ = std::fs::remove_dir_all(sandbox);
}

/// Whether the app's raw output stream ever contained `needle` (the
/// mirror accumulates every frame — a transient overlay leaves its
/// bytes behind).
fn raw_contains(h: &vt_harness::HarnessHandle, needle: &str) -> bool {
    let raw = h.raw_mirror();
    String::from_utf8_lossy(&raw).contains(needle)
}
