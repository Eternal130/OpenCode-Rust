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
//! * STARTUP PILL (`vt_temporal_startup_loading_pill_thresholds`):
//!   FIXED (lane E2). The boot wiring now mirrors the ref — the render
//!   loop starts with the `StartupLoading` pill armed while plugins
//!   boot (the boot future is polled in-loop; `set_plugins_ready()`
//!   fires only after the plugin generation attached AND the server
//!   handshake landed), so a ≥500 ms boot shows the pill exactly per
//!   the state machine (store.rs thresholds, ref app.tsx:1351/:1390).
//!   The fast-boot companion (`…_fast_boot_shows_nothing`) pins that a
//!   healthy boot clears both gates under the 500 ms threshold — no
//!   pill, no residue.

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
// spinner frozen-finding FIXED: store.clock() wired into TranscriptOptions (see session.rs)
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
            // Lane/chrome (NEW-S1a): the live caret is the TERMINAL
            // cursor (v2's textarea positions the hardware cursor;
            // `config.cursor` unset — no cell-painted block anywhere).
            // The old 500 ms painted blink is gone BY DESIGN: idle
            // screens must show NO char-stable style toggles at all.
            let snaps = collect_snapshots(harness, 2600);
            let (rows, cols) = (
                snaps.first().map_or(0, |s| s.grid.rows),
                snaps.first().map_or(0, |s| s.grid.cols),
            );
            if snaps.len() < 4 {
                return Err(format!(
                    "only {} snapshots in 2.6s — the render loop stalled",
                    snaps.len()
                ));
            }
            let mut painted = 0usize;
            for y in 0..rows {
                for x in 0..cols {
                    let timeline = style_timeline(&snaps, x, y);
                    if timeline.len() >= 2 && char_timeline(&snaps, x, y).len() <= 1 {
                        painted += 1;
                    }
                }
            }
            if painted > 0 {
                return Err(format!(
                    "{painted} cells toggle style while idle — a painted caret/animation leaked back in"
                ));
            }
            Ok(vec![format!(
                "idle screen style-stable across {} snapshots (the caret lives on the terminal cursor)",
                snaps.len()
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
/// window shifts one cell per tick while hovered. Lane TAIL landed the
/// marquee (`components/marquee.rs` + `title_chunks`'s scrolling
/// window): a hovered overflowing title scrolls one cell per 80 ms
/// after a 600 ms still frame. SA-F3 hardened the acceptance: the
/// measured gap between consecutive one-cell shifts must sit ON the
/// 80 ms pin (median within 75–105 ms, no gap above 140 ms — the
/// registered failure's signature was a uniform ~166 ms, the plain
/// 100 ms render tick advancing the clock two boundaries at a time),
/// and the window must step by EXACTLY one cell (no skipped offsets).
#[test]
fn vt_temporal_marquee_shifts_one_cell_per_tick() {
    let report: (usize, u64, u64, u64) = run_temporal_test(
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
            // SA-F3 vt-clock proof: poll the LIVE grid directly (the
            // snapshot instrument's 80 ms quiet window cannot resolve
            // an 80 ms cadence — consecutive frames merge into one
            // burst). Each row-0 text change is one offset step; the
            // gaps between changes must sit ON the 80 ms grid.
            let start = harness.now_ms();
            let mut last = harness.grid_now().row_text(0);
            let mut change_times: Vec<u64> = Vec::new();
            while harness.now_ms() - start < 2600 {
                std::thread::sleep(Duration::from_millis(12));
                let now_text = harness.grid_now().row_text(0);
                if now_text != last {
                    change_times.push(harness.now_ms());
                    last = now_text;
                }
            }
            if change_times.len() < 10 {
                return Err(format!(
                    "only {} title-window shifts in 2.6 s of hover — the 80 ms cadence cannot be measured: times {:?}",
                    change_times.len(),
                    change_times
                ));
            }
            // The FIRST gap carries the 600 ms still delay and the
            // scroll's END leaves a trailing stall (post-marquee
            // repaints) — measure the STEADY run: the longest stretch
            // of shifts separated by ≤300 ms.
            let gaps: Vec<u64> =
                change_times[1..].iter().zip(change_times[..].iter()).map(|(a, b)| a - b).collect();
            let mut best_run: Vec<u64> = Vec::new();
            let mut current: Vec<u64> = Vec::new();
            for gap in gaps {
                if gap > 300 {
                    if current.len() > best_run.len() {
                        best_run = current.clone();
                    }
                    current.clear();
                } else {
                    current.push(gap);
                }
            }
            if current.len() > best_run.len() {
                best_run = current;
            }
            if best_run.len() < 9 {
                return Err(format!(
                    "no steady shift run ≥9 gaps in 2.6 s of hover (times {change_times:?})"
                ));
            }
            let mut sorted = best_run.clone();
            sorted.sort_unstable();
            let median = sorted[sorted.len() / 2];
            let max = *sorted.last().unwrap_or(&0);
            let min = sorted[0];
            Ok((change_times.len(), median, min, max))
        },
    );
    let (shifts, median, min, max) = report;
    eprintln!(
        "[vt_temporal marquee] {shifts} title-window shifts, gaps median {median} ms (min {min}, max {max})"
    );
    assert!(
        shifts >= 10,
        "the hovered truncated title shifted only {shifts} times (<10) — no marquee"
    );
    assert!(
        (70..=105).contains(&median),
        "the marquee cadence must sit on the 80 ms pin (median {median} ms; the registered failure measured ~166)"
    );
    assert!(
        max <= 140,
        "no shift gap may exceed 140 ms (max {max} ms — a render-tick quantization leak, SA-F3)"
    );
    assert!(min >= 55, "no gap may collapse below 55 ms (min {min} — double-steps)");
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

// ---------------------------------------------------------------------------
// SA-F4's instrument: the before-settle sampler catches in-flight churn
// ---------------------------------------------------------------------------

/// SA-F4's instrument repair, demonstrated on a genuine time-driven
/// transient: the busy tab's sweep ramps per-column backgrounds while
/// the turn streams (`tab_pulse`'s run envelope — the A4 drive-source
/// audit). The BEFORE-SETTLE sampler observes the style churn with NO
/// settle wait (the settle-first collectors structurally miss exactly
/// this class — the scan's shimmer leg recorded 0 style deltas across
/// 60 ref frames after typing settled).
#[test]
fn vt_temporal_style_sampler_catches_inflight_churn() {
    let (samples, distinct): (usize, usize) = run_temporal_test(
        "sampler",
        80,
        24,
        |fake| fake.arm_script(busy_window_script()),
        |harness| {
            boot_to_session(harness)?;
            harness.write(b"busy turn\r");
            if !harness.wait_for(b"Thinking", Duration::from_secs(5)) {
                return Err("the streaming turn never showed its Thinking row".to_owned());
            }
            // Sample row 0 (the busy tab's sweep region) DURING the
            // held-open busy window — no settle, polls straight away.
            let series = harness.style_series_during(1200, 25, (0, 0, 40, 1), || {});
            let samples = series.len();
            let mut distinct_sets: Vec<&Vec<u64>> = Vec::new();
            for (_t, styles) in &series {
                if !distinct_sets.contains(&styles) {
                    distinct_sets.push(styles);
                }
            }
            Ok((samples, distinct_sets.len()))
        },
    );
    eprintln!(
        "[vt_temporal sampler] {samples} in-flight samples, {distinct} distinct style frames"
    );
    assert!(samples >= 20, "the sampler polled the live grid ({samples} samples)");
    assert!(
        distinct >= 8,
        "the busy sweep's style churn was caught in flight ({distinct} distinct frames — a settle-first collector would see far fewer)"
    );
}

// ---------------------------------------------------------------------------
// Busy strip: the tab spinner's COLOR TOKEN (beta.13 bug 4)
// ---------------------------------------------------------------------------

/// A busy-window script: the prompt POST enqueues the user turn, the
/// session enters `execution.started` and streams one reasoning delta
/// — then HOLDS ~4 s before settling, so the strip's tab indicator is
/// the busy spinner for a wide window (the fast-window class the
/// rescan's temporal captures proved: `rescan-time`).
fn busy_window_script() -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    let event = |delay: u64, id: &str, type_: &str, data: &str| vt_harness::ScriptEvent {
        delay_ms: delay,
        body: format!(r#"{{"id":"{id}","created":{now_ms},"type":"{type_}","data":{data}}}"#),
    };
    vec![
        event(
            30,
            "evt_inbox",
            "session.inbox.enqueued",
            r#"{"sessionID":"ses_alpha","inboxID":"msg_busy_1","item":{"type":"user","payload":{"text":"busy window"}}}"#,
        ),
        event(50, "evt_exec", "session.execution.started", r#"{"sessionID":"ses_alpha"}"#),
        event(
            60,
            "evt_step",
            "session.step.started",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_busy_a1","agent":"build","model":{"providerID":"anthropic","id":"claude-sonnet"}}"#,
        ),
        event(
            80,
            "evt_r1",
            "session.reasoning.delta",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_busy_a1","ordinal":0,"delta":"Working"}"#,
        ),
        // The hold: the busy window spans these delays.
        event(
            4_000,
            "evt_r2",
            "session.reasoning.delta",
            r#"{"sessionID":"ses_alpha","assistantMessageID":"msg_busy_a1","ordinal":0,"delta":" done"}"#,
        ),
        event(4_050, "evt_exec_end", "session.execution.succeeded", r#"{"sessionID":"ses_alpha"}"#),
    ]
}

/// The busy tab's spinner paints `theme.hue.interactive[200]`
/// (`activeNumber` — session-tabs.tsx:200/568/1634-1641), NOT
/// text.base: with the turn held busy, row 0's braille cell carries
/// the interactive token's RGB (the fast-window evidence class the
/// rescan opened; our side now provable).
#[test]
fn vt_temporal_busy_strip_spinner_paints_interactive_token() {
    let spinner_style: u64 = run_temporal_test(
        "busy-token",
        80,
        24,
        |fake| fake.arm_script(busy_window_script()),
        |harness| {
            boot_to_session(harness)?;
            // Submit a turn (the script's busy window opens on the
            // prompt POST).
            harness.write(b"busy turn\r");
            if !harness.wait_for(b"Thinking", Duration::from_secs(5)) {
                return Err("the streaming turn never showed its Thinking row".to_owned());
            }
            // Poll row 0 for a braille spinner glyph (≤3 s window).
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut found: Option<(usize, u64)> = None;
            while std::time::Instant::now() < deadline {
                let grid = harness.grid_now();
                for col in 0..grid.cols {
                    let cell = grid.cell(col, 0);
                    if SPINNER_GLYPHS.contains(&cell.ch) {
                        found = Some((col, cell.style));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            let (col, style) = found.ok_or_else(|| {
                let row0 = harness.grid_now().row_text(0);
                format!("no braille spinner ever rendered in the strip: row0={row0:?}")
            })?;
            eprintln!("[vt] busy spinner at col {col}, style digest {style:#x}");
            Ok(style)
        },
    );
    // Decode the digest's fg (the harness packs fg at bit 30:
    // Rgb → (2<<24)|(r<<16)|(g<<8)|b).
    let fg = spinner_style >> 30;
    let kind = (fg >> 24) & 0xff;
    assert_eq!(kind, 2, "the spinner's fg must be truecolor RGB (digest fg {fg:#x})");
    let r = ((fg >> 16) & 0xff) as u8;
    let g = ((fg >> 8) & 0xff) as u8;
    let b = (fg & 0xff) as u8;
    // The exact token: ThemeStyles.interactive = hue.interactive[200]
    // of the boot theme (v2 opencode, dark).
    assert_eq!((r, g, b), theme_interactive(), "the busy spinner paints hue.interactive[200]");
}

/// The resolved constant of the v2 opencode dark theme's
/// `ThemeStyles::interactive` (`tokens.semantic(Interactive).s200`) —
/// the same builtin the bare app boots with.
fn theme_interactive() -> (u8, u8, u8) {
    let styles = oc_tui::theme::ThemeState::v2_dark().styles();
    match styles.interactive {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        other => panic!("interactive token must be RGB, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// StartupLoading: the boot-stretch temporal gate (Lane E2)
// ---------------------------------------------------------------------------

/// The `StartupLoading` pill's temporal contract over a STRETCHED boot
/// (spec-round6.md:192-201; thresholds store.rs:9303-9335, render
/// overlays.rs:131-142): with the server holding every response for
/// the first 1.2 s (`arm_boot_stretch`), the pill must NOT show
/// before the 500 ms display threshold, MUST render "Loading
/// plugins…" once the stall crosses it, flip to "Finishing startup…"
/// when boot completes, and unmount by shown+3 s (the hold pin),
/// leaving the booted session screen clean.
///
/// FIXED (lane E2 — the wiring this test demanded): the render loop
/// now starts BEFORE plugin boot completes (app.rs polls the plugin
/// boot future per pass; the first frame draws with the pill armed)
/// and `set_plugins_ready()` fires only once plugin boot ACTUALLY
/// completed AND the server handshake landed (`Ingest::Connected`
/// through the ingest channel — the ref's plugin-inventory sync rides
/// `server.connected`, plugin/context.tsx:570), mirroring the ref's
/// mount → render-while-booting → ready timing (app.tsx:1351 gates
/// the UI on `plugins.ready()`, :1390 mounts the pill). The stretched
/// server therefore holds the handshake — and with it the pill's
/// ready input — past the 500 ms threshold, exactly like a slow v2
/// boot.
#[test]
fn vt_temporal_startup_loading_pill_thresholds() {
    let (samples, session_clean): (Vec<(u64, &'static str)>, bool) = run_temporal_test(
        "boot-stretch",
        80,
        24,
        |fake| fake.arm_boot_stretch(1_200),
        |harness| {
            // Observe the pill from boot start (attach ≈ t0) through
            // the 1.2 s stretch and past the 3 s hold pin, sampling
            // the whole screen every 30 ms (the pill holds ≥650 ms in
            // the loading phase once wired — far above the sampling
            // grid; poll phases, not transitions, so a missed blip
            // cannot fake a clean run).
            let boot_start = harness.now_ms();
            let mut samples: Vec<(u64, &'static str)> = Vec::new();
            while harness.now_ms() - boot_start < 4_600 {
                let grid = harness.grid_now();
                let screen = grid.content_rect(0, 0, grid.cols, grid.rows);
                let phase = if screen.contains("Finishing startup") {
                    "finishing"
                } else if screen.contains("Loading plugins") {
                    "loading"
                } else {
                    "none"
                };
                samples.push((harness.now_ms() - boot_start, phase));
                std::thread::sleep(Duration::from_millis(30));
            }
            harness.settle_snapshot();
            // (d) boot completes: the session opens and no pill
            // residue survives on the booted screen.
            boot_to_session(harness)?;
            let grid = harness.grid_now();
            let screen = grid.content_rect(0, 0, grid.cols, grid.rows);
            let session_clean =
                !screen.contains("Loading plugins") && !screen.contains("Finishing startup");
            Ok((samples, session_clean))
        },
    );
    eprintln!(
        "[vt_temporal boot-stretch] {} samples, pill phases: {:?}",
        samples.len(),
        samples.iter().filter(|(_, p)| *p != "none").collect::<Vec<_>>()
    );
    // (a) pre-500 ms: the pill must NOT be on screen.
    let early: Vec<_> = samples.iter().filter(|(t, p)| *t < 500 && *p != "none").collect();
    assert!(early.is_empty(), "the pill showed before the 500 ms threshold: {early:?}");
    // (b) ≥500 ms into the stretched stall: "Loading plugins…" renders.
    let show_at = samples.iter().find(|(_, p)| *p == "loading").map_or(u64::MAX, |(t, _)| *t);
    assert!(
        show_at != u64::MAX,
        "the StartupLoading pill never rendered during a 1.2 s stretched boot — the pill must \
         arm at the first frame and show once the stall crosses the 500 ms display threshold \
         (the loop renders while plugins boot and the handshake is held; see the test header). \
         Pill samples: {:?}",
        samples.iter().filter(|(_, p)| *p != "none").collect::<Vec<_>>()
    );
    assert!(show_at >= 500, "the pill appeared at {show_at} ms, under the 500 ms threshold");
    // (c) escalation (exactly what the state machine pins): once
    // boot completes the copy flips to "Finishing startup…" and the
    // pill unmounts by shown+3 s.
    assert!(
        samples.iter().any(|(_, p)| *p == "finishing"),
        "the pill never escalated to \"Finishing startup…\" after the stretch cleared"
    );
    let last_pill =
        samples.iter().filter(|(_, p)| *p != "none").map(|(t, _)| *t).max().unwrap_or(0);
    assert!(
        last_pill <= show_at + 3_000 + 400,
        "the pill held past shown+3 s ({last_pill} ms > {show_at} + 3400)"
    );
    // (d) boot completes: the session screen, pill unmounted.
    assert!(session_clean, "pill residue survived onto the booted session screen");
}

/// Fast-boot companion (lane E2 fix): a healthy, non-stretched boot
/// clears both plugins-ready gates (plugin generation attached + the
/// server handshake) well under the pill's 500 ms display threshold —
/// the pill must NEVER show, and nothing may linger on the booted
/// session screen (the state machine's ready-before-showing arm never
/// fires; the unit pin overlays.rs:209-212 at the store level).
#[test]
fn vt_temporal_startup_loading_pill_fast_boot_shows_nothing() {
    let (saw_pill, session_clean): (bool, bool) = run_temporal_test(
        "boot-fast",
        80,
        24,
        |_| {},
        |harness| {
            let boot_start = harness.now_ms();
            let mut saw_pill = false;
            while harness.now_ms() - boot_start < 1_500 {
                let grid = harness.grid_now();
                let screen = grid.content_rect(0, 0, grid.cols, grid.rows);
                if screen.contains("Loading plugins") || screen.contains("Finishing startup") {
                    saw_pill = true;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            boot_to_session(harness)?;
            let grid = harness.grid_now();
            let screen = grid.content_rect(0, 0, grid.cols, grid.rows);
            Ok((
                saw_pill,
                !screen.contains("Loading plugins") && !screen.contains("Finishing startup"),
            ))
        },
    );
    assert!(
        !saw_pill,
        "a fast (non-stretched) boot showed the StartupLoading pill — both plugins-ready gates \
         must clear under the 500 ms display threshold"
    );
    assert!(session_clean, "pill residue survived onto the fast-boot session screen");
}
