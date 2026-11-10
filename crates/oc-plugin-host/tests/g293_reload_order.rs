//! G-293 — hot-reload ORDER and revision semantics (ref
//! core/plugin/supervisor.ts:70-86 + plugin.ts:100-180):
//! * a reload whose new source FAILS to load keeps the previous
//!   generation's registrations alive and functional (the core
//!   regression — the old port cleared them first);
//! * an unchanged revision never reloads (no load call / churn);
//! * a FAILED revision is not retried until the source changes again.

mod common;

use std::time::Duration;

use oc_plugin_host::PluginHost;

const TIMEOUT: Duration = Duration::from_secs(15);

fn loads_marker(sandbox: &std::path::Path) -> usize {
    common::read(&sandbox.join("reload-probe-loads.txt"))
        .lines()
        .filter(|line| line == &"load")
        .count()
}

fn description(host: &PluginHost) -> Option<String> {
    host.registrations()
        .commands
        .iter()
        .find(|command| command.name == "probe.ping")
        .map(|command| command.description.clone())
}

/// The core regression: a reload whose new source throws at setup
/// keeps the OLD plugin's registrations present and functional.
#[test]
fn failing_reload_keeps_previous_generation() {
    let sandbox = common::sandbox("g293fail");
    let fixture = common::copy_fixture(&sandbox, "reloadprobe.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(TIMEOUT));
    assert_eq!(
        host.execute_command("probe.ping", None, Duration::from_secs(5)).expect("v1 execute"),
        serde_json::json!("pong-v1")
    );

    // Break the source: setup throws on load.
    let original = std::fs::read_to_string(&fixture).expect("fixture");
    let broken = original.replace(
        "appendFileSync(join(SANDBOX, \"reload-probe-loads.txt\"), \"load\\n\");",
        "throw new Error(\"probe boom\");",
    );
    assert_ne!(broken, original, "the edit must apply");
    std::fs::write(&fixture, broken).expect("edit fixture");

    // The reload fails — reloaded count 0, old registrations intact.
    assert_eq!(host.reload_now().expect("reload outcome"), 0, "failing reload counts 0");
    assert!(
        common::wait_for(TIMEOUT, || description(&host).is_some()),
        "the old registration survives the failed reload"
    );
    assert_eq!(description(&host).as_deref(), Some("reload probe command"));
    assert_eq!(
        host.execute_command("probe.ping", None, Duration::from_secs(5))
            .expect("old execute still works"),
        serde_json::json!("pong-v1"),
        "the previous generation is functional after the failed reload"
    );
    assert_eq!(loads_marker(&sandbox), 1, "the failed setup never completed");

    // A subsequent no-change reload does NOT retry the failed revision.
    assert_eq!(host.reload_now().expect("no-change reload"), 0);
    assert_eq!(loads_marker(&sandbox), 1, "the failed revision is not retried");

    // Another source change (a good one) retries and wins.
    let good = original.replace("pong-v1", "pong-v2");
    std::fs::write(&fixture, good).expect("fix fixture");
    assert_eq!(host.reload_now().expect("retry reload"), 1, "a changed revision retries");
    assert_eq!(
        host.execute_command("probe.ping", None, Duration::from_secs(5)).expect("v2 execute"),
        serde_json::json!("pong-v2")
    );
    assert_eq!(loads_marker(&sandbox), 2);
    host.shutdown();
}

/// Unchanged source (same revision) → the plugin is NOT reloaded: no
/// load passes the guest (the marker file is the observable counter).
#[test]
fn unchanged_revision_is_not_reloaded() {
    let sandbox = common::sandbox("g293same");
    let fixture = common::copy_fixture(&sandbox, "reloadprobe.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(TIMEOUT));
    assert_eq!(loads_marker(&sandbox), 1, "one boot-time load");

    assert_eq!(host.reload_now().expect("no-change reload"), 0);
    assert_eq!(loads_marker(&sandbox), 1, "no setup re-ran on an unchanged revision");

    // And a changed revision DOES reload (the diff works).
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    std::fs::write(&fixture, source.replace("pong-v1", "pong-v2")).expect("edit fixture");
    assert_eq!(host.reload_now().expect("changed reload"), 1);
    assert_eq!(loads_marker(&sandbox), 2);
    host.shutdown();
}

/// A successful reload replaces the previous generation's registration
/// entries (the swap happens, atomically on success).
#[test]
fn successful_reload_swaps_the_registration() {
    let sandbox = common::sandbox("g293swap");
    let fixture = common::copy_fixture(&sandbox, "reloadprobe.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(TIMEOUT));
    // Register a SECOND command only in v1: after the swap it must be
    // GONE (the new generation never registered it).
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    let v1_extra = source.replace(
        "return async () => {};",
        "await ctx.command.add({ name: \"probe.old\", description: \"v1 only\", async execute() { return 1; } });\n        return async () => {};",
    );
    std::fs::write(&fixture, v1_extra).expect("edit fixture");
    assert_eq!(host.reload_now().expect("reload v1-extra"), 1);
    assert!(
        host.registrations().commands.iter().any(|command| command.name == "probe.old"),
        "the v1-only command registered"
    );

    // Back to the plain v2 source: probe.old must not survive the swap.
    std::fs::write(&fixture, source.replace("pong-v1", "pong-v2")).expect("edit fixture");
    assert_eq!(host.reload_now().expect("reload v2"), 1);
    assert!(
        common::wait_for(TIMEOUT, || {
            !host.registrations().commands.iter().any(|command| command.name == "probe.old")
        }),
        "the old generation's stale registration is dropped on a successful swap"
    );
    host.shutdown();
}
