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
//!
//! Timing contract (2026-11-27 slowC dwell audit — every wait is
//! event-gated, no fixed padding): the plugin-load gate waits on the
//! daemon's `/api/plugin` table (the load replies carry the hook
//! registrations the discriminator needs — an early send would carry
//! the plugin-less request shape and false-hit the plain corpus); the
//! reply wait is gated by gateway ARRIVAL (a served strict miss can
//! never render the reply needle, so its window ends at the observed
//! miss — the RED shape, where the turn never arrives, keeps the full
//! 60 s window); the failed turn's modal error toast is dismissed
//! through its own escape key before teardown so the exit chord lands.

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
    default_pace, dump_grid, http_get, lane_lock, plugin_lane_skip_reason, sandbox, service_start,
    service_stop, stage_plugins_g454, worktree_dir, Gateway, TuiProcess, WORDMARK,
};
use std::time::{Duration, Instant};

const ROWS: u16 = 36;

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// Wait until the daemon's plugin table (`GET /api/plugin`) reports
/// every staged spec LOADED: pre-load, an entry's `id` falls back to
/// the versioned config target (`pkg@ver`); once the guest's `load`
/// reply lands, it becomes the bare plugin id the guest reported
/// (supervisor.rs `load_entry` — the reply that also carries the hook
/// registrations). Replaces the old fixed 8 s settle: the wait fires
/// in one poll on a healthy boot (the daemon starts loading plugins
/// ~2 s before the TUI's wordmark) and can never fire EARLY — a
/// slower boot polls until the loads actually land, bounded by `cap`
/// (the old padding as the worst case). `false` = the cap elapsed.
fn wait_plugins_loaded(
    server: &realchain_harness::ManagedServer,
    specs: &[String],
    cap: Duration,
) -> bool {
    let bare: Vec<String> = specs
        .iter()
        .map(|spec| spec.rsplit_once('@').map_or(spec.clone(), |(pkg, _)| pkg.to_owned()))
        .collect();
    let deadline = Instant::now() + cap;
    while Instant::now() < deadline {
        let (status, body) =
            http_get(&format!("{}/api/plugin", server.url), server.password.as_deref())
                .unwrap_or((0, String::new()));
        let loaded = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value.get("data").and_then(|data| data.as_array()).map(|rows| {
                    rows.iter()
                        .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
                        .map(str::to_owned)
                        .collect::<Vec<String>>()
                })
            })
            .is_some_and(|ids| bare.iter().all(|want| ids.contains(want)));
        if status == 200 && loaded {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

/// The reply wait, gated by ARRIVAL: poll the needle, but stop once
/// the gateway has SERVED the main turn a strict miss (requests ≥ 2
/// with misses ≥ 1 — the gateway counts `requests` at serve time as
/// hits + misses). A missed turn gets a 404 and can never render
/// `REALCHAIN-REPLAY-OK`, so the rest of the window is pure dwell;
/// the G-454 RED shape (the turn never ARRIVES: requests stays below
/// 2) keeps the full window. The 2 s grace after the observed miss
/// covers the title-missed/main-hit pairing (the reply would still
/// stream and land inside the grace) — in this rig the title call
/// rides the shared corpus and always hits.
fn wait_reply_or_missed(
    tui: &TuiProcess,
    gateway: &Gateway,
    from: usize,
    window: Duration,
) -> bool {
    const NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";
    let deadline = Instant::now() + window;
    loop {
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            return false;
        }
        if tui.wait_for(NEEDLE, from, remain.min(Duration::from_millis(250))) {
            return true;
        }
        let stats = gateway.stats();
        let requests =
            stats.get("requests").and_then(serde_json::Value::as_u64).unwrap_or_default();
        let misses = stats.get("misses").and_then(serde_json::Value::as_u64).unwrap_or_default();
        if requests >= 2 && misses >= 1 {
            let grace =
                deadline.saturating_duration_since(Instant::now()).min(Duration::from_secs(2));
            return tui.wait_for(NEEDLE, from, grace);
        }
    }
}

/// One discriminator/regression run: boot with the staged plugin set,
/// send `hello`, and return (replied, gateway requests, specs).
fn run(tag: &str, only: Option<&str>, writable: bool) -> (bool, u64, Vec<String>) {
    let logs = worktree_dir(".test-logs");
    let gateway = Gateway::spawn_paced(&format!("g454-{tag}"), None, &default_pace());
    let rust = sandbox(&format!("g454-{tag}"), gateway.port);
    let specs = stage_plugins_g454(&rust, false, only, writable);
    // A PLUGIN_SPECS rename (or a filtered-out registry name) would
    // stage NOTHING and still boot plugin-less — an empty-staging
    // hollow green. The single-select cases must stage exactly one.
    if only.is_some() {
        assert_eq!(specs.len(), 1, "G454 {tag}: single-select staging staged {specs:?}");
    } else {
        assert!(!specs.is_empty(), "G454 {tag}: staging staged no plugins");
    }
    let spec_refs: Vec<&str> = specs.iter().map(String::as_str).collect();
    rust.write_config_ext(&[], "replay/claude-opus-4-7", &spec_refs);
    let server = service_start(&rust);
    let mut rust_tui = TuiProcess::spawn(&rust, 96, ROWS);
    let booted = rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    assert!(booted, "G454 {tag}: rust TUI never booted");
    // Plugin load is async — the load replies (and their hook
    // registrations) must land before the send, or the main turn
    // would carry the plugin-less request shape and false-hit the
    // plain corpus. Wait on the daemon's plugin table (bounded by
    // the old 8 s padding) instead of sleeping past it.
    let _plugins_ready = wait_plugins_loaded(&server, &specs, Duration::from_secs(8));
    // Ordering floor for the load-reply → dispatch settle.
    settle(1);
    let home = rust_tui.screen_text();
    dump_grid(&format!("g454-{tag}-home"), &home);
    let from = rust_tui.raw_len();
    let sent_at = Instant::now();
    rust_tui.write(b"hello\r");
    let replied = wait_reply_or_missed(&rust_tui, &gateway, from, Duration::from_secs(60));
    settle(1);
    let session = rust_tui.screen_text();
    dump_grid(&format!("g454-{tag}-session"), &session);
    // The failed turn raises a MODAL "Session failed" error toast (the
    // F11 lane: keystrokes swallow until dismissed — escape or ctrl+c
    // are its dismiss keys, app.rs `error_toast_live`). A chord sent
    // while it is up gets consumed, the exit never lands, and the
    // teardown burns its whole grace before the kill. Dismiss the
    // toast through its own key face (bounded; a no-op when no toast
    // is up — the pass/replied cases exit instantly).
    {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if !rust_tui.screen_text().contains("Session failed") {
                break;
            }
            rust_tui.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(250));
        }
    }
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
