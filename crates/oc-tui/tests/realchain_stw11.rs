//! Lane stw11 — the G-596 paired anchor (the 修锚同 lane rule): the
//! multi-click window's clock domain moved from the tick-sampled
//! store clock to the DISPATCH-time clock (`App::
//! selection_dispatch_now_ms`, sampling `system_now_ms` like the
//! ref's `nextClickBehavior` reads `performance.now()` at dispatch —
//! renderer.ts:4976). The anchor face: a DOUBLE-CLICK WORD-SELECT
//! with BOTH PRESSES IN ONE BURST — the four SGR strokes
//! (press, release, press, release) written to the PTY master in a
//! single `write`, so the press-to-press dispatch delta is
//! sub-millisecond and both engines classify the pair as a
//! double-click deterministically (the window's time domain is
//! exactly what G-596 restored; a store-clock sample under load
//! could only ever misfire on the SLOW side — one burst is far
//! inside the 500 ms window on any domain).
//!
//! W12 discipline (sandbox no-judgment): the test EXPORTS the
//! captured grids + the paired diff to
//! `.test-logs/diff-reports/stw11-wordselect.{json,txt}` and makes
//! NO rust-side content assertions (rig health + ref-side sanity
//! only); judgment belongs to the controller's adjudication
//! registry. The captured face is BOTH halves of the word-select:
//! the `Copied to clipboard` toast (the release's copy-on-select)
//! AND the retained word highlight — the reply row's per-cell
//! STYLE-CLASS map (each distinct fg/bg/modifier digest one class
//! letter, first-appearance order; the G-440 selection highlight is
//! the fg/bg SWAP, not SGR 7) appended to the exported grid as
//! labeled digest lines (`style[-1]`/`style[+0]`/`style[+1]`,
//! relative to each end's OWN reply row — the two ends' wrap
//! layouts may place the row differently, the digest stays
//! comparable). The word under the press cell is the reply needle's
//! first hyphen-segment `REALCHAIN` (the word model stops at `-`:
//! `is_word_codepoint` — ASCII alnum + `_` — the same
//! `selectWord` boundary rule on both engines).
//!
//! WORKTREE BINARY (the va21e lesson, W8 escape): run the anchors
//! from a worktree with the lane's own target dir
//! (`CARGO_TARGET_DIR=<worktree>/target`) after
//! `cargo build -p oc-cli --bin opencode-rust --locked` in it —
//! without it the rig silently spawns the MAIN checkout's binary
//! (the freshness guard cannot catch a fresh main tree).

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
    default_pace, dump_grid, export_grid_diff, lane_lock, normalize_grid, ref_lane_skip_reason,
    ref_service_stop, ref_stage, ref_tui_spawn, sandbox, service_start, service_stop, Gateway,
    LaneGuard, RefStage, Sandbox, TuiProcess, WORDMARK,
};
use std::time::{Duration, Instant};

macro_rules! skip_without_ref_lane {
    () => {
        if let Some(reason) = ref_lane_skip_reason() {
            eprintln!("STW11 SKIP: {reason}");
            return;
        }
    };
}

const COLS: u16 = 120;
const ROWS: u16 = 36;
const REPLY_NEEDLE: &[u8] = b"REALCHAIN-REPLAY-OK";

struct Stw11Rig {
    _lane: LaneGuard,
    _gateway: Gateway,
    rust: Sandbox,
    refbox: Sandbox,
    stage: RefStage,
    rust_tui: TuiProcess,
    ref_tui: TuiProcess,
}

