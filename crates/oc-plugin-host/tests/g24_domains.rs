//! G-24 — the four newly-bridged plugin domains end-to-end against a
//! REAL bun guest: the `ctx.vcs` / `ctx.websearch` / `ctx.worktree` /
//! `ctx.integration` registrations land in the supervisor's snapshot
//! (no "unknown hook"/"unknown host method" rejection), the mock ports
//! answer `vcs.get` / `websearch.providers`, and the host's
//! `hook.dispatch` round-trips the guest closures (provider execute,
//! worktree create/list, OAuth authorize → callback → refresh).

mod common;

use std::path::Path;
use std::time::Duration;

use oc_plugin_host::{PluginHostConfig, VcsPort, WebsearchPort};
use serde_json::{json, Value};

fn futures_util_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

/// The mock websearch port: one provider row (the engine side of
/// `ctx.websearch.providers`).
struct MockWebsearchPort;

#[async_trait::async_trait]
impl WebsearchPort for MockWebsearchPort {
    async fn providers(&self) -> Result<Value, String> {
        Ok(json!([{ "id": "mocksearch", "name": "Mock Search" }]))
    }

    async fn query(&self, query: &str, provider_id: Option<&str>) -> Result<Value, String> {
        Ok(json!({ "providerID": provider_id.unwrap_or("mocksearch"), "results": [{ "url": format!("https://mock.invalid/{query}"), "time": {} }] }))
    }
}

/// The mock vcs port: a static branch shape (the engine side of
/// `ctx.vcs.get`).
struct MockVcsPort;

#[async_trait::async_trait]
impl VcsPort for MockVcsPort {
    async fn get(&self) -> Result<Value, String> {
        Ok(json!({ "branch": { "current": "lane-h", "default": "main" }, "provider": "git" }))
    }

    async fn base(&self) -> Result<Value, String> {
        Ok(json!("origin/main"))
    }
}

#[test]
fn g24_domains_register_and_round_trip() {
    let sandbox = common::sandbox("gdomains");
    let entry = common::copy_fixture(&sandbox, "gdomains.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        websearch_port: Some(std::sync::Arc::new(MockWebsearchPort)),
        vcs_port: Some(std::sync::Arc::new(MockVcsPort)),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // -- setup-time host-port reads answered (the sandbox files) --------
    let vcs_file = sandbox.join("vcs.json");
    let vcs = std::fs::read_to_string(&vcs_file)
        .unwrap_or_else(|_| panic!("ctx.vcs.get resolved (vcs.error: {:?})",
            std::fs::read_to_string(sandbox.join("vcs.error")).ok().or(Some("(none)".to_owned()))));
    assert!(vcs.contains("lane-h"), "vcs.get crossed the port: {vcs}");
    let providers = std::fs::read_to_string(sandbox.join("ws-providers.json"))
        .expect("ctx.websearch.providers resolved");
    assert!(providers.contains("mocksearch"), "providers crossed the port: {providers}");

    // -- registration coverage: the transforms + hooks survived the
    //    guest's whitelists ---------------------------------------------
    let snapshot = host.registrations();
    assert!(
        snapshot.websearch_providers.iter().any(|p| p.id == "guestsearch"),
        "websearch provider registered: {:?}",
        snapshot.websearch_providers
    );
    assert!(
        snapshot.worktree_strategies.iter().any(|s| s.id == "pluginwt"),
        "worktree strategy registered: {:?}",
        snapshot.worktree_strategies
    );
    assert_eq!(snapshot.integration_methods.len(), 1, "integration method registered");
    assert_eq!(snapshot.integration_methods[0].registration["integrationID"], json!("acme"));
    let hook_keys: Vec<String> = snapshot
        .hooks
        .iter()
        .map(|hook| format!("{}.{}", hook.domain, hook.name))
        .collect();
    for expected in [
        "websearch.execute",
        "worktree.create",
        "worktree.remove",
        "worktree.list",
        "integration.authorize",
        "integration.callback",
        "integration.refresh",
    ] {
        assert!(hook_keys.iter().any(|key| key == expected), "{expected} registered: {hook_keys:?}");
    }

    // -- websearch.execute: the guest provider's results ride back ------
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "websearch",
        "execute",
        json!({ "provider": "guestsearch", "query": "rust opencode" }),
    ))
    .expect("websearch.execute dispatch");
    assert_eq!(event["results"][0]["url"], json!("https://example.invalid/guest?q=rust%20opencode"),
        "guest provider executed: {event}");

    // -- worktree.create / list: the guest strategy's ops ride back -----
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "worktree",
        "create",
        json!({ "strategy": "pluginwt", "input": { "sourceDirectory": "/tmp/opencode", "directory": "/tmp/opencode/wt", "branch": null } }),
    ))
    .expect("worktree.create dispatch");
    assert_eq!(event["directory"], json!("/tmp/opencode/wt-guest"), "guest create: {event}");
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "worktree",
        "list",
        json!({ "strategy": "pluginwt", "input": { "directory": "/tmp/opencode" } }),
    ))
    .expect("worktree.list dispatch");
    assert_eq!(event["entries"][0]["kind"], json!("root"), "guest list: {event}");

    // -- integration authorize → callback: the OAuth flow rides back ----
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "integration",
        "authorize",
        json!({ "integrationID": "acme", "answer": {} }),
    ))
    .expect("integration.authorize dispatch");
    assert_eq!(event["result"]["url"], json!("https://acme.invalid/auth"), "authorize: {event}");
    assert_eq!(event["result"]["mode"], json!("code"));
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "integration",
        "callback",
        json!({ "integrationID": "acme", "code": "the-code" }),
    ))
    .expect("integration.callback dispatch");
    assert_eq!(event["credential"]["access"], json!("access-the-code"), "callback: {event}");
    let event = futures_util_block_on(host.dispatch_hook_async(
        "gdomains",
        "integration",
        "refresh",
        json!({ "integrationID": "acme", "credential": { "type": "oauth", "methodID": "acme-oauth", "refresh": "r", "access": "a", "expires": 0 } }),
    ))
    .expect("integration.refresh dispatch");
    assert_eq!(event["credential"]["access"], json!("refreshed-a"), "refresh: {event}");

    let _ = Path::new(&sandbox);
    host.shutdown();
}
