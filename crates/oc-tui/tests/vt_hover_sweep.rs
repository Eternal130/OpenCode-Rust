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

// ---- va10 wait-mechanism constants ------------------------------------
//
// Derived from `vt_hover_paint_latency_harness` (the #[ignore]d
// measurement probe at the bottom of this file) under a quiet box:
//
//   n=240 median=10ms p90=11ms p99=11ms max=12ms
//
// The effective NoChange attribution window is
// RESPONSE_WINDOW + LATE_WATCH = 36 ms ≥ 3× median (10) and 3× the
// measured tail (12) — a responding surface is never truncated by the
// window, and a diff observed late is still attributed to its own
// probe (see `probe_hover`). Stability/clear spans cover one paint's
// span-group settling plus the frame-queue drain of a later motion.
// Re-run the harness to re-derive.

/// Poll granularity while waiting for a hover paint (and between
/// stability checks).
const HOVER_POLL_MS: u64 = 2;
/// Max wait for the FIRST baseline diff after the probe motion.
const HOVER_RESPONSE_WINDOW_MS: u64 = 30;
/// Quiet span a diff signature must hold before it is classified.
const HOVER_STABLE_MS: u64 = 12;
/// Quiet span confirming the return-to-baseline after the neutral
/// corner motion (for probes whose paint was already observed).
const HOVER_CLEAR_QUIET_MS: u64 = 6;
/// Extra late-paint watch after the neutral corner motion for probes
/// whose observation window closed with NO diff — a paint arriving
/// here is still attributed to this probe (never the next one).
const HOVER_LATE_WATCH_MS: u64 = 6;

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
    /// Targeted probes at the `+` glyph column and its two padding
    /// neighbors (stride-2 sweeps can miss the glyph column; the
    /// glyph-tight hit-zone rule — corpus2 #28 — pins the exact rect:
    /// the GLYPH highlights, the padding cells are no-ops).
    plus_probes: Vec<(usize, Response)>,
}

/// Hover-paint latencies observed by [`probe_hover`] (ms from the
/// probe motion to the FIRST baseline diff) — the va10 measurement
/// sink. Every band test prints the running distribution; the
/// `#[ignore]`d [`vt_hover_paint_latency_harness`] below samples it
/// densely for re-deriving the wait constants.
static HOVER_LATENCIES: Mutex<Vec<u128>> = Mutex::new(Vec::new());

fn note_latency(elapsed: Duration) {
    if let Ok(mut sink) = HOVER_LATENCIES.lock() {
        sink.push(elapsed.as_millis());
    }
}

/// Median / p90 / p99 / max of the recorded paint latencies.
fn latency_report() -> String {
    let mut samples = HOVER_LATENCIES.lock().expect("latency sink").clone();
    if samples.is_empty() {
        return "n=0".to_owned();
    }
    samples.sort_unstable();
    #[allow(clippy::cast_precision_loss)] // harness instants are bounded magnitudes
    let pick = |q: f64| samples[((samples.len() as f64 - 1.0) * q).floor() as usize];
    format!(
        "n={} median={}ms p90={}ms p99={}ms max={}ms",
        samples.len(),
        pick(0.5),
        pick(0.9),
        pick(0.99),
        samples[samples.len() - 1]
    )
}

// ---- slowD lane instrumentation: per-phase probe budget -------------------
//
// Per-PROCESS counters (nextest = one process per test, so these are
// per-test): where a probe's wall goes — phase 1 (the response
// window: motion → first diff or window close) vs phase 2 (the
// neutral-corner clear + its watch), split by verdict. The sweep's
// budget model (validated against the solo baseline 2026-11-28):
// an inert probe ≈ RESPONSE_WINDOW + LATE_WATCH + poll overshoot
// ≈ 37 ms, an observed probe ≈ paint latency + STABLE + CLEAR_QUIET.
static PROBE_STATS: Mutex<ProbeStats> = Mutex::new(ProbeStats::EMPTY);

#[derive(Default)]
struct ProbeStats {
    probes: u64,
    probe_ms: u128,
    inert: u64,
    inert_ms: u128,
    phase1_ms: u128,
    phase2_ms: u128,
    observed: u64,
    observed_ms: u128,
    retries: u64,
}

