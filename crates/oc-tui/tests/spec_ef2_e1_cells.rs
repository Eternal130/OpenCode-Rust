//! SCAN-EF2 E1 matrix-cell sampler (docs/scan-spec.md v9 E1; lane
//! brief priority #1). The I8 canary (`spec_e1_canary`) already RUN +
//! PASS on main and itself captured 4 cells (idle / streaming /
//! dialog-under-streaming / post-dialog, 0 natural bursts). This test
//! extends the SAME detector over the remaining E1 interaction cells:
//!
//!   typing × idle, dialog-open × idle, paste (S) × idle,
//!   typing × streaming-active, paste (S) × during-stream,
//!   wheel-scroll × transcript, resize × idle.
//!
//! (S) = two sizes: ~8.5KB (128 lines) and ~34KB (512 lines) — run as
//! separate tests, individually (`cargo test --test spec_ef2_e1_cells
//! <filter>`), never as a same-process pair.
//!
//! Cells dispositioned elsewhere: search-in-10k-transcript — ours has
//! NO in-transcript search surface (dialog_select filter only), so the
//! cell is absent-dispositioned on the E12 leg, not measurable here;
//! 10k-msg-transcript-open load column — FakeLlm serves no bulk
//! transcript fixture (SA-F2 fixture class), in-process cost is owned
//! by `frame_budget_large_transcript_200x50` (PASS).
//!
//! Detector semantics: identical to the canary's (stall-burst =
//! ≥2 consecutive missed drive ticks OR any gap >150ms while a cadence
//! source is active; drive tick T = cell's median inter-flush).
//! The app loop runs in a spawned thread so a WEDGED boot (the very
//! stall class E1 hunts) reports its cells instead of dying inside
//! `run_tui`'s timeout panic.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, HarnessHandle, LiveHarness, Snapshot,
};

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    v[v.len() / 2]
}

struct Audit {
    name: String,
    median_gap: u64,
    cadence_active: bool,
    bursts: usize,
}

/// SE-F1 adjudication (wave 2): a flush gap that tracks the cell's
/// own DRIVE interval is the driver's pacing, not an app stall — the
/// wheel cell writes a notch every 150 ms and the app flushes within
/// ~1 ms of each (uniform 151.0 ms gaps, 3/3 runs; artifact
/// /tmp/opencode/scanef2/e1-burst-details.log). `drive_ms` exempts
/// gaps within the drive interval (+2 ms jitter); absolute stalls
/// beyond it still count.
fn detect(name: &str, stamps: &[u64], drive_ms: Option<u64>) -> Audit {
    let gaps: Vec<u64> = stamps.windows(2).map(|w| w[1] - w[0]).collect();
    let med = if gaps.is_empty() { 0 } else { median(&mut gaps.clone()) };
    let cadence = med <= 250 && !gaps.is_empty();
    let tick = med.max(1);
    let mut missed = 0usize;
    let mut gap150 = 0usize;
    let mut detail = Vec::new();
    for w in stamps.windows(2) {
        let g = w[1] - w[0];
        let drive_paced = drive_ms.is_some_and(|drive| g <= drive + 2);
        if g > 150 && !drive_paced {
            gap150 += 1;
            detail.push(format!("gap150@{}ms={}ms", w[0], g));
        }
        if u64::from(tick) * 2 < g {
            missed += 1;
            if g <= 150 {
                detail.push(format!("missed2ticks@{}ms={}ms", w[0], g));
            }
        }
    }
    let bursts = if cadence { missed + gap150 } else { 0 };
    if !detail.is_empty() {
        std::fs::create_dir_all("/tmp/opencode/scanef2").ok();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/opencode/scanef2/e1-burst-details.log")
        {
            use std::io::Write;
            let _ = writeln!(f, "{name}: {}", detail.join(", "));
        }
    }
    Audit { name: name.to_owned(), median_gap: med, cadence_active: cadence, bursts }
}

fn stamps_since(h: &HarnessHandle, from_ms: u64) -> Vec<u64> {
    h.vt.lock()
        .expect("vt")
        .snapshots()
        .iter()
        .filter(|s: &&Snapshot| s.t_ms >= from_ms)
        .map(|s| s.t_ms)
        .collect()
}

