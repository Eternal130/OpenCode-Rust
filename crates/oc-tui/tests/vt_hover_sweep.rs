//! HOVER SWEEP over the cell-grid stream: for EVERY cell of the
//! settled screen (in per-band tests — see the 60-second rule below),
//! emit an SGR 1006 mouse-move there, capture the resulting grid, and
//! classify the screen's response — `NoChange` / `StyleOnly` (hover
//! highlight) / `ContentChange` (re-render/reveal/marquee). The result
//! is the map of the app's INTERACTIVE SURFACES, as the terminal
//! actually sees it.
//!
//! Today's truth this pins (beta.9 landed tab-strip hover):
//! * `HOME` has NO hover surfaces — every cell is a no-op (the mouse
//!   surface is the session route's tab strip + transcript thought
//!   headers only; `app.rs`'s `mouse_event` clears hover state off-strip
//!   without a redraw).
//! * The SESSION tab strip (row 0) responds: hovering a tab lifts its
//!   background (`StyleOnly`) AND reveals the `✕` close mark
//!   (`ContentChange`); hovering the ` + ` affordance highlights it
//!   (`StyleOnly`). Everything off the strip is a no-op.
//! * Marquee-on-truncated-title is a REGISTERED v2 GAP (the tab-pulse
//!   envelopes, marquee included, stay C2 — `tab_pulse.rs`): hovering
//!   the long truncated title must NOT shift its content window
//!   today. That expectation is pinned in `vt_temporal.rs`
//!   (`vt_temporal_marquee_shifts_one_cell_per_tick`, `#[ignore]`d)
//!   so it turns green the day the feature lands; the sweep here
//!   merely REPORTS the strip's classifications.
//!
//! Sweep coverage: 80×24 fully (three 8-row band tests); 160×50
//! strided every 2nd cell both axes (five 10-row band tests).
//!
//! ## The 60-second rule (why bands exist)
//!
//! The PTY attach dup2's the slave onto the TEST PROCESS's fd 1 —
//! which is also how the app's output reaches us — so ANYTHING the
//! libtest harness prints mid-test is TYPED INTO THE APP as input.
//! libtest prints `test … has been running for over 60 seconds` at
//! the 60 s mark (then every 60 s): a sweep longer than a minute
//! poisons its own composer draft (observed: the warning text landed
//! in the session composer mid-sweep). Every test here therefore
//! finishes well under 60 s — the full sweep is simply split into
//! row bands, one boot each. Same reason the repo's other PTY suites
//! stay under a minute.
//!
//! Anti-flake: the caret blinks (500 ms phase) while we sweep —
//! `capture_baseline` derives the VOLATILE cells from grid samples
//! spanning ≥650 ms (a 650 ms span always flips the blink phase, so
//! the caret is caught deterministically) and diffs skip them, so
//! animation can never masquerade as a hover change. Strip probes
//! that read `NoChange` get one retry (response latency is a few ms —
//! the run loop wakes on input — but a loaded runner may jitter). A
//! mid-sweep blank grid (alternate-screen teardown) aborts the sweep
//! as fatal — no cell may crash the app.

// Terminal coordinates are u16 by VT definition; harness instants are
// bounded magnitudes — the truncation lints add nothing here.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, open_second_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    FakeLlm, Grid, HarnessHandle, LiveHarness, WORDMARK,
};

/// A cell's response to being hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Response {
    NoChange,
    StyleOnly,
    ContentChange,
}

impl Response {
    fn label(self) -> &'static str {
        match self {
            Response::NoChange => "NoChange",
            Response::StyleOnly => "StyleOnly",
            Response::ContentChange => "ContentChange",
        }
    }
}

/// One swept cell's verdict.
#[derive(Debug, Clone, Copy)]
struct Probe {
    x: usize,
    y: usize,
    response: Response,
}