impl ProbeStats {
    /// A const initializer (statics cannot call `Default::default`).
    const EMPTY: Self = Self {
        probes: 0,
        probe_ms: 0,
        inert: 0,
        inert_ms: 0,
        phase1_ms: 0,
        phase2_ms: 0,
        observed: 0,
        observed_ms: 0,
        retries: 0,
    };
}

/// Record one probe's phase split (slowD instrumentation).
fn note_probe(inert: bool, phase1: Duration, phase2: Duration) {
    let mut sink = PROBE_STATS.lock().expect("probe stats");
    sink.probes += 1;
    let total = phase1.as_millis() + phase2.as_millis();
    sink.probe_ms += total;
    sink.phase1_ms += phase1.as_millis();
    sink.phase2_ms += phase2.as_millis();
    if inert {
        sink.inert += 1;
        sink.inert_ms += total;
    } else {
        sink.observed += 1;
        sink.observed_ms += total;
    }
}

/// One line: the per-test probe budget (mean ms per class).
fn probe_stats_line() -> String {
    let sink = PROBE_STATS.lock().expect("probe stats");
    let mean = |n: u64, ms: u128| {
        if n == 0 {
            "n=0".to_owned()
        } else {
            #[allow(clippy::cast_precision_loss)] // bounded magnitudes
            let mean_ms = (ms / u128::from(n.max(1))) as f64;
            format!("n={n} mu={mean_ms:.1}ms")
        }
    };
    format!(
        "probes={} total={}ms | inert {} (p1={}ms p2={}ms) | obs {} | retries={}",
        sink.probes,
        sink.probe_ms,
        mean(sink.inert, sink.inert_ms),
        sink.phase1_ms,
        sink.phase2_ms,
        mean(sink.observed, sink.observed_ms),
        sink.retries,
    )
}

/// The neutral clear cell: the trailing end of the PROBE'S OWN row —
/// a no-op on every swept view (strip trailing cells past the '+'
/// affordance and all off-strip cells are inert), and it keeps the
/// pointer inside the same mouse region (an off-strip corner motion
/// turned out to leave the app's mouse handling in a state where a
/// subsequent strip hover never repaints — va10 experiment, see the
/// plus-probe debug trace).
fn clear_cell(baseline: &Grid, y: u16) -> (u16, u16) {
    ((baseline.cols.max(1) - 1) as u16, y)
}

fn classify_diffs(diffs: &[vt_harness::CellDiff]) -> Response {
    if diffs.is_empty() {
        Response::NoChange
    } else if diffs.iter().all(|diff| diff.kind == vt_harness::DiffKind::Style) {
        Response::StyleOnly
    } else {
        Response::ContentChange
    }
}

/// Poll until the (volatile-skipped) diff signature vs `baseline`
/// holds UNCHANGED for `stable_ms`, then classify it; gives up at
/// `deadline` with the last signature's classification. The digest
/// fast path keeps "unchanged" polls O(1) — no grid clone per poll.
fn settle_classify(
    harness: &HarnessHandle,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
    baseline_digest: u64,
    stable_ms: u128,
    deadline: std::time::Instant,
) -> Response {
    let mut last: Option<(String, Response)> = None;
    let mut held_since = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(HOVER_POLL_MS));
        let response = if harness.grid_digest() == baseline_digest {
            Response::NoChange
        } else {
            let diffs = baseline.diff(&harness.grid_now(), volatile);
            classify_diffs(&diffs)
        };
        let sig = if response == Response::NoChange {
            String::new()
        } else {
            format!("{:?}", baseline.diff(&harness.grid_now(), volatile))
        };
        let now = std::time::Instant::now();
        if last.as_ref().is_some_and(|(prev, _)| *prev == sig) {
            if now.duration_since(held_since).as_millis() >= stable_ms {
                return response;
            }
        } else {
            held_since = now;
        }
        last = Some((sig, response));
        if now >= deadline {
            return last.map_or(Response::NoChange, |(_, response)| response);
        }
    }
}

