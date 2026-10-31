//! PLUG-G2 — the three formerly-unbridged hook domains
//! (`permission.evaluate`, `aisdk.sdk`, `aisdk.language`): a REAL
//! plugin's `ctx.permission.hook` / `ctx.aisdk.hook` registrations
//! land in the supervisor's snapshot (no "unknown hook" rejection)
//! and the host's `hook.dispatch` round-trips each guest mutation.

mod common;

use std::time::Duration;

use oc_plugin_host::PluginHostConfig;
use serde_json::json;

fn futures_util_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

#[test]
fn permission_and_aisdk_hooks_register_and_round_trip() {
    let sandbox = common::sandbox("permaisdk");
    let entry = common::copy_fixture(&sandbox, "permaisdk.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // Registration coverage: all three subscriptions survive the
    // guest's domain whitelist (the former "unknown hook" failure).
    let hooks = &host.registrations().hooks;
    let keys: Vec<String> =
        hooks.iter().map(|hook| format!("{}.{}", hook.domain, hook.name)).collect();
    for expected in ["permission.evaluate", "aisdk.sdk", "aisdk.language"] {
        assert!(keys.iter().any(|key| key == expected), "{expected} registered: {keys:?}");
    }

    // permission.evaluate: the guest's effect/message rewrite rides back.
    let event = futures_util_block_on(host.dispatch_hook_async(
        "permaisdk",
        "permission",
        "evaluate",
        json!({
            "sessionID": "ses_perm",
            "action": "read",
            "resources": [".env"],
            "effect": "ask"
        }),
    ))
    .expect("permission dispatch");
    assert_eq!(event["effect"], json!("deny"), "effect rewritten: {event}");
    assert_eq!(event["message"], json!("denied-by-plugin"));

    // aisdk.sdk: the package + the JSON sdk handle swap.
    let event = futures_util_block_on(host.dispatch_hook_async(
        "permaisdk",
        "aisdk",
        "sdk",
        json!({ "model": { "id": "m" }, "package": "@ai-sdk/anthropic", "options": {} }),
    ))
    .expect("aisdk.sdk dispatch");
    assert_eq!(event["package"], json!("@ai-sdk/swapped"), "package swapped: {event}");
    assert_eq!(event["sdk"], json!({ "wrapped": true }), "sdk handle swapped");

    // aisdk.language: the options mutation rides back.
    let event = futures_util_block_on(host.dispatch_hook_async(
        "permaisdk",
        "aisdk",
        "language",
        json!({ "model": { "id": "m" }, "options": {} }),
    ))
    .expect("aisdk.language dispatch");
    assert_eq!(event["options"]["touched"], json!("by-plugin"), "options mutated: {event}");

    host.shutdown();
}
