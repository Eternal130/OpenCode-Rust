//! RC-03 — two-class per-dispatch timeouts: body-mutating hook points
//! (`tool.transform` / `tool.execute.*` / `chat.*` / `session.*`) ride
//! a 120 s hard budget aligned with oc-server's `EXECUTE_TIMEOUT`;
//! cheap query hooks keep the configured dispatch timeout; the 2 s
//! registration budget is unchanged.

mod common;

use std::time::Duration;

use oc_plugin_host::{PluginHostConfig, MUTATING_DISPATCH_TIMEOUT};

#[test]
fn mutating_hook_past_five_seconds_completes() {
    let sandbox = common::sandbox("hookslow-mutating");
    let entry = common::copy_fixture(&sandbox, "hookslow.ts");
    // A query budget far below the handler's sleep: only the
    // two-class split lets the session.* dispatch finish.
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        dispatch_timeout: Duration::from_millis(300),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    assert_eq!(MUTATING_DISPATCH_TIMEOUT, Duration::from_secs(120));

    let started = std::time::Instant::now();
    let event = futures_util_block_on(host.dispatch_hook_async(
        "hookslow",
        "session",
        "context",
        serde_json::json!({ "messages": [] }),
    ))
    .expect("mutating hook completes past the 5 s query budget");
    assert!(started.elapsed() >= Duration::from_secs(6), "handler slept 6.5 s");
    assert!(sandbox.join("session-done.txt").exists(), "handler ran to completion");
    assert_eq!(event, serde_json::json!({ "messages": [] }));
    host.shutdown();
}

#[test]
fn query_hook_past_dispatch_budget_times_out() {
    let sandbox = common::sandbox("hookslow-query");
    let entry = common::copy_fixture(&sandbox, "hookslow.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        dispatch_timeout: Duration::from_millis(300),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let started = std::time::Instant::now();
    let error = futures_util_block_on(host.dispatch_hook_async(
        "hookslow",
        "shell",
        "create.before",
        serde_json::json!({ "command": "ls" }),
    ))
    .expect_err("query hook over budget is dropped");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded by the dispatch budget, not the mutating class"
    );
    assert!(
        error.to_lowercase().contains("timeout") || error.to_lowercase().contains("timed out"),
        "error mentions the timeout: {error}"
    );
    host.shutdown();
}

/// The classification table itself (registration stays 2 s — covered
/// by the existing registration suites; asserted here as the constant
/// pair the bridge is built on).
#[test]
fn hook_timeout_classification() {
    use oc_plugin_host::is_mutating_hook;
    for (domain, name) in [
        ("tool", "transform"),
        ("tool", "execute.before"),
        ("tool", "execute.after"),
        ("chat", "message"),
        ("session", "prompt"),
        ("session", "context"),
        ("session", "model.request"),
        ("session", "http.request"),
    ] {
        assert!(is_mutating_hook(domain, name), "{domain}.{name} is mutating");
    }
    for (domain, name) in [("shell", "create.before"), ("tool", "list"), ("event", "tick")] {
        assert!(!is_mutating_hook(domain, name), "{domain}.{name} stays query-class");
    }
    assert_eq!(
        oc_plugin_host::PluginHostConfig::default().registration_timeout,
        Duration::from_secs(2),
        "registration budget unchanged"
    );
    assert_eq!(
        oc_plugin_host::PluginHostConfig::default().dispatch_timeout,
        Duration::from_secs(5),
        "query-class default unchanged"
    );
}

fn futures_util_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}
