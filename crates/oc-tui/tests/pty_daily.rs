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
// ---------------------------------------------------------------------------
// TUI-TD07 — queue undo/steer server semantics (C6b session-route)
// ---------------------------------------------------------------------------

#[path = "fixtures/route_server.rs"]
mod td07_route_server;

use td07_route_server::{
    message_page_body, session_json, user_row, MessagePage, ServerState as Td07Server,
};

/// The pty runner over the C6b route server (`pty_session_route`'s
/// shape, local to this proof).
#[allow(clippy::type_complexity)]
fn run_td07_test<T: Send + 'static>(
    tag: &str,
    state: Td07Server,
    drive: impl FnOnce(&mut HarnessHandle) -> Result<T, String> + Send + 'static,
) -> (T, std::sync::Arc<Td07Server>) {
    let _lock = pty_lock();
    let sandbox = sandbox_env(tag);
    drop(sandbox);
    let (url, shared, shutdown) = td07_route_server::spawn(state);
    let harness = LiveHarness::attach(100, 30);
    let slot: std::sync::Arc<std::sync::Mutex<Option<Result<T, String>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let result_slot = std::sync::Arc::clone(&slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        vt_harness::exit_chord(&mut driver);
        *result_slot.lock().expect("slot") = Some(outcome);
    });
    vt_harness::run_tui_boot(
        &url,
        100,
        30,
        Duration::from_secs(90),
        oc_tui::app::BootArgs {
            continue_last: false,
            session: Some("ses_route".to_owned()),
            prompt: None,
            auto: false,
        },
    );
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = match slot.lock().expect("slot").take() {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(800)..]);
            panic!("the driver thread never reported (deadlock?); raw TAIL: {tail:?}")
        }
    };
    (outcome, shared)
}

fn td07_wait_text(h: &HarnessHandle, needle: &str, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let grid = h.grid_now();
        if (0..grid.rows).any(|y| grid.row_text(y).contains(needle)) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let dump = (0..grid.rows)
                .map(|y| format!("{y:2}|{}", grid.row_text(y)))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("{needle:?} never painted; grid:\n{dump}"));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// Wait until `needle` leaves the grid (the post-keypress redraw
/// settles — an instant absent can race the frame).
fn td07_wait_absent(h: &HarnessHandle, needle: &str, timeout: Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let grid = h.grid_now();
        if (0..grid.rows).all(|y| !grid.row_text(y).contains(needle)) {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let dump = (0..grid.rows)
                .map(|y| format!("{y:2}|{}", grid.row_text(y)))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("{needle:?} never left the grid:\n{dump}"));
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// TUI-TD07: `<leader>return` over a RUNNING turn POSTs with
/// `delivery: "queue"` + the client-minted id (the server owns the
/// queue — no local drain re-POST), and the manager's ctrl+u undo
/// cancels the SAME inbox id server-side (`session.inbox.cancel`)
/// while `appendPrompt` refills the composer with the payload text
/// (index.tsx:627-675/2201-2249).
#[test]
fn queue_undo_refills() {
    // A trailing user message with no assistant reply and no idle
    // marker: the session reads RUNNING (the queue path's gate).
    let rows = vec![user_row("msg_run", &td07_route_server::json_string("running turn"))];
    let state = Td07Server {
        requests: std::sync::Mutex::new(Vec::new()),
        sse: std::sync::Mutex::new(Vec::new()),
        events: std::sync::atomic::AtomicU64::new(1),
        sessions: vec![session_json("ses_route", "Route", None)],
        pages: vec![MessagePage {
            session: "ses_route".to_owned(),
            cursor: None,
            body: message_page_body(&rows, None),
        }],
    };
    let ((), server) = run_td07_test("td07", state, |h| {
        // Boot probe: the composer paints + takes keys.
        td07_wait_text(h, "shift+tab agents", Duration::from_secs(15))?;
        h.write(b"q1w2e3");
        td07_wait_text(h, "q1w2e3", Duration::from_secs(15))?;
        h.write(b"\x7f\x7f\x7f\x7f\x7f\x7f");
        // The queued submit: the text lands in the SERVER inbox (the
        // dock renders the optimistic admit).
        h.write(b"undo me refill");
        td07_wait_text(h, "undo me refill", Duration::from_secs(10))?;
        h.write(b"\x18\r"); // <leader>return — prompt.queue
        td07_wait_text(h, "1 queued", Duration::from_secs(10))?;
        // The manager: `<leader>q`, then ctrl+u (undo) — the cancel
        // fires server-side, the payload refills the composer, the
        // dialog + dock clear.
        h.write(b"\x18q");
        td07_wait_text(h, "Queued prompts", Duration::from_secs(10))?;
        h.write(b"\x15"); // ctrl+u — queued_prompt.undo
                          // The undo closes the manager (a POSITIVE wait — the composer
                          // refill paints the payload) and empties the queue (the dock
                          // leaves after the redraw).
        td07_wait_absent(h, "Queued prompts", Duration::from_secs(10))?;
        td07_wait_text(h, "undo me refill", Duration::from_secs(10))?;
        td07_wait_absent(h, "1 queued", Duration::from_secs(10))?;
        Ok(())
    });
    // The queue submit POST carried the delivery + the minted id.
    let queue_post = server
        .requests_with("/prompt")
        .into_iter()
        .find(|request| request.body.contains("undo me refill"))
        .expect("the queue submit POST");
    assert!(
        queue_post.body.contains("\"delivery\":\"queue\""),
        "the queued POST carries the queue delivery: {}",
        queue_post.body
    );
    let inbox_id = queue_post
        .body
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("the minted inbox id in the POST body")
        .to_owned();
    assert!(!inbox_id.is_empty(), "the id is non-empty: {inbox_id:?}");
    // The undo cancelled the SAME id server-side.
    let cancel_path = format!("/api/session/ses_route/inbox/{inbox_id}");
    assert!(
        server.requests_with(&cancel_path).iter().any(|request| request.method == "DELETE"),
        "the undo cancelled inbox id {inbox_id}: {:?}",
        server
            .requests
            .lock()
            .expect("requests")
            .iter()
            .map(|request| format!("{} {}", request.method, request.path))
            .collect::<Vec<_>>()
    );
    // No drain re-POST: exactly ONE prompt POST for the text.
    assert_eq!(
        server
            .requests_with("/prompt")
            .iter()
            .filter(|request| request.body.contains("undo me refill"))
            .count(),
        1,
        "the queued text POSTed exactly once (server-owned queue)"
    );
}

// ---------------------------------------------------------------------------
// lane-dialogs (wave 5): TUI-8 / TUI-9 / TUI-10 — the keyboard-visible
// dialog proofs on the real app loop.
// ---------------------------------------------------------------------------

/// TUI-8: `/skills` opens the `DialogSkill` (large, the padded name
/// column + the description); enter inserts `@{skill} ` into the
/// composer (prompt/index.tsx:583-605).
#[test]
fn skills_dialog() {
    run_daily_test(
        "lane-dialogs-skills",
        120,
        30,
        |fake, _sandbox| {
            fake.arm_skills();
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // `/skills` in the composer: the slash popup offers the
            // entry; enter RUNS it (the no-arguments rule).
            h.write(b"/skills");
            wait_text(h, "Skills", Duration::from_secs(5))?;
            h.write(b"\r");
            // The dialog mounts: the title + the skill rows.
            let grid = wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("greet"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("Greet the user"))
                },
                Duration::from_secs(8),
            )?;
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("lint")),
                "the second skill row lists"
            );
            // Enter selects the filtered row (greet) → the composer
            // carries the @mention.
            h.write(b"\r");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("@greet")),
                Duration::from_secs(8),
            )?;
            Ok(())
        },
    );
}