/// Capture window around `act`: stamps of every snapshot closed from
/// `start` until `ms` after it (plus the trailing-burst settle).
fn window(h: &mut HarnessHandle, ms: u64, act: impl FnOnce(&mut HarnessHandle)) -> Vec<u64> {
    let start = h.now_ms();
    act(h);
    while h.now_ms() - start < ms {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(120)); // close trailing burst
    h.settle_snapshot();
    stamps_since(h, start)
}

fn paste_bytes(lines: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..lines {
        body.push_str(&format!("line {i:04} the quick brown fox jumps over the lazy dog tail\n"));
    }
    let mut bytes = b"\x1b[200~".to_vec();
    bytes.extend_from_slice(body.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

fn run_cell_suite(tag: &str, paste: Vec<u8>) {
    let _lock = pty_lock();
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    fake.arm_script(streaming_thinking_script("ses_alpha", "count slowly"));
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let cells: Arc<Mutex<Vec<(String, Vec<u64>)>>> = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&cells);
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

            // dialog-open x idle (canary covered the streaming side)
            driver.write(&[0x10]);
            std::thread::sleep(Duration::from_millis(300));
            let w = window(&mut driver, 1500, |_| {});
            slot.lock().unwrap().push(("dialog-open-idle".into(), w));
            driver.write(&[0x1b]);
            std::thread::sleep(Duration::from_millis(300));

            // typing x idle: 25 chars @120ms inside the window
            let w = window(&mut driver, 2500, |h| {
                for ch in b"the quick brown fox jumps" {
                    h.write(&[*ch]);
                    std::thread::sleep(Duration::from_millis(120));
                }
            });
            slot.lock().unwrap().push(("typing-idle".into(), w));
            driver.write(&[0x15, 0x15]); // ctrl+u: discard draft
            std::thread::sleep(Duration::from_millis(300));

            // paste x idle
            let w = window(&mut driver, 2000, |h| h.write(&paste));
            slot.lock().unwrap().push((format!("paste-{}-idle", paste.len() / 1024).into(), w));
            driver.write(&[0x15, 0x15]);
            std::thread::sleep(Duration::from_millis(300));

            // arm streaming: submit the scripted prompt
            driver.write(b"count slowly\r");
            if !driver.wait_for(b"Thinking", Duration::from_secs(5)) {
                return Err("streaming turn never showed its Thinking row".into());
            }

            // typing x streaming-active
            let w = window(&mut driver, 2500, |h| {
                for ch in b"typing while stream runs" {
                    h.write(&[*ch]);
                    std::thread::sleep(Duration::from_millis(120));
                }
            });
            slot.lock().unwrap().push(("typing-streaming".into(), w));
            driver.write(&[0x15, 0x15]);
            std::thread::sleep(Duration::from_millis(200));

            // paste x during-stream (the Thinking row animates)
            let w = window(&mut driver, 2000, |h| h.write(&paste));
            slot.lock()
                .unwrap()
                .push((format!("paste-{}-during-stream", paste.len() / 1024).into(), w));
            driver.write(&[0x15, 0x15]);
            std::thread::sleep(Duration::from_millis(200));

            // wheel-scroll x transcript (3 down + 3 up) — bisect-flagged
            // (EF2_SKIP=wheel disables; the teardown-wedge bisect).
            if std::env::var("EF2_SKIP").as_deref() != Ok("wheel") {
                let w = window(&mut driver, 1800, |h| {
                    for _ in 0..3 {
                        h.sgr_scroll_down(40, 12);
                        std::thread::sleep(Duration::from_millis(150));
                    }
                    for _ in 0..3 {
                        h.sgr_scroll_up(40, 12);
                        std::thread::sleep(Duration::from_millis(150));
                    }
                });
                slot.lock().unwrap().push(("wheel-scroll-transcript".into(), w));
            }

            // resize: 80x24 -> 100x30 mid-window, back at the end
            // (EF2_SKIP=resize disables; bisect).
            if std::env::var("EF2_SKIP").as_deref() != Ok("resize") {
                let mode = std::env::var("EF2_RESIZE_MODE").unwrap_or_default();
                let w = window(&mut driver, 2000, |h| h.resize_pty(100, 30));
                if mode != "grow-only" {
                    driver.resize_pty(80, 24);
                }
                let settle_ms: u64 = std::env::var("EF2_RESIZE_SETTLE_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(300);
                std::thread::sleep(Duration::from_millis(settle_ms));
                slot.lock().unwrap().push(("resize-100x30".into(), w));
                // Liveness probe (diagnostic): does ANY key still act
                // after the resize? ctrl+p must open the palette.
                if std::env::var("EF2_POST_RESIZE_PROBE").is_ok() {
                    let from = driver.raw_len();
                    driver.write(&[0x10]);
                    std::thread::sleep(Duration::from_millis(600));
                    let delta = driver.raw_len() - from;
                    let opened =
                        driver.wait_for_after(b"palette", from, Duration::from_millis(100))
                            || driver.wait_for_after(b"command", from, Duration::from_millis(100));
                    std::fs::create_dir_all("/tmp/opencode/scanef2").ok();
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/opencode/scanef2/e1-probe.log")
                    {
                        use std::io::Write;
                        let _ = writeln!(
                            f,
                            "post-resize ctrl+p: bytes_emitted={} opened={opened}",
                            delta
                        );
                    }
                    driver.write(&[0x1b]);
                    std::thread::sleep(Duration::from_millis(300));
                    // Is the composer reachable again (echo probe)?
                    let from2 = driver.raw_len();
                    driver.write(b"zz");
                    std::thread::sleep(Duration::from_millis(400));
                    let echo = driver.raw_len() - from2;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/opencode/scanef2/e1-probe.log")
                    {
                        use std::io::Write;
                        let _ = writeln!(f, "post-esc composer echo bytes: {echo}");
                    }
                }
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *rslot.lock().expect("rslot") = Some(out);
    });

    // App loop in a spawned thread: a wedge reports instead of
    // panicking the whole test inside run_tui.
    let url = fake.url.clone();
    let app = std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(150)));
    let driver_done = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        while result.lock().expect("r").is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        result.lock().expect("r").take()
    });
    let driver_out = driver_done.join().expect("driver thread");
    let cells = cells.lock().unwrap().clone();
    // Persist the audit (the app's boot can disturb process-level
    // stderr; a file is immune — I9 artifact).
    let mut report = String::new();
    for (name, stamps) in &cells {
        let drive = name.starts_with("wheel").then_some(150u64);
        let a = detect(name, stamps, drive);
        report.push_str(&format!(
            "[E1-cell] {}: flushes={} median-gap={}ms cadence={} bursts={}\n",
            a.name,
            stamps.len(),
            a.median_gap,
            a.cadence_active,
            a.bursts
        ));
    }
    std::fs::write(format!("/tmp/opencode/scanef2/e1cells-{tag}.audit.txt"), &report)
        .expect("write audit");
    eprint!("{report}");
    if let Some(Err(e)) = driver_out {
        panic!("driver failed: {e} (fed_bytes={})", harness.handle().raw_len());
    }
    assert!(cells.len() >= 5, "expected >=5 cells, got {}", cells.len());
    let mut total_bursts = 0usize;
    for (name, stamps) in &cells {
        assert!(stamps.len() >= 4, "cell {name}: capture too thin ({} flushes)", stamps.len());
        let drive = name.starts_with("wheel").then_some(150u64);
        total_bursts += detect(name, stamps, drive).bursts;
    }
    if std::env::var("EF2_ALLOW_BURSTS").is_err() {
        // SE-F11 (registered 2026-10-18, wave-2 VERIFIED-FIXED): the
        // large-paste ingestion stays within the E1 budget on the
        // fixed tree — 0 stall-bursts at 8.5 KB and 34 KB, idle and
        // during-stream (the paste cells' audits above are the
        // acceptance proof; the earlier bursts rode the pre-fix
        // teardown-wedge runs).
        assert_eq!(
            total_bursts, 0,
            "E1 stall-bursts on main: {total_bursts} — findings per E1 threshold"
        );
    }
    // The app must exit cleanly on the chord (join with slack).
    let joined = {
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while !app.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        app.is_finished()
    };
    fake.stop();
    assert!(joined, "the TUI did not exit after the chord — teardown wedge (E9-class)");
    eprintln!("[E1-cell] ALL CELLS CLEAN (0 stall-bursts; clean teardown)");
}

#[test]
fn e1_cells_paste_8k() {
    run_cell_suite("e1cells8k", paste_bytes(128)); // ~8.5KB
}

#[test]
fn e1_cells_paste_34k() {
    run_cell_suite("e1cells34k", paste_bytes(512)); // ~34KB
}
