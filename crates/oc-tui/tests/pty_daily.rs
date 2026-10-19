//! C6c TUI-DAILY first slice — the three ledger rows' PTY proofs on
//! the REAL app loop (`oc_tui::app::run_with_size`) over a pty at
//! fixed sizes, driven with real keystrokes/SGR mouse:
//!
//! * TUI-3 `vertical_tabs_layout` (+ TD-25's compact rail children
//!   `vertical_rail_compact_search_button`, `vertical_rail_resize_
//!   drag`): `tabs.layout = "vertical"` renders the LEFT rail
//!   (`app.tsx:1347-1349`, `component/session-tabs.tsx:513+`), the
//!   content column reflows to `width − rail` (`app.tsx:585`), the
//!   compact rail carries the `⌕` search button + the `▄/▀` half-row
//!   seam (`component/session-tabs-rail.tsx:20-66`), and the
//!   `PaneResizeHandle` drag re-widths + persists the rail
//!   (`ui/pane-resize.ts`, `app.tsx:503-517`).
//! * TUI-11 `missing_slashes`: `/share /rename /timeline /fork
//!   /compact /undo /redo` appear in the composer's slash popup AND
//!   execute with v2 semantics (`routes/session/index.tsx:891-1010`).
//! * TUI-13 `cost_visible`: `session.usage.updated` lands a
//!   per-session cost; the sidebar context row paints `$X.XX spent`
//!   (`feature-plugins/sidebar/context.tsx:37`) and the composer
//!   footer's usage arm paints `$X.XX`
//!   (`feature-plugins/prompt/footer.tsx:44`).

// Terminal coordinates are u16 by VT definition; harness magnitudes
// are bounded — the truncation lints add nothing here.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

#[path = "vt_harness.rs"]
mod vt_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, HarnessHandle,
    LiveHarness, WORDMARK,
};

/// The vertical rail's default width (`SESSION_SIDEBAR_WIDTH`).
const RAIL_WIDTH: usize = 42;

/// Sandbox + `FakeLlm` + PTY + driver thread, real run loop on the test
/// body (the vt-stream suite's runner shape — the driver always
/// leaves the exit chord so `run_tui` returns).
fn run_daily_test<T: Send + 'static>(
    tag: &str,
    cols: u16,
    rows: u16,
    arm: impl FnOnce(&FakeLlm, &std::path::PathBuf),
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> T {
    let _lock = pty_lock();
    let sandbox = sandbox_env(tag);
    let fake = FakeLlm::spawn(standard_sessions());
    arm(&fake, &sandbox);
    let harness = LiveHarness::attach(cols, rows);
    let result_slot: Arc<Mutex<Option<Result<T, String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, cols, rows, Duration::from_secs(90));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match result_slot.lock().expect("slot").take() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!("the driver thread never reported (deadlock?); raw TAIL: {tail:?}")
        }
    };
    outcome
}

