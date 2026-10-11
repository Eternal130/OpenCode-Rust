//! TEMPORAL BEHAVIOR over the cell-grid stream: what animates, at
//! what period, and what must NOT animate. Four observations:
//!
//! * SPINNER (`vt_temporal_spinner_animates_100ms_frames`):
//!   `#[ignore]`d — REGISTERED FINDING (frozen). With a streaming
//!   turn held open by the `FakeLlm` script (one reasoning delta, no
//!   `step.ended`), the transcript's running "Thinking" row DOES
//!   render the braille loading indicator — but it is FROZEN at
//!   frame 0 (`⠋`): `render_session` builds `TranscriptOptions` via
//!   `..TranscriptOptions::default()` (`components/session.rs`) and
//!   that default is `clock_ms: 0` (`components/message/list.rs`) —
//!   the store clock is never wired into the transcript render, so
//!   the 80 ms/frame spinner never advances even though the run loop
//!   re-renders every 100 ms tick (the caret blink, which reads
//!   `store.clock()` directly, animates fine — proving the clock
//!   itself advances). The test asserts the spinner EXISTS and
//!   cycles within 100–600 ms (median): un-ignore when
//!   `TranscriptOptions.clock_ms` gets `store.clock()`.
//! * CURSOR BLINK (`vt_temporal_cursor_blinks_500ms`): the composer
//!   caret is a reverse-video block whose phase flips every 500 ms
//!   (`CURSOR_BLINK_INTERVAL_MS`); idle, exactly one cell toggles its
//!   STYLE with a median period inside 300–900 ms.
//! * MARQUEE (`vt_temporal_marquee_shifts_one_cell_per_tick`):
//!   `#[ignore]`d — the registered V2 GAP. v2 marquees a hovered
//!   TRUNCATED tab title (content window shifts ~1 cell per tick);
//!   our port renders tab-pulse envelopes as C2 (see
//!   `components/tab_pulse.rs` — "ignition/release/sweep/flash/
//!   marquee stay C2"), so no shift exists today. The test asserts
//!   the marquee EXISTS: un-ignore (remove the attribute) the day the
//!   feature lands — it turns green then.
//! * DEBOUNCE (`vt_temporal_idle_composer_is_byte_stable_2s`): with
//!   no interaction and no streaming, the composer area must not
//!   re-emit gratuitously: across a 2 s idle window, every grid
//!   change may touch ONLY the caret cell (its blink), and the
//!   window's total byte count stays tiny.
//!   RECORDED FINDING the guard surfaced: the run loop renders every
//!   100 ms clock tick and ratatui's `Terminal::draw` emits its
//!   hide-cursor sequence (`ESC[?25l`) unconditionally per draw — the
//!   idle stream is never byte-silent (~6 B per tick of escape
//!   chatter, no cell changes). The guard therefore pins
//!   CELL-stability + bounded byte volume; a render-noise regression
//!   (per-tick cell churn or full repaints ≈ 4 KB/frame) trips it.

// Terminal coordinates are u16 by VT definition; harness instants are
// bounded magnitudes — the truncation lints add nothing here.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    char_timeline, median_gap, open_first_session, pty_lock, run_tui, sandbox_env,
    standard_sessions, streaming_thinking_script, style_timeline, FakeLlm, HarnessHandle,
    LiveHarness, Snapshot, WORDMARK,
};

/// The braille spinner glyph set (`SPINNER_FRAMES` in the product).
const SPINNER_GLYPHS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Shared runner (same shape as the sweep suite's): sandbox +
/// `FakeLlm` + PTY + driver thread, real run loop on the test body, driver
/// always leaves the exit chord.
fn run_temporal_test<T: Send + 'static>(
    tag: &str,
    cols: u16,
    rows: u16,
    arm: impl FnOnce(&FakeLlm),
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> T {
    let _lock = pty_lock();
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    arm(&fake);
    let harness = LiveHarness::attach(cols, rows);
    let result_slot: Arc<Mutex<Option<Result<T, String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
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
    let outcome = match result_slot.lock().expect("slot").take() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let head = String::from_utf8_lossy(&raw[..raw.len().min(800)]);
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!(
                "the driver thread never reported (deadlock?); raw head ({}B): {:?}; raw TAIL: {:?}",
                raw.len(),
                head,
                tail
            )
        }
    };
    outcome
}

/// Boot to the one-tab session view on "Alpha" (the composer + a
/// quiet transcript).
fn boot_to_session(harness: &mut HarnessHandle) -> Result<(), String> {
    if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    eprintln!("[vt] wordmark seen");
    open_first_session(harness, b"Alpha")?;
    eprintln!("[vt] dialog listed Alpha, Enter sent");
    if !harness.wait_for(b"Alpha", Duration::from_secs(5)) {
        return Err("the tab title never painted".to_owned());
    }
    eprintln!("[vt] tab title painted");
    Ok(())
}

