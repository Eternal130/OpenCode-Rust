//! SA-F14 (SCAN-AB) — with a user config FILE present, the first
//! exit chord was "sometimes ignored" (observed during canary bring-up
//! with `cli.json` planted, pre-TEARDOWN-HANG runs; double-chord
//! recovered; env-overlay config probes always passed). Needs a
//! dedicated repro per the register.
//!
//! Hypothesis under test: those observations were SECOND boots in one
//! process (the canary's leg B) — the input-reader zombie SA-F13
//! fixed ate the first chord; the config FILE was incidental.
//!
//! Both legs share ONE PTY and ONE sandbox (crossterm's global event
//! source registers fd 0's open-file-description once per process —
//! a per-leg re-attach would test a harness artifact, not the app);
//! the config FILE is planted BETWEEN the boots so leg A boots
//! without it and leg B boots with it (the canary's exact shape).

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{pty_lock, run_tui, sandbox_env, FakeLlm, LiveHarness, WORDMARK};

/// One boot on the shared PTY: settle, ONE chord, assert teardown.
fn boot_and_chord(harness: &LiveHarness, url: &str, tag: &str) {
    eprintln!("[config-chord] leg {tag} starting");
    let mut driver = harness.handle();
    let driver_thread = std::thread::spawn(move || {
        if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        // Settle past the boot churn (the alleged window: the config
        // mtime watch / first reload pass).
        std::thread::sleep(Duration::from_millis(800));
        let from = driver.raw_len();
        vt_harness::exit_chord(&mut driver);
        if !driver.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from, Duration::from_secs(5)) {
            return Err("the FIRST exit chord was ignored".to_owned());
        }
        Ok(())
    });
    run_tui(url, 80, 24, Duration::from_secs(40));
    driver_thread.join().expect("driver thread").expect("driver failed");
    eprintln!("[config-chord] leg {tag} exited on the first chord");
}

/// The alleged class in isolation AND the canary shape, in one
/// process: leg A (no config file) → plant cli.json → leg B (config
/// FILE present) — each boot's FIRST chord must tear the app down.
#[test]
fn first_chord_exits_with_a_config_file_present() {
    let _lock = pty_lock();
    let sandbox = sandbox_env("config-chord");
    let fake = FakeLlm::spawn(vt_harness::standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let _ = &sandbox;

    // Leg A: no config file.
    boot_and_chord(&harness, &fake.url, "A-no-config");

    // Plant the config FILE between the boots (leg B reads it at its
    // own TuiConfig::load). The port resolves its config root as
    // `.config/opencode-rust` (global_roots::resolve_app("opencode-rust"),
    // deliberately distinct from a real v2 install during dogfood) —
    // seeding `.config/opencode` here made leg B vacuous.
    let dir = sandbox.join(".config").join("opencode-rust");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("cli.json"), "{}").expect("cli.json");

    // Leg B: config FILE present, FIRST chord must tear down.
    boot_and_chord(&harness, &fake.url, "B-config-file");
    fake.stop();
}
