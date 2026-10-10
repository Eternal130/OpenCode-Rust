//! V1+V2 dual-support check (plugin-map §10): one entrypoint may
//! default-export v2 `{ id, setup }` **spread together with** a v1
//! `server()` hooks function (v2 ignores `server()`); v1-only object
//! and older v1 function exports are tolerated (loaded inert).

mod common;

use std::time::Duration;

use oc_plugin_host::PluginState;

#[test]
fn v1_and_v2_shapes_coexist() {
    let sandbox = common::sandbox("v1dual");
    let dual = common::copy_fixture(&sandbox, "v1dual.ts");
    let only = common::copy_fixture(&sandbox, "v1only.ts");
    let fun = common::copy_fixture(&sandbox, "v1fn.ts");
    let host = common::host_with(&[dual.clone(), only.clone(), fun.clone()], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready");

    let snapshot = host.registrations();

    // The dual export: the v2 half ran (server() ignored, not fatal).
    let dual_status =
        snapshot.plugins.iter().find(|status| status.entry == dual).expect("v1dual status");
    assert_eq!(dual_status.state, PluginState::Active);
    assert!(snapshot.commands.iter().any(|command| command.name == "v1dual.cmd"));

    // v1-only shapes: loaded inert, distinct derived ids.
    for (entry, id) in [(only, "v1:v1only"), (fun, "v1:v1fn")] {
        let status = snapshot
            .plugins
            .iter()
            .find(|status| status.entry == entry)
            .unwrap_or_else(|| panic!("{id} status"));
        assert_eq!(status.plugin_id, id, "v1 shape tolerated with derived id");
        assert_eq!(status.state, PluginState::Active);
    }
    assert!(snapshot.commands.iter().all(|command| !command.plugin_id.starts_with("v1:")));

    // The host stays responsive across all of it.
    assert!(
        host.execute_command("v1dual.cmd", None, Duration::from_secs(5)).is_ok(),
        "v2-half command executes"
    );
    host.shutdown();
}
