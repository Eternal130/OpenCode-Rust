//! SA-F5 (SCAN-AB) — plugin sidebar fidelity, END TO END on the live
//! run loop: with oh-my-opencode-slim armed through the real plugin
//! host (`file://` entry, bun guest), the session sidebar's plugin
//! section renders the ref's plugin-rendered contract:
//!
//! * the version badge shows the PLUGIN's own package version
//!   (the ref's `v3.0.4` — never the app version);
//! * every section cell (header AND rows) paints fg idx15 / bg idx233
//!   (ref-session-00.styles.txt y13-y19 — decoded digests;
//!   TERM pinned xterm-256color so the emission degrades to indices);
//! * the padded columns materialize: all rows one width, the model
//!   column right-aligned on the same edge as the header's badge.
//!
//! Hermetic fixture: the scan lane's staged npm copy
//! (/tmp/opencode/scanab/omos-npm/stamp-full — I9: the user's real
//! cache copy); the test skips when the fixture or bun is absent.

#[path = "vt_harness.rs"]
mod vt_harness;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vt_harness::{
    open_first_session, pty_lock, run_tui, sandbox_env, FakeLlm, LiveHarness, WORDMARK,
};

const OMOS_SOURCE: &str =
    "/tmp/opencode/scanab/omos-npm/stamp-full/node_modules/oh-my-opencode-slim";
const OMOS_DEPS: &str = "/tmp/opencode/scanab/omos-npm/stamp-full/node_modules";