/// TUI-9: the stash workflow — Stash prompt clears the composer,
/// Stash pop restores it, Stash list opens the `DialogStash` with the
/// entry row (prompt/index.tsx:874-920 + dialog-stash.tsx).
#[test]
fn stash_dialog() {
    run_daily_test("lane-dialogs-stash", 120, 30, |_fake, _sandbox| {}, stash_driver);
}

/// The stash flow body (extracted so failures can dump the raw tail).
fn stash_driver(h: &mut HarnessHandle) -> Result<(), String> {
    boot_to_session(h, b"Alpha")?;
    // Type a draft.
    for chunk in [b"stash me once".as_slice(), b" and twice".as_slice()] {
        h.write(chunk);
        std::thread::sleep(Duration::from_millis(120));
    }
    wait_grid(
        h,
        |g| (0..g.rows).any(|row| g.row_text(row).contains("and twice")),
        Duration::from_secs(5),
    )?;
    // Palette → "Stash prompt" (prompt.stash): the draft stashes +
    // the composer resets. Filter FIRST (the unfiltered list scrolls
    // past the Prompt group at this height).
    h.write(b"\x10");
    wait_text(h, "Commands", Duration::from_secs(5))?;
    for key in b"stash prompt" {
        h.write(&[*key]);
        std::thread::sleep(Duration::from_millis(40));
    }
    wait_text(h, "Stash prompt", Duration::from_secs(5))?;
    h.write(b"\r");
    wait_grid(
        h,
        |g| !(0..g.rows).any(|row| g.row_text(row).contains("and twice")),
        Duration::from_secs(5),
    )?;
    // Palette → "Stash list" (prompt.stash.list): the dialog lists
    // the stashed preview.
    h.write(b"\x10");
    wait_text(h, "Commands", Duration::from_secs(5))?;
    for key in b"stash list" {
        h.write(&[*key]);
        std::thread::sleep(Duration::from_millis(40));
    }
    wait_text(h, "Stash list", Duration::from_secs(5))?;
    h.write(b"\r");
    wait_grid(
        h,
        |g| {
            (0..g.rows).any(|row| g.row_text(row).contains("Stash"))
                && (0..g.rows).any(|row| g.row_text(row).contains("stash me once"))
        },
        Duration::from_secs(8),
    )?;
    // Enter pops + restores the draft (PromptRef.set).
    h.write(b"\r");
    wait_grid(
        h,
        |g| (0..g.rows).any(|row| g.row_text(row).contains("and twice")),
        Duration::from_secs(8),
    )?;
    Ok(())
}

/// TUI-10: `<leader>i` opens the `DialogImagePreview` over the
/// composer's pasted image attachment — the `Image 1 of 1` header,
/// the blocks-quantized pixels, the caption footer
/// (dialog-image-preview.tsx + prompt/index.tsx:493-498).
#[test]
fn image_preview() {
    run_daily_test(
        "lane-dialogs-image",
        120,
        30,
        |_fake, sandbox| {
            // A real decodable PNG into the sandbox: the paste path
            // reads the file and stages the `[Image 1]` chip.
            let png: &[u8] = include_bytes!("fixtures/r07-tiny-4x2.png");
            std::fs::write(sandbox.join("tiny.png"), png).expect("seed the png");
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // Bracketed-paste the image's absolute path: the
            // composer stages the attachment chip.
            let sandbox = std::env::var("OPENCODE_TEST_HOME").expect("the sandbox env");
            let pasted = format!("{sandbox}/tiny.png");
            h.write(b"\x1b[200~");
            h.write(pasted.as_bytes());
            h.write(b"\x1b[201~");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("[Image 1]")),
                Duration::from_secs(8),
            )?;
            // `<leader>i` (prompt.images.view): the preview mounts.
            h.write(b"\x18i");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("Image 1 of 1")),
                Duration::from_secs(8),
            )?;
            // The blocks pixels painted (the block-glyph family).
            wait_grid(
                h,
                |g| {
                    (0..g.rows)
                        .any(|row| g.row_text(row).chars().any(|ch| "▀▄█▌▐▘▝▖▗▚▞▙▛▜▟".contains(ch)))
                },
                Duration::from_secs(5),
            )?;
            // Escape closes.
            h.write(b"\x1b");
            wait_grid(
                h,
                |g| !(0..g.rows).any(|row| g.row_text(row).contains("Image 1 of 1")),
                Duration::from_secs(5),
            )?;
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// lane-tui2 (wave 7): TUI-5 / TUI-6 / TUI-7 — the open-menu and
// session-list dialog proofs on the real app loop.
// ---------------------------------------------------------------------------

/// A rich `SessionInfo` wire row (the harness's armed `session_rows`).
/// An empty `title` OMITS the field (the wire's optional — the
/// timestamped-fallback rows). `updated_ms` is epoch MILLISECONDS —
/// the wire's `DateTimeUtcFromMillis` contract (schema/session.ts:47).
fn rich_session(
    id: &str,
    title: &str,
    updated_ms: f64,
    directory: &str,
    project: &str,
    parent: Option<&str>,
) -> String {
    let title_json =
        if title.is_empty() { String::new() } else { format!(r#""title":"{title}","#) };
    let parent_json = parent.map(|p| format!(r#","parentID":"{p}""#)).unwrap_or_default();
    format!(
        r#"{{"id":"{id}",{title_json}"time":{{"created":0,"updated":{updated_ms}}},"location":{{"directory":"{directory}"}},"projectID":"{project}"{parent_json},"cost":0,"tokens":{{"cache":{{"read":0,"write":0}},"input":0,"output":0,"read":0,"write":0,"reasoning":0}}}}"#
    )
}

/// The café row's title (the ASCII form under `DBG_ASCII` — the
/// bisect scaffold's byte-for-byte variant).
fn cafe_title() -> &'static str {
    if std::env::var("DBG_ASCII").is_ok() {
        "Cafe notes"
    } else {
        "Café notes"
    }
}

/// The test-side wall clock in epoch MILLISECONDS (`Date.now()`).
#[allow(clippy::cast_precision_loss)] // wall-clock ms fits the f64 mantissa exactly
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64
}

/// TUI-5: `ctrl+o` (`open.menu`) opens `DialogOpen` — title "Open",
/// the Sessions recents AND the Projects sections together, the
/// project row's basename title + abbreviated directory footer
/// (dialog-open.tsx:197-274), large size.
#[test]
fn open_menu_sections() {
    let now = now_ms();
    run_daily_test(
        "tui2-open-menu",
        120,
        30,
        |fake, _sandbox| {
            fake.arm_session_rows(vec![
                rich_session("ses_alpha", "Alpha", now - 120_000.0, "/tmp/proj-a", "prj_a", None),
                // 30 s old: the footer's `timeAgo` reads "now" at
                // render (dialog-open.tsx:499 — `minutes < 1`).
                rich_session(
                    "ses_recent",
                    "Recent Fix",
                    now - 30_000.0,
                    "/tmp/proj-a",
                    "prj_a",
                    None,
                ),
                rich_session("ses_old", "Ancient Work", 100_000.0, "/tmp/proj-a", "prj_a", None),
            ]);
            fake.arm_projects(
                r#"[{"canonical":"/tmp/proj-a","id":"prj_a","name":"Proj A","sandboxes":[],"time":{"active":0,"created":0,"updated":0},"vcs":"git"}]"#.to_owned(),
            );
        },
        |h| {
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            // ctrl+o — the Open menu (NOT the Sessions list).
            h.write(b"\x0f");
            let grid = wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Recent Fix"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("Proj A"))
                },
                Duration::from_secs(8),
            )?;
            // The mounted dialog's title is "Open" (dialog-open.tsx:338)
            // with the two section headers (category "Sessions" /
            // "Projects", dialog-open.tsx:225/264).
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("Open")),
                "the Open title mounted"
            );
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("Projects")),
                "the Projects section header lists"
            );
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("Sessions")),
                "the Sessions section header lists"
            );
            // The session row's footer: the project label + time-ago
            // (dialog-open.tsx:226 — `{label} · {timeAgo}`, "now" for
            // a minute-old session).
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("now")),
                "the recent row's time-ago footer"
            );
            // The project row's footer carries the abbreviated
            // directory (dialog-open.tsx:249).
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("/tmp/proj-a")),
                "the project row's directory footer"
            );
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            Ok(())
        },
    );
}

