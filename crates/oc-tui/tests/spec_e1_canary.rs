//! E1 perf-feel stall canary (SPEC-R6; I8 per-axis tier — R5 §5 "E1 stall
//! canary"), against the v5 E1 clause method:
//!
//! * capture: real write-stamp series (harness-clock burst timestamps of
//!   our TUI in a PTY — the same observable the clause's ref-side
//!   "timestamped PTY capture (write-stamp deltas)" uses);
//! * detector: per cell, stall-burst = (>=2 consecutive missed drive
//!   ticks) OR (any gap >150ms while a cadence source is active);
//!   drive tick T = the cell's measured median inter-flush interval
//!   (the drive-source audit the clause names); cadence-active = a
//!   periodic flush stream is present in-window (median <= 250ms).
//!
//! LEG 1 (localization): the defect — a one-shot 250ms render stall — is
//! injected into a THROWAWAY COPY of exactly one cell's stamp series
//! (dialog-open under streaming); the detector must flag >=1 stall-burst
//! in that cell AND 0 in the three control cells.
//! LEG 2 (false-positive): a synthetic evenly-spaced flush series must
//! produce 0 findings.
//!
//! The product is never modified — the stall lives in the copied series.

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, HarnessHandle, LiveHarness, Snapshot, WORDMARK,
};

// ------------------------------------------------------------- detector

#[derive(Debug)]
struct CellAudit {
    name: &'static str,
    gaps_ms: Vec<u64>,
    median_gap: u64,
    cadence_active: bool,
    bursts: Vec<StallBurst>,
}
#[derive(Debug)]
struct StallBurst {
    at_ms: u64,
    gap_ms: u64,
    rule: &'static str,
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// The v5 E1 detector, verbatim semantics.
fn detect(name: &'static str, stamps: &[u64]) -> CellAudit {
    let mut gaps: Vec<u64> = stamps.windows(2).map(|w| w[1] - w[0]).collect();
    let mut sorted = gaps.clone();
    let med = if sorted.is_empty() { 0 } else { median(&mut sorted) };
    // Cadence source active: a periodic flush stream exists in-window
    // (spinner/toast/render tick emit every tick). Median <= 250ms.
    let cadence_active = med > 0 && med <= 250;
    let mut bursts = Vec::new();
    if cadence_active {
        let tick = med.max(1);
        for w in stamps.windows(2) {
            let gap = w[1] - w[0];
            let missed = gap.saturating_sub(tick) / tick; // ticks missed in this gap
            if gap > 150 {
                bursts.push(StallBurst { at_ms: w[0], gap_ms: gap, rule: ">150ms gap" });
            } else if missed >= 2 {
                bursts.push(StallBurst { at_ms: w[0], gap_ms: gap, rule: ">=2 missed ticks" });
            }
        }
    }
    CellAudit { name, gaps_ms: gaps, median_gap: med, cadence_active, bursts }
}

/// DEFECT INJECTION (throwaway copy): one-shot +250ms stall at `frac`
/// through the series — everything from that point shifts 250ms later,
/// exactly what a 250ms render-flush sleep looks like at the PTY boundary.
fn inject_one_shot_stall(stamps: &[u64], frac: f64, stall_ms: u64) -> Vec<u64> {
    let idx = ((stamps.len() as f64) * frac) as usize;
    stamps.iter().enumerate().map(|(i, t)| if i >= idx { t + stall_ms } else { *t }).collect()
}

// ------------------------------------------------------------- orchestration

fn boot_to_session(harness: &mut HarnessHandle) -> Result<(), String> {
    if !harness.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(harness, b"Alpha")?;
    if !harness.wait_for(b"Alpha", Duration::from_secs(5)) {
        return Err("the tab title never painted".to_owned());
    }
    Ok(())
}

fn window_stamps(harness: &HarnessHandle, window_ms: u64) -> Vec<u64> {
    let start = harness.now_ms();
    while harness.now_ms() - start < window_ms {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(120)); // close trailing burst
    harness.settle_snapshot();
    stamps_since(harness, start)
}

fn stamps_since(harness: &HarnessHandle, from_ms: u64) -> Vec<u64> {
    harness
        .vt
        .lock()
        .expect("vt")
        .snapshots()
        .iter()
        .filter(|s: &&Snapshot| s.t_ms >= from_ms)
        .map(|s| s.t_ms)
        .collect()
}

#[test]
fn e1_stall_canary_localizes_and_no_false_positives() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("e1canary");
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(streaming_thinking_script("ses_alpha", "count slowly"));
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();