/// Hover one cell, then classify the settled grid against the
/// baseline (volatile cells skipped). Grid-based, not byte-based: the
/// run loop re-emits the writer's hide-cursor sequence every 100 ms tick
/// regardless of hover, so byte deltas are noise; only CELL changes
/// count as a response.
///
/// ## The va10 wait mechanism (event-driven, replacing the fixed 60 ms
/// sleep)
///
/// Phase 1 sends the probe motion, then POLLS the grid
/// (`HOVER_POLL_MS`): a responding surface paints within a few ms
/// (the run loop wakes on input) and the FIRST diff's latency is
/// recorded in [`HOVER_LATENCIES`]; classification then waits for the
/// diff signature to hold [`HOVER_STABLE_MS`] so one paint's spans
/// settle together. Only a cell still at baseline after
/// `HOVER_RESPONSE_WINDOW_MS` is a `NoChange` candidate — the window
/// covers the measured tail paint latency (≥3× median), so a slow
/// surface is never truncated.
///
/// Phase 2 moves to the neutral corner and waits back to baseline:
/// any diff appearing there is a LATE paint of THIS probe (the corner
/// is a no-op on every swept view) and is attributed to it, so a
/// barely-missed slow surface can neither poison the next probe nor
/// shift the interactive map. Assertion semantics are unchanged —
/// only the waiting mechanism was rebuilt.
fn probe_hover(
    harness: &mut HarnessHandle,
    x: u16,
    y: u16,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
    baseline_digest: u64,
) -> Response {
    let sent = std::time::Instant::now();
    harness.sgr_motion(x, y);
    let response_window = sent + Duration::from_millis(HOVER_RESPONSE_WINDOW_MS);
    let mut observed: Option<Response> = None;
    while std::time::Instant::now() < response_window {
        std::thread::sleep(Duration::from_millis(HOVER_POLL_MS));
        // Fast path: unchanged screen (digest; O(1), no clone) → keep
        // polling without touching the parser grid.
        if harness.grid_digest() == baseline_digest {
            continue;
        }
        let diffs = baseline.diff(&harness.grid_now(), volatile);
        if diffs.is_empty() {
            continue;
        }
        note_latency(sent.elapsed());
        observed = Some(settle_classify(
            harness,
            baseline,
            volatile,
            baseline_digest,
            u128::from(HOVER_STABLE_MS),
            sent + Duration::from_millis(HOVER_RESPONSE_WINDOW_MS + HOVER_STABLE_MS),
        ));
        break;
    }
    let phase1_done = std::time::Instant::now();
    // Phase 2: neutral corner, back to the known baseline. An
    // observed probe only needs one quiet baseline span; an
    // unobserved one keeps watching for a late paint for
    // `HOVER_LATE_WATCH_MS`.
    let clear = clear_cell(baseline, y);
    harness.sgr_motion(clear.0, clear.1);
    let watch_ms =
        if observed.is_some() { HOVER_CLEAR_QUIET_MS + HOVER_POLL_MS } else { HOVER_LATE_WATCH_MS };
    let clear_deadline = std::time::Instant::now() + Duration::from_millis(watch_ms);
    let mut baseline_since: Option<std::time::Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(HOVER_POLL_MS));
        let now = std::time::Instant::now();
        let diffs = if harness.grid_digest() == baseline_digest {
            Vec::new()
        } else {
            baseline.diff(&harness.grid_now(), volatile)
        };
        if diffs.is_empty() {
            // For an observed probe the baseline only needs to hold
            // one quiet span; an unobserved probe keeps watching for
            // its late paint until the window closes.
            if observed.is_some()
                && baseline_since.is_some_and(|since| {
                    now.duration_since(since).as_millis() >= u128::from(HOVER_CLEAR_QUIET_MS)
                })
            {
                break;
            }
            if observed.is_none() {
                baseline_since.get_or_insert(now);
            }
        } else {
            baseline_since = None;
            if observed.is_none() {
                note_latency(now.duration_since(sent));
                observed = Some(settle_classify(
                    harness,
                    baseline,
                    volatile,
                    baseline_digest,
                    u128::from(HOVER_STABLE_MS),
                    clear_deadline,
                ));
            }
        }
        if now >= clear_deadline {
            break;
        }
    }
    let verdict = observed.unwrap_or(Response::NoChange);
    note_probe(verdict == Response::NoChange, phase1_done.duration_since(sent), phase1_done.elapsed());
    verdict
}