fn copy_dir(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create dir");
    for entry in std::fs::read_dir(source).expect("read dir") {
        let entry = entry.expect("dir entry");
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

/// The packed digest fields (vt_harness::style_digest):
/// style = fg<<30 | bg<<4 | mods; Idx = (1<<8)|index.
const fn idx_packed(index: u64) -> u64 {
    (1 << 8) | index
}

#[test]
fn vt_plugin_sidebar_renders_the_reference_contract() {
    let _lock = pty_lock();
    let source = PathBuf::from(OMOS_SOURCE);
    if !source.join("dist/tui2.js").is_file() || !Path::new(OMOS_DEPS).is_dir() {
        eprintln!("omo-slim fixture unavailable; skipping");
        return;
    }
    let version = {
        let package = std::fs::read_to_string(source.join("package.json")).expect("package.json");
        package
            .split("\"version\"")
            .nth(1)
            .and_then(|rest| rest.split('"').nth(1))
            .unwrap_or_default()
            .to_owned()
    };
    assert!(!version.is_empty(), "the fixture package carries a version");

    let _sandbox = sandbox_env("omos-sidebar");
    let sandbox = _sandbox.clone();
    // Stage the plugin + its hoisted deps under a REAL node_modules
    // layout (the package's parent dir must BE node_modules — the
    // host's @opentui shims land there and bare imports resolve
    // through node's walk).
    let node_modules = sandbox.join("plugins").join("node_modules");
    copy_dir(&source, &node_modules.join("oh-my-opencode-slim"));
    for dep in ["jsonc-parser", "zod", "solid-js", "lru-cache", "semver"] {
        let dir = Path::new(OMOS_DEPS).join(dep);
        if dir.is_dir() {
            copy_dir(&dir, &node_modules.join(dep));
        }
    }
    // The plugin entry (the TUI config chain: a config FILE's
    // plugins[] — discover_config_files honors OPENCODE_CONFIG).
    let config_path = sandbox.join(".config").join("opencode.json");
    std::fs::write(
        &config_path,
        format!(
            "{{\"plugins\":[\"file://{}\"]}}",
            node_modules.join("oh-my-opencode-slim").display()
        ),
    )
    .expect("config");
    std::env::set_var("OPENCODE_CONFIG", &config_path);
    std::env::set_var("HOME", &sandbox);
    // I6 discipline: pin the color environment so the emission
    // degrades to xterm-256 indices (the ref capture's class).
    std::env::set_var("TERM", "xterm-256color");
    std::env::remove_var("COLORTERM");
    for key in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
        std::env::remove_var(key);
    }

    let fake = FakeLlm::spawn(vt_harness::standard_sessions());
    let harness = LiveHarness::attach(130, 30);
    let result_slot: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&result_slot);
    let mut driver = harness.handle();
    let version_for_driver = version.clone();
    std::thread::spawn(move || {
        let outcome = (|| {
            if !driver.wait_for(WORDMARK, Duration::from_secs(8)) {
                return Err("the home wordmark never rendered".to_owned());
            }
            open_first_session(&mut driver, b"Alpha")?;
            // The plugin section lands after the host's materialize.
            if !driver.wait_for(b"OMO-Slim", Duration::from_secs(25)) {
                return Err("the plugin sidebar section never rendered".to_owned());
            }
            std::thread::sleep(Duration::from_millis(600));
            let grid = driver.grid_now();
            let rows: Vec<String> = (0..grid.rows).map(|y| grid.row_text(y)).collect();
            // The section: the header row with the badge, the • rows.
            let header_y = rows
                .iter()
                .position(|row| row.contains("OMO-Slim"))
                .ok_or_else(|| format!("no OMO-Slim row: {rows:?}"))?;
            let header = rows[header_y].trim_end();
            assert!(
                header.contains(&format!("v{version_for_driver}")),
                "the badge shows the PLUGIN's version {version_for_driver}: {header:?}"
            );
            assert!(
                !header.contains(concat!(env!("CARGO_PKG_VERSION"))),
                "the app version must not be the badge: {header:?}"
            );
            let agent_rows: Vec<&String> = rows
                .iter()
                .skip(header_y + 1)
                .take_while(|row| row.trim_start().starts_with('•'))
                .collect();
            assert!(agent_rows.len() >= 5, "the agent roster: {agent_rows:?}");
            let width = |row: &str| row.trim_end().chars().count();
            let header_width = width(header);
            for (index, row) in agent_rows.iter().enumerate() {
                let trimmed = row.trim_end();
                assert_eq!(
                    width(trimmed),
                    header_width,
                    "row {index} pads to the header's edge (the right-aligned column): {trimmed:?} vs {header:?}"
                );
                assert!(
                    trimmed.contains("  "),
                    "the padded column shape (not the tight join): {trimmed:?}"
                );
                // The cells' emission: fg idx15 / bg idx233 on every
                // non-blank cell of the row (the ref's digests).
                let y = header_y + 1 + index;
                for x in 0..grid.cols {
                    let cell = grid.cell(x, y);
                    if cell.ch != ' ' && cell.ch != '\0' {
                        let fg = cell.style >> 30;
                        let bg = (cell.style >> 4) & ((1 << 26) - 1);
                        assert_eq!(
                            fg,
                            idx_packed(15),
                            "cell ({x},{y}) {:?} fg must be idx15 (white)",
                            cell.ch
                        );
                        assert_eq!(
                            bg,
                            idx_packed(233),
                            "cell ({x},{y}) {:?} bg must be idx233 (raised)",
                            cell.ch
                        );
                    }
                }
            }
            // The header's own cells too.
            for x in 0..grid.cols {
                let cell = grid.cell(x, header_y);
                if cell.ch != ' ' && cell.ch != '\0' {
                    let fg = cell.style >> 30;
                    let bg = (cell.style >> 4) & ((1 << 26) - 1);
                    assert_eq!(fg, idx_packed(15), "header cell ({x}) fg idx15");
                    assert_eq!(bg, idx_packed(233), "header cell ({x}) bg idx233");
                }
            }
            Ok(())
        })();
        vt_harness::exit_chord(&mut driver);
        *slot.lock().expect("slot") = Some(outcome);
    });
    run_tui(&fake.url, 130, 30, Duration::from_secs(90));
    fake.stop();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while result_slot.lock().expect("slot").is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let outcome = result_slot.lock().expect("slot").take();
    if let Some(Err(error)) = outcome {
        panic!("driver failed: {error}");
    }
}
