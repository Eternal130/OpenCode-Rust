//! SA-F13 (SCAN-AB) — a second `run_with_size` in ONE process must be
//! fully live: leg-2's exit chord must tear the app down and the
//! run-loop future must RETURN (the registered failure: leg-2 rendered
//! home but ignored the chord, emitted teardown bytes only sometimes,
//! and the future never returned after `?1049l`).
//!
//! Root cause (the fix under test): the input reader thread parked in
//! `crossterm::event::read()` holds crossterm's GLOBAL reader lock and
//! only exits when a key arrives AFTER its receiver dropped — a zombie
//! from boot 1 eats boot 2's first key(s). The reader now polls in
//! bounded windows against a stop flag fired by a Drop guard (normal
//! teardown AND outer cancellation).
//!
//! Both boots share ONE PTY (the scan repro's shape — crossterm's
//! global event source registers fd 0's open-file-description ONCE
//! per process, so a per-leg re-attach would leave leg 2's reader
//! watching a dead description; a real embedder's second boot keeps
//! the same terminal).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{pty_lock, run_tui, sandbox_env, FakeLlm, LiveHarness, WORDMARK};

/// The SA-F13 acceptance: two full boots in one process — the second
/// boot's input path and exit chord must be as live as the first's,
/// and both run-loop futures return.
#[test]
fn second_run_with_size_in_one_process_exits_cleanly() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("double-boot");
    let fake = FakeLlm::spawn(vt_harness::standard_sessions());
    let harness = LiveHarness::attach(80, 24);

    for leg in 1..=2 {
        eprintln!("[double-boot] leg {leg} starting");
        let mut driver = harness.handle();
        let (slot_tx, slot_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let driver_thread = std::thread::spawn(move || {
            let outcome = (|| {
                if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
                    return Err("the home wordmark never rendered".to_owned());
                }
                std::thread::sleep(Duration::from_millis(400));
                // A keystroke exchange proves the input path is live
                // BEFORE the exit (the zombie reader ate exactly this
                // class of key: the first key(s) after a boot).
                driver.write(b"hi");
                if !driver.wait_for(b"hi", Duration::from_secs(3)) {
                    return Err("the composer never echoed a typed key — input is dead".to_owned());
                }
                // Empty the draft again (ctrl+c exits only on an EMPTY
                // draft — B1's end-behavior table), then the chord.
                driver.write(b"\x7f\x7f");
                std::thread::sleep(Duration::from_millis(250));
                let from = driver.raw_len();
                vt_harness::exit_chord(&mut driver);
                if !driver.wait_for_after(
                    vt_harness::ALT_SCREEN_LEAVE,
                    from,
                    Duration::from_secs(5),
                ) {
                    return Err("the exit chord did not tear the app down".to_owned());
                }
                Ok(())
            })();
            // The driver always leaves: one more chord for a loop that
            // lagged past the first (the assertions above already
            // judged it).
            let _ = slot_tx.send(outcome);
            driver
        });
        // The loop must return on its own (run_tui asserts the bound).
        run_tui(&fake.url, 80, 24, Duration::from_secs(40));
        eprintln!("[double-boot] leg {leg} run loop RETURNED");
        let mut driver = driver_thread.join().expect("driver thread");
        slot_rx.recv().expect("driver outcome").expect("driver failed");
        let _ = &mut driver;
        // A beat between boots (teardown bytes flush; the next boot's
        // alt-screen enter must be observable after the last leave).
        std::thread::sleep(Duration::from_millis(300));
    }
    fake.stop();
}