/// Poll the cell grid until `probe` passes (or the timeout elapses).
fn wait_grid(
    h: &HarnessHandle,
    probe: impl Fn(&vt_harness::Grid) -> bool,
    timeout: Duration,
) -> Result<vt_harness::Grid, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let grid = h.grid_now();
        if probe(&grid) {
            return Ok(grid);
        }
        if std::time::Instant::now() > deadline {
            return Err(format!("grid condition never met; last grid:\n{}", dump(&grid)));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// A compact multi-row dump for failure messages.
fn dump(grid: &vt_harness::Grid) -> String {
    (0..grid.rows).map(|y| format!("{y:2}|{}", grid.row_text(y))).collect::<Vec<_>>().join("\n")
}

/// Grid-based text probe (the raw stream interleaves SGR escapes
/// inside styled runs — needles must match the REASSEMBLED cells).
fn wait_text(h: &HarnessHandle, needle: &str, timeout: Duration) -> Result<(), String> {
    match wait_grid(h, |g| (0..g.rows).any(|row| g.row_text(row).contains(needle)), timeout) {
        Ok(_) => Ok(()),
        Err(grid) => Err(format!("{needle:?} never painted; grid:\n{grid}")),
    }
}

/// Boot: wordmark → open the first history session (one tab).
fn boot_to_session(h: &mut HarnessHandle, needle: &[u8]) -> Result<(), String> {
    if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
        return Err("the home wordmark never rendered".to_owned());
    }
    open_first_session(h, needle)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// TUI-3 — the vertical tab rail
// ---------------------------------------------------------------------------

/// TUI-3: `tabs.layout = "vertical"` renders the LEFT rail at 120
/// cols (`sessionTabsFitVertically`: 120 ≥ 42 + 64), the strip row 0
/// is GONE (v2 gates the horizontal strip on `!tabsVertical()`,
/// app.tsx:1353-1355), the tab title + `+ New session` add slot live
/// inside the 42-wide rail, and the content column (the composer's
/// `┃` border) reflows to x = 42 + inset. The add row's click
/// navigates home (v2 `tabs.add?.()` → route home).
#[test]
fn vertical_tabs_layout() {
    // The config content env overlay injects `tabs.layout` (the
    // CliConfig service's in-process seam — no config file needed).
    std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", r#"{"tabs":{"layout":"vertical"}}"#);
    let add_row = run_daily_test(
        "vert-layout",
        120,
        30,
        |_, _| {},
        |h| {
            boot_to_session(h, b"Alpha")?;
            // The rail: pad row 0, then the tab's number indicator +
            // title at row 1 (indicator box = numberWidth(2) wide,
            // right-aligned " 1", one pad cell, title from x=3 — the
            // non-compact rail geometry, session-tabs.tsx:1042-1057).
            let grid = wait_grid(
                h,
                |g| g.content_rect(0, 1, RAIL_WIDTH, 1).starts_with(" 1 Alpha"),
                Duration::from_secs(8),
            )?;
            // No horizontal strip: row 0 is the rail's blank pad —
            // no pill, no ` + ` affordance anywhere on it.
            assert!(
                grid.content_rect(0, 0, 120, 1).trim().is_empty(),
                "row 0 carries no horizontal strip in vertical mode"
            );
            // The detail row under the title (the tab's second row).
            // The add slot: `+ New session` inside the rail column.
            let grid = wait_grid(
                h,
                |g| (1..30).any(|y| g.content_rect(0, y, RAIL_WIDTH, 1).contains("+ New session")),
                Duration::from_secs(5),
            )?;
            let add_y = (1..30)
                .find(|y| grid.content_rect(0, *y, RAIL_WIDTH, 1).contains("+ New session"))
                .ok_or("the add row vanished")?;
            // The content column reflowed: the composer's `┃` border
            // sits at x = rail(42) + the route's paddingLeft inset
            // (2) = 44 on the composer rows (the box spans the
            // column's content width, session_screen.rs's mount).
            let composer_border = (20..30).any(|y| grid.cell(44, y).ch == '┃')
                || (20..30).any(|y| grid.cell(43, y).ch == '┃');
            if !composer_border {
                return Err(format!("no composer border at x≥42:\n{}", dump(&grid)));
            }
            // Interact: release on the add row → route home (the
            // wordmark re-renders in the content column).
            let add_row_u16 = u16::try_from(add_y).unwrap_or(29);
            h.sgr_motion(5, add_row_u16);
            h.sgr_press(5, add_row_u16);
            h.sgr_release(5, add_row_u16);
            if !h.wait_for(WORDMARK, Duration::from_secs(5)) {
                return Err("the add row's release never navigated home".to_owned());
            }
            Ok(add_y)
        },
    );
    std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
    assert!(add_row < 30);
}

/// TD-25 (TUI-3's compact child): a persisted narrow rail
/// (`verticalTabsWidth: 6` < `SESSION_TABS_COMPACT_BREAKPOINT` 12)
/// renders the rail controls — the centered `⌕` search button on row
/// 1 and the half-row seam on row 2 (`▄` while the tab below is
/// highlighted, session-tabs-rail.tsx:54-66) — and the `⌕` release
/// dispatches `session.list` (the Sessions dialog mounts).
#[test]
fn vertical_rail_compact_search_button() {
    // Seed the persisted rail width (v2's `useStorage().store
    // ("layout")` doc — `<state>/opencode/<channel>/tui/layout.json`)
    // once the sandbox state root exists.
    std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", r#"{"tabs":{"layout":"vertical"}}"#);
    run_daily_test(
        "vert-compact",
        120,
        30,
        |_, sandbox| {
            // The state root derives from the build's app name through
            // the same resolver the boot read uses (the sandbox's
            // XDG_STATE_HOME is live by the time the arm runs).
            let _ = sandbox;
            let state = oc_util::global_roots::resolve().state;
            let dir = state.join(oc_tui::storage::channel()).join("tui");
            std::fs::create_dir_all(&dir).expect("tui state dir");
            std::fs::write(dir.join("layout.json"), r#"{"verticalTabsWidth":6}"#)
                .expect("seed layout");
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // Row 1: the centered search glyph inside the 6-wide rail.
            let grid =
                wait_grid(h, |g| g.content_rect(0, 1, 6, 1).contains('⌕'), Duration::from_secs(8))?;
            // Row 2: the half-row seam — `▄` repeated across the rail
            // while the first tab below is highlighted (selected).
            let seam = grid.content_rect(0, 2, 6, 1);
            if seam != "▄▄▄▄▄▄" {
                return Err(format!("the half-row seam is not ▄×6: {seam:?}"));
            }
            // Compact tabs render as centered indicator rows (the idle
            // label = the title's first grapheme, "A").
            wait_grid(h, |g| g.content_rect(0, 3, 6, 1).contains('A'), Duration::from_secs(5))?;
            // Click the search button → `session.list`: the Sessions
            // dialog mounts over the content column (the history rows
            // re-list inside the dialog frame).
            let (x, y) = (2usize, 1usize);
            h.sgr_motion(x as u16, y as u16);
            h.sgr_press(x as u16, y as u16);
            h.sgr_release(x as u16, y as u16);
            wait_grid(
                h,
                |g| {
                    (0..30).any(|row| {
                        let text = g.content_rect(6, row, 114, 1);
                        text.contains("Sessions") || text.contains("Long Session Title")
                    })
                },
                Duration::from_secs(5),
            )?;
            Ok(())
        },
    );
    std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
}

/// TUI-3's drag-resize: pressing the `PaneResizeHandle` (the rail's
/// last column, `left = size − 1`) and dragging re-widths the rail to
/// `event.x + 1` (`fromMouse`, app.tsx:510), the release commits
/// (`clampSessionTabsWidth`: 5..min(72, total − 44)), and the width
/// persists to the `layout` storage doc.
#[test]
fn vertical_rail_resize_drag() {
    std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", r#"{"tabs":{"layout":"vertical"}}"#);
    let persisted_slot: std::sync::Arc<Mutex<Option<std::path::PathBuf>>> =
        Arc::new(Mutex::new(None));
    let slot = Arc::clone(&persisted_slot);
    run_daily_test(
        "vert-resize",
        120,
        30,
        move |_, _| {
            let path = oc_util::global_roots::resolve()
                .state
                .join(oc_tui::storage::channel())
                .join("tui")
                .join("layout.json");
            *slot.lock().expect("slot") = Some(path);
        },
        move |h| {
            boot_to_session(h, b"Alpha")?;
            wait_grid(
                h,
                |g| g.content_rect(0, 1, RAIL_WIDTH, 1).starts_with(" 1 Alpha"),
                Duration::from_secs(8),
            )?;
            // Drag from the handle (x = 41) to x = 9 → rail width 10.
            h.sgr_motion(41, 4);
            h.sgr_press(41, 4);
            h.sgr_drag(9, 4);
            h.sgr_release(9, 4);
            // The rail narrowed: the composer border moved to x = 10 + 2.
            wait_grid(h, |g| (20..30).any(|y| g.cell(12, y).ch == '┃'), Duration::from_secs(5))?;
            // The commit persisted the width (the `layout` doc's
            // verticalTabsWidth — v2's onCommit, app.tsx:512-517).
            let persisted = persisted_slot
                .lock()
                .expect("slot")
                .clone()
                .expect("the arm seeded the layout path");
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(text) = std::fs::read_to_string(&persisted) {
                    if text.contains(r#""verticalTabsWidth":10"#) {
                        break;
                    }
                }
                if std::time::Instant::now() > deadline {
                    return Err(format!("the drag never persisted; doc: {persisted:?}"));
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Ok(())
        },
    );
    std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
}

// ---------------------------------------------------------------------------
// TUI-11 — the seven missing slash commands
// ---------------------------------------------------------------------------

/// TUI-11: `/share /rename /timeline /fork /compact /undo /redo`
/// register in the slash popup (the ref table's titles as
/// descriptions) and EXECUTE on select — `session.share` toasts the
/// unimplemented notice, `session.timeline`/`session.fork` mount
/// their dialogs, `session.undo` toasts "Nothing to undo" on an empty
/// transcript, and the argument-taking `/rename` INSERTS
/// (`insertSlash`, autocomplete.tsx:481) whose submit intercept never
/// reaches the prompt POST (`argumentSlash`, prompt/index.tsx:1133-
/// 1146).
#[test]
fn missing_slashes() {
    let counter: Arc<Mutex<Option<Arc<std::sync::atomic::AtomicUsize>>>> =
        Arc::new(Mutex::new(None));
    let slot = Arc::clone(&counter);
    run_daily_test(
        "slashes",
        100,
        30,
        |fake, _| {
            *slot.lock().expect("slot") = Some(Arc::clone(&fake.prompt_posts));
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // Wait for the SESSION to own the frame before typing (the
            // dialog's Enter races the route mount; the tab strip
            // re-paints the session's title AFTER this point — the
            // dialog's own listing already put "Alpha" in the buffer).
            let from = h.raw_len();
            if !h.wait_for_after(b"Alpha", from, Duration::from_secs(5)) {
                return Err("the session route never mounted its tab".to_owned());
            }
            // Every one of the seven offers in the popup (title column =
            // the ref command titles). Grid-based probe: the raw stream
            // interleaves SGR escapes inside the padded title runs.
            for (typed, display, description) in [
                ("/sh", "/share", "Share session"),
                ("/rena", "/rename", "Rename session"),
                ("/tim", "/timeline", "Jump to message"),
                ("/fo", "/fork", "Fork session"),
                ("/comp", "/compact", "Compact session"),
                ("/un", "/undo", "Undo previous message"),
                ("/red", "/redo", "Redo"),
            ] {
                h.write(typed.as_bytes());
                let result = wait_grid(
                    h,
                    |g| {
                        (0..30).any(|row| {
                            let text = g.row_text(row);
                            text.contains(display) && text.contains(description)
                        })
                    },
                    Duration::from_secs(5),
                );
                if let Err(grid) = result {
                    return Err(format!("{display} never offered in the popup; grid:\n{grid}"));
                }
                h.write(b"\x1b"); // escape: dismiss the popup, keep the draft
                std::thread::sleep(Duration::from_millis(120));
                h.write(b"\x03"); // ctrl+c: clear the draft
                std::thread::sleep(Duration::from_millis(120));
            }
            // /share executes: the unavailable toast.
            h.write(b"/share");
            h.write(b"\r");
            wait_text(h, "Sharing is not implemented", Duration::from_secs(5))?;
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            // /timeline executes: the Timeline dialog mounts.
            h.write(b"/timeline\r");
            wait_text(h, "Timeline", Duration::from_secs(5))?;
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            // /fork executes: the Fork dialog's "Full session" row.
            h.write(b"/fork\r");
            wait_text(h, "Full session", Duration::from_secs(5))?;
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            // /undo executes: an empty transcript toasts the miss.
            h.write(b"/undo\r");
            wait_text(h, "Nothing to undo", Duration::from_secs(5))?;
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            // /rename is argument-taking: the popup selection INSERTS the
            // slash token (never the rename dialog). Deterministic on slow
            // runners: the original one-shot `/rename\r` write let the
            // first grid poll observe the composer draft ("/rename") while
            // the palette row ("Rename session") was still painted —
            // the Enter effect had not rendered yet (CI 2026-10-19). Split
            // the writes and wait for the CLOSED-palette state instead.
            h.write(b"/rename");
            if let Err(grid) = wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("Rename session")),
                Duration::from_secs(5),
            ) {
                return Err(format!("/rename palette never opened; grid:\n{grid}"));
            }
            h.write(b"\r");
            let grid = match wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("/rename"))
                        && (0..g.rows).all(|row| !g.row_text(row).contains("Rename session"))
                },
                Duration::from_secs(5),
            ) {
                Ok(grid) => grid,
                Err(grid) => return Err(format!("/rename never inserted; grid:\n{grid}")),
            };
            if (0..grid.rows).any(|y| grid.row_text(y).contains("Rename session")) {
                return Err("/rename select opened the dialog instead of inserting".to_owned());
            }
            // Submitting `/rename New name` runs the argument intercept:
            // the draft clears and NO prompt POST leaves the composer.
            h.write(b" New name\r");
            std::thread::sleep(Duration::from_millis(500));
            Ok(())
        },
    );
    // The whole flow must never have submitted prompt text (the
    // FakeLlm's prompt counter — the argument-slash intercept and the
    // popup selects all run locally, prompt/index.tsx:1142-1146).
    let posts = counter
        .lock()
        .expect("slot")
        .as_ref()
        .map_or(usize::MAX, |c| c.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(posts, 0, "no slash step may submit a prompt");
}

