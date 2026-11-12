//! G-333 — the node fallback track, exercised for real: host.js
//! spawned by the system node (`node host.js`) with the `BunShim`
//! covering the three-API production surface. A representative
//! subset of the bun suite (hello-world registration + command
//! round-trip, hot reload through the shim's regex digest scan, and
//! the narrowed sandbox face) — the full bun suite stays untouched.
//!
//! The node track is forced via `OC_PLUGIN_NODE` (which genuinely
//! forces node even on bunful machines); skipped when node is
//! absent, mirroring the bun suite's skip discipline.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use oc_plugin_host::PluginHost;

/// The absolute node path (the override needs one; PATH-presence
/// resolution is covered by the bun.rs unit tests).
fn node_path() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("node")).find(|candidate| {
        candidate.is_file()
            && candidate.metadata().is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
    })
}

/// A host forced onto the node track over `entries`.
fn host_with_node(entries: &[PathBuf], sandbox: &std::path::Path) -> PluginHost {
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    PluginHost::detect().start_detached_with(config, entries)
}

/// The env forcing lives in a guard so a failing test still restores
/// the process env (nextest runs per-process, but stay tidy).
struct ForcedNode(Option<std::ffi::OsString>);

impl ForcedNode {
    fn force() -> Option<Self> {
        let node = node_path()?;
        let saved = std::env::var_os("OC_PLUGIN_NODE");
        std::env::set_var("OC_PLUGIN_NODE", node);
        Some(Self(saved))
    }
}

impl Drop for ForcedNode {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("OC_PLUGIN_NODE", value),
            None => std::env::remove_var("OC_PLUGIN_NODE"),
        }
    }
}

#[test]
fn node_track_hello_world_end_to_end() {
    let Some(_guard) = ForcedNode::force() else {
        eprintln!("node unavailable; skipping");
        return;
    };
    let sandbox = common::sandbox("node-hello");
    let hello = common::copy_fixture(&sandbox, "helloworld.ts");
    let bad = common::copy_fixture(&sandbox, "badshape.ts");
    let host = host_with_node(&[hello.clone(), bad.clone()], &sandbox);
    assert!(host.enabled(), "the forced node track must enable the host");
    assert!(host.wait_ready(Duration::from_secs(30)), "node host ready: {:?}", host.start_error());

    let snapshot = host.registrations();
    let command = snapshot
        .commands
        .iter()
        .find(|command| command.name == "hw.hello")
        .expect("hw.hello registered under node");
    assert_eq!(command.plugin_id, "helloworld");
    let hello_status = snapshot
        .plugins
        .iter()
        .find(|status| status.plugin_id == "helloworld")
        .expect("helloworld status");
    assert_eq!(hello_status.state, oc_plugin_host::PluginState::Active);
    // The bad-shape neighbor fails CONTAINED under node too.
    let bad_status =
        snapshot.plugins.iter().find(|status| status.entry == bad).expect("badshape status");
    assert!(
        matches!(&bad_status.state, oc_plugin_host::PluginState::Failed(reason) if reason.contains("plugin")),
        "badshape failed containedly under node: {:?}",
        bad_status.state
    );

    let result = host.execute_command("hw.hello", None, Duration::from_secs(10));
    assert_eq!(result.expect("execute ok under node"), serde_json::json!("hello v1"));
    assert!(
        common::wait_for(Duration::from_secs(5), || sandbox.join("executed.txt").exists()),
        "execute marker written by the plugin under node"
    );
    host.shutdown();
}

#[test]
fn node_track_hot_reload_through_the_bun_shim() {
    let Some(_guard) = ForcedNode::force() else {
        eprintln!("node unavailable; skipping");
        return;
    };
    let sandbox = common::sandbox("node-hot");
    let fixture = common::copy_fixture(&sandbox, "helloworld.ts");
    let host = host_with_node(std::slice::from_ref(&fixture), &sandbox);
    assert!(host.wait_ready(Duration::from_secs(30)), "node host ready");
    let description = |host: &oc_plugin_host::PluginHost| {
        host.registrations()
            .commands
            .iter()
            .find(|command| command.name == "hw.hello")
            .map(|command| command.description.clone())
    };
    assert_eq!(description(&host).as_deref(), Some("Say hello from the helloworld plugin (v1)"));

    // No change → no reload (the shim's scanImports feeds the digest
    // walk; the entry itself is covered either way).
    assert_eq!(host.reload_now().expect("scan clean"), 0);

    // Edit → the digest changes → the re-imported module's new
    // behavior is live (node's ESM cache is busted by the `?t=`
    // generation query, the source.node.ts parity).
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    let edited = source.replace("v1", "v2");
    std::fs::write(&fixture, edited).expect("edit fixture");
    assert_eq!(host.reload_now().expect("reload"), 1, "digest change reloaded under node");
    assert_eq!(description(&host).as_deref(), Some("Say hello from the helloworld plugin (v2)"));
    assert_eq!(
        host.execute_command("hw.hello", None, Duration::from_secs(10)).expect("new execute"),
        serde_json::json!("hello v2")
    );
    host.shutdown();
}

#[test]
fn node_track_global_bun_stays_undefined() {
    let Some(_guard) = ForcedNode::force() else {
        eprintln!("node unavailable; skipping");
        return;
    };
    let sandbox = common::sandbox("node-bunglobal");
    // An inline probe plugin: reads globalThis.Bun at setup AND per
    // command execution. Under the node track the BunShim is a
    // module-scope binding — globalThis.Bun must be undefined
    // (strictly narrower than the G-334c bun sandbox face).
    let probe = sandbox.join("probeglobal.ts");
    std::fs::write(
        &probe,
        r#"import { writeFileSync } from "node:fs";
export default {
  id: "probeglobal",
  async setup(ctx) {
    writeFileSync(new URL("./bun-global.txt", import.meta.url), String(globalThis.Bun));
    await ctx.command.add({
      name: "probe.bun",
      description: "report the Bun global",
      execute: async () => String(globalThis.Bun),
    });
  },
};
"#,
    )
    .expect("write probe plugin");
    let host = host_with_node(&[probe], &sandbox);
    assert!(host.wait_ready(Duration::from_secs(30)), "node host ready");
    let result = host.execute_command("probe.bun", None, Duration::from_secs(10));
    assert_eq!(result.expect("probe ok"), serde_json::json!("undefined"));
    assert_eq!(common::read(&sandbox.join("bun-global.txt")), "undefined");
    host.shutdown();
}