/// One band's sweep outcome.
#[derive(Debug, Default)]
struct SweepResult {
    probes: Vec<Probe>,
    no_change: usize,
    style_only: usize,
    content_change: usize,
    /// Fatal anomaly mid-sweep (a blank grid = teardown/crash).
    fatal: Option<String>,
    /// The first off-strip anomaly (post-strip rows only), if any.
    anomaly: Option<String>,
}

impl SweepResult {
    fn changed(&self) -> impl Iterator<Item = &Probe> {
        self.probes.iter().filter(|probe| probe.response != Response::NoChange)
    }

    fn summary(&self) -> String {
        let map: Vec<String> =
            self.changed().map(|p| format!("({},{})={}", p.x, p.y, p.response.label())).collect();
        let anomaly = self.anomaly.as_deref().map_or(String::new(), |a| format!("; ANOMALY: {a}"));
        format!(
            "{} probes: {} NoChange / {} StyleOnly / {} ContentChange; interactive map [{}]{}",
            self.probes.len(),
            self.no_change,
            self.style_only,
            self.content_change,
            map.join(","),
            anomaly,
        )
    }
}

/// Everything the assertions need after one band.
struct SweepOutcome {
    sweep: SweepResult,
    /// Baseline row-0 text (the strip).
    strip: String,
    /// Baseline volatile cells (the caret blink etc.), for the report.
    volatile: Vec<(usize, usize)>,
}

/// Hover one cell, then classify the settled grid against the
/// baseline (volatile cells skipped). Grid-based, not byte-based: the
/// run loop re-emits ratatui's hide-cursor sequence every 100 ms tick
/// regardless of hover, so byte deltas are noise; only CELL changes
/// count as a response.
fn probe_hover(
    harness: &mut HarnessHandle,
    x: u16,
    y: u16,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
) -> Response {
    const SETTLE_MS: u64 = 60;
    harness.sgr_motion(x, y);
    // The run loop wakes on input immediately (the crossterm reader is
    // a dedicated thread); a hover that changes anything has painted
    // well inside the settle window.
    std::thread::sleep(Duration::from_millis(SETTLE_MS));
    let diffs = baseline.diff(&harness.grid_now(), volatile);
    if diffs.is_empty() {
        Response::NoChange
    } else if diffs.iter().all(|diff| diff.kind == vt_harness::DiffKind::Style) {
        Response::StyleOnly
    } else {
        Response::ContentChange
    }
}

/// Sweep the strided cells of rows `[y0, y1)` of the settled screen,
/// guarding against teardown (a blank grid mid-sweep means the app
/// died). Strip-row (`y == 0`) `NoChange` probes get ONE retry (jitter
/// guard — the surface assertions live on that row).
fn sweep_band(
    harness: &mut HarnessHandle,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
    stride: usize,
    y0: usize,
    y1: usize,
) -> SweepResult {
    let mut result = SweepResult::default();
    let mut y = y0;
    while y < y1.min(baseline.rows) {
        let mut x = 0;
        while x < baseline.cols {
            let mut response = probe_hover(harness, x as u16, y as u16, baseline, volatile);
            if y == 0 && response == Response::NoChange && x % stride == 0 {
                response = probe_hover(harness, x as u16, y as u16, baseline, volatile);
            }
            let grid = harness.grid_now();
            if grid.is_blank() {
                result.fatal = Some(format!(
                    "the screen went blank after hovering ({x},{y}) — the app tore down mid-sweep"
                ));
                return result;
            }
            if y > 0 && response != Response::NoChange && result.anomaly.is_none() {
                let diffs = baseline.diff(&grid, volatile);
                let cells: Vec<(usize, usize)> = diffs.iter().map(|d| (d.x, d.y)).take(8).collect();
                result.anomaly = Some(format!(
                    "{} at ({x},{y}) diffed cells {:?} row {y} now {:?} base {:?}",
                    response.label(),
                    cells,
                    grid.row_text(y.min(grid.rows.saturating_sub(1))),
                    baseline.row_text(y),
                ));
            }
            match response {
                Response::NoChange => result.no_change += 1,
                Response::StyleOnly => result.style_only += 1,
                Response::ContentChange => result.content_change += 1,
            }
            result.probes.push(Probe { x, y, response });
            x += stride;
        }
        y += stride;
    }
    result
}

