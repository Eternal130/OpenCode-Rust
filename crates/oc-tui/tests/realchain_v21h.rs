//! Lane va21h — the G-554 resume-agent paired anchor (the 修锚同 lane
//! rule): the `rs` rig pattern from `realchain_v21g.rs`'s resume
//! anchors (the va19sub two-phase re-export) with the session SEEDED
//! with agent `plan` —
//!
//! * phase 1: `mini --agent plan` runs the subagent round-trip (the
//!   va19sub turn scripts, turns 0..2) — the session is CREATED with
//!   the explicit agent (both ends agree on the with-input face:
//!   "Plan" in the statusline + the turn summary);
//! * phase 2: `mini --continue` (NO `--agent`) re-attaches the
//!   session — the resume seam (`agent: input.agent ??
//!   selected?.agent`, session-target.ts:58) resolves the STORED
//!   agent: the settled statusline's agent group + the SECOND turn's
//!   summary row (R30's un-anchored "a new turn on --continue"
//!   state). The turn-003 seed is lane-local (a simple text turn —
//!   the `anthropic-text.txt` shape with a v21h needle) staged beside
//!   copies of the committed va19sub turn-000..002 scripts.
//!
//! W12 discipline (sandbox no-judgment): the tests EXPORT the settled
//! grids + the paired diff to
//! `.test-logs/diff-reports/v21h-rs-<state>.{json,txt}` and make NO
//! rust-side content assertions (rig health + ref-side sanity only);
//! judgment belongs to the controller's adjudication registry.
//!
//! WORKTREE BINARY (va21e lesson): build the bin in this worktree
//! first (`cargo build -p oc-cli --bin opencode-rust --locked`) and
//! run the anchor with `CARGO_TARGET_DIR=<worktree>/target` — unset,
//! the rig redirects to the MAIN checkout's binary.
//!
//! THREADS (controller steering 2026-11-27): run with `--profile
//! anchors` (test-threads=2).

#![allow(
    clippy::doc_markdown,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

#[path = "realchain_harness.rs"]
mod realchain_harness;

use realchain_harness::{
    capture_settled, dump_grid, export_grid_diff, lane_lock, normalize_grid, ref_lane_skip_reason,
    ref_mini_spawn_with_args, ref_service_stop, ref_stage, sandbox, service_start, service_stop,
    Gateway, Sandbox, TuiProcess,
};
use std::time::Duration;

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("V21H SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;

/// This WORKTREE's root (the seed scripts + record dirs are
/// lane-local — the same shape `realchain_v21g.rs` documents).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// The turn-003 seed body — a simple text turn (the
/// `anthropic-text.txt` shape) with a v21h-specific completion
/// needle: the RESUME pair's second prompt must produce a settled
/// turn (the turn summary row rides the export).
const TURN3_SEED: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_v21h_t3\",\"type\":\"message\",",
    "\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[],\"stop_reason\":null,",
    "\"stop_sequence\":null,\"usage\":{\"input_tokens\":32,\"output_tokens\":1}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,",
    "\"delta\":{\"type\":\"text_delta\",\"text\":\"V21H-RESUME-TURN-OK\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},",
    "\"usage\":{\"input_tokens\":32,\"output_tokens\":6}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n"
);

/// Stage the lane-local seeds dir: copies of the committed va19sub
/// turn-000..002 scripts (the phase-1 subagent round-trip) + the
/// turn-003 body above (the phase-2 second turn — turn ordinals are
/// per-gateway-process and cumulative, so the resume pair's fresh
/// prompt lands on turn 3).
fn stage_seeds() -> std::path::PathBuf {
    let source = wt_root().join("tools/diff/realchain/seeds/va19sub");
    assert!(
        source.join("turn-000.sse").is_file(),
        "V21H RED: missing seed scripts under {}",
        source.display()
    );
    let staged = wt_root().join(".test-logs/v21h-rs-seeds");
    std::fs::create_dir_all(&staged).expect("create staged seeds dir");
    for name in ["turn-000.sse", "turn-001.sse", "turn-002.sse"] {
        let text = std::fs::read_to_string(source.join(name))
            .unwrap_or_else(|error| panic!("read seed {name}: {error}"));
        std::fs::write(staged.join(name), text)
            .unwrap_or_else(|error| panic!("stage {name}: {error}"));
    }
    std::fs::write(staged.join("turn-003.sse"), TURN3_SEED)
        .unwrap_or_else(|error| panic!("stage turn-003.sse: {error}"));
    staged
}

/// The tool pre-authorization block (the v21g shape — the subagent
/// round-trip needs the subagent permission).
fn allow_subagent(box_: &Sandbox, ref_side: bool) {
    let path = if ref_side {
        box_.root.join("xdg-config/opencode/opencode.json")
    } else {
        box_.config_path()
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read config {}: {error}", path.display()));
    let mut config: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse config: {error}"));
    config["permission"] = serde_json::json!({
        "bash": "allow",
        "edit": "allow",
        "write": "allow",
        "read": "allow",
        "subagent": "allow",
    });
    std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
        .unwrap_or_else(|error| panic!("write config {}: {error}", path.display()));
}

/// The squashed-screen needle (the rig-health completion wait — the
/// row shape itself rides the export).
fn screen_squashed(tui: &TuiProcess) -> String {
    tui.screen_text().chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn wait_squashed(tui: &TuiProcess, needle: &str, secs: u64) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    let mut ok = screen_squashed(tui).contains(needle);
    while !ok && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        ok = screen_squashed(tui).contains(needle);
    }
    ok
}

