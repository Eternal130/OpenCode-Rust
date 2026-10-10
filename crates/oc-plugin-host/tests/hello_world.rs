//! The wave-1 acceptance test (oracle-gate11 §4): a hello-world
//! plugin — `{ id: "helloworld", setup }` — loaded by the `PluginHost`
//! (bun spawned from the fixture's sandbox copy), registering a
//! command (+ owned keymap entry), a theme document, and an event
//! subscription; a synthetic event dispatched through the tap reaches
//! the plugin (marker file), command execution round-trips, and a
//! bad-shape neighbor plugin fails CONTAINED.

mod common;

use std::time::Duration;

use oc_plugin_host::TapEvent;

#[test]
fn hello_world_end_to_end() {
    let sandbox = common::sandbox("hello");
    let hello = common::copy_fixture(&sandbox, "helloworld.ts");
    let bad = common::copy_fixture(&sandbox, "badshape.ts");
    let host = common::host_with(&[hello.clone(), bad.clone()], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready: {:?}", host.start_error());

    // -- registration snapshot -------------------------------------------------
    let snapshot = host.registrations();
    let command = snapshot
        .commands
        .iter()
        .find(|command| command.name == "hw.hello")
        .expect("hw.hello registered");
    assert_eq!(command.plugin_id, "helloworld");
    assert_eq!(command.description, "Say hello from the helloworld plugin (v1)");
    assert_eq!(command.keybinds, vec!["ctrl+x h".to_owned()]);
    assert!(command.palette);
    let hidden = snapshot
        .commands
        .iter()
        .find(|command| command.name == "hw.hidden")
        .expect("hw.hidden registered");
    assert!(!hidden.palette);
    let theme = snapshot
        .themes
        .iter()
        .find(|theme| theme.name == "helloworld-theme")
        .expect("theme registered");
    assert!(
        theme.document.contains("#3b82f6"),
        "theme document carried the JSON: {}",
        theme.document
    );
    let hello_status = snapshot
        .plugins
        .iter()
        .find(|status| status.plugin_id == "helloworld")
        .expect("helloworld status");
    assert_eq!(hello_status.state, oc_plugin_host::PluginState::Active);
    // The bad-shape neighbor fails CONTAINED — the host kept serving.
    let bad_status =
        snapshot.plugins.iter().find(|status| status.entry == bad).expect("badshape status");
    assert!(
        matches!(&bad_status.state, oc_plugin_host::PluginState::Failed(reason) if reason.contains("plugin")),
        "badshape failed containedly: {:?}",
        bad_status.state
    );

    // -- command execution (async, round-tripped through the guest) ------------
    let result = host.execute_command("hw.hello", None, Duration::from_secs(5));
    assert_eq!(result.expect("execute ok"), serde_json::json!("hello v1"));
    assert!(
        common::wait_for(Duration::from_secs(5), || sandbox.join("executed.txt").exists()),
        "execute marker written by the plugin"
    );

    // -- post-ingest tap: a synthetic event reaches the subscriber -------------
    let marker = sandbox.join("event.txt");
    let tap = host.tap();
    assert!(tap.is_live());
    tap.publish(TapEvent::new("tui.test", serde_json::json!({ "payload": "wave1" })));
    assert!(
        common::wait_for(Duration::from_secs(10), || marker.exists()),
        "plugin received the tapped event"
    );
    assert_eq!(common::read(&marker), "tui.test:wave1");

    // A non-subscribed type must NOT reach the plugin.
    let _ = std::fs::remove_file(&marker);
    tap.publish(TapEvent::new("other.type", serde_json::json!({})));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!marker.exists(), "unsubscribed type filtered");

    let (dispatched, dropped) = host.dispatch_counters();
    assert!(dispatched >= 1, "dispatch counter advanced ({dispatched}/{dropped})");

    host.shutdown();
}
