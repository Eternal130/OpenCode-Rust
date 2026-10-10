//! Hot reload (digest-scan parity): edit the fixture on disk, run
//! `reload_now`, and the re-imported module's new behavior shows up
//! in BOTH the registration snapshot and command execution.

mod common;

use std::time::Duration;

#[test]
fn digest_change_triggers_reload_with_new_behavior() {
    let sandbox = common::sandbox("hot");
    let fixture = common::copy_fixture(&sandbox, "helloworld.ts");
    let host = common::host_with(std::slice::from_ref(&fixture), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));
    let description = |host: &oc_plugin_host::PluginHost| {
        host.registrations()
            .commands
            .iter()
            .find(|command| command.name == "hw.hello")
            .map(|command| command.description.clone())
    };
    assert_eq!(description(&host).as_deref(), Some("Say hello from the helloworld plugin (v1)"));

    // No change → no reload.
    assert_eq!(host.reload_now().expect("scan clean"), 0);

    // Edit the module (the copy in the sandbox, never the repo file).
    let source = std::fs::read_to_string(&fixture).expect("fixture");
    let edited = source.replace("v1", "v2");
    std::fs::write(&fixture, edited).expect("edit fixture");

    assert_eq!(host.reload_now().expect("reload"), 1, "digest change reloaded");
    assert_eq!(
        description(&host).as_deref(),
        Some("Say hello from the helloworld plugin (v2)"),
        "snapshot reflects the new module"
    );
    assert_eq!(
        host.execute_command("hw.hello", None, Duration::from_secs(5)).expect("new execute"),
        serde_json::json!("hello v2"),
        "the re-imported execute closure is live"
    );
    host.shutdown();
}