// ---------------------------------------------------------------------------
// TUI-13 — the session cost surfaces
// ---------------------------------------------------------------------------

/// The `session.usage.updated` fixture: one event lands a per-session
/// cost after the first prompt POST (the `FakeLlm` script anchor).
fn usage_cost_script(session: &str) -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    vec![vt_harness::ScriptEvent {
        delay_ms: 60,
        body: format!(
            r#"{{"id":"evt_usage","created":{now_ms},"type":"session.usage.updated","data":{{"sessionID":"{session}","cost":0.4235,"tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}}}"#
        ),
    }]
}

/// TUI-13: the sidebar context row paints `$0.42 spent`
/// (sidebar/context.tsx:37 — `cost > 0` gates the row even with no
/// token usage) and the composer footer's usage arm paints `$0.42`
/// (prompt/footer.tsx:44-46 — `status = [usage?, cost?]`).
#[test]
fn cost_visible() {
    run_daily_test(
        "cost",
        160,
        30,
        |fake, _| {
            fake.arm_script(usage_cost_script("ses_alpha"));
            // Pre-dismiss the sidebar's onboarding card (the boot read
            // restores `getting-started.json`) so the Context section
            // renders in the pane's first screen.
            let dir =
                oc_util::global_roots::resolve().state.join(oc_tui::storage::channel()).join("tui");
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("getting-started.json"), r#"{"dismissed":true}"#);
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // Submit a prompt: the armed script's usage event lands
            // right after the POST.
            h.write(b"hello\r");
            // The sidebar (docked at 160 cols): the Context block's
            // cost row.
            wait_grid(
                h,
                |g| (0..30).any(|y| g.content_rect(118, y, 42, 1).contains("$0.42 spent")),
                Duration::from_secs(8),
            )?;
            // The composer footer's usage arm: the money segment in
            // the status row (the content column's bottom chrome).
            wait_grid(
                h,
                |g| {
                    (20..30).any(|y| {
                        let text = g.content_rect(2, y, 116, 1);
                        text.contains("$0.42") && !text.contains("spent")
                    })
                },
                Duration::from_secs(8),
            )?;
            Ok(())
        },
    );
}
