//! CHANNEL 9 va19 — the dialog-family fix wave's paired anchors (pair
//! rig, EXPORT ONLY — W12/2.2#11: no adjudication in-test, every
//! anchor rides `.test-logs/diff-reports/v19*` and the controller's
//! adjudication registry).
//!
//! Each scenario live-drives BOTH ends (ref 2.0.24 via bun AND the
//! rust port) through the same keystrokes and exports the settled /
//! mid-state frames. In-test assertions are rig-health class
//! exclusively (boot settle, ref-side scenario sanity, both TUIs
//! alive); every content judgment is the controller's.
//!
//! Anchors covered here (see the lane dispatch):
//!  - v19sa (G-493): the select dialog's focused-action RESETS —
//!    tab (footer action focus) → typing a filter char resets the
//!    action focus back to the list, enter submits the selected ROW;
//!    tab → ↓ likewise returns the focus to the list.
//!  - v19rn (G-494①): the session-rename success semantics — the
//!    post-submit frame (the dialog closes on the rename echo, the
//!    tab strip carries the new title) + the re-opened list row.
//!  - v19sub (G-482): the subagent footer MENU — a foreground
//!    `subagent` tool call (turn-scripted replay) runs a child while
//!    `down` opens the searchable menu, enter routes the inspector,
//!    and the settled post-completion frame closes the loop.
//!
//! Undrivable arms (documented for the controller, no scenario):
//!  - G-494②'s integration-rename success face — the sandbox rig has
//!    no configured integration/credential, so the accounts view has
//!    no renameable row; store-level evidence only (the settled
//!    re-open + prompt-until-POST semantics).

// Test-harness infra lints (see realchain_pair.rs's list for the
// rationale): `{:?}` forensics in panics, wire-shape names in docs.
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
    capture_settled, default_pace, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_mini_spawn, ref_service_stop, ref_stage, ref_tui_spawn, sandbox,
    service_start, service_stop, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";
const TITLE_TEXT: &str = "Sandbox Replay Session";
const SUB_DONE_NEEDLE: &[u8] = b"round-trip completed";

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("VA19 SKIP: {reason}");
            return;
        }
    };
}

/// This WORKTREE's root (no main-checkout redirect: the committed
/// va19sub seed scripts + the runtime record dirs are lane-local).
fn wt_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf()
}

struct V19Rig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl V19Rig {
    fn roots(&self) -> Vec<String> {
        vec![
            self.refbox.root.to_string_lossy().into_owned(),
            self.rust.root.to_string_lossy().into_owned(),
        ]
    }

    fn normalize(&self, grid: &str) -> String {
        let roots = self.roots();
        let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
        normalize_grid(grid, &refs)
    }

    fn write_both(&mut self, bytes: &[u8]) {
        self.rust_tui.write(bytes);
        self.ref_tui.write(bytes);
    }
}

impl Drop for V19Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

/// The turn-anchored subagent rig: ONE gateway PER ENGINE (independent
/// turn counters — the parent→child→parent main-loop sequence is
/// deterministic for a FOREGROUND subagent), seeded from the committed
/// turn scripts, paced so the child STREAMS slowly enough to open the
/// footer menu mid-run.
struct SubRig {
    _lane: LaneGuard,
    _rust_gateway: Gateway,
    _ref_gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl SubRig {
    fn roots(&self) -> Vec<String> {
        vec![
            self.refbox.root.to_string_lossy().into_owned(),
            self.rust.root.to_string_lossy().into_owned(),
        ]
    }

    fn normalize(&self, grid: &str) -> String {
        let roots = self.roots();
        let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
        normalize_grid(grid, &refs)
    }

    fn write_both(&mut self, bytes: &[u8]) {
        self.rust_tui.write(bytes);
        self.ref_tui.write(bytes);
    }
}

impl Drop for SubRig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

fn v19_rig(tag: &str) -> V19Rig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    V19Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui }
}

