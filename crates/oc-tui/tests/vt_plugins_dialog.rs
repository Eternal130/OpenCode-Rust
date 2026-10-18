//! SA-F6 (SCAN-AB) — the live `/plugins` typed command: typing the
//! slash command and pressing Enter opens the Plugins dialog overlay
//! (the ref's behavior — feature-plugins/system/plugins.tsx:337-347
//! registers the `plugins.list` command with `slash: { name:
//! "plugins" }`; the autocomplete popup filters to `/plugins` and
//! Enter SELECTS it, `onSelect: command.run`).
//!
//! Proven on the real run loop over a PTY: the dialog renders the
//! reference's shape — the "Plugins" title, the "Search" placeholder,
//! the shared "No items available" empty state (ui/dialog-select.tsx
//! :723) and the `ctrl+a show internal` footer hint — and the
//! composer does NOT keep the typed text (select() removes the
//! trigger token, autocomplete.tsx:615-626).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{pty_lock, run_tui, sandbox_env, FakeLlm, LiveHarness, WORDMARK};

#[test]
fn vt_plugins_slash_command_opens_the_live_dialog() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("pluginsdlg");
    let fake = FakeLlm::spawn(vt_harness::standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let result_slot: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = (|| {
            if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            // Type the live slash command (no Enter yet): the
            // autocomplete popup filters to /plugins.
            for ch in "/plugins".chars() {
                driver.write(ch.to_string().as_bytes());
                std::thread::sleep(Duration::from_millis(25));
            }
            std::thread::sleep(Duration::from_millis(300));
            // Enter SELECTS the popup entry — the dialog opens.
            driver.write(b"\r");
            if !driver.wait_for(b"No items available", Duration::from_secs(5)) {
                return Err("the Plugins dialog never opened (no empty state)".to_owned());
            }
            let grid = driver.grid_now();
            let screen: Vec<String> = (0..grid.rows).map(|y| grid.row_text(y)).collect();
            let text = screen.concat();
            if !text.contains("Plugins") {
                return Err(format!("no Plugins title: {screen:?}"));
            }
            if !text.contains("Search") {
                return Err(format!("no Search placeholder: {screen:?}"));
            }
            if !text.contains("ctrl+a show internal") {
                return Err(format!("no footer hint: {screen:?}"));
            }
            // The composer must NOT retain the typed text — the
            // select removed the trigger token before the run.
            if text.contains("/plugins") {
                return Err(format!("the composer kept the typed text: {screen:?}"));
            }
            // esc closes the dialog (the app stays alive for the chord).
            driver.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, 80, 24, Duration::from_secs(60));
    fake.stop();
    // The driver reports after the exit chord; the app may tear down a
    // beat before the slot lands.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match result_slot.lock().expect("slot").take() {
        Some(Ok(())) => Ok(()),
        Some(Err(error)) => Err(error),
        None => Err("the driver thread never reported (deadlock?)".to_owned()),
    };
    if let Err(error) = outcome {
        panic!("driver failed: {error}");
    }
}
