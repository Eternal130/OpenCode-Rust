//! G-478 anchor debt (va20 T8) — the update DIALOG's mounted faces,
//! vt-level. The wire-event rig drive (be1_lane's
//! `installation.update-available`/`installation.updated` forms) is
//! NOT reachable from this lane's write domain: the paired rigs'
//! installation events would need a gateway-side publish hook
//! (oc-server/oc-cli — out of the lane's oc-tui-only tests scope),
//! and the ref TUI's updater channel is its CLI host's in-process
//! `latest` (a boot-time network check that fails closed in the
//! sandbox). Degraded per the lane spec's fallback clause (the
//! v19sub precedent): the drive rides the CLI checker's NOTICE FILE
//! (the port's own client channel — `update-notice.log` under the
//! sandbox's `XDG_STATE_HOME`, watched by the run loop's mtime
//! compare) through the full machine: notice → banner → `/update` →
//! Checking → Available → Installing → Installed. The dialog's copy
//! per state is unit-pinned (`update.rs::dialog_states_carry_
//! actions`, va18's re-pin); this test proves the MOUNTED faces and
//! the state transitions on the real run loop.

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{pty_lock, run_tui, sandbox_env, FakeLlm, HarnessHandle, LiveHarness, WORDMARK};

/// The available-notice line the CLI checker writes
/// (autoupdate.rs `announce_available`).
const AVAILABLE_LINE: &str =
    "opencode-rust: update available: v9.9.9 (current v2.0.24); run 'opencode-rust upgrade'";

/// The installed-notice line (`announce_installed`).
const INSTALLED_LINE: &str = "opencode-rust: update installed (v9.9.9), restart to apply";

/// The sandbox's notice file (XDG_STATE_HOME/opencode-rust/
/// update-notice.log — the roots the TUI's `notice_file_path`
/// resolves against: `resolve_app("opencode-rust")` joins the
/// app-name subdir).
fn notice_file() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("XDG_STATE_HOME").expect("sandbox state root"))
        .join("opencode-rust")
        .join("update-notice.log")
}

/// The full state-machine walk: the notice lands the banner, `/update`
/// opens the dialog (Checking resolves to Available through the notice),
/// Enter installs (Installing), and the installed line flips the face to
/// Installed.
#[test]
fn vt_update_dialog_walks_the_state_machine_faces() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("v20updlg");
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
            // 1. The CLI checker's notice line lands the banner
            // (the run loop's mtime watch → update_notify).
            let dir = notice_file().parent().expect("state dir").to_path_buf();
            std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir state: {e}"))?;
            std::fs::write(notice_file(), format!("{AVAILABLE_LINE}\n"))
                .map_err(|e| format!("write notice: {e}"))?;
            // The banner paints ONE chrome Text unit per glyph (the
            // RE-04 fade mask is per-cell), so the raw stream carries
            // escape sequences between the characters — the needle
            // must be polled on the GRID, not the stream.
            if !wait_for_grid(&driver, "to install v9.9.9", Duration::from_secs(5)) {
                return Err(format!(
                    "the available banner never rendered: {}",
                    screen_text(&driver)
                ));
            }
            let banner = screen_text(&driver);
            if !banner.contains("/update") {
                return Err(format!("the banner's /update prefix: {banner}"));
            }
            // 2. `/update` opens the dialog; the check resolves through
            // the notice (Checking → Available).
            for ch in "/update".chars() {
                driver.write(ch.to_string().as_bytes());
                std::thread::sleep(Duration::from_millis(25));
            }
            // Let the autocomplete popup settle before Enter selects.
            std::thread::sleep(Duration::from_millis(300));
            driver.write(b"\r");
            if !wait_for_grid(&driver, "An update is available", Duration::from_secs(5)) {
                return Err(format!(
                    "the Available face never rendered: {}",
                    screen_text(&driver)
                ));
            }
            let available = screen_text(&driver);
            if !available.contains("Update available") {
                return Err(format!("the Available title: {available}"));
            }
            // 3. Enter confirms the install (the dialog's "Update"
            // button): the Installing face.
            driver.write(b"\r");
            if !wait_for_grid(&driver, "Installing OpenCode 9.9.9", Duration::from_secs(5)) {
                return Err("the Installing face never rendered".to_owned());
            }
            let installing = screen_text(&driver);
            if !installing.contains("Updating OpenCode") {
                return Err(format!("the Installing title: {installing}"));
            }
            // 4. The detached upgrade's installed line (appended to the
            // notice file) flips the face: Installing → Installed.
            std::fs::write(notice_file(), format!("{AVAILABLE_LINE}\n{INSTALLED_LINE}\n"))
                .map_err(|e| format!("write installed notice: {e}"))?;
            if !wait_for_grid(&driver, "Update successful! A restart is required.", Duration::from_secs(5))
            {
                return Err("the Installed face never rendered".to_owned());
            }
            let installed = screen_text(&driver);
            if !installed.contains("Restart") {
                return Err(format!("the Installed confirm button: {installed}"));
            }
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
        std::thread::sleep(Duration::from_millis(50));
    }
    let outcome = result_slot.lock().expect("slot").take();
    assert!(
        matches!(&outcome, Some(Ok(()))),
        "update dialog walk RED: {outcome:?}"
    );
}

/// Poll the settled GRID for a needle (the per-glyph chrome surfaces
/// split their text across escape runs — the raw stream never carries
/// a contiguous phrase).
fn wait_for_grid(driver: &HarnessHandle, needle: &str, limit: Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline {
        if screen_text(driver).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn screen_text(driver: &HarnessHandle) -> String {
    let grid = driver.grid_now();
    (0..grid.rows).map(|y| grid.row_text(y)).collect::<Vec<_>>().join("\n")
}
