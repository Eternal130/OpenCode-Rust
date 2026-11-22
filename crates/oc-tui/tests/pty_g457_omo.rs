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
    // FORENSIC (G-457 T5, env-gated; fixed in G-462): runs only with an
    // explicitly staged install cache (OC_OMO_CACHE) + bun (OC_OMO_BUN)
    // — the repro of beta.20's real-machine symptom. G-462 root-caused
    // the two faces this scenario pinned:
    // * the OMO-Slim section never rendered because the TUI's raw-file
    //   plugin discovery read only the v2 `plugins` key — the user's
    //   bare v1 `plugin: ["oh-my-opencode-slim"]` never booted the
    //   host (fixed in plugin_boot.rs `collect_plugin_ops`, which now
    //   merges the v1 key like the ref's normalize.ts:185-190);
    // * the pre-send WEDGE (dialog stalled open + empty ?2026 repaint
    //   envelope spin) was a HERMETICITY artifact of this very
    //   scenario — it ran against the live developer state/db (no
    //   OPENCODE_TEST_HOME/XDG_STATE_HOME sandbox) and never ran the
    //   v2→rust migration pass the production CLI main performs, so
    //   the staged config/cache never landed where discovery reads
    //   them. Both are now part of the scenario (the same production
    //   boot face), and the run asserts the sections render with the
    //   plugin ACTIVE. Still env-gated so the default suite stays
    //   green.
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
    let cache = std::path::PathBuf::from(std::env::var("OC_OMO_CACHE").unwrap_or_else(|_| {
        std::env::var("XDG_CACHE_HOME")
            .unwrap_or_else(|_| std::env::var("HOME").map(|h| h + "/.cache").unwrap_or_default())
    }));
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
    // G-462: FULL state/home isolation (the same scheme as every other
    // PTY suite's `sandbox_env`) — the original scenario left
    // HOME/XDG_STATE_HOME live, so the run touched the developer's
    // real state/db (the pre-send wedge face).
    let home = std::path::PathBuf::from(format!("/tmp/opencode/g462-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(home.join(".local/state")).expect("state dir");
    // The legacy v2 data dir gates the migration pass
    // (v2_migration.rs: `if !legacy.data.is_dir()` short-circuits) —
    // stage it empty so the copy actually runs like on the user's
    // real machine.
    std::fs::create_dir_all(home.join(".local/share/opencode")).expect("legacy data dir");
    std::env::set_var("OPENCODE_TEST_HOME", &home);
    // uid-agnostic sandbox: the caller's real HOME/data/cache must never
    // leak into the in-process TUI (2026-11-22 root-identity incident).
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
    std::env::set_var("XDG_CONFIG_HOME", &cfg_root);
    std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    std::fs::create_dir_all(home.join(".cache")).expect("cache dir");
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    // Resolution reads `<XDG_CACHE_HOME>/opencode/npm/...` (install.rs
    // default_root) — the staging dir holds exactly that `opencode/npm`
    // subtree; the sandbox cache above stays the fallback.
    if let Ok(cache) = std::env::var("OC_OMO_CACHE") {
        std::env::set_var("XDG_CACHE_HOME", cache);
    }
    // G-462: production boots through the CLI, whose main runs the
    // v2→rust migration (legacy `opencode` roots → `opencode-rust`)
    // BEFORE the TUI — the in-process `run_with_size` harness must run
    // the same pass or the config/cache under test never lands where
    // discovery reads it. Runs AFTER the cache env staging so the
    // legacy npm subtree is among what the pass copies.
    oc_core::v2_migration::ensure_migrated_logged();
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
    std::env::remove_var("XDG_STATE_HOME");
    std::env::remove_var("XDG_DATA_HOME");
    std::env::remove_var("OPENCODE_TEST_HOME");
    std::env::remove_var("HOME");
    result.expect("the bare-name omo-slim sidebar renders on rust");
}
