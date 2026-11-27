//! CHANNEL 9 va17 — the component-audit CANDIDATE VERIFICATION WAVE
//! (pair rig, EXPORT ONLY — W12/2.2#11: no adjudication in-test, every
//! candidate rides `.test-logs/diff-reports/v17-*` and the
//! controller's adjudication registry).
//!
//! Each scenario live-drives BOTH ends (ref 2.0.24 via bun AND the
//! rust port) through the same keystrokes and exports the settled /
//! mid-state frames. In-test assertions are rig-health class
//! exclusively (boot settle, ref-side scenario sanity, both TUIs
//! alive); every content judgment — including each candidate's
//! confirmed/refuted frame evidence — is the controller's.
//!
//! Candidates covered here (see the lane dispatch):
//!  - v17a: models palette footer action (`tab` focus + `return` →
//!    ref submit() consults focusedAction first — dialog-select.tsx
//!    421-432; rust submits the selected row) + the scroll-window
//!    scrollTo(0) micro candidate (End → Home on the unfiltered
//!    categorized list).
//!  - v17b: the Sessions dialog — footer-action tab/enter (ref pins
//!    the selected row, dialog stays open), `ctrl+r` rename (ref opens
//!    the rename dialog on the selected row — keybind.ts:265; the rust
//!    pump only wires pin/delete — dialog.rs:528-558), the searchState
//!    faces (searching mid-state + no-match settled state), and the
//!    TabMenu row-context frame (right-click on the session tab).
//!  - v17c: SessionExport keyboard/shape (`<leader>x`; the ref's pill
//!    box — dialog-export-options.tsx:20-83 tab-cycles the active
//!    group, return activates — vs the rust select row model,
//!    command_palette.rs:1397-1440).
//!  - v17d: the diff viewer route — empty (non-git project) state,
//!    DiffHelp (`?`), DiffSource (`d`) and DiffBase shapes, then the
//!    SAME faces over a seeded git repo (branch data state).
//!  - v17e: the ToolOutput/ShellOutput dialog family (candidate 3 +
//!    the centered candidate's shell-output face): the toolloop
//!    corpus session's shell row is CLICKED on both ends; the ref
//!    opens its custom xlarge centered viewer (dialog-shell-output),
//!    the rust side's outcome rides the export.
//!  - v17f: the file-mention dedup key (C-road candidate): two
//!    same-named files in different directories, both @-mentioned —
//!    ref moves the mention per uri, rust folds per name; the
//!    composer frame is exported.
//!
//! Undrivable candidates (documented for the controller, no scenario):
//!  - centered family `btw.tsx`/`api.tsx` faces — no TUI trigger path
//!    in the sandbox rig (plugin update notice / plugin-API dialog).
//!  - settings `saving` re-entry — needs a slow config server.
//!  - sessions searchState error faces — need a failing server
//!    (`Could not search sessions…`); code-level evidence only.
//!  - grouping adjacency vs full aggregation ([A(x),B(y),C(x)]) — no
//!    data-injection path for an interleaved categorized list.
//!  - home form dock-vs-overlay (MCP elicitation at home) — no MCP
//!    server in the rig; G-467.2 family, needs-controller.

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
    capture_settled, default_pace, dump_grid, export_grid_diff, lane_lock, normalize_grid,
    ref_lane_skip_reason, ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start,
    service_stop, worktree_dir, Gateway, LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";
const TITLE_TEXT: &str = "Sandbox Replay Session";
const CONFIRM_NEEDLE: &str = "TOOLLOOP-CONFIRMED";

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("VA17 SKIP: {reason}");
            return;
        }
    };
}

