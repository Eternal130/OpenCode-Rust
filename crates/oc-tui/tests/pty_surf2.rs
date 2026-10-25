//! SURF-2 PTY proofs — the six pure-TUI command wirings (auditE's
//! traceability row) and the diff viewer's command→variants→scroll→
//! close cycle (auditB #6), asserted on the parsed CELL GRID against
//! the real app loop over a pty:
//!
//! * `/mcps` opens the REAL server rows (Connected ✓ / Failed !) with
//!   the error-details drill-down;
//! * `/status` opens the inventory summary (`N MCP servers` + the
//!   per-server bullet rows);
//! * `/pair` opens the link list (loading → links + expiry);
//! * `/debug` opens the env dump (Version / Session ID rows);
//! * `/diff` mounts the viewer (the empty body over the fixture's
//!   diff-less repo, the source dialog, close back to the route);
//! * the no-variants `/variants` guard toasts (the model fixture has
//!   no variants) — never a dead-stub dialog.

#![allow(unsafe_code)] // pty(7) is only reachable through libc FFI
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
#![allow(clippy::too_many_lines)] // one linear scenario per proof

#[path = "vt_harness.rs"]
mod vt_harness;

use std::time::Duration;
use vt_harness::{
    pty_lock, run_tui, sandbox_env, standard_sessions, FakeLlm, LiveHarness, WORDMARK,
};

fn find_row(h: &vt_harness::HarnessHandle, needle: &str) -> Option<usize> {
    let grid = h.grid_now();
    (0..grid.rows).find(|row| grid.row_text(*row).contains(needle))
}

fn wait_row(h: &vt_harness::HarnessHandle, needle: &str, timeout: Duration) -> Option<usize> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Some(row) = find_row(h, needle) {
            return Some(row);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

/// The slash-command path: type `/mcps` etc. into the home composer
/// and submit.
fn slash(h: &mut vt_harness::HarnessHandle, command: &str) {
    h.write(format!("/{command}").as_bytes());
    std::thread::sleep(Duration::from_millis(150));
    h.write(b"\r");
}

/// `/mcps`, `/status`, `/pair`, `/debug` mount their REAL surfaces
/// (the stub "Not wired in this build" is gone).
#[test]
fn pty_four_commands_open_real_surfaces() {
    let _guard = pty_lock();
    let sandbox = sandbox_env("surf2-commands");
    let server = FakeLlm::spawn(standard_sessions());
    server.arm_mcp();
    server.arm_pair();
    let url = server.url.clone();
    let harness = LiveHarness::attach(100, 30);
    let mut driver = harness.handle();
    let exit_driver = harness.handle();
    std::thread::spawn(move || run_tui(&url, 100, 30, Duration::from_secs(90)));
    drop(sandbox);
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "the wordmark rendered");

    // /mcps — the server rows with status footers.
    slash(&mut driver, "mcps");
    assert!(wait_row(&driver, "MCP servers", Duration::from_secs(5)).is_some(), "the dialog title");
    assert!(wait_row(&driver, "Connected ✓", Duration::from_secs(3)).is_some(), "connected footer");
    assert!(wait_row(&driver, "Failed !", Duration::from_secs(2)).is_some(), "failed footer");
    // The failed row sorts FIRST (broken < filesystem < github) —
    // submit opens its error details.
    driver.write(b"\r");
    assert!(
        wait_row(&driver, "MCP server: broken", Duration::from_secs(3)).is_some(),
        "the error details title"
    );
    assert!(wait_row(&driver, "connect ECONNREFUSED", Duration::from_secs(2)).is_some());
    driver.write(b"\x1b"); // close the details
    std::thread::sleep(Duration::from_millis(300));
    {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut gone = false;
        while std::time::Instant::now() < deadline {
            if find_row(&driver, "MCP server: broken").is_none() {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(gone, "the error details closed");
    }

    // /status — the inventory.
    slash(&mut driver, "status");
    assert!(wait_row(&driver, "3 MCP servers", Duration::from_secs(3)).is_some(), "the count row");
    assert!(wait_row(&driver, "• github Connected", Duration::from_secs(2)).is_some());
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));

    // /pair — loading then the links.
    slash(&mut driver, "pair");
    assert!(
        driver.wait_for(b"Loading server information", Duration::from_secs(3)),
        "the loading row"
    );
    assert!(
        wait_row(&driver, "expire in 10 minutes", Duration::from_secs(3)).is_some(),
        "the expiry copy"
    );
    assert!(
        wait_row(
            &driver,
            "http://localhost:4096/auth/connect/pair-code-123",
            Duration::from_secs(3)
        )
        .is_some(),
        "the localhost link"
    );
    // The QR block (ref dialog-pair.tsx:75-84): the first link as a
    // compact unicode QR — a half-block row is on screen.
    assert!(wait_row(&driver, "▀▀▀", Duration::from_secs(3)).is_some(), "the QR block renders");
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));

    // /debug — the env dump.
    slash(&mut driver, "debug");
    assert!(wait_row(&driver, "Version", Duration::from_secs(3)).is_some(), "the version row");
    assert!(find_row(&driver, "Session ID").is_some(), "the session row");
    assert!(find_row(&driver, "Terminal").is_some(), "the terminal row");
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));

    // /variants — the fixture models carry none: the guard toast, no
    // dialog.
    slash(&mut driver, "variants");
    assert!(
        driver.wait_for(b"No variants available", Duration::from_secs(3)),
        "the no-variants guard toast"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(find_row(&driver, "Select variant").is_none(), "no variant dialog mounted");

    server.stop();
    let mut exit_driver = exit_driver;
    exit_driver.write(b"\x03");
}

