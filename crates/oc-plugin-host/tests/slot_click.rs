//! beta.13 bug 2's plugin-world path — the CLICK pipe end-to-end at
//! the host level:
//!
//! * a TUI-plugin's `sidebar_content` rows may carry the plugin's own
//!   `onMouseUp` (`decorateInteractiveRow`, tui2.js:30462-30467 — the
//!   omo-slim sidebar's session/preset targets);
//! * `slot.materialize` marks those rows `click: true` and stashes
//!   their handlers;
//! * `slot.click` runs the handler, whose
//!   `route.navigate("session", {sessionID})` flows back to the host
//!   as a `tui.route.navigate` notification → a
//!   [`UiRequest::NavigateToSession`] on the UI-request queue (what
//!   the TUI's drain turns into a session route).
//!
//! Fixture: `fixtures/clickprobe` (a minimal package with both
//! entries — no @opentui needed; the bridge's element records are
//! plain objects).

mod common;

use std::path::PathBuf;
use std::time::Duration;

use oc_plugin_host::{PluginHost, PluginHostConfig, UiRequest};

fn stage() -> Option<(PathBuf, PathBuf)> {
    let sandbox = common::sandbox("slot-click");
    let package = sandbox.join("plugins").join("node_modules").join("clickprobe");
    std::fs::create_dir_all(&package).ok()?;
    let source = oc_plugin_host::fixture_dir().join("clickprobe");
    for file in ["package.json", "index.js", "tui.js"] {
        std::fs::copy(source.join(file), package.join(file)).ok()?;
    }
    std::fs::create_dir_all(sandbox.join("project")).ok()?;
    let entry = package.join("index.js");
    Some((sandbox, entry))
}

fn boot() -> Option<PluginHost> {
    let (sandbox, entry) = stage()?;
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        host_state: oc_plugin_host::HostState {
            app_version: "2.0.24-rust".to_owned(),
            project_id: "proj_click".to_owned(),
            directory: sandbox.join("project").display().to_string(),
            agents: Vec::new(),
            mcps: std::collections::BTreeMap::default(),
        },
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, std::slice::from_ref(&entry));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(30)), "host ready: {:?}", host.start_error());
    Some(host)
}

/// The section lands with the clickable flag on the handler row; the
/// click runs the handler and the navigation ask follows.
#[test]
fn slot_click_runs_the_plugin_row_handler_and_navigates() {
    let Some(host) = boot() else { return };

    // (a) The section materializes with per-row clickability.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut landed = false;
    while !landed {
        for request in host.drain_ui_requests() {
            if let UiRequest::SidebarSection { plugin_id, title, rows, clickable } = request {
                if plugin_id == "clickprobe:tui" {
                    assert_eq!(title, "Probe");
                    assert_eq!(rows, vec!["Go to child".to_owned(), "plain row".to_owned()]);
                    assert_eq!(clickable, vec![true, false], "only the onMouseUp row is clickable");
                    landed = true;
                }
            }
        }
        assert!(std::time::Instant::now() < deadline, "the probe section never landed");
        std::thread::sleep(Duration::from_millis(50));
    }

    // (b) The click runs the row's handler → the navigate ask lands
    // on the UI-request queue.
    let clicked = host
        .click_sidebar_row("clickprobe:tui", 0, Duration::from_secs(5))
        .expect("the click call ran");
    assert!(clicked, "the row had a handler");
    // A non-clickable row answers false (contained).
    assert!(!host.click_sidebar_row("clickprobe:tui", 1, Duration::from_secs(5)).unwrap());

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        for request in host.drain_ui_requests() {
            if let UiRequest::NavigateToSession { session_id } = request {
                assert_eq!(session_id, "ses_target");
                host.shutdown();
                return;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the navigate ask never landed: {:?}",
            host.drain_ui_requests()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
