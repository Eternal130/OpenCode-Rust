//! E2 startup-TTI baseline (SPEC-R6 ratification; v5 E2 clause).
//!
//! Marker per the clause, operationally pinned for THIS artifact: T0 =
//! harness attach (the in-process boot the vt harness drives; the
//! standalone-binary process-spawn variant is the P3 field-run
//! refinement), T_interactive = first frame where a text input accepts+
//! echoes a key. Our boot lands on the HOME screen (no composer); the
//! first text input surface is the command palette (ctrl+p) — its
//! filter field is the echo target. The composer-on-session-screen
//! marker needs a session open (dialog interaction) and is therefore a
//! P3 field-run refinement; both markers are recorded where visible.
//!
//! N=10 wall-clock runs, median + p95 = the BASELINE artifact (clause
//! assert (b) also needs the ref measured identically under I10 —
//! BLOCKED this round: the ref runtime copy lacks full deps; see
//! docs/spec-round6.md).
//!
//! Protocol per run: attach -> wait wordmark (boot painted) -> ctrl+p
//! -> poll-send "z" every 50ms until the grid echoes it. Poll overhead
//! is included (conservative) and identical across runs.

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

fn percentile(sorted: &[u128], p: f64) -> u128 {
    let idx = (((sorted.len() as f64) - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
fn e2_tti_baseline_n10() {
    let _lock = pty_lock();
    let run = std::env::var("E2_RUN_IDX").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    let mut times = Vec::new();
    let mut wordmark_times = Vec::new();
    {
        let _sandbox = sandbox_env(&format!("e2tti{run}"));
        let fake = FakeLlm::spawn(standard_sessions());
        let harness = LiveHarness::attach(80, 24);
        let mut driver = harness.handle();
        let result: Arc<Mutex<Option<(u128, u128, u128)>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&result);
        let _app = {
            let url = fake.url.clone();
            std::thread::spawn(move || run_tui(&url, 80, 24, Duration::from_secs(60)))
        };
        std::thread::spawn(move || {
            // Boot painted?
            if !driver.wait_for(WORDMARK, Duration::from_secs(15)) {
                eprintln!("[E2] FAIL-STAGE: wordmark never painted");
                return;
            }
            let t_wordmark = driver.now_ms() as u128;
            // Composer marker (the clause's own marker): open the first
            // session (proven drive path from the E1 canary), then probe.
            if vt_harness::open_first_session(&mut driver, b"Alpha").is_err() {
                eprintln!("[E2] FAIL-STAGE: session dialog");
                return;
            }
            if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                eprintln!("[E2] FAIL-STAGE: session screen");
                return;
            }
            let t_session = driver.now_ms() as u128;
            // Poll-send until the composer echoes the probe key.
            loop {
                driver.write(b"z");
                std::thread::sleep(Duration::from_millis(50));
                let echoed = {
                    let vt = driver.vt.lock().expect("vt");
                    let grid = vt.grid_now();
                    (0..grid.rows).any(|y| grid.row_text(y).contains('z'))
                };
                if echoed {
                    *slot.lock().expect("slot") =
                        Some((t_wordmark, t_session, driver.now_ms() as u128));
                    vt_harness::exit_chord(&mut driver);
                    return;
                }
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(40);
        while result.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let t = result.lock().expect("slot").take();
        fake.stop();
        match t {
            Some((t_wordmark, t_session, t_echo)) => {
                eprintln!("[E2] run {run}: wordmark {t_wordmark} ms; session screen {t_session} ms; T_interactive (composer echo) = {t_echo} ms");
                wordmark_times.push(t_wordmark);
                times.push(t_echo);
            }
            None => panic!("run {run}: no echo within the window"),
        }
    }
    let _ = times.sort_unstable();
    let _ = wordmark_times.sort_unstable();
    let line = format!(
        "run={run} wordmark_ms={} session_ms={} t_interactive_ms={}\n",
        wordmark_times[0], times[0], times[0]
    );
    eprintln!("[E2] RUN-DONE {line}");
    let _ = std::fs::write(format!("/tmp/opencode/spec-r6/e2_run_{run}.txt"), &line);
}
