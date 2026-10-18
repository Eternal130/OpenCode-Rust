//! FIXW2 lane repro (TEMPORARY — deleted before the final commit):
//! isolate SE-F2 (exit chord dead) triggers. Variants via FIXW2_MODE:
//!   seq    = boot 10x6, exit; boot 40x12, exit (the width-sweep shape)
//!   single = one boot 40x12, exit
//!   resize = one boot 80x24, TIOCSWINSZ 100x30, exit

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness};

fn boot_and_exit(cols: u16, rows: u16, resize_to: Option<(u16, u16)>, tag: &str) {
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(cols, rows);
    let mut driver = harness.handle();
    let done = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
    let slot = std::sync::Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            open_first_session(&mut driver, b"Alpha")?;
            if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("tab title never painted".into());
            }
            if let Some((w, h)) = resize_to {
                driver.resize_pty(w, h);
                std::thread::sleep(Duration::from_millis(600));
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().unwrap() = Some(out);
    });
    let handle = {
        let url = fake.url.clone();
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_tui(&url, cols, rows, Duration::from_secs(25));
            }));
            (r.is_ok(), start.elapsed())
        })
    };
    let (ok, elapsed) = handle.join().expect("boot thread");
    fake.stop();
    let out = done.lock().unwrap().take();
    eprintln!("[fixw2] {tag} {cols}x{rows} exited={ok} elapsed={elapsed:?} driver_out={out:?}");
    assert!(ok, "{tag}: the TUI did not exit on the chord (SE-F2 wedge)");
}

fn boot_and_exit2(cols: u16, rows: u16, tag: &str) {
    let _sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(cols, rows);
    let mut driver = harness.handle();
    let done = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
    let slot = std::sync::Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            open_first_session(&mut driver, b"Alpha")?;
            if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("tab title never painted".into());
            }
            // resize out and back (the e1_cells shape)
            driver.resize_pty(100, 30);
            std::thread::sleep(Duration::from_millis(600));
            driver.resize_pty(80, 24);
            std::thread::sleep(Duration::from_millis(600));
            // probe: is plain input alive? ctrl+p palette
            let from = driver.raw_len();
            driver.write(&[0x10]);
            std::thread::sleep(Duration::from_millis(600));
            let opened = driver.raw_len() - from;
            driver.write(&[0x1b]);
            std::thread::sleep(Duration::from_millis(300));
            eprintln!("[fixw2] post-resize ctrl+p emitted {opened} bytes");
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().unwrap() = Some(out);
    });
    let handle = {
        let url = fake.url.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_tui(&url, cols, rows, Duration::from_secs(25));
            }));
            r.is_ok()
        })
    };
    let ok = handle.join().expect("boot thread");
    fake.stop();
    let out = done.lock().unwrap().take();
    eprintln!("[fixw2] {tag} {cols}x{rows} exited={ok} driver_out={out:?}");
    assert!(ok, "{tag}: the TUI did not exit on the chord (SE-F2 wedge)");
}

#[test]
fn fixw2_repro() {
    let _lock = pty_lock();
    let mode = std::env::var("FIXW2_MODE").unwrap_or_else(|_| "seq".into());
    match mode.as_str() {
        "seq" => {
            boot_and_exit(10, 6, None, "seq-leg1");
            std::thread::sleep(Duration::from_millis(400));
            boot_and_exit(40, 12, None, "seq-leg2");
        }
        "single" => {
            let w: u16 = std::env::var("FIXW2_W").ok().and_then(|s| s.parse().ok()).unwrap_or(40);
            let h: u16 = std::env::var("FIXW2_H").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
            boot_and_exit(w, h, None, "single");
        }
        "resize" => boot_and_exit(80, 24, Some((100, 30)), "resize"),
        "resize-back" => {
            boot_and_exit2(80, 24, "resize-back");
        }
        other => panic!("unknown FIXW2_MODE {other}"),
    }
}