/// Boot choreography shared by the session-view tests: settle HOME,
/// open BOTH history sessions as tabs (short + long-truncating
/// title), settle the 2-tab session view.
fn boot_to_two_tabs(harness: &mut HarnessHandle) -> Result<(), String> {
    if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(harness, b"Alpha")?;
    if !harness.wait_for(b"Alpha", Duration::from_secs(5)) {
        return Err("the first tab's title never painted".to_owned());
    }
    std::thread::sleep(Duration::from_millis(400));
    open_second_session(harness, b"Long Session Title")?;
    // The long title truncates in the strip — match on its head.
    if !harness.wait_for(b"Long Session", Duration::from_secs(5)) {
        return Err("the second (long-titled) tab never painted".to_owned());
    }
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

/// The shared runner: sandbox + `FakeLlm` + PTY + a driver thread that
/// performs `drive` (boot → settle → sweep one band) and always leaves
/// the exit chord, while the test body runs the real TUI loop; then
/// asserts the app was alive through the whole sweep (the teardown
/// sequence arrived after the sweep finished).
fn run_sweep_test(
    tag: &str,
    cols: u16,
    rows: u16,
    drive: impl FnOnce(&mut HarnessHandle) -> Result<SweepOutcome, String> + Send + 'static,
) -> SweepOutcome {
    let _lock = pty_lock();
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(cols, rows);
    let result_slot: Arc<Mutex<Option<Result<SweepOutcome, String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let ack_from = harness.handle().raw_len();
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        // ALWAYS leave the exit chord so the run loop terminates even
        // on driver error (the ack is verified below).
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, cols, rows, Duration::from_secs(120));
    fake.stop();
    // The driver writes its result AFTER the exit chord — the app may
    // tear down (and unblock run_tui) a beat before the slot lands.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let swept = match result_slot.lock().expect("slot").take() {
        Some(Ok(outcome)) => outcome,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => panic!("the driver thread never reported (deadlock?)"),
    };
    // The app must have been alive through the whole sweep: the exit
    // chord's teardown arrived AFTER the sweep finished.
    assert!(
        harness.handle().wait_for_after(
            vt_harness::ALT_SCREEN_LEAVE,
            ack_from,
            Duration::from_secs(5)
        ),
        "the exit teardown never arrived — the app died before the sweep completed: {}",
        swept.sweep.summary()
    );
    assert!(
        swept.sweep.fatal.is_none(),
        "fatal mid-sweep: {:?} — {}",
        swept.sweep.fatal,
        swept.sweep.summary()
    );
    swept
}

// ---------------------------------------------------------------------------
// HOME view: no hover surfaces today
// ---------------------------------------------------------------------------

/// One HOME band: boot, settle, sweep rows `[y0, y1)` at `stride`.
fn home_band(y0: usize, y1: usize) -> SweepOutcome {
    let tag = format!("hover-home-{y0}-{y1}");
    let outcome = run_sweep_test(&tag, 80, 24, move |harness| {
        if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
            return Err("the home wordmark never rendered".to_owned());
        }
        std::thread::sleep(Duration::from_millis(600));
        let (baseline, volatile) = harness.capture_baseline();
        if baseline.is_blank() {
            return Err("the settled home screen is blank".to_owned());
        }
        let result = sweep_band(harness, &baseline, &volatile, 1, y0, y1);
        Ok(SweepOutcome {
            sweep: result,
            strip: baseline.row_text(0),
            volatile: volatile.iter().copied().collect(),
        })
    });
    eprintln!(
        "[vt_hover home rows {y0}-{y1}] {} (volatile: {:?})",
        outcome.sweep.summary(),
        outcome.volatile
    );
    // TODAY'S TRUTH: HOME has no hover surfaces — the mouse surface is
    // session-route-only, so every cell classifies NoChange.
    assert_eq!(
        outcome.sweep.style_only,
        0,
        "home hover must not restyle anything (no surfaces yet): {}",
        outcome.sweep.summary()
    );
    assert_eq!(
        outcome.sweep.content_change,
        0,
        "home hover must not re-render content: {}",
        outcome.sweep.summary()
    );
    // The volatile set is just the caret blink (one cell, style-only).
    assert!(
        outcome.volatile.len() <= 2,
        "more volatile cells than the caret blink on idle home: {:?}",
        outcome.volatile
    );
    outcome
}