fn boot_both(rig: &mut V19Rig) {
    let rust_settled = rig.rust_tui.wait_for(WORDMARK, 0, Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for(WORDMARK, 0, Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "VA19 rig RED: boot rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
         screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

/// The subagent rig: pre-authorized tools (the loop must not stop on a
/// permission dialog) + the turn-scripted, slow-paced gateways.
fn sub_rig(tag: &str) -> SubRig {
    let lane = lane_lock();
    let seeds = wt_root().join("tools/diff/realchain/seeds/va19sub");
    assert!(
        seeds.join("turn-000.sse").is_file(),
        "VA19 sub rig RED: missing seed scripts under {}",
        seeds.display()
    );
    // The record dirs are runtime-only (git-ignored): the gateway
    // seeds every strict miss from the committed turn scripts and
    // writes the cassette for the log — deterministic per run.
    let record_rust = wt_root().join(".test-logs/va19sub-seeds-rust");
    let record_ref = wt_root().join(".test-logs/va19sub-seeds-ref");
    std::fs::create_dir_all(&record_rust).expect("create rust record dir");
    std::fs::create_dir_all(&record_ref).expect("create ref record dir");
    // ttft 2.5s + 8 chars/s: the child's ~104-char reply streams for
    // ~15s — the menu window. Both ends share the spec (identical
    // chunking); the T9_PACE override is deliberately ignored (the
    // anchor needs the window, like the toolloop lane pins instant).
    let pace = "synthetic:ttft=2500:tps=8";
    let rust_gateway = Gateway::spawn_turn_paced(tag, &[], Some((&record_rust, &seeds)), pace);
    let ref_gateway = Gateway::spawn_turn_paced(tag, &[], Some((&record_ref, &seeds)), pace);
    let mut rust = sandbox(tag, rust_gateway.port);
    let mut refbox = sandbox(&format!("{tag}ref"), ref_gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    allow_tools(&rust, false);
    allow_tools(&refbox, true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    // The MINI frontend on both ends (the G-482 surfaces live in the
    // mini footer — footer.command.tsx, not the full TUI). `PWD` is
    // pinned to the sandbox project (the mini's localDirectory chdir
    // prefers `$PWD` — the inherited test-runner PWD would drag the
    // session into the worktree) and the cursor probe gets answered
    // (the layout docks at the probed row).
    let mut rust_command = rust.command(&realchain_harness::opencode_bin_pub());
    rust_command.arg("mini");
    rust_command.env("PWD", rust.project.display().to_string());
    let rust_tui = TuiProcess::spawn_command(rust_command, COLS, ROWS);
    rust_tui.arm_cursor_probe_answers(ROWS);
    let ref_tui = ref_mini_spawn(&refbox, &stage, COLS, ROWS);
    SubRig {
        _lane: lane,
        _rust_gateway: rust_gateway,
        _ref_gateway: ref_gateway,
        rust,
        refbox,
        stage,
        rust_tui,
        ref_tui,
    }
}

/// The tool pre-authorization block (the subagent round-trip must not
/// stop on a permission dialog): bash/edit/write/read plus the
/// `subagent` tool itself.
fn allow_tools(box_: &Sandbox, ref_side: bool) {
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

fn boot_both_sub(rig: &mut SubRig) {
    // The mini splash carries the "oc mini" wordmark on both ends
    // (splash.ts:288-290; the port's splash mirrors it).
    let rust_settled = rig.rust_tui.wait_for_screen("oc mini", Duration::from_secs(30));
    let ref_settled = rig.ref_tui.wait_for_screen("oc mini", Duration::from_secs(60));
    assert!(
        rust_settled && ref_settled,
        "VA19 sub rig RED: boot rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
         screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    settle(3);
}

impl TuiProcess {
    fn screen_has(&self, needle: &str) -> bool {
        self.screen_text().contains(needle)
    }

    fn wait_for_screen(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.screen_has(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

/// One state's normalized screens, exported as a grid diff.
fn export_state(rig: &V19Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// A TRANSIENT state (no quiescence settle): read both screens NOW
/// and export immediately (the normalization snapshot is this
/// report's own — M-1 discipline).
fn export_state_now(rig: &V19Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// The SubRig twins of the export helpers above.
fn export_sub_state(rig: &SubRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

fn export_sub_state_now(rig: &SubRig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// One settled exchange on the digest rig: the seeded reply + title
/// land on both ends, so the Sessions dialog has its row.
fn hello_turn(rig: &mut V19Rig) {
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_replied = rig.rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30));
    let ref_replied = rig.ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
    let _ = rig.ref_tui.wait_for(TITLE_TEXT.as_bytes(), ref_from, Duration::from_secs(30));
    assert!(
        rust_replied && ref_replied,
        "VA19 rig RED: hello turn rust={rust_replied} ref={ref_replied}"
    );
    settle(3);
}

// ---------------------------------------------------------------------------
// v19sa — G-493: the select dialog's focused_action RESETS.
// ---------------------------------------------------------------------------

#[test]
fn v19sa_select_action_reset_pair() {
    skip_without_ref_lane!();
    let mut rig = v19_rig("v19sa");
    boot_both(&mut rig);
    hello_turn(&mut rig);
    // Open the Sessions dialog (footer actions carry delete/rename).
    rig.write_both(b"\x18l");
    let ref_open = rig.ref_tui.wait_for_screen(TITLE_TEXT, Duration::from_secs(10));
    settle(2);
    export_state(&rig, "v19sa", "open");
    // Arm 1: tab focuses the footer action row; TYPING a filter char
    // resets the action focus back to the list (G-493's set_filter
    // reset) — enter then submits the selected ROW (navigate), not
    // the stale action.
    rig.write_both(b"\t");
    settle(1);
    export_state(&rig, "v19sa", "tab");
    rig.write_both(b"san");
    std::thread::sleep(Duration::from_millis(250));
    export_state_now(&rig, "v19sa", "tabfilter-mid");
    settle(2);
    export_state(&rig, "v19sa", "tabfilter");
    rig.write_both(b"\r");
    settle(3);
    export_state(&rig, "v19sa", "tabfilterenter");
    // Arm 2: reopen, tab, then ↓ — the list move likewise drops the
    // action focus (G-493's move_selection reset); enter submits the
    // row.
    rig.write_both(b"\x1b");
    settle(1);
    rig.write_both(b"\x18l");
    let ref_reopen = rig.ref_tui.wait_for_screen(TITLE_TEXT, Duration::from_secs(10));
    settle(2);
    rig.write_both(b"\t");
    settle(1);
    export_state(&rig, "v19sa", "tab2");
    rig.write_both(b"\x1b[B");
    settle(1);
    export_state(&rig, "v19sa", "tabdown");
    rig.write_both(b"\r");
    settle(3);
    export_state(&rig, "v19sa", "tabdownenter");
    assert!(
        ref_open && ref_reopen,
        "VA19 sa rig RED: the sessions dialog never opened on ref (open={ref_open} \
         reopen={ref_reopen})"
    );
    drop(rig);
}

// ---------------------------------------------------------------------------
// v19rn — G-494①: the session-rename success semantics.
// ---------------------------------------------------------------------------

#[test]
fn v19rn_rename_success_pair() {
    skip_without_ref_lane!();
    let mut rig = v19_rig("v19rn");
    boot_both(&mut rig);
    hello_turn(&mut rig);
    // Open the Sessions dialog, ctrl+r on the selected row → the
    // rename prompt (prefilled with the current title).
    rig.write_both(b"\x18l");
    let ref_open = rig.ref_tui.wait_for_screen(TITLE_TEXT, Duration::from_secs(10));
    settle(2);
    rig.write_both(b"\x12"); // ctrl+r
    let ref_rename = rig.ref_tui.wait_for_screen("Rename session", Duration::from_secs(10));
    let rust_rename = rig.rust_tui.wait_for_screen("Rename session", Duration::from_secs(10));
    settle(1);
    export_state(&rig, "v19rn", "open");
    // Type the suffix (the input carries the current title): the new
    // title is deterministic on both ends.
    rig.write_both(b" MARK");
    settle(1);
    export_state(&rig, "v19rn", "typed");
    // Submit: the dialog closes on the rename echo (the port waits
    // for `session.renamed`; the ref closes on the POST then()) — the
    // immediate post-enter frame + the settled one (tab strip title).
    rig.write_both(b"\r");
    std::thread::sleep(Duration::from_millis(250));
    export_state_now(&rig, "v19rn", "enter-now");
    settle(3);
    export_state(&rig, "v19rn", "enter-settled");
    // The re-opened list carries the renamed row.
    rig.write_both(b"\x18l");
    let ref_relist = rig.ref_tui.wait_for_screen("MARK", Duration::from_secs(10));
    settle(2);
    export_state(&rig, "v19rn", "list-after");
    assert!(
        ref_open && ref_rename && rust_rename && ref_relist,
        "VA19 rn rig RED: open={ref_open} rename(ref)={ref_rename} rename(rust)={rust_rename} \
         relist={ref_relist}"
    );
    drop(rig);
}

// ---------------------------------------------------------------------------
// v19sub — G-482: the subagent footer menu (turn-scripted child).
//
// DEGRADED + `#[ignore]` (controller steer, 5-round red budget spent):
// the sandbox rig cannot drive the running-child pane deterministically
// (the round-trip itself completes server-side on both engines; the
// mini pane/menu faces are not stably reachable through this rig —
// probe/PWD/layout interactions documented in the lane report). The
// unit-level paired anchor for G-482 is the re-pinned
// `mini_subagent_footer` PTY scenario (commit 55455967); the
// turn-scripted corpus (seeds/va19sub) is retained for the anchor
// wave (va25).
// ---------------------------------------------------------------------------

#[ignore = "rig cannot drive a running child session in the sandbox (5 gated rounds red); realchain pair anchor deferred to the anchor wave (va25) — vt/unit-level coverage + seeds/va19sub corpus retained"]
#[test]
fn v19sub_menu_pair() {
    skip_without_ref_lane!();
    let mut rig = sub_rig("v19sub");
    boot_both_sub(&mut rig);
    // The foreground subagent turn: turn 0's tool call spawns the
    // child; the footer pane counts the RUNNING child while turn 1
    // streams (~15s window).
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let rust_pane = rig.rust_tui.wait_for_screen("1 subagent", Duration::from_secs(20));
    let ref_pane = rig.ref_tui.wait_for_screen("1 subagent", Duration::from_secs(30));
    if !(rust_pane && ref_pane) {
        eprintln!(
            "V19SUB note: the running-child pane row not seen (rust={rust_pane} ref={ref_pane})"
        );
    }
    export_sub_state_now(&rig, "v19sub", "running");
    // `down` (session.child.first) opens the searchable MENU.
    rig.write_both(b"\x1b[B");
    let rust_menu = rig.rust_tui.wait_for_screen("Select subagent", Duration::from_secs(5));
    let ref_menu = rig.ref_tui.wait_for_screen("Select subagent", Duration::from_secs(5));
    if !(rust_menu && ref_menu) {
        eprintln!("V19SUB note: the menu face not seen (rust={rust_menu} ref={ref_menu})");
    }
    export_sub_state_now(&rig, "v19sub", "menu");
    // Enter routes the inspector on the menu's first row; the settled
    // export after the round-trip completes is the phase-stable face.
    rig.write_both(b"\r");
    std::thread::sleep(Duration::from_millis(500));
    export_sub_state_now(&rig, "v19sub", "inspector-mid");
    let rust_done = rig.rust_tui.wait_for(SUB_DONE_NEEDLE, rust_from, Duration::from_secs(60));
    let ref_done = rig.ref_tui.wait_for(SUB_DONE_NEEDLE, ref_from, Duration::from_secs(90));
    if !(rust_done && ref_done) {
        eprintln!(
            "V19SUB note: the round-trip completion not seen (rust={rust_done} ref={ref_done})"
        );
    }
    settle(3);
    export_sub_state(&rig, "v19sub", "inspector-settled");
    // Esc back to the composer: the settled post-round-trip frame.
    rig.write_both(b"\x1b");
    settle(2);
    export_sub_state(&rig, "v19sub", "done");
    drop(rig);
}
