//! Adversarial fuzz (wave 4, deliverable 2): kill -9 the bun process
//! MID-DISPATCH (the host must restart, the outstanding dispatch fails
//! or retries cleanly, nothing else is impacted), and a plugin sleeping
//! beyond every bridge timeout — registration, event dispatch, command
//! execution, tool execution, and the session-hook bridge — all
//! bounded, all logged, no deadlock.

mod common;

use std::time::{Duration, Instant};

use oc_plugin_host::{PluginHostConfig, TapEvent};

/// The guest's bun PID for this host (the supervisor publishes it —
/// exact even with several hosts per process).
fn guest_pid(host: &oc_plugin_host::PluginHost) -> Option<i32> {
    host.guest_pid()
}

#[test]
fn kill9_mid_dispatch_restarts_and_the_dispatch_settles() {
    let sandbox = common::sandbox("fuzz-kill");
    let slow = common::copy_fixture(&sandbox, "slowall.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        dispatch_timeout: Duration::from_millis(400),
        // Tight backoff so the restart lands inside the pump's retry
        // window (or right after it — both outcomes must be clean).
        restart_backoff_base: Duration::from_millis(60),
        restart_backoff_max: Duration::from_millis(240),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[slow]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    assert!(
        host.registrations().commands.iter().any(|command| command.name == "slowall.crawl"),
        "slowall loaded"
    );

    // Start the slow dispatch (the handler sleeps 4s), then kill -9 the
    // guest WHILE the dispatch is outstanding.
    host.tap().publish(TapEvent::new("slow.tick", serde_json::json!({})));
    let pid = common::wait_for(Duration::from_secs(5), || guest_pid(&host).is_some())
        .then(|| guest_pid(&host))
        .flatten()
        .expect("the guest bun process is running mid-dispatch");
    let killed = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
    assert!(killed.is_ok_and(|status| status.success()), "kill -9 delivered");

    // The host restarts: the wake event delivers to the RELOADED guest.
    let marker = sandbox.join("slowall-wake.txt");
    let recovered = common::wait_for(Duration::from_secs(20), || {
        host.tap().publish(TapEvent::new("wake.tick", serde_json::json!({})));
        marker.exists()
    });
    assert!(recovered, "the guest restarted and reloaded slowall (wake delivered)");

    // The in-flight dispatch settled: dispatched + dropped counters
    // moved (delivered-after-retry or dropped — both are clean exits).
    let settled = common::wait_for(Duration::from_secs(10), || {
        let (one, _) = host.dispatch_counters();
        std::thread::sleep(Duration::from_millis(300));
        let (two, _) = host.dispatch_counters();
        one == two && one >= 1
    });
    assert!(settled, "the dispatch pump settled after the kill");

    // No host-wide impact: commands + taps keep working.
    let bounded = Instant::now();
    assert!(
        host.execute_command("slowall.crawl", None, Duration::from_millis(400)).is_err(),
        "a command call after the crash cycle is bounded"
    );
    assert!(
        bounded.elapsed() < Duration::from_secs(3),
        "the command path never wedged: {:?}",
        bounded.elapsed()
    );
    assert!(host
        .registrations()
        .plugins
        .iter()
        .any(|status| status.state == oc_plugin_host::PluginState::Active));
    host.shutdown();
}

#[test]
fn sleeping_beyond_every_timeout_is_bounded_everywhere() {
    let sandbox = common::sandbox("fuzz-slow");
    let slowall = common::copy_fixture(&sandbox, "slowall.ts");
    let slowsetup = common::copy_fixture(&sandbox, "slowsetup.ts");
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(
        PluginHostConfig {
            env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
            registration_timeout: Duration::from_millis(400),
            dispatch_timeout: Duration::from_millis(400),
            session_port: Some(std::sync::Arc::new(common::RecordingSessionPort::default())),
            ..PluginHostConfig::default()
        },
        &[slowsetup.clone(), slowall],
    );
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // -- registration: the sleeping setup fails contained (a Failed
    //    plugin, not a crash, not a disable — the neighbor is Active).
    let statuses = host.registrations().plugins;
    let setup = statuses
        .iter()
        .find(|status| status.entry == slowsetup)
        .map(|status| status.state.clone())
        .expect("slowsetup tracked");
    assert!(
        matches!(&setup, oc_plugin_host::PluginState::Failed(why) if why.contains("timed out")),
        "the sleeping setup times out contained: {setup:?}"
    );
    assert!(statuses.iter().any(|status| status.state == oc_plugin_host::PluginState::Active));

    // -- event dispatch: the sleeping handler's event drops (counted),
    //    the NEXT event still delivers.
    let wake = sandbox.join("slowall-wake.txt");
    host.tap().publish(TapEvent::new("slow.tick", serde_json::json!({})));
    assert!(
        common::wait_for(Duration::from_secs(10), || {
            host.tap().publish(TapEvent::new("wake.tick", serde_json::json!({})));
            wake.exists()
        }),
        "the fast subscription delivers behind the sleeping handler"
    );
    assert!(
        common::wait_for(Duration::from_secs(10), || host.dispatch_counters().1 >= 1),
        "the sleeping dispatch was dropped (counted)"
    );

    // -- command execution: bounded.
    let started = Instant::now();
    let command = host.execute_command("slowall.crawl", None, Duration::from_millis(400));
    assert!(command.is_err(), "the sleeping command times out");
    assert!(started.elapsed() < Duration::from_secs(3), "bounded: {:?}", started.elapsed());

    // -- tool execution: bounded (the wave-4 dispatch seam).
    let started = Instant::now();
    let tool = host.execute_tool("slow_tool", &serde_json::json!({}), Duration::from_millis(400));
    assert!(tool.is_err(), "the sleeping tool times out");
    assert!(started.elapsed() < Duration::from_secs(3), "bounded: {:?}", started.elapsed());

    // -- the session-hook bridge: the sleeping context hook times out
    //    inside the port's dispatch and the prompt REJECTS cleanly (the
    //    recording port surfaces the hook failure).
    let started = Instant::now();
    let prompt = host.execute_command("slowall.ask", None, Duration::from_millis(900));
    assert!(prompt.is_err(), "the sleeping hook fails the prompt (dispatch-bounded): {prompt:?}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the hook path never wedged: {:?}",
        started.elapsed()
    );

    // -- no deadlock: a fresh dispatch still delivers afterwards.
    let after = sandbox.join("slowall-wake-2.txt");
    let _ = std::fs::remove_file(&after);
    assert!(
        common::wait_for(Duration::from_secs(10), || {
            host.tap().publish(TapEvent::new("wake2.tick", serde_json::json!({})));
            after.exists()
        }),
        "events still flow after every surface timed out"
    );
    host.shutdown();
}