#[test]
fn vt_hover_home_rows_0_7() {
    home_band(0, 8);
}

#[test]
fn vt_hover_home_rows_8_15() {
    home_band(8, 16);
}

#[test]
fn vt_hover_home_rows_16_23() {
    home_band(16, 24);
}

// ---------------------------------------------------------------------------
// SESSION view (2 tabs): the strip is the interactive surface
// ---------------------------------------------------------------------------

/// Assertions for the band containing the strip (row 0).
fn assert_strip_surfaces(outcome: &SweepOutcome, label: &str) {
    let sweep = &outcome.sweep;
    let changed: Vec<Probe> = sweep.changed().copied().collect();
    // Row 0 is the strip: the band's changed cells must ALL live in
    // row 0 (no other surface responds today).
    assert!(
        changed.iter().all(|probe| probe.y == 0),
        "{label}: interactive cells outside the strip: {}",
        sweep.summary()
    );
    assert!(!changed.is_empty(), "{label}: the strip never responded: {}", sweep.summary());

    let strip = &outcome.strip;
    // The response verdict nearest column `x` (stride-2 sweeps probe
    // every other column; surfaces span >= 3 columns, so the nearest
    // probed verdict is faithful).
    let response_at = |x: usize| -> Response {
        let mut best: Option<(usize, Response)> = None;
        for probe in &sweep.probes {
            if probe.y != 0 {
                continue;
            }
            let distance = probe.x.abs_diff(x);
            if best.as_ref().is_none_or(|(d, _)| distance < *d) {
                best = Some((distance, probe.response));
            }
        }
        best.map_or(Response::NoChange, |(_, response)| response)
    };
    // The add affordance: the `+` column must respond — StyleOnly
    // (the affordance highlights; nothing re-renders).
    let plus_cols: Vec<usize> =
        strip.chars().enumerate().filter(|(_, ch)| *ch == '+').map(|(x, _)| x).collect();
    assert!(
        plus_cols.iter().any(|&x| response_at(x) == Response::StyleOnly),
        "{label}: the '+' affordance did not classify StyleOnly (strip: {strip:?}): {}",
        sweep.summary()
    );
    // Tab titles: the majority of the strip's letter columns must
    // respond (hover lift + `✕` reveal).
    let letter_cols: Vec<usize> =
        strip.chars().enumerate().filter(|(_, ch)| ch.is_alphabetic()).map(|(x, _)| x).collect();
    let responding = letter_cols.iter().filter(|&&x| response_at(x) != Response::NoChange).count();
    assert!(
        !letter_cols.is_empty() && responding >= letter_cols.len() / 2,
        "{label}: fewer than half the title columns responded (strip: {strip:?}): {}",
        sweep.summary()
    );
    // The `✕` reveal: hovering a tab introduces NEW CHARACTERS (the
    // close mark) — at least one strip cell classifies ContentChange.
    assert!(
        changed.iter().any(|probe| probe.response == Response::ContentChange),
        "{label}: no ContentChange on the strip — the ✕ reveal is missing: {}",
        sweep.summary()
    );
}