/// TUI-6: the Sessions dialog's rows carry the date-group headers
/// ("Today" for same-day sessions — dialog-session-list.tsx:187-189),
/// the timestamped-fallback title for untitled sessions, the
/// outside-project footer basename (:152-160) and the footer action
/// hints (delete/rename, :247-287).
#[test]
fn session_list_rows() {
    let now = now_ms();
    run_daily_test(
        "tui2-session-rows",
        120,
        30,
        |fake, _sandbox| {
            fake.arm_session_rows(vec![
                rich_session("ses_alpha", "Alpha", now - 120_000.0, "/tmp/proj-a", "prj_a", None),
                rich_session(
                    "ses_today",
                    "Today Work",
                    now - 300_000.0,
                    "/tmp/proj-a",
                    "prj_a",
                    None,
                ),
                // Untitled + outside the project: the ISO fallback
                // title + the directory basename footer.
                rich_session("ses_fallback", "", 100_000.0, "/elsewhere/scratch", "prj_a", None),
            ]);
            fake.arm_projects(
                r#"[{"canonical":"/tmp/proj-a","id":"prj_a","name":"Proj A","sandboxes":[],"time":{"active":0,"created":0,"updated":0},"vcs":"git"}]"#.to_owned(),
            );
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            h.write(b"\x18l");
            let grid = wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("Today Work")),
                Duration::from_secs(8),
            )?;
            // The "Today" group header (same-day time.updated).
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).trim().contains("Today")),
                "the Today group header lists"
            );
            // The untitled row shows the timestamped fallback title
            // ("New session - <ISO>", session-title-fallback.ts:38).
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("New session - ")),
                "the untitled row's timestamped fallback title"
            );
            // The outside-project row's footer basename.
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("scratch")),
                "the outside-project footer basename"
            );
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            Ok(())
        },
    );
}

/// TUI-7 (E12 F1-F6): the session-list filter is trim + case-fold +
/// byte-exact substring — a whitespace-only query is the FULL list,
/// fuzzy cross-letter sequences do NOT match, children never row,
/// and case-insensitive substrings do.
#[test]
#[allow(clippy::too_many_lines)] // one E2E driver, the six filter probes
fn session_filter_semantics() {
    let now = now_ms();
    run_daily_test(
        "tui2-filter",
        120,
        30,
        |fake, _sandbox| {
            if std::env::var("DBG_CLONE_TUI6").is_ok() {
                let today_title = if std::env::var("DBG_LOGIN_TITLE").is_ok() {
                    "Login Redirect Flow"
                } else {
                    "Today Work"
                };
                let (alpha_age, today_age) = if std::env::var("DBG_TIMES").is_ok() {
                    (600_000.0, 60_000.0)
                } else {
                    (120_000.0, 300_000.0)
                };
                let mut tui6_rows = vec![
                    rich_session(
                        "ses_alpha",
                        "Alpha",
                        now - alpha_age,
                        "/tmp/proj-a",
                        "prj_a",
                        None,
                    ),
                    rich_session(
                        "ses_today",
                        today_title,
                        now - today_age,
                        "/tmp/proj-a",
                        "prj_a",
                        None,
                    ),
                    if std::env::var("DBG_CAFE_ROW").is_ok() {
                        rich_session(
                            "ses_fallback",
                            "Café notes",
                            now - 90_000.0,
                            "/tmp/proj-a",
                            "prj_a",
                            None,
                        )
                    } else {
                        rich_session(
                            "ses_fallback",
                            "",
                            100_000.0,
                            "/elsewhere/scratch",
                            "prj_a",
                            None,
                        )
                    },
                ];
                if std::env::var("DBG_CHILD_ROW").is_ok() {
                    tui6_rows.push(rich_session(
                        "ses_child",
                        "Login Child Secret",
                        now - 30_000.0,
                        "/tmp/proj-a",
                        "prj_a",
                        Some("ses_today"),
                    ));
                }
                fake.arm_session_rows(tui6_rows);
                fake.arm_projects(
                    r#"[{"canonical":"/tmp/proj-a","id":"prj_a","name":"Proj A","sandboxes":[],"time":{"active":0,"created":0,"updated":0},"vcs":"git"}]"#.to_owned(),
                );
            } else {
                let (dir, prj) = if std::env::var("DBG_DIRS").is_ok() {
                    ("/tmp/proj-a", "prj_a")
                } else {
                    ("/tmp", "p")
                };
                let cafe_age =
                    if std::env::var("DBG_OLD_CAFE").is_ok() { 100_000.0 } else { now - 90_000.0 };
                let mut rows = vec![
                    rich_session("ses_alpha", "Alpha", now - 600_000.0, dir, prj, None),
                    rich_session(
                        "ses_login",
                        "Login Redirect Flow",
                        now - 60_000.0,
                        dir,
                        prj,
                        None,
                    ),
                    rich_session("ses_cafe", "Café notes", cafe_age, dir, prj, None),
                ];
                rows.push(rich_session(
                    "ses_child",
                    "Login Child Secret",
                    now - 30_000.0,
                    dir,
                    prj,
                    Some("ses_login"),
                ));
                fake.arm_session_rows(rows);
            }
        },
        |h| {
            eprintln!("DBG: driver alive");
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("DBG wordmark never came".to_owned());
            }
            eprintln!("DBG: wordmark ok");
            let from = h.raw_len();
            h.write(b"\x18l");
            eprintln!("DBG: leader-l written");
            if !h.wait_for_after(b"Alpha", from, Duration::from_secs(5)) {
                return Err("DBG alpha never listed".to_owned());
            }
            eprintln!("DBG: alpha listed");
            h.write(b"\r");
            eprintln!("DBG: enter written");
            std::thread::sleep(Duration::from_millis(300));
            h.write(b"\x18l");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("Login Redirect Flow")),
                Duration::from_secs(8),
            )?;
            eprintln!("DBG: list shown");
            // F6 (empty query): the full list — both parents row…
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains(cafe_title())),
                Duration::from_secs(5),
            )?;
            eprintln!("DBG: cafe visible");
            // …F1 (children NEVER row — the options builder's
            // parentID filter, dialog-session-list.tsx:145).
            let grid = h.grid_now();
            if (0..grid.rows).any(|row| grid.row_text(row).contains("Login Child Secret")) {
                return Err("the child session rowed in the session list".to_owned());
            }
            // F4 (substring, case-folded): "LOGIN RED" matches the
            // parent; the child stays excluded under a query.
            h.write(b"LOGIN RED");
            eprintln!("DBG: typed LOGIN RED");
            let _ = wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("Login Redirect Flow")),
                Duration::from_secs(5),
            )?;
            eprintln!("DBG: login matched");
            // TUI-7 probe repair (synchronization only — the asserts
            // below are untouched): the open tab's strip keeps this
            // title painted UNDER the dialog, so the presence wait
            // above is satisfied before the query's keystrokes land
            // (~100 ms of pty→input-thread→loop latency). The
            // EXCLUSION — the non-matching row leaving — is the
            // observable that proves the query reached the filter.
            wait_grid(
                h,
                |g| (0..g.rows).all(|row| !g.row_text(row).contains(cafe_title())),
                Duration::from_secs(5),
            )?;
            let grid = h.grid_now();
            if (0..grid.rows).any(|row| grid.row_text(row).contains(cafe_title())) {
                return Err("a non-matching row stayed visible under the filter".to_owned());
            }
            // F4 (no fuzzy): "zln" (cross-letter) matches NOTHING.
            // TUI-7 probe repair (arithmetic only): "LOGIN RED" is
            // NINE characters — nine backspaces clear it fully, so
            // the bare "zln" query (and F6's three-backspace clear
            // below) type exactly what the probes' comments say.
            for _ in 0..9 {
                h.write(b"\x7f");
            }
            h.write(b"zln");
            eprintln!("DBG: typed zln");
            let grid = wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("No sessions found")),
                Duration::from_secs(5),
            )?;
            eprintln!("DBG: no-match shown");
            // TUI-7 probe repair (scoping only): row 0 is the tab
            // strip, which legitimately keeps the OPEN TAB's title
            // painted under the dialog (v2 overlays the strip, it
            // never blanks it) — the no-fuzzy contract is about the
            // DIALOG's rows (1..).
            if (1..grid.rows).any(|row| grid.row_text(row).contains("Login Redirect Flow")) {
                return Err("a fuzzy cross-letter query matched (zln)".to_owned());
            }
            // F6 (whitespace-only = trimmed to empty = the full list).
            for _ in 0..3 {
                h.write(b"\x7f");
            }
            h.write(b"   ");
            wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Login Redirect Flow"))
                        && (0..g.rows).any(|row| g.row_text(row).contains(cafe_title()))
                },
                Duration::from_secs(5),
            )?;
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            Ok(())
        },
    );
}

