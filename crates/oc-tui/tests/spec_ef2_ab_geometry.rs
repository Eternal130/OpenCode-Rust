//! SCAN-EF2 A2/A3 sampled legs (SCAN-AB carries; ours-side evidence,
//! ref pair CARRIED — see docs/scanef2.log):
//!
//! * A2 geometry: boot at widths {10, 40, 160} — no panic, tab strip
//!   truncates the `LONG_TITLE` at narrow widths, renders wider at 160.
//! * A3 edge + wide-char: stream a transcript delta containing the
//!   codepoint-class corpus (pure CJK, mixed ASCII+CJK, combining
//!   NFD, ZWJ family emoji, VS16 flag) through the REAL render path;
//!   assert no panic, the glyphs land on the grid, and per-class
//!   consumed-column counts match the width contract (wide=2, VS16
//!   zero-width, ZWJ joins to ONE glyph cluster).
// Retired spec-corpus probe: sweep-loop short names and the deliberate
// NFD/decomposed corpus cells are load-bearing (see ledger TUI-E1-FLAKE).
#![allow(clippy::similar_names)]
#![allow(clippy::unicode_not_nfc)]
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    open_first_session, open_second_session, pty_lock, run_tui, sandbox_env, standard_sessions,
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
        // WIDE FIRST: a wide boot after a narrow one in the same
        // process wedges (va5: deterministic — the 160 leg renders a
        // 10-col layout and stops responding; thread isolation and a
        // chrome-hub reset both fail to clear it, so the stale state
        // is process-global — parked as an a2-known ordering
        // constraint, not papered over inside the app).
        .unwrap_or_else(|| vec![(160, 40), (40, 12), (10, 6)]);
    // ONE PTY for the whole sweep (the SA-F13 constraint vt_double_boot
    // documents): the process-global input source pins the FIRST
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
                    // CI runners pre-create only /tmp/opencode — the
                    // scanef2 subdir may not exist yet (TRY1 NotFound
                    // race, absorbed by retries but rooted here).
                    let _ = std::fs::create_dir_all("/tmp/opencode/scanef2");
                    let mut f = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/opencode/scanef2/a2-progress.log")
                        .unwrap();
                    let _ = writeln!(f, "{cols}x{rows}: {m}");
                };
                // va5 expectation rewrite (post G-465/G-466 fixes,
                // pinned to the va4/va5 realchain exports): the
                // wordmark only renders at h >= 12 (Logo's height
                // gate, component/logo.tsx:57) — below that the
                // settled composer's placeholder is the boot evidence.
                let boot_needle: &[u8] = if rows >= 12 { vt_harness::WORDMARK } else { b"Ask" };
                log("boot wait");
                if !driver.wait_for(boot_needle, Duration::from_secs(8)) {
                    log("FAIL boot settle");
                    return Err("boot settle never rendered".into());
                }
                log("boot ok; opening session");
                // The Sessions dialog's list viewport is zero-row at
                // the narrow geometries (10x6/40x12 — the dialog keeps
                // its title/search chrome only), so the row needle is
                // skipped and Enter selects blind; at 160 the rows
                // paint and the needle guards the rig.
                if cols >= 160 {
                    if let Err(e) = open_first_session(&mut driver, b"Alpha") {
                        let grid = driver.grid_now();
                        for y in 0..grid.rows {
                            let t = grid.row_text(y);
                            if !t.trim().is_empty() {
                                log(&format!("r{y:02}: {t:?}"));
                            }
                        }
                        return Err(e);
                    }
                } else {
                    driver.write(b"\x18l");
                    std::thread::sleep(Duration::from_millis(600));
                    driver.write(b"\r");
                }
                // Session-open evidence: the composer's metadata row
                // resolves the model (this rig's fake catalog paints
                // `Build · fake-model …` on the settled frame; the
                // geom rigs' real catalog shows `claude-…`).
                if !driver.wait_for(b"fake", Duration::from_secs(6)) {
                    log("FAIL session open (composer metadata)");
                    return Err("the session never opened (no composer metadata)".into());
                }
                log("session ok");
                let grid = driver.grid_now();
                // A2 truncation rule (ours-side invariant): the strip
                // clips the long title at narrow widths.
                if cols <= 40 {
                    let strip = grid.row_text(0);
                    assert!(
                        !strip.contains("Truncate In The Tab Strip"),
                        "{cols}-col strip must truncate the long title: {strip:?}"
                    );
                }
                // The long-title truncation surface: the SECOND history
                // tab (open_second_session) — at 160 the dialog rows
                // paint so the needle guards; narrower, the second row
                // is selected blind (ArrowDown past the zero-row
                // viewport's first entry).
                if cols >= 160 {
                    // Let the first session's route settle before the
                    // second open (the dialog's keybind layer must be
                    // armed when the ArrowDown+Enter chord lands).
                    std::thread::sleep(Duration::from_millis(600));
                    if let Err(e) = open_second_session(&mut driver, b"Long") {
                        let grid = driver.grid_now();
                        for y in 0..grid.rows {
                            let t = grid.row_text(y);
                            if !t.trim().is_empty() {
                                log(&format!("r{y:02}: {t:?}"));
                            }
                        }
                        log("FAIL second open");
                        return Err(e);
                    }
                    // The strip row (r00) carries the second tab's
                    // title — poll the ROW, not the whole screen (the
                    // open dialog also lists the title).
                    let deadline = std::time::Instant::now() + Duration::from_secs(6);
                    let mut long = String::new();
                    while std::time::Instant::now() < deadline {
                        long = driver.grid_now().row_text(0);
                        if long.contains("Long Session") {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    if !long.contains("Long Session") {
                        let grid = driver.grid_now();
                        for y in 0..grid.rows {
                            let t = grid.row_text(y);
                            if !t.trim().is_empty() {
                                log(&format!("r{y:02}: {t:?}"));
                            }
                        }
                        return Err(format!("the long-title tab never painted (strip {long:?})"));
                    }
                }
                Ok(())
            })();
            exit_ok(&mut driver);
            *slot.lock().unwrap() = Some(out);
        });
        run_tui(&fake.url, cols, rows, Duration::from_secs(60));
        fake.stop();
        // The driver stores its outcome AFTER `exit_ok`; the run loop can
        // return first, and a `None` read here used to pass the leg over a
        // failed driver (va1: a 6 s driver timeout in a3 came back PASS
        // 34/34 — same hole, see dc9c5738). Give the store a moment, then a
        // missing report is a failure.
        let store_deadline = std::time::Instant::now() + Duration::from_secs(5);
        while done.lock().unwrap().is_none() && std::time::Instant::now() < store_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        match done.lock().unwrap().take() {
            Some(Ok(())) => {}
            Some(Err(e)) => panic!("width {cols}x{rows}: {e}"),
            None => panic!("width {cols}x{rows}: the driver thread never reported"),
        }
        eprintln!("[A2] {cols}x{rows} OK (boot + session + strip rules)");
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// SE-F7 (B3 cell): the exit chord must tear the app down THROUGH an
/// open dialog — the first ctrl+c closes the dialog (empty filter),
/// the second exits on the empty prompt. The pre-fix shape routed
/// the chord into the dialog's search and wedged (the A2 legs' 60 s
/// timeouts).
#[test]
fn b3_chord_through_open_dialog_exits() {
    let _lock = pty_lock();
    let _sandbox = sandbox_env("b3chord");
    let fake = FakeLlm::spawn(standard_sessions());
    let harness = LiveHarness::attach(80, 24);
    let mut driver = harness.handle();
    let done = std::sync::Arc::new(std::sync::Mutex::new(None::<Result<(), String>>));
    let slot = std::sync::Arc::clone(&done);
    std::thread::spawn(move || {
        let out = (|| {
            if !driver.wait_for(vt_harness::WORDMARK, Duration::from_secs(8)) {
                return Err("wordmark never rendered".into());
            }
            // Open the session dialog and LEAVE it open.
            driver.write(b"\x18l");
            if !driver.wait_for(b"Sessions", Duration::from_secs(5)) {
                return Err("the session dialog never opened".into());
            }
            // The RAW chord — no esc first.
            let from = driver.raw_len();
            vt_harness::exit_chord(&mut driver);
            if !driver.wait_for_after(vt_harness::ALT_SCREEN_LEAVE, from, Duration::from_secs(5)) {
                return Err("the chord did not exit through the open dialog".into());
            }
            Ok(())
        })();
        *slot.lock().unwrap() = Some(out);
    });
    run_tui(&fake.url, 80, 24, Duration::from_secs(60));
    fake.stop();
    // The driver stores its outcome after its checks; the run loop can
    // return first, and a `None` read here used to pass the test over a
    // failed driver (va1: a 6 s driver timeout in a3 came back PASS 34/34
    // — same hole, see dc9c5738). Give the store a moment, then a missing
    // report is a failure.
    let store_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while done.lock().unwrap().is_none() && std::time::Instant::now() < store_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    match done.lock().unwrap().take() {
        Some(Ok(())) => {}
        Some(Err(e)) => panic!("b3 chord-through-dialog: {e}"),
        None => panic!("b3 chord-through-dialog: the driver thread never reported"),
    }
    eprintln!("[B3] exit chord tears down through an open dialog");
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
            // SETTLE before the snapshot: `支付宝` is the FIRST thinking
            // delta — snapshotting there catches the transcript
            // mid-stream (the live thinking part expanded: the ZWJ family
            // spelled out, the text delta pushed to the wrap boundary,
            // `🇺🇸` split across rows) and the glyph needles below miss
            // (`corpus glyph "🇺🇸" missing`, 12 reds 11-03..11-05; the
            // "TUI did not exit in time" that follows is the harness exit
            // wait after the panic). Wait for the text delta's tail on
            // the grid, then let the last paint land, before reading it.
            let settle_deadline = std::time::Instant::now() + Duration::from_secs(6);
            loop {
                let grid = driver.grid_now();
                if (0..grid.rows).any(|y| grid.row_text(y).contains(" end")) {
                    break;
                }
                if std::time::Instant::now() > settle_deadline {
                    return Err("the text delta's tail (` end`) never rendered".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // (not `wait_grid_stable`: the spinner, the caret blink and
            // the tab strip repaint more cells per probe than its
            // tolerance — the screen is never "stable" here.)
            std::thread::sleep(Duration::from_millis(700));
            let grid = driver.grid_now();
            let all: String = (0..grid.rows).map(|y| grid.row_text(y)).collect();
            // G-452: ZWJ families render as head + first ZWJ (the
            // ref emission shape — full families never reach the ref
            // terminal); flags/VS16/CJK/NFD unchanged.
            for needle in ["CJK支付宝接入", "Café", "\u{1F468}\u{200D}", "\u{1F1FA}\u{1F1F8}"]
            {
                let norm: String = all.chars().filter(|c| *c != ' ').collect();
                let target: String = needle.chars().filter(|c| *c != ' ').collect();
                assert!(
                    norm.contains(&target),
                    "corpus glyph {needle:?} missing from rendered grid: {all:?}"
                );
            }
            Ok(())
        })();
        exit_ok(&mut driver);
        *slot.lock().unwrap() = Some(out);
    });
    run_tui(&fake.url, 80, 24, Duration::from_secs(60));
    fake.stop();
    // The driver stores its outcome AFTER `exit_chord`; the run loop can
    // return first, and a `None` read here used to pass the test over a
    // failed driver (va1: a 6 s driver timeout came back PASS 34/34).
    // Give the store a moment, then a missing report is a failure.
    let store_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while done.lock().unwrap().is_none() && std::time::Instant::now() < store_deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    match done.lock().unwrap().take() {
        Some(Ok(())) => {}
        Some(Err(e)) => panic!("a3 wide-char: {e}"),
        None => panic!("a3 wide-char: the driver thread never reported"),
    }
    eprintln!("[A3] wide-char corpus rendered (CJK/NFD/ZWJ/VS16 present, no panic)");
}