struct V17Rig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl V17Rig {
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

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// 2026-11-27 slow-audit fix A (battery family, the pattern proven in
/// the v21 waves): wait BOTH ends CONCURRENTLY — the serial two-line
/// form (rust first, then ref) doubled every wait's wall-clock; the
/// concurrent legs run in threads and the tuple carries both
/// verdicts.
fn wait_both(
    rust_tui: &TuiProcess,
    ref_tui: &TuiProcess,
    needle: &[u8],
    rust_from: usize,
    rust_secs: u64,
    ref_from: usize,
    ref_secs: u64,
) -> (bool, bool) {
    std::thread::scope(|scope| {
        let rust_leg = scope
            .spawn(move || rust_tui.wait_for(needle, rust_from, Duration::from_secs(rust_secs)));
        let ref_leg =
            scope.spawn(move || ref_tui.wait_for(needle, ref_from, Duration::from_secs(ref_secs)));
        (rust_leg.join().unwrap_or(false), ref_leg.join().unwrap_or(false))
    })
}

/// The screen-needle twin of [`wait_both`] (same slow-audit fix A).
fn wait_both_screens(
    rust_tui: &TuiProcess,
    ref_tui: &TuiProcess,
    needle: &str,
    rust_secs: u64,
    ref_secs: u64,
) -> (bool, bool) {
    let poll = |tui: &TuiProcess, secs: u64| {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if tui.screen_text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    std::thread::scope(|scope| {
        let rust_leg = scope.spawn(move || poll(rust_tui, rust_secs));
        let ref_leg = scope.spawn(move || poll(ref_tui, ref_secs));
        (rust_leg.join().unwrap_or(false), ref_leg.join().unwrap_or(false))
    })
}

/// The hello-turn wait cluster (the reply needle on BOTH ends + the
/// seeded title on ref) in ONE scope — the serial two-line form paid
/// the rust reply wait on top of the ref's, and the title leg ran
/// only after both.
fn wait_hello_turn(
    rust_tui: &TuiProcess,
    ref_tui: &TuiProcess,
    rust_from: usize,
    ref_from: usize,
) -> (bool, bool) {
    std::thread::scope(|scope| {
        let rust_leg = scope
            .spawn(move || rust_tui.wait_for(REPLY_NEEDLE, rust_from, Duration::from_secs(30)));
        let ref_leg = scope.spawn(move || {
            let replied = ref_tui.wait_for(REPLY_NEEDLE, ref_from, Duration::from_secs(60));
            // The title lands with (usually before) the reply —
            // ref-only needle, waited out in the same leg.
            let _ = ref_tui.wait_for(TITLE_TEXT.as_bytes(), ref_from, Duration::from_secs(30));
            replied
        });
        (rust_leg.join().unwrap_or(false), ref_leg.join().unwrap_or(false))
    })
}

fn v17_rig(tag: &str) -> V17Rig {
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
    V17Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui }
}

impl Drop for V17Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn boot_both(rig: &mut V17Rig) {
    // slow-audit fix A: the boot legs run CONCURRENTLY (the serial
    // two-line form paid the rust boot on top of the ref's slower
    // one).
    let (rust_settled, ref_settled) =
        wait_both(&rig.rust_tui, &rig.ref_tui, WORDMARK, 0, 30, 0, 60);
    assert!(
        rust_settled && ref_settled,
        "VA17 rig RED: boot rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
         screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    // Timeline duty, NOT padding: the session-list fetch + composer
    // focus complete AFTER the wordmark paints — the first
    // keystrokes must land in the ready app.
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
fn export_state(rig: &V17Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// A TRANSIENT state (no quiescence settle): read both screens NOW
/// and export immediately (the normalization snapshot is this
/// report's own — M-1 discipline).
fn export_state_now(rig: &V17Rig, scenario: &str, state: &str) {
    let ref_grid = rig.normalize(&rig.ref_tui.screen_text());
    let rust_grid = rig.normalize(&rig.rust_tui.screen_text());
    export_grid_diff(scenario, state, &ref_grid, &rust_grid);
}

/// The needle's (char-col, row) on one end's CURRENT grid.
fn find_on_screen(tui: &TuiProcess, needle: &str) -> Option<(usize, usize)> {
    for (row, line) in tui.screen_text().lines().enumerate() {
        if let Some(at) = line.find(needle) {
            let col = line[..at].chars().count();
            return Some((col, row));
        }
    }
    None
}

/// An SGR left-click (press + release) at a grid cell.
fn sgr_click(tui: &mut TuiProcess, x: usize, y: usize) {
    tui.write(format!("\x1b[<0;{};{}M", x + 1, y + 1).as_bytes());
    std::thread::sleep(Duration::from_millis(120));
    tui.write(format!("\x1b[<0;{};{}m", x + 1, y + 1).as_bytes());
}

// ---------------------------------------------------------------------------
// v17a — the models palette: the footer-action tab/return candidate +
// the categorized-list End→Home (scrollTo(0)) micro candidate.
// ---------------------------------------------------------------------------

#[test]
fn v17a_palette_action_and_scroll_pair() {
    skip_without_ref_lane!();
    let mut rig = v17_rig("v17a");
    boot_both(&mut rig);
    // Open the models palette on the UNFILTERED list (many rows — the
    // scroll micro needs a real window).
    rig.write_both(b"\x18m");
    // slow-audit fix A: the open legs run CONCURRENTLY.
    let (rust_open, ref_open) =
        wait_both_screens(&rig.rust_tui, &rig.ref_tui, "claude-opus-4-7", 15, 30);
    // Timeline duty, NOT padding: the palette rows keep filling past
    // the selected row's needle — the End→Home micro below reads the
    // FULL list tail, so the fill must complete first.
    settle(2);
    // Micro: End jumps to the list tail, Home back to the top — the
    // ref's reveal() special-cases scrollTo(0) when the selection is
    // the first entry (dialog-select.tsx:406-418); the frames ride
    // the export.
    rig.write_both(b"\x1b[F");
    settle(1); // the End selection/scroll repaint
    export_state(&rig, "v17a", "palette-end");
    rig.write_both(b"\x1b[H");
    settle(1); // the Home selection/scroll repaint
    export_state(&rig, "v17a", "palette-home");
    // Candidate 1: tab (focus the footer action on ref / the rust
    // pump's own reading), then return. Ref submit() consults
    // focusedAction FIRST — the "View all integrations" action fires
    // (dialog-model.tsx:144-150); the rust Enter submits the selected
    // row. The POST-key frames are the evidence.
    rig.write_both(b"\t");
    settle(1); // the footer-action focus repaint
    export_state(&rig, "v17a", "palette-tab");
    rig.write_both(b"\r");
    // Timeline duty, NOT padding: the footer action's effect (the
    // "View all integrations" navigation on ref) is a post-submit
    // repaint — the settled export must carry the landed action.
    settle(3);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("v17a-tabenter-ref", &ref_grid);
    dump_grid("v17a-tabenter-rust", &rust_grid);
    drop(rig);
    assert!(
        rust_open && ref_open,
        "V17A rig RED: the palette never listed the replay model rust={rust_open} ref={ref_open}"
    );
    export_grid_diff("v17a", "palette-tabenter", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// v17b — the Sessions dialog: footer action tab/enter, ctrl+r rename,
// the searchState faces, and the TabMenu right-click frame.
// ---------------------------------------------------------------------------

#[test]
fn v17b_sessions_dialog_faces_pair() {
    skip_without_ref_lane!();
    let mut rig = v17_rig("v17b");
    boot_both(&mut rig);
    // One settled turn: the session (and its seeded title) exist on
    // both ends, so the Sessions dialog has rows.
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    // slow-audit fix A: the reply/title legs run CONCURRENTLY (the
    // serial two-line form paid the rust reply wait on top of the
    // ref's).
    let (rust_replied, ref_replied) =
        wait_hello_turn(&rig.rust_tui, &rig.ref_tui, rust_from, ref_from);
    // Timeline duty, NOT padding: under a T9_PACE override the reply
    // STREAMS past the needle — the stream tail + the composer
    // re-enable must land before the dialog keys (under the default
    // instant pace it is cheap insurance for the same window).
    settle(3);
    // Open the Sessions dialog.
    rig.write_both(b"\x18l");
    let ref_open = rig.ref_tui.wait_for_screen(TITLE_TEXT, Duration::from_secs(10));
    // The TITLE needle is the ref dialog's own row — the transition
    // is proven landed; the rust end's open+fill is absorbed by its
    // capture's 10s poll cap (the serial settle(2) this replaces was
    // approximation padding).
    export_state(&rig, "v17b", "sessions");
    // Candidate 1 (sessions face): tab focuses the footer action row
    // (pin/unpin, delete), return TRIGGERS it on ref — the selected
    // row pins, the dialog STAYS OPEN; the rust return submits the
    // selected row (navigates / closes).
    rig.write_both(b"\t");
    settle(1); // the footer focus repaint (no stable text needle)
    export_state(&rig, "v17b", "sessions-tab");
    rig.write_both(b"\r");
    // Timeline duty, NOT padding: the footer action's effect is a
    // server round-trip on ref (the pin) — the post-enter frame must
    // carry the landed effect.
    settle(3);
    export_state(&rig, "v17b", "sessions-tabenter");
    // Candidate 5: reopen, ctrl+r on the selected row. Ref opens the
    // rename dialog (dialog-session-rename); rust's pump only wires
    // pin/delete — ctrl+r falls through.
    rig.write_both(b"\x18l");
    let ref_reopen = rig.ref_tui.wait_for_screen(TITLE_TEXT, Duration::from_secs(10));
    // Same as the first open: the needle proves the ref's reopen; the
    // rust end's reopen+fill rides its capture's poll cap.
    // The ref's rename-dialog mount is needle-gated (the v19rn
    // needle "Rename session" — the serial settle(2) this replaces
    // only approximated it); the rust end's ctrl+r is a fallthrough
    // (no screen change to wait out).
    rig.write_both(b"\x12"); // ctrl+r
    if !rig.ref_tui.wait_for_screen("Rename session", Duration::from_secs(10)) {
        eprintln!("V17B note: the ref rename dialog did not open within its 10s window");
    }
    export_state(&rig, "v17b", "sessions-ctrlr");
    // Candidate 7: the searchState faces. Ref's rename dialog (if
    // open) closes on esc first; then clear any filter and type a
    // no-match query — the MID capture catches the searching state,
    // the settled capture the no-match state.
    rig.write_both(b"\x1b");
    settle(1);
    rig.write_both(b"\x08\x08"); // clear any leftover filter chars, defensively
    settle(1);
    rig.write_both(b"zz");
    std::thread::sleep(Duration::from_millis(250));
    export_state_now(&rig, "v17b", "sessions-search-mid");
    // Timeline duty, NOT padding: the ref's dialog search rides the
    // gateway — the searching face resolves to no-match only after
    // the round-trip (the v21b toast-dwell lesson: this window IS
    // the state machine).
    settle(3);
    export_state(&rig, "v17b", "sessions-search-settled");
    // Close everything back to home (ref: filter esc / dialog esc;
    // a couple of escs cover both layers on both ends). These stay
    // FIXED windows (not needle-gated): the title text is on the
    // dialog's own rows too, so a positive needle would false-fire
    // before the escs process.
    rig.write_both(b"\x1b");
    // the filter-clear repaint
    settle(1);
    rig.write_both(b"\x1b");
    // the dialog-close → home repaint (the tabmenu target)
    settle(2);
    // TabMenu bookkeeping frame: right-click the session tab (top
    // strip) — the row context menu's first column rides the export.
    let rust_tab = find_on_screen(&rig.rust_tui, TITLE_TEXT);
    let ref_tab = find_on_screen(&rig.ref_tui, TITLE_TEXT);
    if let (Some((rx, ry)), Some((fx, fy))) = (rust_tab, ref_tab) {
        // Right-click press + release at each end's own tab cell.
        rig.rust_tui.write(format!("\x1b[<2;{};{}M", rx + 1, ry + 1).as_bytes());
        std::thread::sleep(Duration::from_millis(120));
        rig.rust_tui.write(format!("\x1b[<2;{};{}m", rx + 1, ry + 1).as_bytes());
        rig.ref_tui.write(format!("\x1b[<2;{};{}M", fx + 1, fy + 1).as_bytes());
        std::thread::sleep(Duration::from_millis(120));
        rig.ref_tui.write(format!("\x1b[<2;{};{}m", fx + 1, fy + 1).as_bytes());
        // Timeline duty: the row-context menu's mount (no verified
        // two-end needle — the menu's rows are the export itself).
        settle(2);
        export_state(&rig, "v17b", "tabmenu");
    } else {
        eprintln!(
            "V17B tabmenu: the session tab was not on-screen (rust={rust_tab:?} ref={ref_tab:?}) \
             — the tabmenu frame was not captured"
        );
    }
    drop(rig);
    assert!(rust_replied && ref_replied, "V17B rig RED: the reply did not settle");
    assert!(
        ref_open && ref_reopen,
        "V17B RED (ref-side rig sanity): the ref Sessions dialog never listed the session \
         (open={ref_open} reopen={ref_reopen})"
    );
}

// ---------------------------------------------------------------------------
// v17c — the SessionExport dialog: shape + tab/return semantics.
// ---------------------------------------------------------------------------

#[test]
fn v17c_export_dialog_pair() {
    skip_without_ref_lane!();
    let mut rig = v17_rig("v17c");
    boot_both(&mut rig);
    // The export command targets the CURRENT session — one settled
    // turn first, so both ends have one.
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    // slow-audit fix A: the reply/title legs run CONCURRENTLY.
    let (rust_replied, ref_replied) =
        wait_hello_turn(&rig.rust_tui, &rig.ref_tui, rust_from, ref_from);
    // Timeline duty, NOT padding: under a T9_PACE override the reply
    // STREAMS past the needle — the stream tail + the composer
    // re-enable must land before the dialog keys (under the default
    // instant pace it is cheap insurance for the same window).
    settle(3);
    // `<leader>x` opens the export options on both ends (the ref pill
    // box, the rust select rows — both titled "Export session").
    rig.write_both(b"\x18x");
    // slow-audit fix A: the open legs run CONCURRENTLY.
    let (rust_open, ref_open) =
        wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Export session", 10, 15);
    // The BOTH-END needle proves the mounts landed — the serial
    // settle(2) this replaces was approximation padding (the pill/row
    // fill is a single local commit past the title).
    export_state(&rig, "v17c", "export-open");
    // Candidate 6: tab — ref cycles the active pill (markdown → json),
    // the rust pump's own reading rides the export.
    rig.write_both(b"\t");
    settle(1);
    export_state(&rig, "v17c", "export-tab");
    // Return — ref ACTIVATES the focused pill (json → format flips);
    // rust submits the selected row.
    rig.write_both(b"\r");
    settle(2); // the activation effect's repaint window
    export_state(&rig, "v17c", "export-enter");
    drop(rig);
    assert!(
        rust_replied && ref_replied,
        "V17C rig RED: the reply did not settle rust={rust_replied} ref={ref_replied}"
    );
    assert!(
        rust_open && ref_open,
        "V17C rig RED: the export dialog never opened rust={rust_open} ref={ref_open}"
    );
}

// ---------------------------------------------------------------------------
// v17d — the diff viewer route: empty (non-git) state, DiffHelp (?),
// DiffSource (d), DiffBase; then the same faces over a seeded repo.
// ---------------------------------------------------------------------------

#[test]
fn v17d_diff_viewer_faces_pair() {
    skip_without_ref_lane!();
    let mut rig = v17_rig("v17d");
    boot_both(&mut rig);
    // Route into the diff viewer through the command palette (both
    // ends: ctrl+p, filter "diff", return on "Open diff viewer").
    rig.write_both(b"\x10");
    // Sync barrier (timeline duty): the palette must be listening
    // before the filter types — "diff" landing in the composer would
    // break the route.
    settle(1);
    rig.write_both(b"diff");
    settle(1);
    rig.write_both(b"\r");
    // The sandbox project is NOT a git repo — the viewer's empty /
    // error face is exactly the state candidate 4 wants. The route's
    // completion is NEEDLE-gated on both ends (the settled face's own
    // copy — verified on both grids), replacing the fixed 4s window;
    // the trailing 1s is the late-transition buffer.
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "No changes to show", 30, 30);
    settle(1);
    export_state(&rig, "v17d", "diff-route-empty");
    // DiffHelp (?) — centered on ref (diff-viewer.tsx:1100-1192), the
    // rust select dialog's own geometry rides the export. The overlay
    // mount is needle-gated on both ends ("Diff shortcuts" is on both
    // grids).
    rig.write_both(b"?");
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Diff shortcuts", 15, 15);
    export_state(&rig, "v17d", "diff-help");
    rig.write_both(b"\x1b");
    settle(1);
    // DiffSource (d) — no filter row on ref (:735-767). The dialog
    // mount is needle-gated on both ends (the dialog title).
    rig.write_both(b"d");
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Diff source", 15, 15);
    export_state(&rig, "v17d", "diff-source");
    // Selecting the BASE source surfaces the DiffBase face (title
    // "Base branch", placeholder "Search local and remote branches"
    // on ref; the "No branches" empty view here) — the entries are
    // All/Committed/Uncommitted/Base, so three downs first. The base
    // dialog's mount is needle-gated on its title.
    rig.write_both(b"\x1b[B\x1b[B\x1b[B");
    settle(1);
    rig.write_both(b"\r");
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Base branch", 15, 15);
    export_state(&rig, "v17d", "diff-base-empty");
    // Back out to home. The escs MUST stay spaced: a lone ESC is
    // prefix-ambiguous in the input stream — back-to-back ESCs
    // coalesce into alt-esc combos (the dialogs then never close and
    // the next arm's filter text lands in the wrong field; verified
    // by the false-EXACT route-git export of the unspaced attempt).
    rig.write_both(b"\x1b");
    // ESC-parse disambiguation (closes the base dialog)
    settle(1);
    rig.write_both(b"\x1b");
    // ESC-parse disambiguation (closes the source dialog)
    settle(1);
    rig.write_both(b"\x1b");
    // the route-exit → home repaint (the re-entry target)
    settle(2);
    // Seed a real repo on BOTH projects (data state): a commit plus a
    // `dev` branch, so DiffBase carries rows and the search field.
    for project in [&rig.rust.project, &rig.refbox.project] {
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-C", &project.to_string_lossy()])
                .args(args)
                .output()
                .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
            assert!(
                out.status.success(),
                "V17D rig RED: git {:?} failed: {}{}",
                args,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=va17",
            "-c",
            "user.email=va17@t.local",
            "commit",
            "-q",
            "-m",
            "seed",
            "--allow-empty",
        ]);
        git(&["branch", "dev"]);
    }
    // Re-enter the viewer over the seeded repo.
    rig.write_both(b"\x10");
    // Sync barrier (timeline duty, same as the first route): the
    // palette must be listening before the filter types.
    settle(1);
    rig.write_both(b"diff");
    settle(1);
    rig.write_both(b"\r");
    // Same needle-gated route completion as the first arm (the
    // clean-seeded repo shows the empty face too).
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "No changes to show", 30, 30);
    settle(1);
    export_state(&rig, "v17d", "diff-route-git");
    rig.write_both(b"d");
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Diff source", 15, 15);
    export_state(&rig, "v17d", "diff-source-git");
    rig.write_both(b"\x1b[B\x1b[B\x1b[B");
    settle(1);
    rig.write_both(b"\r");
    let _ = wait_both_screens(&rig.rust_tui, &rig.ref_tui, "Base branch", 15, 15);
    export_state(&rig, "v17d", "diff-base-data");
    drop(rig);
}

// ---------------------------------------------------------------------------
// v17e — the ToolOutput/ShellOutput family: the toolloop corpus
// session's shell row is CLICKED on both ends; the ref's custom
// centered xlarge viewer (dialog-shell-output.tsx) vs the rust flat
// select (command_palette.rs:900-963) ride the export.
// ---------------------------------------------------------------------------

fn corpus_tools_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools")
}
fn corpus_tools_ref_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-tools-ref")
}
fn toolloop_seeds_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/seeds/toolloop")
}

