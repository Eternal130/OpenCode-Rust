//! PLUG-ENGINE-MERGE's host-side contract test: the `mcpreg` fixture
//! (a plugin whose setup declares a server through
//! `ctx.mcp.transform`, host.ts:374-403) materializes into
//! `registrations.mcps` as a v2 `Mcp.ServerConfig` JSON — the exact
//! shape the engine's per-location merge consumes (oc-core
//! `LocationPluginSource::mcps` → `McpRegistry::add`; a HEADLESS
//! server lists the server with no TUI attached — see
//! oc-server/tests/plugin_engine_mcp.rs). Locks the fixture↔seam
//! contract: name + parseable config with the declared fields.

mod common;

use std::time::Duration;

#[test]
fn mcpreg_materializes_the_declared_server_into_registrations() {
    let sandbox = common::sandbox("mcpreg");
    let fixture = common::copy_fixture(&sandbox, "mcpreg.ts");
    let host = common::host_with(&[fixture], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready: {:?}", host.start_error());

    let snapshot = host.registrations();
    assert_eq!(snapshot.mcps.len(), 1, "one declared server: {:?}", snapshot.mcps);
    let mcp = &snapshot.mcps[0];
    assert_eq!(mcp.plugin_id, "mcpreg");
    assert_eq!(mcp.name, "plugindemo");
    // The v2 `Mcp.ServerConfig` the engine seam re-validates (the ref
    // decodes it with `Schema.decodeUnknownSync(ServerConfig)`,
    // host.ts:387-397) — the declared command + environment survive.
    assert_eq!(mcp.config["type"], "local");
    assert_eq!(mcp.config["command"], serde_json::json!(["cat"]));
    assert_eq!(mcp.config["environment"]["OC_PLUGIN_MCP"], "1");
}