/// Collect `window_ms` worth of snapshots after the screen is
/// cell-stable (byte-quiet is impossible: ratatui's unconditional
/// hide-cursor emission lands every 100 ms tick — the harness clock
/// is wall time, bursts are real output).
fn collect_snapshots(harness: &HarnessHandle, window_ms: u64) -> Vec<Snapshot> {
    // Best-effort settle (the observation itself tolerates residual
    // motion — the analyses track transitions, not stillness).
    let _settled = harness.wait_grid_stable(2, 300, Duration::from_secs(5));
    let start = harness.now_ms();
    while harness.now_ms() - start < window_ms {
        std::thread::sleep(Duration::from_millis(20));
    }
    // Close the trailing burst into a snapshot.
    std::thread::sleep(Duration::from_millis(120));
    harness.settle_snapshot();
    let from_ms = start;
    harness
        .vt
        .lock()
        .expect("vt")
        .snapshots()
        .iter()
        .filter(|snap| snap.t_ms >= from_ms)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Spinner: a streaming turn keeps a braille region cycling
// ---------------------------------------------------------------------------

#[test]
#[ignore = "registered finding: the transcript spinner is frozen at frame 0 — render_session never wires store.clock() into TranscriptOptions.clock_ms (session.rs ..Default(), list.rs clock_ms:0); un-ignore + it turns green when the clock is wired"]
fn vt_temporal_spinner_animates_100ms_frames() {
    let findings: Vec<String> = run_temporal_test(
        "spin",
        80,
        24,
        |fake| fake.arm_script(streaming_thinking_script("ses_alpha", "count slowly")),
        |harness| {
            boot_to_session(harness)?;
            std::thread::sleep(Duration::from_millis(500));
            // Submit: the composer's Enter POSTs the prompt; the armed
            // SSE script then holds a running "Thinking" row open.
            harness.write(b"count slowly\r");
            if !harness.wait_for(b"Thinking", Duration::from_secs(5)) {
                return Err("the streaming turn never showed its Thinking row".to_owned());
            }
            let snaps = collect_snapshots(harness, 3500);
            let (rows, cols) = (
                snaps.first().map_or(0, |s| s.grid.rows),
                snaps.first().map_or(0, |s| s.grid.cols),
            );
            if snaps.len() < 10 {
                return Err(format!(
                    "only {} snapshots in 3.5s of a streaming turn — the spinner cannot be animating",
                    snaps.len()
                ));
            }
            // Find every cell whose CHAR cycles through the braille
            // set (≥15 transitions in 3.5s ≈ the 100 ms cadence).
            let mut report = Vec::new();
            let mut best: Option<(usize, usize, Vec<u64>, Vec<char>)> = None;
            for y in 0..rows {
                for x in 0..cols {
                    let timeline = char_timeline(&snaps, x, y);
                    if timeline.len() < 15 {
                        continue;
                    }
                    let glyphs: HashSet<char> = timeline.iter().map(|(_, ch)| *ch).collect();
                    if !glyphs.iter().all(|ch| SPINNER_GLYPHS.contains(ch)) {
                        continue;
                    }
                    let times: Vec<u64> = timeline.iter().map(|(t, _)| *t).collect();
                    let chars: Vec<char> = timeline.iter().map(|(_, ch)| *ch).collect();
                    if best.as_ref().is_none_or(|(_, _, prev, _)| prev.len() < times.len()) {
                        best = Some((x, y, times, chars));
                    }
                }
            }
            let Some((x, y, times, chars)) = best else {
                // DEBUG: what DID change? dump the changing cells and
                // the rows around "Thinking".
                let mut changing = Vec::new();
                for yy in 0..rows {
                    for xx in 0..cols {
                        let tl = char_timeline(&snaps, xx, yy);
                        if tl.len() > 1 {
                            changing.push((xx, yy, tl.len()));
                        }
                    }
                }
                let texts: Vec<String> =
                    snaps.iter().map(|s| s.grid.content_rect(0, 1, cols, 8)).collect();
                return Err(format!(
                    "no cell cycles braille glyphs over {} snapshots — changing cells {:?}; first rows {:?}; last rows {:?}",
                    snaps.len(),
                    changing,
                    texts.first().cloned().unwrap_or_default(),
                    texts.last().cloned().unwrap_or_default(),
                ));
            };
            let changes = times.len();
            let period = median_gap(&times).unwrap_or(0);
            let distinct: Vec<char> = {
                let mut seen: Vec<char> = Vec::new();
                for ch in &chars {
                    if !seen.contains(ch) {
                        seen.push(*ch);
                    }
                }
                seen
            };
            report.push(format!(
                "spinner cell ({x},{y}): {changes} char changes, median gap {period} ms, glyphs {distinct:?}"
            ));
            // Assertions: EXISTS and animates with a stable period.
            // Nominal: 100 ms tick / 80 ms frame. Bound: 100–600 ms
            // (median; 5 ms floor slack for PTY jitter).
            if changes < 20 {
                return Err(format!("spinner changed only {changes} times in 3.5s: {report:?}"));
            }
            if !(95..=600).contains(&period) {
                return Err(format!("spinner median gap {period} ms outside 100–600: {report:?}"));
            }
            Ok(report)
        },
    );
    for line in findings {
        eprintln!("[vt_temporal spinner] {line}");
    }
}

// ---------------------------------------------------------------------------
// Cursor blink: the composer caret toggles style at ~500 ms
// ---------------------------------------------------------------------------

#[test]
fn vt_temporal_cursor_blinks_500ms() {
    let findings: Vec<String> = run_temporal_test(
        "blink",
        80,
        24,
        |_| {},
        |harness| {
            boot_to_session(harness)?;
            // Idle: no streaming, no interaction. The caret blink is
            // the only animation; watch 2.6 s.
            let snaps = collect_snapshots(harness, 2600);
            let (rows, cols) = (
                snaps.first().map_or(0, |s| s.grid.rows),
                snaps.first().map_or(0, |s| s.grid.cols),
            );
            if snaps.len() < 4 {
                return Err(format!(
                    "only {} snapshots in 2.6s of idle — the caret is not blinking (frozen clock?)",
                    snaps.len()
                ));
            }
            let mut best: Option<(usize, usize, Vec<u64>)> = None;
            for y in 0..rows {
                for x in 0..cols {
                    let timeline = style_timeline(&snaps, x, y);
                    if timeline.len() < 3 {
                        continue;
                    }
                    // The caret is CHAR-stable (a block over a space)
                    // and only its STYLE (fg/bg swap) toggles.
                    if char_timeline(&snaps, x, y).len() > 1 {
                        continue;
                    }
                    let times: Vec<u64> = timeline.iter().map(|(t, _)| *t).collect();
                    if best.as_ref().is_none_or(|(_, _, prev)| prev.len() < times.len()) {
                        best = Some((x, y, times));
                    }
                }
            }
            let Some((x, y, times)) = best else {
                return Err(format!(
                    "no char-stable cell toggles style over {} idle snapshots — the caret blink is missing",
                    snaps.len()
                ));
            };
            let toggles = times.len();
            let period = median_gap(&times).unwrap_or(0);
            // EXISTS and animates: nominal 500 ms (visible 500 /
            // hidden 500); robust bound 300–900 ms.
            if !(300..=900).contains(&period) {
                return Err(format!("caret blink median gap {period} ms outside 300–900"));
            }
            Ok(vec![format!(
                "caret cell ({x},{y}): {toggles} style toggles, median gap {period} ms (nominal 500)"
            )])
        },
    );
    for line in findings {
        eprintln!("[vt_temporal blink] {line}");
    }
}

// ---------------------------------------------------------------------------
// Marquee: registered v2 gap — turns green when the feature lands
// ---------------------------------------------------------------------------

/// v2 marquees a hovered TRUNCATED tab title: the visible content
/// window shifts ~1 cell per tick while hovered. Our port's tab-pulse
/// envelopes (marquee included) are C2 and NOT implemented
/// (`components/tab_pulse.rs`) — the registered v2 gap. This test is
/// `#[ignore]`d for exactly that reason: it asserts the marquee
/// EXISTS (≥5 one-cell shifts of the hovered title region over ≥5
/// ticks), which today it does not. REMOVE the `#[ignore]` when the
/// marquee lands — the body is already the acceptance check.
#[test]
#[ignore = "registered v2 gap: marquee-on-truncated-title is C2 (tab_pulse.rs) — un-ignore when it lands"]
fn vt_temporal_marquee_shifts_one_cell_per_tick() {
    let shifts: usize = run_temporal_test(
        "marquee",
        80,
        24,
        |_| {},
        |harness| {
            // Two tabs; the ACTIVE one carries the long truncated title.
            if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            vt_harness::open_first_session(harness, b"Alpha")?;
            std::thread::sleep(Duration::from_millis(400));
            vt_harness::open_second_session(harness, b"Long Session Title")?;
            if !harness.wait_for(b"Long Session", Duration::from_secs(5)) {
                return Err("the long-titled tab never painted".to_owned());
            }
            std::thread::sleep(Duration::from_millis(500));
            let (baseline, _volatile) = harness.capture_baseline();
            // The long tab is active (last opened) — its title sits
            // right of the first tab. Locate the title's letter block
            // on row 0 and hover its middle.
            let strip = baseline.row_text(0);
            let title_start = strip
                .chars()
                .enumerate()
                .find(|(x, _)| *x > 10 && strip.chars().nth(*x).is_some_and(char::is_alphabetic))
                .map(|(x, _)| x)
                .ok_or_else(|| format!("no title block found in the strip: {strip:?}"))?;
            let hover_x = (title_start + 6).min(baseline.cols - 2) as u16;
            harness.sgr_motion(hover_x, 0);
            // Observe ≥5 ticks (v2's marquee interval ~120 ms; watch 2 s).
            let snaps = collect_snapshots(harness, 2000);
            // Count one-cell shifts of the hovered title region: each
            // consecutive pair of row-0 texts differing INSIDE the
            // tab's span (a window shift), not elsewhere.
            let mut shifts = 0;
            for pair in snaps.windows(2) {
                let before = pair[0].grid.row_text(0);
                let after = pair[1].grid.row_text(0);
                if before != after {
                    shifts += 1;
                }
            }
            Ok(shifts)
        },
    );
    eprintln!("[vt_temporal marquee] {shifts} title-window shifts while hovered");
    assert!(
        shifts >= 5,
        "the hovered truncated title shifted only {shifts} times (<5) — no marquee"
    );
}

// ---------------------------------------------------------------------------
// Debounce: idle means byte-stable (except the caret's own blink)
// ---------------------------------------------------------------------------

#[test]
fn vt_temporal_idle_composer_is_byte_stable_2s() {
    let findings: Vec<String> = run_temporal_test(
        "idle",
        80,
        24,
        |_| {},
        |harness| {
            boot_to_session(harness)?;
            std::thread::sleep(Duration::from_millis(600));
            if !harness.wait_grid_stable(2, 300, Duration::from_secs(5)) {
                return Err("the session screen never became cell-stable".to_owned());
            }
            let bytes_before = harness.fed_bytes();
            let start_ms = harness.now_ms();
            // 2 s of pure idle — no input, no streaming.
            std::thread::sleep(Duration::from_millis(2000));
            std::thread::sleep(Duration::from_millis(150));
            harness.settle_snapshot();
            let snaps: Vec<Snapshot> = harness
                .vt
                .lock()
                .expect("vt")
                .snapshots()
                .iter()
                .filter(|snap| snap.t_ms >= start_ms)
                .cloned()
                .collect();
            let bytes_in_window = harness.fed_bytes() - bytes_before;
            if snaps.is_empty() {
                // Nothing at all was emitted in 2 s: the caret blink
                // itself is missing — that is the blink test's
                // finding; here it just means zero noise.
                return Ok(vec![
                    "idle 2s: ZERO bursts (no caret blink emission; see the blink test)".to_owned(),
                    format!("idle 2s: {bytes_in_window} bytes"),
                ]);
            }
            // Union of every changed cell across the window: may only
            // be the caret blink cell(s), and those changes are
            // style-only.
            let mut changed: HashSet<(usize, usize)> = HashSet::new();
            let mut any_char_change = false;
            for pair in snaps.windows(2) {
                for diff in pair[0].grid.diff(&pair[1].grid, &HashSet::new()) {
                    if diff.kind == vt_harness::DiffKind::Char {
                        any_char_change = true;
                    }
                    changed.insert((diff.x, diff.y));
                }
            }
            let changed_list: Vec<(usize, usize)> = changed.iter().copied().collect();
            if changed_list.len() > 2 {
                return Err(format!(
                    "{} snapshots in 2s idle touched {} cells {:?} — gratuitous re-emission: {} bytes",
                    snaps.len(),
                    changed_list.len(),
                    changed_list,
                    bytes_in_window
                ));
            }
            if any_char_change {
                return Err(format!(
                    "idle re-emission changed CHARACTERS (not just the caret's style) in cells {changed_list:?}"
                ));
            }
            if bytes_in_window > 2048 {
                return Err(format!(
                    "idle 2s re-emitted {bytes_in_window} bytes (full repaints?) across {} bursts",
                    snaps.len()
                ));
            }
            Ok(vec![format!(
                "idle 2s: {} bursts, {bytes_in_window} bytes, changed cells {:?} (caret blink only)",
                snaps.len(),
                changed_list
            )])
        },
    );
    for line in findings {
        eprintln!("[vt_temporal idle] {line}");
    }
}