fn stage_shims(rust: &Sandbox) -> PathBuf {
    let out = rust.root.join("shimstage");
    let status = Command::new("python3")
        .arg(worktree_dir("tools/diff/realchain/shim_extract.py"))
        .arg("--spec")
        .arg(toolloop_seeds_dir().join("shims.json"))
        .arg("--out")
        .arg(&out)
        .output()
        .expect("spawn shim_extract.py");
    assert!(
        status.status.success(),
        "shim_extract.py failed: {}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    out
}

#[test]
fn v17e_tool_output_dialog_pair() {
    skip_without_ref_lane!();
    let lane = lane_lock();
    // The toolloop rig shape (realchain_toolloop.rs): one
    // turn-anchored gateway PER ENGINE, PATH shims, pre-authorized
    // tools, aligned READMEs.
    let corpus = [corpus_tools_dir(), corpus_tools_ref_dir()];
    let rust_gateway = Gateway::spawn_turn("v17e", &corpus, None);
    let ref_gateway = Gateway::spawn_turn("v17e", &corpus, None);
    let mut rust = sandbox("v17e", rust_gateway.port);
    let mut refbox = sandbox("v17eref", ref_gateway.port);
    let shim_dir = stage_shims(&rust);
    rust.shim_dir = Some(shim_dir.clone());
    refbox.shim_dir = Some(shim_dir);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    rust.preauthorize_tools(false);
    refbox.preauthorize_tools(true);
    let stage = ref_stage(&refbox);
    ref_service_stop(&refbox, &stage);
    std::fs::write(rust.project.join("README.md"), "# t9 sandbox project\n").expect("rust README");
    std::fs::write(refbox.project.join("README.md"), "# t9 sandbox project\n").expect("ref README");
    let server = service_start(&rust);
    let _ = server.url;
    let mut rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let mut ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let roots =
        [refbox.root.to_string_lossy().into_owned(), rust.root.to_string_lossy().into_owned()];
    let root_refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let normalize = |grid: &str| normalize_grid(grid, &root_refs);

    // slow-audit fix A: the boot legs run CONCURRENTLY.
    let (rust_booted, ref_booted) = wait_both(&rust_tui, &ref_tui, WORDMARK, 0, 30, 0, 60);
    assert!(rust_booted && ref_booted, "V17E rig RED: boot rust={rust_booted} ref={ref_booted}");
    settle(2);
    rust_tui.write(b"hello\r");
    ref_tui.write(b"hello\r");
    // The tool loop settles on both ends (the shell row + the
    // confirmation needle are the rig's own health). slow-audit fix
    // A: each needle pair runs CONCURRENTLY.
    let (rust_cmd, ref_cmd) = wait_both_screens(&rust_tui, &ref_tui, "cargo test", 30, 60);
    let (rust_done, ref_done) = wait_both_screens(&rust_tui, &ref_tui, CONFIRM_NEEDLE, 40, 90);
    // Timeline duty, NOT padding: the turn-scripted loop's stream
    // tail (the confirmation row's usage half) lands after the
    // needle — the pre-click grids must carry the finished turn.
    settle(3);
    // CLICK the shell row on each end (each end's own coords — the
    // rows may sit at different lines).
    let pre_ref = normalize(&ref_tui.screen_text());
    let pre_rust = normalize(&rust_tui.screen_text());
    dump_grid("v17e-pre-ref", &pre_ref);
    dump_grid("v17e-pre-rust", &pre_rust);
    let rust_row = find_on_screen(&rust_tui, "cargo test");
    let ref_row = find_on_screen(&ref_tui, "cargo test");
    let mut clicked = false;
    if let Some((x, y)) = rust_row {
        sgr_click(&mut rust_tui, x + 2, y);
        clicked = true;
    }
    if let Some((x, y)) = ref_row {
        sgr_click(&mut ref_tui, x + 2, y);
        clicked = true;
    }
    settle(3);
    let ref_grid = normalize(&capture_settled(&ref_tui));
    let rust_grid = normalize(&capture_settled(&rust_tui));
    dump_grid("v17e-dialog-ref", &ref_grid);
    dump_grid("v17e-dialog-rust", &rust_grid);
    // Diagnostic only (W12): whether each end opened ANY dialog face.
    eprintln!(
        "V17E diagnostic: shell-row click — rust dialog face = {}, ref dialog face = {}",
        rust_grid.contains("Shell output") || rust_grid.contains("Execute"),
        ref_grid.lines().any(|l| l.contains("cargo test") && l.trim() != "cargo test")
    );
    rust_tui.shutdown(Duration::from_secs(10));
    ref_tui.shutdown(Duration::from_secs(10));
    ref_service_stop(&refbox, &stage);
    service_stop(&rust);
    drop(rust_gateway);
    drop(ref_gateway);
    drop(lane);
    assert!(
        rust_cmd && ref_cmd && rust_done && ref_done,
        "V17E rig RED: toolloop settle cmd rust={rust_cmd} ref={ref_cmd} done rust={rust_done} \
         ref={ref_done}"
    );
    assert!(clicked, "V17E rig RED: the shell row was never on-screen to click");
    export_grid_diff("v17e", "tooldialog", &ref_grid, &rust_grid);
}

// ---------------------------------------------------------------------------
// v17f — the file-mention dedup key (C-road): two same-named files in
// different directories, both @-mentioned in the composer. Ref moves
// the existing mention per uri; rust folds per name — the composer
// frame (both mentions vs one) rides the export.
// ---------------------------------------------------------------------------

#[test]
fn v17f_mention_dedup_pair() {
    skip_without_ref_lane!();
    // A custom rig: the dup-file pair must exist BEFORE boot (the
    // ref's file index builds at boot and refreshes every 10s), and
    // the ref's `client.api.file.find` backs onto ripgrep — the ref
    // server resolves `rg` from PATH (any version; the system binary
    // short-circuits the pinned download) — `ref_stage` stages the
    // host rg into the ref sandbox's staged bin dir (FIRST on the
    // ref PATH, permanent since va20 T6). Without it the ref popup
    // renders its "Could not search files" error face (va17 round 1
    // evidence).
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced("v17f", None, &default_pace());
    let rust = sandbox("v17f", gateway.port);
    let refbox = sandbox("v17fref", gateway.port);
    for project in [&rust.project, &refbox.project] {
        std::fs::create_dir_all(project.join("adir")).expect("adir");
        std::fs::create_dir_all(project.join("bdir")).expect("bdir");
        std::fs::write(project.join("adir/dup.txt"), "alpha\n").expect("adir/dup.txt");
        std::fs::write(project.join("bdir/dup.txt"), "beta\n").expect("bdir/dup.txt");
    }
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    // va20 (T6): the rg staging is now PERMANENT in `ref_stage` (all
    // lanes) — this rig only asserts the staged binary exists (the
    // @-file popup's search needs it; the "Could not search files"
    // error face was va17 round 1's evidence).
    assert!(
        stage.bun.parent().expect("bin dir").join("rg").is_file(),
        "V17F rig: the staged rg is missing (ref_stage found no host rg)"
    );
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    let mut rig = V17Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui };
    boot_both(&mut rig);
    // First mention: @ → filter "dup" → return (the first entry).
    // The ref's file search only answers once the query is non-empty
    // (bare "@" renders no rows on ref — round-2 evidence), so the
    // listing waits run AFTER the filter text lands.
    rig.write_both(b"@");
    settle(1);
    rig.write_both(b"dup");
    // slow-audit fix A: the listing legs run CONCURRENTLY.
    let (rust_listed, ref_listed) =
        wait_both_screens(&rig.rust_tui, &rig.ref_tui, "dup.txt", 15, 25);
    settle(1);
    export_state_now(&rig, "v17f", "mention-complete");
    rig.write_both(b"\r");
    settle(2);
    // Second mention: the OTHER same-named entry (down + return).
    rig.write_both(b" @dup");
    settle(2);
    rig.write_both(b"\x1b[B");
    settle(1);
    export_state_now(&rig, "v17f", "mention-second-arrow");
    rig.write_both(b"\r");
    settle(2);
    let ref_grid = rig.normalize(&capture_settled(&rig.ref_tui));
    let rust_grid = rig.normalize(&capture_settled(&rig.rust_tui));
    dump_grid("v17f-mention-ref", &ref_grid);
    dump_grid("v17f-mention-rust", &rust_grid);
    drop(rig);
    // Ref-side scenario sanity ONLY (W12): a ref whose @ completion
    // never listed the seeded pair is a rig break, never a pass.
    // (The rg staging itself is ref_stage's job since T6 — this rig
    // asserted the staged binary up front.)
    assert!(
        ref_listed,
        "V17F RED (ref-side rig sanity): the ref @ completion never listed dup.txt"
    );
    eprintln!("V17F diagnostic: the rust @ completion listed dup.txt = {rust_listed}");
    export_grid_diff("v17f", "mention-dedup", &ref_grid, &rust_grid);
}

// (make_executable was removed with T6: the rg staging moved into
// ref_stage, which owns its exec bit — no other user remained.)
