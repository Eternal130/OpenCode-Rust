//! W16 ⑦''② — the busy tab's breathing lamp ("呼吸灯时有时无") under
//! TEMPORAL PRESSURE: N busy→idle cycles over a real PTY, sampling the
//! actual emitted frames (the VT stream), hunting the track
//! registration race (a cycle where the busy face never appears or
//! freezes while the session IS running).
//!
//! The script drives `session.step.started`/`session.step.ended` pairs
//! (the running-set's only sources, store.rs:14399/14465) — one
//! prompt POST, N busy windows of ~600 ms (≈6 pulse frames at the
//! pump tick) separated by ~400 ms idle gaps. Per cycle the harness
//! asserts:
//!
//! * ENTER: the strip shows a busy face (a spinner glyph on row 0)
//!   while the step runs;
//! * MOVE: the row's style profile changes across in-window frames
//!   (the sweep band advances — the "breathing");
//! * EXIT: the strip returns to the idle face after `step.ended`.
//!
//! A registration race reproduces as a red assert on some cycle
//! (frozen stretch / window without movement / no exit). All cycles
//! passed in the W16 lane runs — the miss did NOT reproduce under
//! this pressure; it rides real-machine timing, so the observation
//! hook stays the user channel (a frozen-lamp repro screenshot with
//! `OC_TUI_DEBUG=1` key/trace output narrows it immediately).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness,
};

const CYCLES: u64 = 6;
const BUSY_MS: u64 = 600;
const IDLE_MS: u64 = 700;

/// The N-cycle busy/idle script (one prompt POST; each cycle is a
/// started→delta→ended triple on its own assistant message id).
fn busy_cycle_script(session: &str) -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    let mut events = Vec::new();
    let mut t = 60u64;
    for k in 0..CYCLES {
        events.push(vt_harness::ScriptEvent {
            delay_ms: t,
            body: format!(
                r#"{{"id":"evt_s{k}","created":{now_ms},"type":"session.execution.started","data":{{"sessionID":"{session}","assistantMessageID":"msg_a{k}","agent":"build","model":{{"providerID":"anthropic","id":"claude-sonnet"}}}}}}"#
            ),
        });
        t += BUSY_MS;
        events.push(vt_harness::ScriptEvent {
            delay_ms: t,
            body: format!(
                r#"{{"id":"evt_e{k}","created":{now_ms},"type":"session.execution.succeeded","data":{{"sessionID":"{session}"}}}}"#
            ),
        });
        t += IDLE_MS;
    }
    events
}

#[test]
#[allow(clippy::too_many_lines)]
fn busy_pulse_pressure_cycles_breathe_every_time() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("w16pulse");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(busy_cycle_script("ses_alpha"));
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let result: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let rslot = Arc::clone(&result);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            open_first_session(&mut driver, b"Alpha")?;
            if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("tab title never painted".into());
            }
            driver.write(b"pulse\r");
            // Let the whole N-cycle script play out.
            let total = CYCLES * (BUSY_MS + IDLE_MS) + 2_500;
            std::thread::sleep(Duration::from_millis(total));
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *rslot.lock().expect("rslot") = Some(out);
    });

    let url = fake.url.clone();
    let app = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(120)));
    let deadline = std::time::Instant::now() + Duration::from_secs(100);
    while result.lock().expect("r").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(Err(e)) = result.lock().expect("r").clone() {
        panic!("driver failed: {e} (fed_bytes={})", harness.handle().raw_len());
    }
    harness.handle().settle_snapshot();

    let handle = harness.handle();
    let snaps: Vec<(u64, Vec<u64>, String)> = {
        let vt = handle.vt.lock().expect("vt");
        vt.snapshots()
            .iter()
            .map(|s| {
                let profile: Vec<u64> = (0..80).map(|x| s.grid.cell(x, 0).style).collect();
                (s.t_ms, profile, s.grid.row_text(0))
            })
            .collect()
    };
    // The per-frame observable: styles + text (the spinner glyph
    // lives in the text; the sweep band in the styles).
    let frame_key = |s: &(u64, Vec<u64>, String)| (s.1.clone(), s.2.clone());
    assert!(snaps.len() > 20, "capture too thin ({} snapshots)", snaps.len());

    // Cycle k's busy window (in harness-clock ms, relative to the
    // first step.started paint): find it by the spinner glyph — a
    // window is [first, last] snapshot whose row 0 carries a spinner
    // glyph. Between windows the row must be glyph-free.
    let glyph = |row: &str| row.chars().any(|c| "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(c));
    let mut windows: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<usize> = None;
    for (i, (_, _, row)) in snaps.iter().enumerate() {
        if glyph(row) {
            if open.is_none() {
                open = Some(i);
            }
        } else if let Some(start) = open.take() {
            windows.push((start, i));
        }
    }
    if let Some(start) = open.take() {
        windows.push((start, snaps.len()));
    }
    assert!(
        !windows.is_empty(),
        "the busy face never appeared (strip: {:?})",
        snaps.iter().map(|(_, _, r)| r.trim()).take(6).collect::<Vec<_>>()
    );
    // (a) BREATHING: no frozen stretch — while the busy face is on,
    // the row's style profile must keep changing (a ≥5-snapshot
    // identical run while a spinner glyph is up = the frozen lamp).
    let mut frozen = 0usize;
    let mut i = 0usize;
    while i + 5 <= snaps.len() {
        let stretch = &snaps[i..i + 5];
        if stretch.iter().all(|(_, _, row)| glyph(row))
            && stretch.windows(2).all(|w| frame_key(&w[0]) == frame_key(&w[1]))
        {
            frozen += 1;
            i += 5; // count once per stretch
        } else {
            i += 1;
        }
    }
    assert!(
        frozen == 0,
        "the busy strip froze for ≥5 consecutive frames in {frozen} stretch(es) — the 呼吸灯 miss reproduced"
    );
    // (b) MOVEMENT per window: every busy window with ≥2 snapshots
    // must show at least two distinct style profiles (the sweep band
    // crosses).
    let mut moving_windows = 0usize;
    for &(start, end) in &windows {
        let profiles: Vec<&Vec<u64>> = snaps[start..end].iter().map(|(_, p, _)| p).collect();
        let distinct: std::collections::HashSet<&Vec<u64>> = profiles.iter().copied().collect();
        if distinct.len() >= 2 {
            moving_windows += 1;
        }
    }
    assert!(
        moving_windows == windows.len(),
        "busy sweep frozen in {} of {} windows (breathing-lamp miss)",
        windows.len() - moving_windows,
        windows.len()
    );
    // (c) EXIT: the final snapshots (past the last scripted cycle)
    // must be idle — the running set cleared.
    let tail_idle = snaps.iter().rev().take(3).all(|(_, _, row)| !glyph(row));
    assert!(tail_idle, "the busy face never exited after the last session.execution.succeeded");
    eprintln!(
        "[w16-pulse] windows={} moving={} snapshots={}",
        windows.len(),
        moving_windows,
        snaps.len()
    );
    // The app must exit cleanly on the chord.
    let joined = {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !app.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        app.is_finished()
    };
    fake.stop();
    assert!(joined, "the TUI did not exit after the chord — teardown wedge (E9-class)");
}