/// The settled corpus capture for the resume pair (both ends
/// normalized over their sandbox roots, exported to diff-reports).
fn capture_rs_pair(
    rust: &Sandbox,
    refbox: &Sandbox,
    rust_tui: &TuiProcess,
    ref_tui: &TuiProcess,
    state: &str,
) {
    let roots =
        [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let normalize = |grid: &str| normalize_grid(grid, &refs);
    let ref_grid = normalize(&capture_settled(ref_tui));
    let rust_grid = normalize(&capture_settled(rust_tui));
    dump_grid(&format!("v21h-rs-{state}-ref"), &ref_grid);
    dump_grid(&format!("v21h-rs-{state}-rust"), &rust_grid);
    export_grid_diff("v21h-rs", state, &ref_grid, &rust_grid);
}

/// G-554 — the resume-agent anchor: phase 1 seeds a session with
/// agent `plan` (`mini --agent plan` + the va19sub round-trip), phase
/// 2 resumes it (`mini --continue`, no `--agent`) and runs a NEW turn
/// — the stored agent must resolve on both faces (the settled
/// statusline's agent group + the second turn's summary row).
#[test]
fn v21h_resume_agent_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The staged seeds: va19sub turns 0..2 (phase 1's round-trip) +
    // the lane-local turn-003 (phase 2's second turn).
    let seeds = stage_seeds();
    let record_rust = wt_root().join(".test-logs/v21h-rs-seeds-rust");
    let record_ref = wt_root().join(".test-logs/v21h-rs-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    let pace = "synthetic:ttft=2500:tps=8";
    let rust_gateway =
        Gateway::spawn_turn_paced("v21h-rs", &[], Some((&record_rust, &seeds)), pace);
    let ref_gateway = Gateway::spawn_turn_paced("v21h-rsr", &[], Some((&record_ref, &seeds)), pace);
    let rust = sandbox("v21h-rs", rust_gateway.port);
    let refbox = sandbox("v21h-rsref", ref_gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    allow_subagent(&rust, false);
    allow_subagent(&refbox, true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;

    // Phase 1 — the AGENT-SEEDED pair: `mini --agent plan` creates
    // the session with the explicit agent and runs the subagent
    // round-trip (the with-input face: both ends carry "Plan").
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini").arg("--agent").arg("plan");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn_with_args(&refbox, &stage, COLS, ROWS, &["--agent", "plan"]);
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    assert!(
        rust_boot && ref_boot,
        "V21H RED: first boot rust={rust_boot} ref={ref_boot}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    rust_tui.write(b"hello\r");
    ref_tui.write(b"hello\r");
    let rust_done = wait_squashed(&rust_tui, "round-tripcompleted", 90);
    let ref_done = wait_squashed(&ref_tui, "round-tripcompleted", 120);
    assert!(
        rust_done && ref_done,
        "V21H RED: the round-trip never completed rust={rust_done} ref={ref_done}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(3);
    capture_rs_pair(&rust, &refbox, &rust_tui, &ref_tui, "first-turn-settled");
    // The first pair EXITS (the two-press chord); the SERVERS stay up
    // (the session stays stored with its agent).
    rust_tui.write(b"\x03\x03");
    ref_tui.write(b"\x03\x03");
    settle(3);
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));

    // Phase 2 — the RESUME pair: `mini --continue` (NO `--agent`)
    // re-attaches the stored session; the replay feed walks the
    // transcript, then a NEW prompt runs turn-003 — the settled
    // statusline's agent group + the turn summary row read the
    // STORED agent through the resume seam.
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini").arg("--continue");
    rust_command.env("PWD", rust.project.display().to_string());
    let mut rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let mut ref_tui = ref_mini_spawn_with_args(&refbox, &stage, COLS, ROWS, &["--continue"]);
    let rust_boot = rust_tui.wait_for(b"oc mini", 0, Duration::from_secs(30));
    let ref_boot = ref_tui.wait_for(b"oc mini", 0, Duration::from_secs(60));
    let rust_replayed = wait_squashed(&rust_tui, "round-tripcompleted", 60);
    let ref_replayed = wait_squashed(&ref_tui, "round-tripcompleted", 90);
    assert!(
        rust_boot && ref_boot && rust_replayed && ref_replayed,
        "V21H RED: boot rust={rust_boot} ref={ref_boot} replayed rust={rust_replayed} ref={ref_replayed}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    // The settled statusline: the agent group face rides the export
    // (the G-554 seam resolves the stored agent pre-turn).
    settle(5);
    capture_rs_pair(&rust, &refbox, &rust_tui, &ref_tui, "resume-settled");
    // The NEW turn on --continue (R30's un-anchored state): the
    // second prompt runs turn-003; its summary row lands after the
    // turn settles.
    rust_tui.write(b"again\r");
    ref_tui.write(b"again\r");
    let rust_turn = wait_squashed(&rust_tui, "V21H-RESUME-TURN-OK", 90);
    let ref_turn = wait_squashed(&ref_tui, "V21H-RESUME-TURN-OK", 120);
    assert!(
        rust_turn && ref_turn,
        "V21H RED: the resume turn never completed rust={rust_turn} ref={ref_turn}; rust screen:\n{}\nref screen:\n{}",
        rust_tui.screen_text(),
        ref_tui.screen_text()
    );
    settle(5);
    capture_rs_pair(&rust, &refbox, &rust_tui, &ref_tui, "resume-second-turn");
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    drop(lane);
    drop(rust_gateway);
    drop(ref_gateway);
}
