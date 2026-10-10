//! Hard-timeout behavior (deliverable 1: "hung plugin → timeout +
//! log + skip, never blocks"): a handler sleeping past the dispatch
//! deadline is dropped (counted), the NEXT event still delivers, and
//! the tap publish itself never blocks.

mod common;

use std::time::Duration;

use oc_plugin_host::{PluginHostConfig, TapEvent};

#[test]
fn hung_plugin_times_out_and_stream_continues() {
    let sandbox = common::sandbox("slow");
    let slow = common::copy_fixture(&sandbox, "slow.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        dispatch_timeout: Duration::from_millis(400),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[slow]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let tap = host.tap();
    let started = std::time::Instant::now();

    // The hung dispatch: publish must return immediately (never
    // blocks), the dispatch times out after ~400 ms.
    tap.publish(TapEvent::new("slow.tick", serde_json::json!({})));
    assert!(started.elapsed() < Duration::from_millis(250), "tap publish is non-blocking");

    // The very next event still delivers while the slow one pends.
    tap.publish(TapEvent::new("fast.tick", serde_json::json!({})));
    assert!(
        common::wait_for(Duration::from_secs(10), || sandbox.join("fast.txt").exists()),
        "fast event delivered behind a hung dispatch"
    );

    assert!(
        common::wait_for(Duration::from_secs(10), || host.dispatch_counters().1 >= 1),
        "the hung dispatch was dropped (counted)"
    );
    let (dispatched, dropped) = host.dispatch_counters();
    assert!(dropped >= 1);
    assert!(dispatched >= 1);

    // And the host still executes commands afterwards.
    assert!(host.execute_command("nope", None, Duration::from_millis(500)).is_err());
    host.shutdown();
}