/// TUI-7 — the LIVE-LOOP pin of the session-list filter's semantics,
/// with probes synchronized on state CHANGES (not on needles the tab
/// strip keeps painted): the open tab is titled "Login Redirect Flow"
/// and the strip stays visible under the dialog, so every wait below
/// keys on rows only the DIALOG can paint (Café notes / Alpha / the
/// no-match copy). The matching row's survival is asserted after the
/// EXCLUSION has been observed — the exclusion is the query-landed
/// proof. Semantics (dialog-session-list.tsx):
/// * :87-99 — trim + case-fold + byte-exact substring;
/// * :96 — the empty query is the FULL set;
/// * :98/:145/:185 — `!parentID`, children never row;
/// * :210 — `skipFilter`, never the shared fuzzy picker.
#[test]
fn tui7_filter_semantics_live_loop() {
    let now = now_ms();
    run_daily_test(
        "tui7-live-filter",
        120,
        30,
        |fake, _sandbox| {
            fake.arm_session_rows(vec![
                rich_session("ses_alpha", "Alpha", now - 600_000.0, "/tmp", "p", None),
                rich_session("ses_login", "Login Redirect Flow", now - 60_000.0, "/tmp", "p", None),
                rich_session("ses_cafe", "Café notes", now - 90_000.0, "/tmp", "p", None),
                rich_session(
                    "ses_child",
                    "Login Child Secret",
                    now - 30_000.0,
                    "/tmp",
                    "p",
                    Some("ses_login"),
                ),
            ]);
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // The dialog over the session route: the FULL parent set
            // lists (the café row only the dialog can paint).
            h.write(b"\x18l");
            let grid = wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains(cafe_title())),
                Duration::from_secs(8),
            )
            .map_err(|error| format!("the full list never showed: {error}"))?;
            // Parent exclusion: the child NEVER rows, even unfiltered.
            assert!(
                !(0..grid.rows).any(|row| grid.row_text(row).contains("Login Child Secret")),
                "the child session rowed in the session list"
            );
            // Substring, case-folded: "LOGIN RED" keeps the match…
            h.write(b"LOGIN RED");
            // On a loaded runner the filter lands in two repaint phases: the
            // dialog rows drop the café entry first, the leaked underlying
            // cells (the active tab's "Alpha") a frame later. Wait for BOTH
            // non-matching rows to leave instead of asserting on the first
            // intermediate snapshot (the production filter is atomic; the
            // screen composition is not).
            let grid = wait_grid(
                h,
                |g| {
                    (0..g.rows).all(|row| {
                        !g.row_text(row).contains(cafe_title())
                            && !g.row_text(row).contains("Alpha")
                    })
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the non-matching rows never left: {error}"))?;
            // …and the exclusion is the query-landed proof, so the
            // match's survival is now a real observation.
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("Login Redirect Flow")),
                "the case-folded substring match vanished"
            );
            // No fuzzy: the cross-letter sequence matches NOTHING.
            for _ in 0..9 {
                h.write(b"\x7f");
            }
            h.write(b"zln");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("No sessions found")),
                Duration::from_secs(5),
            )?;
            let grid = h.grid_now();
            assert!(
                !(0..grid.rows).any(|row| grid.row_text(row).contains("Alpha")),
                "a fuzzy cross-letter query matched (zln)"
            );
            assert!(
                !(0..grid.rows).any(|row| grid.row_text(row).contains("Login Child Secret")),
                "the child rowed under a no-match query"
            );
            // Whitespace-only = trimmed to empty = the FULL set.
            for _ in 0..3 {
                h.write(b"\x7f");
            }
            h.write(b"   ");
            let grid = wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains(cafe_title()))
                        && (0..g.rows).any(|row| g.row_text(row).contains("Alpha"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| {
                format!("the whitespace-only query never restored the list: {error}")
            })?;
            assert!(
                !(0..grid.rows).any(|row| grid.row_text(row).contains("Login Child Secret")),
                "the child rowed under the whitespace-only query"
            );
            h.write(b"\x1b");
            std::thread::sleep(Duration::from_millis(150));
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// TUI-12 — the composer footer's live N subagents / N shells segment
// ---------------------------------------------------------------------------

/// TUI-12's event script: a running CHILD session in `ses_alpha`'s
/// family (`session.created` with `parentID` + `session.execution.
/// started`) and one in-flight shell owned by `ses_alpha`
/// (`shell.created` carrying `metadata.sessionID` — the client data
/// cache's live shell map, solid/data.ts:1245-1261).
fn live_counts_script(session: &str) -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    vec![
        vt_harness::ScriptEvent {
            delay_ms: 60,
            body: format!(
                r#"{{"id":"evt_child","created":{now_ms},"type":"session.created","data":{{"sessionID":"ses_child","parentID":"{session}","title":"Child work","directory":"/tmp"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 80,
            body: format!(
                r#"{{"id":"evt_exec","created":{now_ms},"type":"session.execution.started","data":{{"sessionID":"ses_child"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 100,
            body: format!(
                r#"{{"id":"evt_shell","created":{now_ms},"type":"shell.created","data":{{"info":{{"id":"sh_live","command":"ls","cwd":"/tmp","file":"","shell":"/bin/sh","status":"running","metadata":{{"sessionID":"{session}"}},"time":{{"started":0}}}}}}}}"#
            ),
        },
    ]
}

/// TUI-12: the composer footer's live segment
/// (`feature-plugins/prompt/footer.tsx:20-33,47,65-94`) — the family's
/// RUNNING subagents (excluding self) + the session's in-flight shells
/// paint as `1 subagent · 1 shell` (the `session.child.first`
/// shortcut prefix in text.base), the segment LIFTS under the pointer
/// (muted → text.base), and a left release toggles the subagent picker
/// (`onMouseUp` → `session.child.first`).
#[test]
fn composer_live_counts() {
    run_daily_test(
        "tui12-live",
        120,
        30,
        |fake, _| {
            fake.arm_script(live_counts_script("ses_alpha"));
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // Trigger the armed script (it plays after the first
            // prompt POST): the family child + the live shell land.
            h.write(b"hello\r");
            // The live segment paints on the composer's footer row
            // (the status row at the bottom of the content column).
            let grid = wait_grid(
                h,
                |g| {
                    (20..30).any(|y| {
                        let text = g.content_rect(0, y, 120, 1);
                        text.contains("1 subagent") && text.contains("1 shell")
                    })
                },
                Duration::from_secs(10),
            )
            .map_err(|error| format!("the live segment never painted: {error}"))?;
            let y = (20..30)
                .find(|y| {
                    let text = grid.content_rect(0, *y, 120, 1);
                    text.contains("1 subagent") && text.contains("1 shell")
                })
                .ok_or("the live row vanished")?;
            let row = grid.content_rect(0, y, 120, 1);
            let seg_x = row
                .find("1 subagent")
                .map(|byte| row[..byte].chars().count())
                .ok_or("the subagent count's x")?;
            // The shortcut prefix paints with the counts (v2 renders
            // `shortcut("session.child.first")` — `down` — before the
            // segment, footer.tsx:77-79).
            assert!(
                row.contains("down 1 subagent"),
                "the session.child.first shortcut prefixes the segment: {row}"
            );
            let prefix_x = row
                .find("down 1 subagent")
                .map(|byte| row[..byte].chars().count())
                .expect("the prefix x");
            // Hover: the counts lift muted → text.base (the style
            // digest of a count cell changes while the char stays).
            let before = grid.cell(seg_x, y);
            h.sgr_motion(u16::try_from(seg_x).unwrap_or(60), u16::try_from(y).unwrap_or(29));
            let lifted = wait_grid(
                h,
                |g| g.cell(seg_x, y).ch == before.ch && g.cell(seg_x, y).style != before.style,
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the live segment never lifted under the pointer: {error}"))?;
            // Moving off the segment clears the lift (onMouseOut).
            h.sgr_motion(60, 5);
            wait_grid(h, |g| g.cell(seg_x, y).style == before.style, Duration::from_secs(5))
                .map_err(|error| format!("the lift never cleared off-segment: {error}"))?;
            // Release ON the segment dispatches session.child.first
            // (the subagent picker's toggle — the Subagents tab strip
            // mounts over the composer).
            let x = u16::try_from(prefix_x + 2).unwrap_or(60);
            let y16 = u16::try_from(y).unwrap_or(29);
            h.sgr_motion(x, y16);
            h.sgr_press(x, y16);
            h.sgr_release(x, y16);
            wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Subagents"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("Shell"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the live release never opened the picker: {error}"))?;
            let _ = lifted;
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// TUI-14 — the shell-mode `!` prefix
// ---------------------------------------------------------------------------

/// TUI-14: `!` typed at caret 0 on an empty normal-mode draft enters
/// SHELL MODE (prompt/index.tsx:961-981's `bind: "!"` layer — the `!`
/// itself never inserts), the footer flips to `esc exit shell mode`
/// (footer.tsx:107-112), the metadata row labels `Shell`
/// (prompt/index.tsx:1570), escape/backspace-at-0 exit back to normal,
/// and Enter submits through `session.shell` (POST
/// /api/session/:id/shell, prompt/index.tsx:1303-1307) — the mode
/// resets to normal after the send.
/// TUI-14/TUI-15's shared POST-registry slot type (the fake's
/// recorder handed out through the arm closure).
type SharedPosts = Arc<Mutex<Option<Arc<Mutex<Vec<String>>>>>>;

#[test]
fn shell_bang_mode() {
    // The fake's shell-POST registry, shared out through the arm
    // closure (the assertion reads it after the app loop returns).
    let shell_slot: SharedPosts = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&shell_slot);
    run_daily_test(
        "tui14-bang",
        120,
        30,
        |fake, _| {
            *slot.lock().expect("slot") = Some(Arc::clone(&fake.shell_posts));
        },
        |h| {
            boot_to_session(h, b"Alpha")?;
            // The composer settles (the agents hint paints).
            wait_text(h, "shift+tab agents", Duration::from_secs(10))?;
            // `!` at caret 0 enters shell mode: the footer's esc hint
            // replaces the shortcut hints (footer.tsx:107-112 — the
            // whole PromptFooter content swaps for the shell Match).
            h.write(b"!");
            wait_grid(
                h,
                |g| {
                    (20..30).any(|y| {
                        let text = g.content_rect(0, y, 120, 1);
                        text.contains("exit shell mode") && !text.contains("agents")
                    })
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the shell footer never painted: {error}"))?;
            // The metadata row labels the mode "Shell"
            // (prompt/index.tsx:1570's agentLabel override — the row
            // keeps the box's `┃` rail + pads, so probe the painted
            // run). The dogfood3 harness now serves a model catalog
            // (TUI-TD15's gate), so the row carries the resolved
            // model after the mode label — match the prefix.
            wait_grid(
                h,
                |g| {
                    (20..30).any(|y| {
                        let row = g.content_rect(2, y, 116, 1);
                        row.trim().starts_with("┃  Shell")
                    })
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the Shell metadata label never painted: {error}"))?;
            // The `!` itself never inserts — typing continues the
            // command draft.
            h.write(b"echo hello");
            wait_grid(
                h,
                |g| (20..30).any(|y| g.content_rect(2, y, 116, 1).contains("echo hello")),
                Duration::from_secs(5),
            )?;
            // Escape exits shell mode (the draft KEEPS its text —
            // v2 only flips the mode, index.tsx:989-992).
            h.write(b"\x1b");
            wait_grid(
                h,
                |g| {
                    (20..30).all(|y| !g.content_rect(0, y, 120, 1).contains("exit shell mode"))
                        && (20..30).any(|y| g.content_rect(2, y, 116, 1).contains("echo hello"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("escape never exited shell mode: {error}"))?;
            // Clear the draft (ctrl+c), re-enter, and submit: the
            // command leaves through session.shell.
            h.write(b"\x03");
            wait_grid(
                h,
                |g| (20..30).all(|y| !g.content_rect(2, y, 116, 1).contains("echo hello")),
                Duration::from_secs(5),
            )?;
            h.write(b"!");
            wait_grid(
                h,
                |g| (20..30).any(|y| g.content_rect(0, y, 120, 1).contains("exit shell mode")),
                Duration::from_secs(5),
            )?;
            h.write(b"pwd");
            h.write(b"\r");
            // The mode resets (the esc hint leaves) and the footer's
            // normal hints return.
            wait_grid(
                h,
                |g| (20..30).any(|y| g.content_rect(0, y, 120, 1).contains("shift+tab agents")),
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the mode never reset after the shell send: {error}"))?;
            // Backspace at caret 0 also exits: re-enter on the now
            // empty draft, then backspace.
            h.write(b"!");
            wait_grid(
                h,
                |g| (20..30).any(|y| g.content_rect(0, y, 120, 1).contains("exit shell mode")),
                Duration::from_secs(5),
            )?;
            h.write(b"\x7f");
            wait_grid(
                h,
                |g| (20..30).all(|y| !g.content_rect(0, y, 120, 1).contains("exit shell mode")),
                Duration::from_secs(5),
            )
            .map_err(|error| format!("backspace-at-0 never exited shell mode: {error}"))?;
            Ok(())
        },
    );
    // The shell submit reached the server: POST /api/session/ses_alpha/
    // shell carrying the command text.
    let posts = shell_slot
        .lock()
        .expect("slot")
        .as_ref()
        .map(|arc| arc.lock().expect("posts").clone())
        .unwrap_or_default();
    assert!(
        posts.iter().any(|post| post.contains("pwd")),
        "the shell command never POSTed; saw {posts:?}"
    );
}

// ---------------------------------------------------------------------------
// TUI-15 — the Zed editor context
// ---------------------------------------------------------------------------

/// TUI-15's Zed sqlite fixture (editor-zed.ts's table shapes): one
/// workspace whose paths contain `workspace_dir` (the app's cwd), one
/// ACTIVE editor on `notes.md` with multi-line contents, and a
/// selection covering the second and third lines (the `#2-3` label).
/// The db FILE lives under `db_dir` (the sandbox).
fn zed_fixture_db(db_dir: &std::path::Path, workspace_dir: &std::path::Path) -> std::path::PathBuf {
    let db_path = db_dir.join("zed").join("db.sqlite");
    std::fs::create_dir_all(db_path.parent().expect("zed dir")).expect("zed dir");
    let connection = rusqlite::Connection::open(&db_path).expect("open zed db");
    let contents = "line one\nline two\nline three\n";
    let start = contents.find("line two").expect("line two");
    let end = contents.len().saturating_sub(1);
    connection
        .execute_batch(&format!(
            "create table items (item_id integer primary key, kind text, pane_id integer, workspace_id integer, active integer);
             create table panes (pane_id integer, workspace_id integer, active integer);
             create table workspaces (workspace_id integer primary key, paths text, timestamp text);
             create table editors (item_id integer, workspace_id integer, buffer_path text, contents text);
             create table editor_selections (editor_id integer, workspace_id integer, start integer, end integer);
             insert into workspaces values (1, '[\"{0}\"]', '2026-01-02T00:00:00Z');
             insert into panes values (10, 1, 1);
             insert into items values (100, 'Editor', 10, 1, 1);
             insert into editors values (100, 1, '{0}/notes.md', '{1}');
             insert into editor_selections values (100, 1, {2}, {3});",
            workspace_dir.display(),
            contents,
            start,
            end
        ))
        .expect("zed schema");
    drop(connection);
    db_path
}

/// TUI-15: inside Zed's terminal (`ZED_TERM=true` +
/// `OPENCODE_ZED_DB`), the 1 s poll resolves the active editor's
/// selection and the composer footer paints the `prompt.footer.file`
/// label (`notes.md #2-3`, prompt/index.tsx:240-250,1945); submitting
/// attaches the context FIRST through `session.synthetic`
/// (index.tsx:1339-1346) — the `<system-reminder>` block carrying the
/// selected text precedes the prompt POST.
#[test]
fn zed_context() {
    std::env::set_var("ZED_TERM", "true");
    let db_slot: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));
    let synthetic_slot: SharedPosts = Arc::new(Mutex::new(None));
    let arm_db = Arc::clone(&db_slot);
    let arm_syn = Arc::clone(&synthetic_slot);
    let driver_syn = Arc::clone(&synthetic_slot);
    run_daily_test(
        "tui15-zed",
        120,
        30,
        |fake, sandbox| {
            // The db file lives under the sandbox; the WORKSPACE it
            // declares is the app's cwd (the scoring root). Assert the
            // fixture exists BEFORE the app boots — a runner with an
            // odd /tmp (noexec mounts are fine for sqlite, but a
            // failed create would silently fall the poll through to
            // the $HOME platform defaults and the label would never
            // paint).
            let cwd = std::env::current_dir().unwrap_or_else(|_| sandbox.clone());
            let db = zed_fixture_db(sandbox, &cwd);
            assert!(db.is_file(), "the zed fixture db never materialized at {}", db.display());
            *arm_db.lock().expect("db slot") = Some(db.clone());
            std::env::set_var("OPENCODE_ZED_DB", &db);
            *arm_syn.lock().expect("syn slot") = Some(Arc::clone(&fake.synthetic_posts));
        },
        move |h| {
            let synthetic_slot = driver_syn;
            boot_to_session(h, b"Alpha")?;
            // The composer must be SETTLED before typing (the tui7
            // lesson: the route transition from the session dialog is
            // not synchronized with the composer mount — a keystroke
            // racing it lands in the dialog's select, not the prompt).
            wait_text(h, "shift+tab agents", Duration::from_secs(10))
                .map_err(|error| format!("the composer never settled: {error}"))?;
            // The poll's first tick (≤ 1 s) lands the selection: the
            // footer's file label paints left of the hint pack (15 s —
            // a loaded runner's 1 s poll + repaint needs headroom).
            wait_grid(
                h,
                |g| {
                    (20..30).any(|y| {
                        let text = g.content_rect(0, y, 120, 1);
                        text.contains("notes.md #2-3")
                    })
                },
                Duration::from_secs(15),
            )
            .map_err(|error| format!("the editor file label never painted: {error}"))?;
            // Type the prompt, wait for its ECHO in the composer box
            // (the frame that painted it proves the input owns the
            // keys), and only THEN submit — a bare \r racing the
            // composer's focus leaves the text in the draft, the
            // synthetic POST never fires, and the dirty draft gates
            // the exit chord (the CI wedge).
            h.write(b"use the selection");
            wait_text(h, "use the selection", Duration::from_secs(10))
                .map_err(|error| format!("the prompt never echoed into the composer: {error}"))?;
            h.write(b"\r");
            // Echo-gate the SUBMIT: the draft leaves the composer box
            // (this fixture scripts no inbox echo, so the transcript
            // paints no user row — the emptied input line is the
            // submit's observable footprint).
            wait_grid(
                h,
                |g| (22..30).all(|y| !g.content_rect(0, y, 120, 1).contains("use the selection")),
                Duration::from_secs(10),
            )
            .map_err(|error| format!("the submit never left the composer: {error}"))?;
            // The pending context attached through the synthetic POST
            // before the prompt.
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            loop {
                let posted = synthetic_slot
                    .lock()
                    .expect("syn slot")
                    .as_ref()
                    .map(|arc| arc.lock().expect("posts").clone())
                    .unwrap_or_default();
                if posted.iter().any(|body| {
                    body.contains("/synthetic")
                        && body.contains("system-reminder")
                        && body.contains("line two")
                }) {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    return Err(format!("the editor context never POSTed; saw {posted:?}"));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        },
    );
    // Cleanup: the env gate is process-global (nextest isolates test
    // processes, but the in-process unit tests share the env).
    std::env::remove_var("OPENCODE_ZED_DB");
    std::env::remove_var("ZED_TERM");
    let db_path = db_slot.lock().expect("db slot").clone();
    if let Some(db) = db_path {
        let _ = std::fs::remove_file(&db);
    }
}

// ---------------------------------------------------------------------------
// TUI-17 — the mini footer's subagent inspector
// ---------------------------------------------------------------------------

/// TUI-17's event script: two RUNNING children discovered by
/// `parentID` (source 4) — the first with a folded text delta (the
/// detail transcript's row).
fn subagent_script(parent: &str) -> Vec<vt_harness::ScriptEvent> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1_000_000, |since| since.as_millis() as u64);
    vec![
        vt_harness::ScriptEvent {
            delay_ms: 60,
            body: format!(
                r#"{{"id":"evt_kid_a","created":{now_ms},"type":"session.created","data":{{"sessionID":"ses_kid_a","parentID":"{parent}","title":"Check the parsers","directory":"/tmp"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 80,
            body: format!(
                r#"{{"id":"evt_kid_a_exec","created":{now_ms},"type":"session.execution.started","data":{{"sessionID":"ses_kid_a"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 100,
            body: format!(
                r#"{{"id":"evt_kid_a_t","created":{now_ms},"type":"session.text.delta","data":{{"sessionID":"ses_kid_a","messageID":"m_kid","partID":"p_kid","delta":"Working on it"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 120,
            body: format!(
                r#"{{"id":"evt_kid_b","created":{now_ms},"type":"session.created","data":{{"sessionID":"ses_kid_b","parentID":"{parent}","title":"Second look","directory":"/tmp"}}}}"#
            ),
        },
        vt_harness::ScriptEvent {
            delay_ms: 140,
            body: format!(
                r#"{{"id":"evt_kid_b_exec","created":{now_ms},"type":"session.execution.started","data":{{"sessionID":"ses_kid_b"}}}}"#
            ),
        },
    ]
}

/// The mini runner: `opencode mini`'s runtime loop over a pty at a
/// fixed size (the TUI runner's shape — the pty guard restores stdio).
fn run_mini_test<T: Send + 'static>(
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
    // Answer ratatui's inline-viewport cursor probe (`CSI 6n`): the
    // pty has no terminal behind it, so the test replies at the
    // bottom row (the footer viewport docks there).
    {
        let mut prober = harness.handle();
        std::thread::spawn(move || {
            let mut answered = 0usize;
            loop {
                std::thread::sleep(Duration::from_millis(20));
                let raw = prober.raw_mirror();
                let asked = raw.windows(4).filter(|window| window == b"\x1b[6n").count();
                for _ in answered..asked {
                    prober.write(b"\x1b[39;1R");
                }
                answered = asked;
            }
        });
    }
    let slot: Arc<Mutex<Option<Result<T, String>>>> = Arc::new(Mutex::new(None));
    let result_slot = Arc::clone(&slot);
    let mut driver = harness.handle();
    std::thread::spawn(move || {
        let outcome = drive(&mut driver);
        // The mini exit chord: esc folds any open panel back to the
        // composer first, then up to three ctrl+c presses — a leftover
        // draft clears on the first, the second arms, the third closes
        // (`handleExit`, footer.ts:1017-1041).
        driver.write(b"\x1b");
        std::thread::sleep(Duration::from_millis(150));
        for _ in 0..3 {
            driver.write(b"\x03");
            std::thread::sleep(Duration::from_millis(150));
        }
        *result_slot.lock().expect("slot") = Some(outcome);
    });
    let client = oc_client::Client::new(&fake.url).expect("client");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    let input = oc_tui::mini::runtime::MiniInput {
        client,
        target: oc_tui::mini::runtime::ResolvedSession {
            session_id: "ses_alpha".to_owned(),
            session_title: Some("Alpha".to_owned()),
            directory: sandbox.display().to_string(),
            model_label: None,
            agent: None,
            resume: false,
        },
        initial_input: None,
        replay: false,
        replay_limit: None,
        demo: false,
        version: "test".to_owned(),
        home: sandbox.display().to_string(),
        state_dir: sandbox.join(".local/state"),
        settings: None,
    };
    let finished = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(90), oc_tui::mini::runtime::run(input)).await
    });
    fake.stop();
    // The driver's outcome first (its failure text explains a runtime
    // that never closed — e.g. a chord swallowed by an open panel).
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let driver_outcome = slot.lock().expect("slot").take();
    match finished {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => panic!("the mini runtime failed: {error}"),
        Err(elapsed) => {
            let raw = harness.handle().raw_mirror();
            let tail = String::from_utf8_lossy(&raw[raw.len().saturating_sub(1200)..]);
            let driver_error = match driver_outcome.as_ref() {
                Some(Err(message)) => Some(message.clone()),
                _ => None,
            };
            panic!(
                "the mini runtime did not exit in time: {elapsed}; driver: {driver_error:?}; raw TAIL: {tail:?}"
            );
        }
    }
    match driver_outcome {
        Some(Ok(value)) => value,
        Some(Err(error)) => panic!("driver failed: {error}"),
        None => panic!("the driver thread never reported"),
    }
}

/// TUI-17: the mini footer tracks the parent's children (the live
/// `parentID` discovery + the child's own event fold), the statusline
/// carries `2 subagents`, `down` opens the 14-row inspector on the
/// newest running child (glyph + title + transcript + hints), `tab`
/// cycles (`1 of 2` / `tab next 1/2`), ctrl+c interrupts the INSPECTED
/// child server-side, and esc returns to the composer.
#[test]
fn mini_subagent_footer() {
    let interrupt_slot: SharedPosts = Arc::new(Mutex::new(None));
    let arm_interrupt = Arc::clone(&interrupt_slot);
    let driver_interrupt = Arc::clone(&interrupt_slot);
    run_mini_test(
        "tui17-mini",
        80,
        40,
        |fake, _sandbox| {
            fake.arm_script(subagent_script("ses_alpha"));
            *arm_interrupt.lock().expect("slot") = Some(Arc::clone(&fake.interrupt_posts));
        },
        move |h| {
            // Boot settles first (the splash), then the prompt: the
            // keystrokes must land after raw mode owns the pty.
            wait_text(h, "oc mini", Duration::from_secs(8))?;
            h.write(b"hello");
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("hello")),
                Duration::from_secs(5),
            )?;
            // Submit: the armed script fires the two child
            // discoveries + the child's folded text.
            h.write(b"\r");
            // The statusline's live count (footer.view.tsx:247). The
            // echoed prompt row ("> hello") stays in scrollback — the
            // count is the submit's landing proof.
            wait_grid(
                h,
                |g| (0..g.rows).any(|row| g.row_text(row).contains("2 subagents")),
                Duration::from_secs(10),
            )
            .map_err(|error| format!("the subagent count never painted: {error}"))?;
            // `down` (session.child.first) opens the inspector on the
            // newest RUNNING child (kid_b).
            h.write(b"\x1b[B");
            let grid = wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Second look"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("ctrl+c interrupt"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the inspector never painted: {error}"))?;
            // The running glyph + the interrupt hint.
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains('◔')),
                "the running glyph paints"
            );
            assert!(
                (0..grid.rows).any(|row| grid.row_text(row).contains("ctrl+c interrupt")),
                "the interrupt hint paints"
            );
            // tab cycles to the first child (kid_a) — the count +
            // cycle hints appear, and its folded text lists.
            h.write(b"\t");
            // The cycle lands on the first child — the count control
            // (`1 of 2`; the hint-row forms only when the header
            // controls do not fit) + its folded transcript row.
            wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Check the parsers"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("1 of 2"))
                        && (0..g.rows).any(|row| g.row_text(row).contains("Working on it"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the cycle never landed on the first child: {error}"))?;
            // ctrl+c interrupts the INSPECTED child (kid_a).
            h.write(b"\x03");
            let deadline = std::time::Instant::now() + Duration::from_secs(8);
            loop {
                let posts = driver_interrupt
                    .lock()
                    .expect("slot")
                    .as_ref()
                    .map(|arc| arc.lock().expect("posts").clone())
                    .unwrap_or_default();
                if posts.iter().any(|post| post.contains("/api/session/ses_kid_a/interrupt")) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the child interrupt never POSTed; saw {posts:?}"
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            // esc returns to the composer (the statusline count back).
            h.write(b"\x1b");
            wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("2 subagents"))
                        && (0..g.rows).all(|row| !g.row_text(row).contains("Second look"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("esc never closed the inspector: {error}"))?;
            Ok(())
        },
    );
}

// ---------------------------------------------------------------------------
// TUI-16 — mouse selection copy
// ---------------------------------------------------------------------------

/// TUI-16: a left DRAG over screen cells captures a selection and the
/// release copies it (`terminal.copy = "select"`, the non-win32
/// default — util/selection.ts:44-60 + app.tsx:546-571): the
/// `Copied to clipboard` info toast paints and the clipboard write
/// emits OSC 52. A BARE click (no crossing) never selects, and
/// `terminal.copy = "manual"` keeps the copy off the release (the
/// selection waits for ctrl+y — the intercept's copy).
#[test]
fn selection_copy() {
    run_daily_test(
        "tui16-select",
        120,
        30,
        |_, _| {},
        |h| {
            boot_to_session(h, b"Alpha")?;
            wait_text(h, "shift+tab agents", Duration::from_secs(10))?;
            // Text on screen the drag can capture: the composer's
            // own draft row (a non-UI surface — no strip/rail drag
            // competes for the pointer).
            h.write(b"parrot selected words");
            wait_grid(
                h,
                |g| (20..30).any(|y| g.content_rect(0, y, 120, 1).contains("parrot selected")),
                Duration::from_secs(5),
            )?;
            let y = (20..30)
                .find(|&row| {
                    let text = h.grid_now().row_text(row);
                    text.contains('p') && text.contains('r') && text.contains('o')
                })
                .unwrap_or(26);
            // A bare click never selects: press + release on one cell
            // leaves no toast.
            h.sgr_motion(10, u16::try_from(y).unwrap_or(26));
            h.sgr_press(10, u16::try_from(y).unwrap_or(26));
            h.sgr_release(10, u16::try_from(y).unwrap_or(26));
            std::thread::sleep(Duration::from_millis(400));
            if (0..h.grid_now().rows).any(|row| h.grid_now().row_text(row).contains("Copied")) {
                return Err("a bare click must not copy".to_owned());
            }
            // The crossing drag: press before the text, drag past it,
            // release — the copy lands.
            h.sgr_motion(2, u16::try_from(y).unwrap_or(26));
            h.sgr_press(2, u16::try_from(y).unwrap_or(26));
            h.sgr_drag(40, u16::try_from(y).unwrap_or(26));
            h.sgr_release(40, u16::try_from(y).unwrap_or(26));
            wait_text(h, "Copied to clipboard", Duration::from_secs(5))
                .map_err(|error| format!("the copy toast never painted: {error}"))?;
            // The clipboard write emitted OSC 52 (the base64 payload
            // follows the `52;c` marker).
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let raw = h.raw_mirror();
                let from =
                    raw.windows(5).position(|window| window == b"\x1b]52;").map(|at| &raw[at..]);
                if from.is_some_and(|tail| tail.contains(&b'c')) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the clipboard write never emitted OSC 52"
                );
                std::thread::sleep(Duration::from_millis(60));
            }
            Ok(())
        },
    );
}

/// TUI-35 — the boot completes against a QUIET server (no SSE script
/// armed, no prompt ever `POST`ed — the event stream never speaks) and
/// the standard exit chord tears the app down THROUGH a dirty dialog
/// filter. Pre-fix, the modal layer's ctrl+c arm cleared a non-empty
/// filter but KEPT the dialog open, so the chord (clear → close →
/// `<leader>q` = `session.queued_prompts`, never `app.exit`) could
/// not exit: the harness wedged at its 90 s bound with the wordmark
/// already painted and every chord key read — the
/// `session_filter_semantics` CI red (run 39501153874). One ctrl+c
/// must close the dialog (`ui/dialog.tsx:115-156` — the text clear is
/// part of the close), leaving the chord's second press to exit on
/// the empty prompt. The `run_tui` return IS the exit proof (the
/// pre-fix tree times out here).
#[test]
fn tui35_exit_chord_through_dirty_dialog_on_quiet_server() {
    run_daily_test(
        "tui35-exit-chord",
        80,
        24,
        |_, _| {},
        |h| {
            // Boot settles on the QUIET server: the connection's
            // first cycle completes (subscribe + resync), the state
            // reaches Connected, and the home wordmark paints.
            if !h.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the wordmark never rendered on the quiet server".to_owned());
            }
            // Open the sessions dialog and DIRTY its filter: type a
            // query that keeps one row and drops the other, proving
            // the filter text is live in the dialog.
            h.write(b"\x18l");
            if !h.wait_for(b"Alpha", Duration::from_secs(5)) {
                return Err("the session dialog never listed Alpha".to_owned());
            }
            h.write(b"Alpha");
            wait_grid(
                h,
                |g| {
                    (0..g.rows).any(|row| g.row_text(row).contains("Alpha"))
                        && (0..g.rows).all(|row| !g.row_text(row).contains("Long Session"))
                },
                Duration::from_secs(5),
            )
            .map_err(|error| format!("the filter never went dirty: {error}"))?;
            // Leave the dialog OPEN + dirty — the harness's trailing
            // exit chord must still terminate the app (see the test
            // doc: this is the pre-fix wedge).
            Ok(())
        },
    );
}
