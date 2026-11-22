//! W16 frame-rhythm quantification — the thinking indicator's flush
//! cadence under a live streaming turn (the ⑦''② "stutter" measurement).
//!
//! The port's frame pacer is the app loop's tick (app.rs). The ref's
//! model is opentui's work-compensated scheduler (renderer.ts:4772-4801:
//! `delay = max(1, targetFrameTime − overallFrameTime)`, rescheduled
//! from the frame's END), so the FRAME PERIOD stays at `target`
//! regardless of per-iteration work. A plain `sleep(100)` stacked
//! after the work makes the period `work + 100` — under a chunky
//! streaming batch the spinner's flushes jitter and occasionally
//! long-tail ("stutter").
//!
//! This test measures the inter-flush gap DISTRIBUTION of the VT
//! snapshot stream while the scripted thinking turn animates and pins
//! the post-fix shape (no >400 ms tail, p95 ≤ 250 ms). The audit line
//! (median/p95/max/σ + tail counts) is the quantification artifact
//! for the lane report (before/after numbers).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, LiveHarness,
};

fn percentile(mut v: Vec<u64>, p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    let idx = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[idx.min(v.len() - 1)]
}

fn stddev(mean: f64, v: &[u64]) -> f64 {
    let n = v.len().max(1) as f64;
    (v.iter()
        .map(|x| {
            let d = *x as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n)
        .sqrt()
}

const WINDOW_MS: u64 = 6_000;

fn audit(tag: &str, stamps: &[u64]) -> (u64, u64, f64, u64, usize, usize) {
    let gaps: Vec<u64> = stamps.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.len() >= 6, "phase {tag} capture too thin ({} gaps)", gaps.len());
    let median = percentile(gaps.clone(), 0.5);
    let p95 = percentile(gaps.clone(), 0.95);
    let max = gaps.iter().copied().max().unwrap_or(0);
    let mean = gaps.iter().sum::<u64>() as f64 / gaps.len() as f64;
    let sd = stddev(mean, &gaps);
    let tail250 = gaps.iter().filter(|g| **g > 250).count();
    let tail400 = gaps.iter().filter(|g| **g > 400).count();
    let seq = if tail250 > 0 { format!("{gaps:?}") } else { String::new() };
    let line = format!(
        "[w16-rhythm:{tag}] gaps={} median={}ms p95={}ms max={}ms stddev={:.1}ms >250ms={} >400ms={} gaps_seq={seq}",
        gaps.len(), median, p95, max, sd, tail250, tail400
    );
    eprintln!("{line}");
    std::fs::create_dir_all("/tmp/opencode/scanef2").ok();
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/opencode/scanef2/w16-frame-rhythm.audit.txt")
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "{line}")
        })
        .ok();
    (p95, max, sd, median, tail250, tail400)
}