/// Targeted probes of the `" + "` affordance's EXACT columns: the
/// glyph and its two padding neighbors (corpus2 #28 — the hit zone is
/// glyph-tight; the padding cells must be no-ops). Stride-2 sweeps can
/// never observe this rect directly.
fn probe_plus_rect(
    harness: &mut HarnessHandle,
    strip: &str,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
    baseline_digest: u64,
) -> Vec<(usize, Response)> {
    let plus = strip.chars().position(|ch| ch == '+').expect("the affordance glyph");
    [plus.saturating_sub(1), plus, plus + 1]
        .into_iter()
        .map(|x| (x, probe_hover(harness, x as u16, 0, baseline, volatile, baseline_digest)))
        .collect()
}

/// Sweep the strided cells of rows `[y0, y1)` of the settled screen,
/// guarding against teardown (a blank grid mid-sweep means the app
/// died). Strip-row (`y == 0`) `NoChange` probes get ONE retry (jitter
/// guard — the surface assertions live on that row).
fn sweep_band(
    harness: &mut HarnessHandle,
    baseline: &Grid,
    volatile: &HashSet<(usize, usize)>,
    baseline_digest: u64,
    stride: usize,
    y0: usize,
    y1: usize,
) -> SweepResult {
    let mut result = SweepResult::default();
    let mut y = y0;
    while y < y1.min(baseline.rows) {
        let mut x = 0;
        while x < baseline.cols {
            let mut response =
                probe_hover(harness, x as u16, y as u16, baseline, volatile, baseline_digest);
            if y == 0 && response == Response::NoChange && x % stride == 0 {
                PROBE_STATS.lock().expect("probe stats").retries += 1;
                response =
                    probe_hover(harness, x as u16, y as u16, baseline, volatile, baseline_digest);
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
    let mark = |h: &HarnessHandle, label: &str| {
        eprintln!("[slowD mark {label}] {}ms", h.now_ms());
    };
    if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    mark(harness, "wordmark");
    open_first_session(harness, b"Alpha")?;
    if !harness.wait_for(b"Alpha", Duration::from_secs(5)) {
        return Err("the first tab's title never painted".to_owned());
    }
    mark(harness, "tab1");
    std::thread::sleep(Duration::from_millis(400));
    open_second_session(harness, b"Long Session Title")?;
    // The long title truncates in the strip — match on its head.
    if !harness.wait_for(b"Long Session", Duration::from_secs(5)) {
        return Err("the second (long-titled) tab never painted".to_owned());
    }
    mark(harness, "tab2");
    std::thread::sleep(Duration::from_millis(500));
    mark(harness, "settle");
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
    // slowD instrumentation: the slot carries the driver's phase split
    // (drive wall, exit-chord wall) alongside the outcome.
    let result_slot: Arc<Mutex<Option<(Result<SweepOutcome, String>, Duration, Duration)>>> =
        Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let ack_from = harness.handle().raw_len();
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let drive_start = std::time::Instant::now();
        let outcome = drive(&mut driver);
        let drive_wall = drive_start.elapsed();
        // ALWAYS leave the exit chord so the run loop terminates even
        // on driver error (the ack is verified below).
        let chord_start = std::time::Instant::now();
        vt_harness::exit_chord(&mut driver);
        let chord_wall = chord_start.elapsed();
        *slot.lock().expect("slot") = Some((outcome, drive_wall, chord_wall));
    });
    let run_tui_start = std::time::Instant::now();
    run_tui(&fake.url, cols, rows, Duration::from_secs(120));
    let run_tui_wall = run_tui_start.elapsed();
    fake.stop();
    // The driver writes its result AFTER the exit chord — the app may
    // tear down (and unblock run_tui) a beat before the slot lands.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (swept, drive_wall, chord_wall) = match result_slot.lock().expect("slot").take() {
        Some((Ok(outcome), drive_wall, chord_wall)) => (outcome, drive_wall, chord_wall),
        Some((Err(error), _, _)) => panic!("driver failed: {error}"),
        None => panic!("the driver thread never reported (deadlock?)"),
    };
    let ack_start = std::time::Instant::now();
    let acked = harness.handle().wait_for_after(
        vt_harness::ALT_SCREEN_LEAVE,
        ack_from,
        Duration::from_secs(5),
    );
    let ack_wall = ack_start.elapsed();
    eprintln!(
        "[slowD budget {tag}] drive={}ms (chord={}ms) run_tui={}ms ack={}ms | {}",
        drive_wall.as_millis(),
        chord_wall.as_millis(),
        run_tui_wall.as_millis(),
        ack_wall.as_millis(),
        probe_stats_line(),
    );
    // The app must have been alive through the whole sweep: the exit
    // chord's teardown arrived AFTER the sweep finished.
    assert!(
        acked,
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
        // Lane/chrome: settle on true GRID stability — boot-time async
        // surfaces (the onboarding card's dismissal, the footer
        // location's vcs round-trip) may land after the wordmark; a
        // fixed sleep raced them.
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut previous = harness.grid_now();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let current = harness.grid_now();
            if current == previous || std::time::Instant::now() > deadline {
                break;
            }
            previous = current;
        }
        std::thread::sleep(Duration::from_millis(200));
        let (baseline, volatile) = harness.capture_baseline();
        eprintln!("[slowD mark baseline] {}ms", harness.now_ms());
        let baseline_digest = harness.grid_digest();
        if baseline.is_blank() {
            return Err("the settled home screen is blank".to_owned());
        }
        let result = sweep_band(harness, &baseline, &volatile, baseline_digest, 1, y0, y1);
        Ok(SweepOutcome {
            sweep: result,
            strip: baseline.row_text(0),
            volatile: volatile.iter().copied().collect(),
            plus_probes: Vec::new(),
        })
    });
    eprintln!(
        "[vt_hover home rows {y0}-{y1}] {} (volatile: {:?}; paint latency: {})",
        outcome.sweep.summary(),
        outcome.volatile,
        latency_report()
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
    // The add affordance: the GLYPH column must respond — StyleOnly
    // (the affordance highlights; nothing re-renders) — and its two
    // padding cells must be NO-OPS (glyph-tight hit zone, corpus2
    // #28). Targeted probes: stride-2 sweeps can miss the glyph.
    if outcome.plus_probes.is_empty() {
        let plus_cols: Vec<usize> =
            strip.chars().enumerate().filter(|(_, ch)| *ch == '+').map(|(x, _)| x).collect();
        assert!(
            plus_cols.iter().any(|&x| response_at(x) == Response::StyleOnly),
            "{label}: the '+' affordance did not classify StyleOnly (strip: {strip:?}): {}",
            sweep.summary()
        );
    } else {
        let plus = outcome.strip.chars().position(|ch| ch == '+').expect("the glyph");
        for &(x, response) in &outcome.plus_probes {
            if x == plus {
                assert_eq!(
                    response,
                    Response::StyleOnly,
                    "{label}: the '+' GLYPH must highlight (strip: {:?}): {}",
                    outcome.strip,
                    sweep.summary()
                );
            } else {
                assert_eq!(
                    response,
                    Response::NoChange,
                    "{label}: the '+' padding cell at col {x} must be a no-op (glyph-tight, corpus2 #28; strip: {:?}): {}",
                    outcome.strip,
                    sweep.summary()
                );
            }
        }
    }
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
        eprintln!("[slowD mark baseline] {}ms", harness.now_ms());
        let baseline_digest = harness.grid_digest();
        if baseline.is_blank() {
            return Err("the settled session screen is blank".to_owned());
        }
        let strip = baseline.row_text(0);
        if !strip.contains('+') {
            return Err(format!("the strip never painted its affordance: {strip:?}"));
        }
        let result = sweep_band(harness, &baseline, &volatile, baseline_digest, 1, y0, y1);
        let plus_probes = probe_plus_rect(harness, &strip, &baseline, &volatile, baseline_digest);
        Ok(SweepOutcome {
            sweep: result,
            strip,
            volatile: volatile.iter().copied().collect(),
            plus_probes,
        })
    });
    eprintln!(
        "[vt_hover session 80x24 rows {y0}-{y1}] {} (volatile: {:?}; paint latency: {})",
        outcome.sweep.summary(),
        outcome.volatile,
        latency_report()
    );
    outcome
}

