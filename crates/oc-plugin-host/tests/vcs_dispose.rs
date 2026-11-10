//! G-379②/G-379③ — the vcs transform's dispose semantics and default
//! editor, against a REAL bun guest:
//!
//! * dispose removes ONLY the disposing plugin's registrations (the
//!   old filter kept `pluginId:`-prefixed ids — which never exist, the
//!   store keys are raw provider ids — and dropped everyone else's:
//!   one dispose cleared the WHOLE table);
//! * the default editor round-trips a selection (ref vcs.ts:85-88's
//!   `editor.selection` get/set — in-memory, initially undefined).

mod common;

use std::time::Duration;

use oc_plugin_host::PluginHostConfig;

#[test]
fn vcs_dispose_is_ownership_scoped_and_default_round_trips() {
    let sandbox = common::sandbox("vcsdisp");
    let entry_a = common::copy_fixture(&sandbox, "vcsdisp-a.ts");
    let entry_b = common::copy_fixture(&sandbox, "vcsdisp-b.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..PluginHostConfig::default()
    };
    let host =
        oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry_a, entry_b]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // Both providers registered.
    let vcs_ids = |host: &oc_plugin_host::PluginHost| {
        host.registrations()
            .vcs_providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect::<Vec<_>>()
    };
    let before = vcs_ids(&host);
    assert!(before.contains(&"vcsA".to_owned()), "vcsA registered: {before:?}");
    assert!(before.contains(&"vcsB".to_owned()), "vcsB registered: {before:?}");

    // Plugin A disposes its transform: vcsA goes, vcsB SURVIVES.
    host.execute_command("vcsdisp.dropA", None, Duration::from_secs(10))
        .expect("dropA command runs");
    let after = vcs_ids(&host);
    assert!(!after.contains(&"vcsA".to_owned()), "the disposed plugin's row is gone: {after:?}");
    assert!(
        after.contains(&"vcsB".to_owned()),
        "the OTHER plugin's row survives the dispose: {after:?}"
    );

    // G-379③ — the default editor round-trips (plugin A set "vcsA"
    // during setup; the selection persists — the ref's in-memory
    // State editor selection).
    host.execute_command("vcsdisp.defaultGet", None, Duration::from_secs(10))
        .expect("defaultGet command runs");
    let default_file = sandbox.join("vcs-default.json");
    let default = std::fs::read_to_string(&default_file)
        .unwrap_or_else(|_| panic!("defaultGet wrote its result (missing {default_file:?})"));
    assert!(default.contains("vcsA"), "the selection round-tripped: {default}");

    host.shutdown();
}