/// The diff viewer cycle: `/diff` mounts → the empty body paints →
/// the source dialog opens → close returns to the route.
#[test]
fn pty_diff_viewer_cycle() {
    let _guard = pty_lock();
    let sandbox = sandbox_env("surf2-diff");
    let server = FakeLlm::spawn(standard_sessions());
    server.arm_vcs();
    let url = server.url.clone();
    let harness = LiveHarness::attach(120, 30);
    let mut driver = harness.handle();
    let exit_driver = harness.handle();
    std::thread::spawn(move || run_tui(&url, 120, 30, Duration::from_secs(60)));
    drop(sandbox);
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "the wordmark rendered");

    slash(&mut driver, "diff");
    // The fixture serves one modified file: the toolbar + the file's
    // patch rows paint.
    assert!(
        wait_row(&driver, "src/lib.rs", Duration::from_secs(3)).is_some(),
        "the diff file header"
    );
    assert!(
        wait_row(&driver, "let new = 1;", Duration::from_secs(2)).is_some(),
        "the added patch row"
    );
    // Scroll: j moves one line; gg returns to the top.
    driver.write(b"j");
    std::thread::sleep(Duration::from_millis(150));
    driver.write(b"g");
    std::thread::sleep(Duration::from_millis(80));
    driver.write(b"g");
    std::thread::sleep(Duration::from_millis(150));
    // The source switch dialog (d) and back.
    driver.write(b"d");
    assert!(
        wait_row(&driver, "Diff source", Duration::from_secs(3)).is_some(),
        "the source dialog"
    );
    assert!(find_row(&driver, "Branch + local changes").is_some(), "the All row copy");
    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));
    // q closes back to the home route (the wordmark returns).
    let mark = driver.raw_len().saturating_sub(8192);
    driver.write(b"q");
    assert!(driver.wait_for_after(WORDMARK, mark, Duration::from_secs(3)), "closed back to home");

    server.stop();
    let mut exit_driver = exit_driver;
    exit_driver.write(b"\x03");
}

/// F15 residual (narrow widths): the pair dialog's URLs WORD-WRAP,
/// never hard-truncate. The ref renders the links through `<Link>`
/// (ui/link.tsx:33-39), which sets NO `wrapMode`, and the
/// TextRenderable default is `"word"` (@opentui/core
/// chunk-node-wp7ct2m6.js:2833,2847) — at a 46-col terminal the
/// Large dialog clamps to 44 cols (content 40), and the 48-char
/// localhost link wraps onto continuation rows whose concatenation
/// is the whole URL.
#[test]
fn pty_pair_narrow_width_url_wraps() {
    let _guard = pty_lock();
    let sandbox = sandbox_env("surf2-pair-narrow");
    let server = FakeLlm::spawn(standard_sessions());
    server.arm_pair();
    let url = server.url.clone();
    let harness = LiveHarness::attach(46, 30);
    let mut driver = harness.handle();
    let exit_driver = harness.handle();
    std::thread::spawn(move || run_tui(&url, 46, 30, Duration::from_secs(60)));
    drop(sandbox);
    assert!(driver.wait_for(WORDMARK, Duration::from_secs(10)), "the wordmark rendered");

    slash(&mut driver, "pair");
    assert!(
        driver.wait_for(b"Loading server information", Duration::from_secs(3)),
        "the loading row"
    );
    assert!(
        wait_row(&driver, "expire in 10 minutes", Duration::from_secs(3)).is_some(),
        "the expiry copy"
    );
    // The 48-char link over a 40-col content area: the first row
    // carries the fitting prefix, a continuation row the rest — the
    // full URL is recoverable from the grid, nothing is dropped.
    let head =
        wait_row(&driver, "http://localhost:4096/auth/connect/pair-", Duration::from_secs(3));
    assert!(head.is_some(), "the wrapped link head");
    let tail = wait_row(&driver, "code-123", Duration::from_secs(2));
    assert!(tail.is_some(), "the wrapped link continuation");
    assert_ne!(head, tail, "the URL spans two rows (wrapped, not truncated)");
    // The QR block still fits (31 cols < 40): its half-block rows
    // render beside the wrapped text.
    assert!(find_row(&driver, "▀▀▀").is_some(), "the QR block renders");

    driver.write(b"\x1b");
    std::thread::sleep(Duration::from_millis(200));
    server.stop();
    let mut exit_driver = exit_driver;
    exit_driver.write(b"\x03");
}