/// One SESSION band at 160×50, strided every 2nd cell both axes.
fn session_band_160(y0: usize, y1: usize) -> SweepOutcome {
    let tag = format!("hover-sess-160-{y0}-{y1}");
    let outcome = run_sweep_test(&tag, 160, 50, move |harness| {
        boot_to_two_tabs(harness)?;
        let (baseline, volatile) = harness.capture_baseline();
        eprintln!("[slowD mark baseline] {}ms", harness.now_ms());
        let baseline_digest = harness.grid_digest();
        if baseline.is_blank() {
            return Err("the settled session screen is blank".to_owned());
        }
        let strip = baseline.row_text(0);
        if !strip.contains('+') {
            return Err(format!("the strip never painted its affordance at 160x50: {strip:?}"));
        }
        // Strided subset (every 2nd cell both axes) for speed — still
        // hits the strip's marks at 2-cell resolution.
        let result = sweep_band(harness, &baseline, &volatile, baseline_digest, 2, y0, y1);
        let plus_probes = probe_plus_rect(harness, &strip, &baseline, &volatile, baseline_digest);
        Ok(SweepOutcome {
            sweep: result,
            strip,
            volatile: volatile.iter().copied().collect(),
            plus_probes,
        })
    });
    eprintln!(
        "[vt_hover session 160x50 strided rows {y0}-{y1}] {} (volatile: {:?}; paint latency: {})",
        outcome.sweep.summary(),
        outcome.volatile,
        latency_report()
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

// ---------------------------------------------------------------------------
// va10 measurement harness: hover paint-latency distribution
// ---------------------------------------------------------------------------

/// Dense sampling of the hover PAINT latency (probe motion → first
/// baseline diff) under `probe_hover`'s exact semantics: boot to the
/// 2-tab session view at 80×24, then sweep every strip letter/glyph
/// column (the responding surface) plus a handful of no-op cells for
/// `ROUNDS` rounds, printing the median/p90/p99/max distribution. This
/// test asserts NOTHING about the interactive map (the sweep tests
/// own those); it is the measurement probe the wait constants
/// (`HOVER_*`) are derived from — `#[ignore]`d so the canonical suites
/// never pay its wall; run it explicitly when re-deriving:
/// `cargo nextest run -p oc-tui -E 'test(vt_hover_paint_latency_harness)'`.
#[test]
#[ignore = "va10 measurement harness — run explicitly to re-derive the hover wait constants"]
fn vt_hover_paint_latency_harness() {
    const ROUNDS: usize = 8;
    let outcome = run_sweep_test("hover-latency-harness", 80, 24, move |harness| {
        boot_to_two_tabs(harness)?;
        let (baseline, volatile) = harness.capture_baseline();
        let baseline_digest = harness.grid_digest();
        if baseline.is_blank() {
            return Err("the settled session screen is blank".to_owned());
        }
        let strip = baseline.row_text(0);
        if !strip.contains('+') {
            return Err(format!("the strip never painted its affordance: {strip:?}"));
        }
        // Every column that should respond (tab letters + the '+'
        // glyph) and a few that must not (off-strip rows, the neutral
        // corner) — both classes exercise the wait mechanism.
        let mut positions: Vec<(u16, u16)> = strip
            .chars()
            .enumerate()
            .filter(|(_, ch)| ch.is_alphabetic() || *ch == '+')
            .map(|(x, _)| (x as u16, 0))
            .collect();
        positions.extend([
            (10, 6),
            (40, 12),
            (70, 18),
            (baseline.cols.max(1) as u16 - 1, baseline.rows.max(1) as u16 - 1),
        ]);
        for round in 0..ROUNDS {
            for (index, (x, y)) in positions.iter().enumerate() {
                let _ = probe_hover(harness, *x, *y, &baseline, &volatile, baseline_digest);
                if (round + index) % 16 == 15 {
                    // Mid-harness blank guard, same as the sweep.
                    if harness.grid_now().is_blank() {
                        return Err("the screen went blank mid-harness".to_owned());
                    }
                }
            }
        }
        Ok(SweepOutcome {
            sweep: SweepResult::default(),
            strip,
            volatile: volatile.iter().copied().collect(),
            plus_probes: Vec::new(),
        })
    });
    let _ = outcome;
    eprintln!("HOVER PAINT LATENCY ({}): {}", std::process::id(), latency_report());
}