impl Stw11Rig {
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

impl Drop for Stw11Rig {
    fn drop(&mut self) {
        self.rust_tui.shutdown(Duration::from_secs(10));
        self.ref_tui.shutdown(Duration::from_secs(10));
        ref_service_stop(&self.refbox, &self.stage);
        service_stop(&self.rust);
    }
}

fn stw11_rig(tag: &str) -> Stw11Rig {
    let lane = lane_lock();
    let gateway = Gateway::spawn_paced(tag, None, &default_pace());
    let rust = sandbox(tag, gateway.port);
    let refbox = sandbox(&format!("{tag}ref"), gateway.port);
    rust.write_config(&[], "replay/claude-opus-4-7");
    refbox.write_ref_config(&[], "replay/claude-opus-4-7");
    let stage = ref_stage(&refbox);
    // Defensive pre-clean: a previous run's ref daemon on this
    // sandbox's fixed port would wedge its own `ensure`.
    ref_service_stop(&refbox, &stage);
    let server = service_start(&rust);
    let _ = server.url;
    let rust_tui = TuiProcess::spawn(&rust, COLS, ROWS);
    let ref_tui = ref_tui_spawn(&refbox, &stage, COLS, ROWS);
    Stw11Rig { _lane: lane, _gateway: gateway, rust, refbox, stage, rust_tui, ref_tui }
}

fn settle(secs: u64) {
    std::thread::sleep(Duration::from_secs(secs));
}

/// 2026-11-27 slow-audit fix A (the battery family's proven shape):
/// wait BOTH ends CONCURRENTLY — the serial two-line form paid the
/// rust wait on top of the ref's slower one.
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

impl TuiProcess {
    fn wait_for_screen(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.screen_text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

fn boot_both(rig: &mut Stw11Rig) {
    let (rust_settled, ref_settled) =
        wait_both(&rig.rust_tui, &rig.ref_tui, WORDMARK, 0, 30, 0, 60);
    assert!(
        rust_settled && ref_settled,
        "STW11 rig RED: boot rust={rust_settled} ref={ref_settled}; rust screen:\n{}\nref \
         screen:\n{}",
        rig.rust_tui.screen_text(),
        rig.ref_tui.screen_text()
    );
    // Timeline duty, NOT padding (the slowF blind gate): the first
    // PROMPT submit typed at wordmark+0 can wedge the ref app's
    // session-create path under rig concurrency — the 3 s beat (the
    // historical form, never a flake in weeks of gates) keeps this
    // anchor off that window.
    let (rust_ask, ref_ask) = std::thread::scope(|scope| {
        let rust_leg =
            scope.spawn(|| rig.rust_tui.wait_for_screen("Ask anything", Duration::from_secs(10)));
        let ref_leg =
            scope.spawn(|| rig.ref_tui.wait_for_screen("Ask anything", Duration::from_secs(15)));
        (rust_leg.join().unwrap_or(false), ref_leg.join().unwrap_or(false))
    });
    assert!(
        rust_ask && ref_ask,
        "STW11 rig RED: composer never mounted rust={rust_ask} ref={ref_ask}"
    );
    settle(3);
}

/// The needle's (col, row) on one end's CURRENT grid (each end's own
/// wrap keeps the stroke inside its own selectable transcript
/// region; SGR wire coords are 1-based).
fn find_needle(tui: &TuiProcess, needle: &str) -> Option<(usize, usize)> {
    for (row, line) in tui.screen_text().lines().enumerate() {
        if let Some(at) = line.find(needle) {
            let col = line[..at].chars().count();
            return Some((col, row));
        }
    }
    None
}

/// G-596's anchor stroke — the DOUBLE-CLICK with BOTH PRESSES IN ONE
/// BURST: press, release, press, release written in a single PTY
/// write (the press-to-press dispatch delta is sub-millisecond —
/// far inside the 500 ms multi-click window on BOTH engines, so the
/// pair classifies as a double-click deterministically on the
/// dispatch clock; press #1 is a plain cell click that resolves
/// nothing, press #2 arms the WORD selection, release #2 resolves
/// it and copy-on-select fires).
fn double_click_burst(tui: &mut TuiProcess, x: usize, y: usize) {
    let press = format!("\x1b[<0;{};{}M", x + 1, y + 1);
    let release = format!("\x1b[<0;{};{}m", x + 1, y + 1);
    let burst = format!("{press}{release}{press}{release}");
    tui.write(burst.as_bytes());
}

/// The word highlight's own face, appended to the exported grid: a
/// per-cell STYLE-CLASS map of the reply row ± 1 (each distinct
/// (fg, bg, inverse, bold, italic, underline) digest gets a class
/// letter in first-appearance order — the selection highlight is
/// the G-440 fg/bg SWAP, not SGR 7, so an inverse-only map misses
/// it; the class run over the word's cells IS the word-select span
/// — `REALCHAIN`, the needle's first hyphen-segment). Labeled
/// RELATIVE to the reply row so the two ends' digest lines stay
/// comparable whatever absolute row their wrap chose. The cell
/// attributes come from a FRESH vt100 parser fed the end's public
/// raw stream — byte-identical to the harness's own vt state (the
/// same chunks in the same order), no harness surface touched.
fn style_map(tui: &TuiProcess, reply_row: usize) -> String {
    let raw = tui.raw.lock().expect("raw").clone();
    let mut parser = vt100::Parser::new(ROWS, COLS, 0);
    parser.process(&raw);
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let rows = i64::from(rows);
    let mut out = String::new();
    for rel in [-1i64, 0, 1] {
        let row: i64 = i64::try_from(reply_row).unwrap_or(0) + rel;
        let mut line = format!("style[{rel:+}]: ");
        if row < 0 || row >= rows {
            line.push_str("<off-grid>");
        } else {
            let row = u16::try_from(row).unwrap_or(0);
            let mut classes: Vec<u64> = Vec::new();
            for col in 0..cols {
                let digest = screen.cell(row, col).map_or(0, cell_style_digest);
                let class = classes.iter().position(|&seen| seen == digest).unwrap_or_else(|| {
                    classes.push(digest);
                    classes.len() - 1
                });
                line.push(class_letter(class));
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Pack one VT cell's style into a comparable digest (the same shape
/// the vt harness's own style digest packs: fg, bg, and the
/// modifiers).
fn cell_style_digest(cell: &vt100::Cell) -> u64 {
    let pack = |color: vt100::Color| -> u64 {
        match color {
            vt100::Color::Default => 0,
            vt100::Color::Idx(index) => (1 << 8) | u64::from(index),
            vt100::Color::Rgb(r, g, b) => {
                (2 << 24) | (u64::from(r) << 16) | (u64::from(g) << 8) | u64::from(b)
            }
        }
    };
    let mods = u64::from(u8::from(cell.bold()))
        | u64::from(u8::from(cell.italic())) << 1
        | u64::from(u8::from(cell.underline())) << 2
        | u64::from(u8::from(cell.inverse())) << 3;
    (pack(cell.fgcolor()) << 30) | (pack(cell.bgcolor()) << 4) | mods
}

/// One style class as one letter (`a`..`z` then `A`..`Z`, `?`
/// beyond — a transcript row carries a handful of classes).
fn class_letter(class: usize) -> char {
    match u8::try_from(class) {
        Ok(value) if value < 26 => char::from(b'a' + value),
        Ok(value) if (26..52).contains(&value) => char::from(b'A' + (value - 26)),
        _ => '?',
    }
}

/// Wait for this end's own copy toast (the word-select release's
/// copy-on-select). TIMING ONLY — the rust side's face is never
/// judged in-test (it rides the exported diff).
fn wait_toast(tui: &TuiProcess) -> bool {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if tui.screen_text().contains("Copied to clipboard") {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Drive ONE end's double-click on its own reply anchor and return
/// (toast painted, reply row at capture).
fn word_select_until_toast(tui: &mut TuiProcess, x: usize, y: usize) -> (bool, usize) {
    double_click_burst(tui, x + 2, y);
    let toasted = wait_toast(tui);
    // The capture reads the grid DIRECTLY at the face-present moment
    // (the toast TTL is 5 s — a quiescence-settled capture outlives
    // the face; no capture_settled, hence no quiescence marks for
    // this state).
    let reply_row = find_needle(tui, "REALCHAIN-REPLAY-OK").map_or(y, |(_c, r)| r);
    (toasted, reply_row)
}

#[test]
fn stw11_doubleclick_word_select_pair() {
    skip_without_ref_lane!();
    let mut rig = stw11_rig("stw11");
    boot_both(&mut rig);
    let rust_from = rig.rust_tui.raw_len();
    let ref_from = rig.ref_tui.raw_len();
    rig.write_both(b"hello\r");
    let (rust_replied, ref_replied) =
        wait_both(&rig.rust_tui, &rig.ref_tui, REPLY_NEEDLE, rust_from, 30, ref_from, 60);
    // Each end double-clicks over ITS OWN reply row (the anchor cell
    // sits two cells inside the needle so the press lands on text,
    // not a rail — inside the word `REALCHAIN`, the first
    // hyphen-segment).
    let rust_anchor = find_needle(&rig.rust_tui, "REALCHAIN-REPLAY-OK");
    let ref_anchor = find_needle(&rig.ref_tui, "REALCHAIN-REPLAY-OK");
    let (rust_toast, rust_reply_row) =
        rust_anchor.map_or((false, 0), |(x, y)| word_select_until_toast(&mut rig.rust_tui, x, y));
    std::thread::sleep(Duration::from_millis(300));
    let rust_grid = format!(
        "{}== style map (reply row ±1) ==\n{}",
        rig.rust_tui.screen_text(),
        style_map(&rig.rust_tui, rust_reply_row)
    );
    let (ref_toast, ref_reply_row) =
        ref_anchor.map_or((false, 0), |(x, y)| word_select_until_toast(&mut rig.ref_tui, x, y));
    std::thread::sleep(Duration::from_millis(300));
    let ref_grid = format!(
        "{}== style map (reply row ±1) ==\n{}",
        rig.ref_tui.screen_text(),
        style_map(&rig.ref_tui, ref_reply_row)
    );
    // Diagnostic ONLY (no assertion — W12): whether the rust side
    // ever painted the toast (distinguishes "never copied" from
    // capture timing).
    eprintln!("STW11 diagnostic: the rust copy toast painted = {rust_toast}");
    let rust_grid = rig.normalize(&rust_grid);
    let ref_grid = rig.normalize(&ref_grid);
    dump_grid("stw11-wordselect-ref", &ref_grid);
    dump_grid("stw11-wordselect-rust", &rust_grid);
    export_grid_diff("stw11", "wordselect", &ref_grid, &rust_grid);
    drop(rig);
    assert!(
        rust_replied && ref_replied,
        "STW11 RED (rig): reply did not settle rust={rust_replied} ref={ref_replied}"
    );
    assert!(rust_anchor.is_some(), "STW11 RED (rig): the rust reply needle left the grid");
    assert!(ref_anchor.is_some(), "STW11 RED (rig): the ref reply needle left the grid");
    // REF-side scenario-content sanity ONLY (W12: the ref toast is
    // the rig's proof the burst double-clicked and the release
    // resolved the word selection over selectable text).
    assert!(ref_toast, "STW11 RED (ref-side sanity): the ref copy toast never painted");
}