/// One SESSION band at 80×24 (stride 1): boot to the 2-tab view,
/// settle, sweep rows `[y0, y1)`.
fn session_band(y0: usize, y1: usize) -> SweepOutcome {
    let tag = format!("hover-sess-{y0}-{y1}");
    let outcome = run_sweep_test(&tag, 80, 24, move |harness| {
        boot_to_two_tabs(harness)?;
        let (baseline, volatile) = harness.capture_baseline();
        if baseline.is_blank() {
            return Err("the settled session screen is blank".to_owned());
        }
        let strip = baseline.row_text(0);
        if !strip.contains('+') {
            return Err(format!("the strip never painted its affordance: {strip:?}"));
        }
        let result = sweep_band(harness, &baseline, &volatile, 1, y0, y1);
        Ok(SweepOutcome { sweep: result, strip, volatile: volatile.iter().copied().collect() })
    });
    eprintln!(
        "[vt_hover session 80x24 rows {y0}-{y1}] {} (volatile: {:?})",
        outcome.sweep.summary(),
        outcome.volatile
    );
    outcome
}

/// One SESSION band at 160×50, strided every 2nd cell both axes.
fn session_band_160(y0: usize, y1: usize) -> SweepOutcome {
    let tag = format!("hover-sess-160-{y0}-{y1}");
    let outcome = run_sweep_test(&tag, 160, 50, move |harness| {
        boot_to_two_tabs(harness)?;
        let (baseline, volatile) = harness.capture_baseline();
        if baseline.is_blank() {
            return Err("the settled session screen is blank".to_owned());
        }
        let strip = baseline.row_text(0);
        if !strip.contains('+') {
            return Err(format!("the strip never painted its affordance at 160x50: {strip:?}"));
        }
        // Strided subset (every 2nd cell both axes) for speed — still
        // hits the strip's marks at 2-cell resolution.
        let result = sweep_band(harness, &baseline, &volatile, 2, y0, y1);
        Ok(SweepOutcome { sweep: result, strip, volatile: volatile.iter().copied().collect() })
    });
    eprintln!(
        "[vt_hover session 160x50 strided rows {y0}-{y1}] {} (volatile: {:?})",
        outcome.sweep.summary(),
        outcome.volatile
    );
    outcome
}

/// Off-strip bands assert TODAY'S TRUTH: nothing outside row 0
/// responds (the anomaly guard reports anything that does).
fn assert_off_strip_clean(outcome: &SweepOutcome, label: &str) {
    assert!(
        outcome.sweep.changed().next().is_none(),
        "{label}: off-strip cells responded: {}",
        outcome.sweep.summary()
    );
}

#[test]
fn vt_hover_session_rows_0_7() {
    let outcome = session_band(0, 8);
    assert_strip_surfaces(&outcome, "session 80x24 rows 0-7");
}

#[test]
fn vt_hover_session_rows_8_15() {
    let outcome = session_band(8, 16);
    assert_off_strip_clean(&outcome, "session 80x24 rows 8-15");
}

#[test]
fn vt_hover_session_rows_16_23() {
    let outcome = session_band(16, 24);
    assert_off_strip_clean(&outcome, "session 80x24 rows 16-23");
}

#[test]
fn vt_hover_session_160_rows_0_9() {
    let outcome = session_band_160(0, 10);
    assert_strip_surfaces(&outcome, "session 160x50 strided rows 0-9");
}

#[test]
fn vt_hover_session_160_rows_10_19() {
    let outcome = session_band_160(10, 20);
    assert_off_strip_clean(&outcome, "session 160x50 strided rows 10-19");
}

#[test]
fn vt_hover_session_160_rows_20_29() {
    let outcome = session_band_160(20, 30);
    assert_off_strip_clean(&outcome, "session 160x50 strided rows 20-29");
}

#[test]
fn vt_hover_session_160_rows_30_39() {
    let outcome = session_band_160(30, 40);
    assert_off_strip_clean(&outcome, "session 160x50 strided rows 30-39");
}

#[test]
fn vt_hover_session_160_rows_40_49() {
    let outcome = session_band_160(40, 50);
    assert_off_strip_clean(&outcome, "session 160x50 strided rows 40-49");
}