#[test]
#[allow(clippy::too_many_lines)]
fn w16_frame_rhythm_thinking_stream() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("w16rhythm");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(streaming_thinking_script("ses_alpha", "count very slowly indeed"));
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let result: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let rslot = Arc::clone(&result);
    let _typer = {
        let _result = Arc::clone(&result);
        std::thread::spawn(move || {
            let out = (|| -> Result<(), String> {
                if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                    return Err("wordmark never rendered".into());
                }
                open_first_session(&mut driver, b"Alpha")?;
                if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                    return Err("tab title never painted".into());
                }
                driver.write(b"count very slowly indeed\r");
                if !driver.wait_for(b"Thinking", Duration::from_secs(5)) {
                    return Err("streaming turn never showed its Thinking row".into());
                }
                // Phase A (quiet): let the spinner animate alone for
                // the first half of the window — the pure tick-cadence
                // measure.
                std::thread::sleep(Duration::from_millis(WINDOW_MS / 2));
                // Phase B (load): type through the rest (ingest+render
                // work per keystroke — the "typing × streaming" cell,
                // the heaviest steady-state iteration mix).
                let began = std::time::Instant::now();
                let mut i = 0usize;
                while began.elapsed() < Duration::from_millis(WINDOW_MS / 2) {
                    let ch = b"the quick brown fox jumps over"[i % 30];
                    driver.write(&[ch]);
                    i += 1;
                    std::thread::sleep(Duration::from_millis(120));
                }
                // Hold the session live briefly past the window (the
                // scripted turn keeps streaming), then exit.
                std::thread::sleep(Duration::from_millis(1_000));
                driver.write(&[0x15, 0x15]); // ctrl+u: discard draft (E1's tail)
                std::thread::sleep(Duration::from_millis(300));
                Ok(())
            })();
            vt_harness::exit_chord(&mut driver);
            *rslot.lock().expect("rslot") = Some(out);
        })
    };

    let url = fake.url.clone();
    let app = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(45)));

    // Wait for the Thinking row (the spinner is animating from here),
    // then let the window elapse and read the flush stream once.
    let started = std::time::Instant::now();
    let mut thinking = false;
    while started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
        let handle = harness.handle();
        let vt = handle.vt.lock().expect("vt");
        let grid = vt.grid_now();
        let has_thinking = (0..grid.rows).any(|y| grid.row_text(y).contains("Thinking"));
        drop(grid);
        drop(vt);
        if has_thinking {
            thinking = true;
            break;
        }
    }
    assert!(thinking, "streaming turn never showed its Thinking row");
    let quiet_start = harness.handle().now_ms();
    std::thread::sleep(Duration::from_millis(WINDOW_MS / 2));
    let typing_start = harness.handle().now_ms();
    std::thread::sleep(Duration::from_millis(WINDOW_MS / 2 + 500));
    harness.handle().settle_snapshot();
    // Surface driver failures before the rhythm asserts.
    if let Some(Err(e)) = result.lock().expect("r").clone() {
        panic!("driver failed: {e} (fed_bytes={})", harness.handle().raw_len());
    }
    let stamps: Vec<u64> =
        harness.handle().vt.lock().expect("vt").snapshots().iter().map(|s| s.t_ms).collect();
    assert!(stamps.len() > 10, "capture too thin ({} flushes)", stamps.len());

    // Phase A: the QUIET spinner cadence (pure tick pacing — each
    // flush is a clock-tick frame advance).
    let phase_a: Vec<u64> =
        stamps.iter().copied().filter(|t| *t >= quiet_start && *t < typing_start).collect();
    // Phase B: typing × streaming (the keystroke cadence rides on top
    // of the tick; a pacing regression long-tails here — the W15
    // revert's E1 findings were exactly this cell's shape).
    let phase_b: Vec<u64> = stamps
        .iter()
        .copied()
        .filter(|t| *t >= typing_start && *t <= typing_start + WINDOW_MS / 2 + 400)
        .collect();

    let (q_a, mx_a, sd_a, med_a, t250_a, t400_a) = audit("quiet", &phase_a);
    let (q_b, mx_b, sd_b, med_b, t250_b, t400_b) = audit("typing", &phase_b);
    // No-stall baseline shape: no long-tail gap in EITHER phase and a
    // bounded p95. The W15/W16 anchor experiments' typing stalls were
    // never reproducible against the G-456 landing (the work-
    // compensated anchor, DEFAULT ON since G-456 — quiet σ 2-3 ms,
    // typing unchanged; the historical stalls attribute to the W16
    // lane's since-removed trace writes, see the app.rs wait-site
    // comment) — this typing phase is the standing regression pin.
    assert!(
        t400_a == 0 && t400_b == 0,
        "long-tail flush gap while the spinner animates (max {mx_a}/{mx_b} ms) — the un-paced work+tick stacking"
    );
    assert!(
        q_a <= 250 && q_b <= 250,
        "flush cadence jitter too wide (p95 {q_a}/{q_b} ms, σ {sd_a:.1}/{sd_b:.1}) — spinner stutter"
    );
    let _ = (med_a, med_b, t250_a, t250_b);
    // The app must exit cleanly on the chord (join with slack; a
    // swallowed teardown would mask a wedge).
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
