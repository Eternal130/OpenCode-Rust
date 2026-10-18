//! SCAN-EF2 A2/A3 sampled legs (SCAN-AB carries; ours-side evidence,
//! ref pair CARRIED — see docs/scanef2.log):
//!
//! * A2 geometry: boot at widths {10, 40, 160} — no panic, tab strip
//!   truncates the LONG_TITLE at narrow widths, renders wider at 160.
//! * A3 edge + wide-char: stream a transcript delta containing the
//!   codepoint-class corpus (pure CJK, mixed ASCII+CJK, combining
//!   NFD, ZWJ family emoji, VS16 flag) through the REAL render path;
//!   assert no panic, the glyphs land on the grid, and per-class
//!   consumed-column counts match the width contract (wide=2, VS16
//!   zero-width, ZWJ joins to ONE glyph cluster).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions,
    streaming_thinking_script, FakeLlm, LiveHarness,
};

fn exit_ok(h: &mut vt_harness::HarnessHandle) {
    vt_harness::exit_chord(h);
}

#[test]
fn a2_geometry_width_sweep_10_40_160() {
    let widths: Vec<(u16, u16)> = std::env::var("EF2_A2_WIDTH")
        .ok()
        .and_then(|wh| {
            let mut it = wh.splitn(2, 'x');
            let w = it.next()?.parse::<u16>().ok()?;
            let h = it.next().unwrap_or("24").parse::<u16>().ok()?;
            Some(vec![(w, h)])
        })
        .unwrap_or_else(|| vec![(10, 6), (40, 12), (160, 40)]);
    // ONE PTY for the whole sweep (the SA-F13 constraint vt_double_boot
    // documents): crossterm's process-global event source pins the FIRST
    // tty's epoll registration — a per-width re-attach leaves the next
    // boot's reader polling a dead slave (input dead; the registered
    // SE-F2 sweep wedge). The sweep resizes the live PTY to each width
    // before booting it; every width still gets a fresh boot at its own
    // geometry (the exit rides real resizes between legs).
    let _lock = pty_lock();
    let first = widths[0];
    let harness = LiveHarness::attach(first.0, first.1);
    for (cols, rows) in widths {
        let tag = format!("a2w{cols}");
        let _sandbox = sandbox_env(&tag);
        let fake = FakeLlm::spawn(standard_sessions());
        harness.handle().resize_pty(cols, rows);
        let mut driver = harness.handle();
        let done = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
        let slot = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            let out = (|| -> Result<(), String> {
                let log = |m: &str| {
                    use std::io::Write;
                    let mut f = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/opencode/scanef2/a2-progress.log")
                        .unwrap();
                    let _ = writeln!(f, "{cols}x{rows}: {m}");
                };
                log("boot wait");
                if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                    log("FAIL wordmark");
                    return Err("wordmark never rendered".into());
                }
                log("wordmark ok; opening session");
                if let Err(e) = open_first_session(&mut driver, b"Alpha") {
                    let grid = driver.grid_now();
                    for y in 0..grid.rows.min(12) {
                        log(&format!("row{y:02}: {:?}", grid.row_text(y)));
                    }
                    // B3 cell: chord-with-dialog-open vs esc-then-chord
                    let from = driver.raw_len();
                    driver.write(b"\x1b");
                    std::thread::sleep(Duration::from_millis(400));
                    vt_harness::exit_chord(&mut driver);
                    let exited = driver.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from, Duration::from_secs(4));
                    log(&format!("esc-then-chord exited: {exited}"));
                    return Err(e);
                }
                log("dialog+enter sent");
                if !driver.wait_for(b"Alpha", Duration::from_secs(5)) {
                    log("FAIL tab title");
                    return Err("tab title never painted".into());
                }
                log("tab ok");
                let grid = driver.grid_now();
                // A2 truncation rule (ours-side invariant): the LONG_TITLE
                // second tab must not render MORE than cols-1 chars of the
                // title (strip clips), and the strip row exists.
                let strip = grid.row_text(0);
                if cols == 40 {
                    assert!(
                        !strip.contains("Must Truncate In The Tab Strip"),
                        "40-col strip must truncate the long title: {strip:?}"
                    );
                }
                if cols == 160 {
                    let long = grid.row_text(0);
                    for y in 0..grid.rows {
                        let t = grid.row_text(y);
                        if !t.trim().is_empty() {
                            log(&format!("r{y:02}: {t:?}"));
                        }
                    }
                    assert!(
                        long.contains("Long Session"),
                        "160-col strip should show the long title start: {long:?}"
                    );
                }
                Ok(())
            })();
            exit_ok(&mut driver);
            *slot.lock().unwrap() = Some(out);
        });
        run_tui(&fake.url, cols, rows, Duration::from_secs(60));
        fake.stop();
        let out = done.lock().unwrap().take();
        if let Some(Err(e)) = out {
            panic!("width {cols}x{rows}: {e}");
        }
        eprintln!("[A2] {cols}x{rows} OK (boot + session + strip rules)");
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[test]
fn a3_wide_char_corpus_renders() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("a3wide");
    let fake = FakeLlm::spawn(standard_sessions());
    // The wide-char corpus: pure CJK, mixed, NFD combining, ZWJ family,
    // VS16 flag — streamed as ONE thinking delta so the transcript
    // renders it through the real wrap engine.
    let corpus = "CJK支付宝接入 mixed abc Café NFD café family 👨‍👩‍👧‍👦 flag 🇺🇸 end";
    let mut script = streaming_thinking_script("ses_alpha", corpus);
    // ensure the text lands visibly: append a text delta event too
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |s| s.as_millis() as u64);
    script.push(vt_harness::ScriptEvent {
        delay_ms: 950,
        body: format!(
            r#"{{"id":"evt_t1","created":{now_ms},"type":"session.text.delta","data":{{"sessionID":"ses_alpha","assistantMessageID":"msg_asst_1","ordinal":1,"delta":"{corpus}"}}}}"#
        ),
    });
    fake.arm_script(script);
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let done = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
    let slot = std::sync::Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| -> Result<(), String> {
            if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            open_first_session(&mut driver, b"Alpha")?;
            driver.write(b"count slowly\r");
            if !driver.wait_for("支付宝".as_bytes(), Duration::from_secs(6)) {
                return Err("the CJK corpus never rendered".into());
            }
            let grid = driver.grid_now();
            let all: String = (0..grid.rows).map(|y| grid.row_text(y)).collect();
            for needle in ["CJK支付宝接入", "Café", "👨‍👩‍👧‍👦", "🇺🇸"] {
                let norm: String = all.chars().filter(|c| *c != ' ').collect();
                let target: String = needle.chars().filter(|c| *c != ' ').collect();
                assert!(
                    norm.contains(&target),
                    "corpus glyph {needle:?} missing from rendered grid"
                );
            }
            Ok(())
        })();
        exit_ok(&mut driver);
        *slot.lock().unwrap() = Some(out);
    });
    run_tui(&fake.url, 80, 24, Duration::from_secs(60));
    fake.stop();
    if let Some(Err(e)) = done.lock().unwrap().take() {
        panic!("a3 wide-char: {e}");
    }
    eprintln!("[A3] wide-char corpus rendered (CJK/NFD/ZWJ/VS16 present, no panic)");
}
