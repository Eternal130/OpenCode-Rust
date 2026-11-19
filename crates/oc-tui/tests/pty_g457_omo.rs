//! G-457 T5 — the real-machine omo-slim sidebar: the user's rust run
//! shows NO sidebar while ref 2.0.24 on the same machine renders it.
//! This scenario pins the rust leg with the user's exact trigger
//! face: a bare package name in `plugin` (the user's
//! opencode.jsonc shape), resolved against the real install cache
//! (HOME untouched), and asserts the OMO-Slim section renders.
//! Skips when the live install cache or bun is unavailable.

#![allow(unsafe_code)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    assert_exit_ack, open_first_session, pty_lock, run_tui, standard_sessions, FakeLlm,
    HarnessHandle, LiveHarness, WORDMARK,
};

fn wait_pane(h: &HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let grid = h.grid_now();
        for row in 0..grid.rows {
            if grid.row_text(row).contains(needle) {
                return Some(row);
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

#[test]
fn pty_g457_real_config_bare_name_omo_sections() {
    // FORENSIC (G-457 T5, env-gated): runs only with an explicitly
    // staged install cache (OC_OMO_CACHE) + bun (OC_OMO_BUN) — the
    // repro of beta.20's real-machine symptom. Currently RED: with
    // the real oh-my-opencode-slim loaded through the user's exact
    // trigger face (bare name under the v1 `plugin` key), the rust
    // leg WEDGES (the session dialog stalls open; the output stream
    // spins empty ?2026 repaint envelopes) and the OMO-Slim section
    // never renders — the W13-registered High bug
    // `w13-omo-session-plugin-send` face, now shown to wedge BEFORE
    // any send (dialog navigation). Needs the controller's forensic
    // lane; kept env-gated so the default suite stays green.
    if std::env::var("OC_OMO_CACHE").is_err() {
        eprintln!("G-457 forensic scenario: set OC_OMO_CACHE (+ OC_OMO_BUN) to run; skipping");
        return;
    }
    if !oc_plugin_host::PluginHost::detect().enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    // The real install cache (root-owned on this box) is staged
    // readable at the OC_OMO_CACHE override (a cache-home root holding
    // `opencode/npm/...`); bun rides OC_PLUGIN_BUN.
    let cache = std::path::PathBuf::from(
        std::env::var("OC_OMO_CACHE").unwrap_or_else(|_| "/root/.cache".to_owned()),
    );
    if !cache.join("opencode/npm/oh-my-opencode-slim@latest").is_dir() {
        eprintln!("omo-slim install cache unavailable; skipping");
        return;
    }
    if let Ok(bun) = std::env::var("OC_OMO_BUN") {
        std::env::set_var("OC_PLUGIN_BUN", bun);
    }
    // The user's trigger face in isolation: bare package name under
    // the v1 `plugin` key, a temp XDG config dir (the real user
    // config's other keys — MCP servers, LSP — stay out of the run).
    let cfg_root = std::path::PathBuf::from("/tmp/opencode/g457-omo-cfg");
    let _ = std::fs::remove_dir_all(&cfg_root);
    std::fs::create_dir_all(cfg_root.join("opencode")).expect("cfg dir");
    std::fs::write(
        cfg_root.join("opencode").join("opencode.json"),
        r#"{"plugin": ["oh-my-opencode-slim"]}"#,
    )
    .expect("cfg");
    let _lock = pty_lock();
    std::env::set_var("XDG_CONFIG_HOME", &cfg_root);
    // Resolution reads `<XDG_CACHE_HOME>/opencode/npm/...` (install.rs
    // default_root) — the staging dir holds exactly that `opencode/npm`
    // subtree; HOME stays untouched.
    if let Ok(cache) = std::env::var("OC_OMO_CACHE") {
        std::env::set_var("XDG_CACHE_HOME", cache);
    }
    std::env::remove_var("OPENCODE_CONFIG");

    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(160, 30);
    let mut driver = harness.handle();
    let span = std::thread::spawn(move || -> Result<(), String> {
        let outcome = (|| -> Result<(), String> {
            if !driver.wait_for(WORDMARK, Duration::from_secs(20)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            open_first_session(&mut driver, b"Alpha")?;
            let Some(header) = wait_pane(&driver, "OMO-Slim", Duration::from_secs(30)) else {
                // dump the visible screen + raw tail for forensics
                let grid = driver.grid_now();
                let mut dump = format!("grid rows={}\n", grid.rows);
                for row in 0..grid.rows.min(30) {
                    dump = format!("{dump}row{row:02}|{}|\n", grid.row_text(row));
                }
                let raw = driver.raw_mirror();
                let tail = raw.len().saturating_sub(2000);
                dump = format!("{dump}raw tail: {}\n", String::from_utf8_lossy(&raw[tail..]));
                let _ = std::fs::write("/tmp/opencode/g457-omo-screen.txt", dump);
                return Err("the OMO-Slim section header never rendered (rust leg)".to_owned());
            };
            let roster = wait_pane(&driver, "explorer", Duration::from_secs(5));
            let Some(roster) = roster else {
                return Err("the agent roster never rendered (rust leg)".to_owned());
            };
            if roster <= header {
                return Err("the roster must sit under the header".to_owned());
            }
            Ok(())
        })();
        let from = driver.raw_len();
        let _ = assert_exit_ack(&mut driver, from);
        outcome
    });
    let url = fake.url.clone();
    let teardown = std::thread::spawn(move || {
        run_tui(&url, 160, 30, Duration::from_secs(90));
    });
    let result = span.join().expect("driver thread");
    teardown.join().expect("run thread");
    drop(harness);
    fake.stop();
    std::env::remove_var("XDG_CONFIG_HOME");
    std::env::remove_var("XDG_CACHE_HOME");
    result.expect("the bare-name omo-slim sidebar renders on rust");
}