    let cells: Arc<Mutex<Vec<(&'static str, Vec<u64>)>>> = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&cells);
    let result: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let rslot = Arc::clone(&result);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            boot_to_session(&mut driver)?;
            // W1 idle (control)
            let w1 = window_stamps(&driver, 1800);
            // W2 streaming (control): submit the prompt, wait for the turn.
            driver.write(b"count slowly\r");
            if !driver.wait_for(b"Thinking", Duration::from_secs(5)) {
                return Err("the streaming turn never showed its Thinking row".to_owned());
            }
            let w2 = window_stamps(&driver, 2200);
            // W3 dialog-open under streaming (the cell the canary patches)
            driver.write(&[0x10]); // ctrl+p -> command palette
            std::thread::sleep(Duration::from_millis(300)); // dialog paints
            let w3 = window_stamps(&driver, 2000);
            // W4 post-dialog (control)
            driver.write(&[0x1b]); // escape closes the palette
            std::thread::sleep(Duration::from_millis(300));
            let w4 = window_stamps(&driver, 1800);
            let mut c = slot.lock().expect("cells");
            c.push(("idle", w1));
            c.push(("streaming", w2));
            c.push(("dialog-under-streaming", w3));
            c.push(("post-dialog", w4));
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *rslot.lock().expect("rslot") = Some(out);
    });
    run_tui(&fake.url, 80, 24, Duration::from_secs(90));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result.lock().expect("rslot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(Err(e)) = result.lock().expect("rslot").take() {
        panic!("driver failed: {e}");
    }

    let cells = cells.lock().expect("cells").clone();
    assert_eq!(cells.len(), 4, "four matrix cells captured");
    for (name, stamps) in &cells {
        assert!(stamps.len() >= 8, "cell {name}: only {} flushes — capture too thin", stamps.len());
    }

    // Controls: UNPATCHED detector must be silent in all four cells.
    let audits: Vec<CellAudit> = cells.iter().map(|(n, s)| detect(n, s)).collect();
    for a in &audits {
        eprintln!(
            "[E1] drive-audit {}: flushes-gap median={}ms cadence_active={} natural_bursts={}",
            a.name,
            a.median_gap,
            a.cadence_active,
            a.bursts.len()
        );
    }
    let natural: usize = audits.iter().map(|a| a.bursts.len()).sum();
    assert_eq!(natural, 0, "false-positive leg on REAL data: {natural} natural stall-bursts — detector thresholds rejected (I8: redesign)");

    // LEG 1: inject the one-shot 250ms stall into a COPY of the
    // dialog-under-streaming cell only.
    let patched_stamps = inject_one_shot_stall(&cells[2].1, 0.6, 250);
    let patched = detect("dialog-under-streaming", &patched_stamps);
    eprintln!(
        "[E1] PATCHED cell bursts: {:?} (median gap {}ms)",
        patched.bursts, patched.median_gap
    );
    assert!(
        !patched.bursts.is_empty(),
        "canary FAILED to catch the injected 250ms stall — detector redesign required"
    );
    assert!(patched.bursts.iter().all(|b| b.rule == ">150ms gap" || b.rule == ">=2 missed ticks"));

    // Controls stay silent (their series untouched).
    for a in &audits {
        assert!(a.bursts.is_empty(), "control {} must stay silent", a.name);
    }

    // LEG 2: false-positive on synthetic evenly-spaced flushes.
    let synthetic: Vec<u64> = (0..80u64).map(|i| i * 100 + (i % 7) * 2).collect();
    let fp = detect("synthetic-even", &synthetic);
    assert_eq!(fp.bursts.len(), 0, "FP leg: evenly-spaced series must yield 0 findings");
    assert!(fp.cadence_active, "FP leg setup sanity: cadence detected");

    eprintln!("[E1] CANARY PASS: 250ms stall localized to 'dialog-under-streaming'; 0 bursts in 3 controls; 0 on synthetic even series");
}
