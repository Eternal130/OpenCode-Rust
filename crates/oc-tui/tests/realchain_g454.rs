//! G-454 regression lane (fixed; forensics in
//! `.superpowers/sdd/r1/g454-report.md` + W13's finding):
//!
//! With real plugins staged, the RUST engine's main turn never reached
//! the provider (W13: rust leg = title call only, forever-spinning
//! turn). Root cause (two cooperating defects):
//!
//! 1. `superpowers@7.0.0`'s `session.context` hook validates the
//!    session record's `parentID`; the port serialized an unset parent
//!    as an explicit `null` (the ref's effect schema OMITS the field),
//!    so every lookup threw and the plugin logged a `console.error`.
//! 2. That error-level guest log hit the host's `log_guest`
//!    `eprintln!` — and the detached daemon's stderr pipe has no
//!    reader once the spawning `service start` process exits, so the
//!    panicking stderr write KILLED the guest-stdout reader task,
//!    freezing the whole plugin RPC bridge: the hook.dispatch answer
//!    never arrived, the 120 s mutating timeout fired, and the main
//!    turn aborted before any HTTP.
//!
//! The regression shape: send one message with plugins staged (both,
//! omo-slim only, superpowers only, and writable-staged payloads) and
//! ASSERT the main turn reaches the gateway (requests ≥ 2: title +
//! main). The reply body itself is replay-corpus business (a strict
//! miss is fine — arrival is the contract here).

#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    default_pace, dump_grid, lane_lock, plugin_lane_skip_reason, sandbox, service_start,
    service_stop, stage_plugins_g454, worktree_dir, Gateway, TuiProcess, WORDMARK,
};
use std::time::{Duration, Instant};

const ROWS: u16 = 36;

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// One discriminator/regression run: boot with the staged plugin set,
/// send `hello`, and return (replied, gateway requests, specs).
fn run(tag: &str, only: Option<&str>, writable: bool) -> (bool, u64, Vec<String>) {
    let logs = worktree_dir(".test-logs");
    let gateway = Gateway::spawn_paced(&format!("g454-{tag}"), None, &default_pace());
    let rust = sandbox(&format!("g454-{tag}"), gateway.port);
    let specs = stage_plugins_g454(&rust, false, only, writable);
    let spec_refs: Vec<&str> = specs.iter().map(String::as_str).collect();
    rust.write_config_ext(&[], "replay/claude-opus-4-7", &spec_refs);
    let server = service_start(&rust);
    let mut rust_tui = TuiProcess::spawn(&rust, 96, ROWS);
    let booted = rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    assert!(booted, "G454 {tag}: rust TUI never booted");
    // Plugin load is async — settle past the wordmark.
    settle(8);
    let home = rust_tui.screen_text();
    dump_grid(&format!("g454-{tag}-home"), &home);
    let from = rust_tui.raw_len();
    let sent_at = Instant::now();
    rust_tui.write(b"hello\r");
    let replied = rust_tui.wait_for(b"REALCHAIN-REPLAY-OK", from, Duration::from_secs(60));
    settle(4);
    let session = rust_tui.screen_text();
    dump_grid(&format!("g454-{tag}-session"), &session);
    let stats = format!("{}", gateway.stats());
    let requests = serde_json::from_str::<serde_json::Value>(&stats)
        .ok()
        .and_then(|value| value.get("requests").and_then(serde_json::Value::as_u64))
        .unwrap_or_default();
    let _ = server.url;
    rust_tui.shutdown(Duration::from_secs(10));
    service_stop(&rust);
    drop(gateway);
    let _ = std::fs::write(
        logs.join(format!("g454-{tag}-verdict.txt")),
        format!(
            "replied={replied}\nspecs={specs:?}\nrequests={requests}\nelapsed={:?}\nstats={stats}",
            sent_at.elapsed()
        ),
    );
    (replied, requests, specs)
}

macro_rules! reach_case {
    ($name:ident, $tag:expr, $only:expr, $writable:expr) => {
        #[test]
        fn $name() {
            if let Some(reason) = plugin_lane_skip_reason() {
                eprintln!("G454 SKIP: {reason}");
                return;
            }
            let _lane = lane_lock();
            let (replied, requests, specs) = run($tag, $only, $writable);
            // G-454's contract: the MAIN turn reaches the provider
            // (title + main ≥ 2 requests). `replied` additionally
            // requires a corpus hit for the plugin-shaped request —
            // exported, never asserted (a strict miss still proves
            // arrival, which is what the send-block denied).
            assert!(
                requests >= 2,
                "G454 RED ({}): main turn never reached the gateway with {specs:?} \
                 (requests={requests}, replied={replied})",
                $tag
            );
        }
    };
}

// The W13 shape (both plugins, read-only payload) — the original block.
reach_case!(g454_both_plugins_main_turn_reaches_gateway, "reg-both", None, false);
// The user-machine shape (omo-slim alone).
reach_case!(
    g454_single_omo_main_turn_reaches_gateway,
    "reg-omo",
    Some("oh-my-opencode-slim"),
    false
);
// The minimal repro shape (superpowers alone — the hook that wedged).
reach_case!(
    g454_single_superpowers_main_turn_reaches_gateway,
    "reg-super",
    Some("superpowers"),
    false
);
// Writable-staged payloads (the read-only-artifact confounder, now a
// plain variant of the same reach contract).
reach_case!(g454_writable_both_main_turn_reaches_gateway, "reg-writable", None, true);
