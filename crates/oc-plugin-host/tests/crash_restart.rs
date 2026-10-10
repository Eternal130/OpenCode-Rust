//! Crash supervision: a plugin that exits the guest mid-dispatch gets
//! the crash attributed; the host restarts with backoff and reloads;
//! after `max_plugin_crashes` strikes the culprit is DISABLED while
//! the healthy neighbor keeps working.

mod common;

use std::time::Duration;

use oc_plugin_host::{PluginHostConfig, TapEvent};

#[test]
fn crash_attributed_restarted_then_plugin_disabled() {
    let sandbox = common::sandbox("crash");
    let crasher = common::copy_fixture(&sandbox, "crasher.ts");
    let hello = common::copy_fixture(&sandbox, "helloworld.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        // Tight backoff so three crash cycles stay quick.
        restart_backoff_base: Duration::from_millis(60),
        restart_backoff_max: Duration::from_millis(240),
        ..PluginHostConfig::default()
    };
    let host =
        oc_plugin_host::PluginHost::detect().start_detached_with(config, &[crasher.clone(), hello]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    assert!(host.registrations().commands.iter().any(|command| command.name == "hw.hello"));

    let crash_status = |host: &oc_plugin_host::PluginHost| {
        host.registrations()
            .plugins
            .iter()
            .find(|status| status.plugin_id == "crasher" || status.entry == crasher)
            .map(|status| status.state.clone())
    };

    // Publish kill ticks whenever the plugin is (again) Active — each
    // landing tick crashes the guest, is attributed, and restarts the
    // host with backoff; at 3 strikes the plugin is disabled and never
    // becomes Active again.
    let disabled = common::wait_for(Duration::from_secs(25), || {
        if let Some(oc_plugin_host::PluginState::Disabled { crashes }) = crash_status(&host) {
            return crashes >= 3;
        }
        if matches!(crash_status(&host), Some(oc_plugin_host::PluginState::Active)) {
            host.tap().publish(TapEvent::new("kill.tick", serde_json::json!({})));
        }
        false
    });
    assert!(disabled, "crasher disabled after repeated crashes (state {:?})", crash_status(&host));

    // Let the pump drain the stray kill.ticks the probe raced in
    // (each resolves fast once failed; wait for the queue to settle).
    // Wave 4 widened this window: the suite's parallel load grew
    // (codemode/fuzz guests) and in-flight dispatches now resolve
    // slower under contention — the w9w2 §5 test-budget note.
    let settled = common::wait_for(Duration::from_secs(20), || {
        let (one, _) = host.dispatch_counters();
        std::thread::sleep(Duration::from_millis(300));
        let (two, _) = host.dispatch_counters();
        one == two
    });
    assert!(settled, "dispatch pump drained after the crash cycles");

    // The neighbor survived every restart: dispatch + execute work.
    // The publish retries — a dispatch that races a crash/restart may
    // still drop after the pump's bounded retry window (logged and
    // counted); the recovery contract is that delivery resumes, not
    // that any single raced event survives.
    let marker = sandbox.join("event.txt");
    let _ = std::fs::remove_file(&marker);
    assert!(
        common::wait_for(Duration::from_secs(20), || {
            host.tap().publish(TapEvent::new(
                "tui.test",
                serde_json::json!({ "payload": "after-crashes" }),
            ));
            marker.exists()
        }),
        "healthy plugin still receives events after the crash cycles"
    );
    assert!(
        host.execute_command("hw.hello", None, Duration::from_secs(5)).is_ok(),
        "healthy plugin still executes after the crash cycles"
    );
    // The disabled plugin's command is gone from the snapshot.
    assert!(
        !host.registrations().commands.iter().any(|command| command.name == "crash.noop"),
        "disabled plugin's registrations cleared"
    );
    host.shutdown();
}
