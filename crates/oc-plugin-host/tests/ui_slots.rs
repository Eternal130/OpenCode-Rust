//! Wave 3 — the TUI slot bridge on the host side: the fixture plugin
//! fills every seam (dialog provider + group rule + home action at
//! registration; toast + sidebar section + footer hint + dialog open
//! as runtime UI requests), the queue drains in order, and the
//! materialize call is HARD-timeout bounded.

mod common;

use std::time::Duration;

use oc_plugin_host::{PluginDialogOption, PluginDialogSpec, UiRequest};

#[test]
fn tui_slots_register_and_queue_end_to_end() {
    let sandbox = common::sandbox("tuislots");
    let entry = common::copy_fixture(&sandbox, "tuislots.ts");
    let host = common::host_with(&[entry], &sandbox);
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready");

    // Registration-time entries land in the snapshot.
    let snapshot = host.registrations();
    assert!(snapshot.commands.iter().any(|command| command.name == "slots.open"));
    assert_eq!(
        snapshot.group_rules.iter().filter(|rule| rule.plugin_id == "tuislots").count(),
        2,
        "the prefix rule and the exact rule"
    );
    assert!(snapshot
        .group_rules
        .iter()
        .any(|rule| rule.tool_prefix.as_deref() == Some("omo_") && rule.header == "OMO tools"));
    assert!(snapshot
        .group_rules
        .iter()
        .any(|rule| rule.tool.as_deref() == Some("specialprobe") && rule.header == "Probe"));
    assert_eq!(snapshot.home_actions.len(), 1);
    assert_eq!(snapshot.home_actions[0].label, "/slots");
    assert_eq!(snapshot.home_actions[0].command, "slots.open");
    let dialogs: Vec<_> =
        snapshot.dialogs.iter().filter(|dialog| dialog.plugin_id == "tuislots").collect();
    assert_eq!(dialogs.len(), 1);
    assert_eq!(dialogs[0].id, "tuislots:picker", "ids qualify per plugin");

    // Runtime: dispatch the poke event through the tap, then drain.
    assert!(host.tap().publish(oc_plugin_host::TapEvent::new(
        "tui.poke",
        serde_json::json!({ "payload": "go" })
    )));
    assert!(common::wait_for(Duration::from_secs(10), || {
        sandbox.join("poked.txt").exists() && sandbox.join("registration.txt").exists()
    }));
    let requests = host.drain_ui_requests();
    assert!(requests.len() >= 4, "toast + section + hint + dialog: {requests:?}");
    assert!(matches!(
        &requests[0],
        UiRequest::Toast { plugin_id, title, message, variant, duration_ms }
            if plugin_id == "tuislots"
                && title.as_deref() == Some("Slots")
                && message == "poked by the fixture"
                && variant == "success"
                && *duration_ms == 1234
    ));
    assert!(requests.iter().any(|request| matches!(
        request,
        UiRequest::SidebarSection { plugin_id, title, rows }
            if plugin_id == "tuislots" && title == "Fixture" && rows.len() == 2
    )));
    assert!(requests.iter().any(|request| matches!(
        request,
        UiRequest::FooterHint { plugin_id, text } if plugin_id == "tuislots" && text == "slots on"
    )));
    assert!(requests.iter().any(|request| matches!(
        request,
        UiRequest::DialogOpen { plugin_id, spec }
            if plugin_id == "tuislots"
                && spec.title == "Poked"
                && spec.rows == ["row one".to_owned(), "row two".to_owned()]
                && spec.options.is_empty()
    )));

    // Materialize-at-open: the provider builds synchronously.
    let spec =
        host.materialize_dialog("tuislots:picker", Duration::from_secs(5)).expect("materialize");
    assert_eq!(
        spec,
        PluginDialogSpec {
            title: "Slot picker".into(),
            rows: vec![],
            options: vec![
                PluginDialogOption {
                    title: "First".into(),
                    value: "slots.open".into(),
                    description: "the palette command".into(),
                },
                PluginDialogOption {
                    title: "Second".into(),
                    value: "slots.other".into(),
                    description: String::new(),
                },
            ],
        }
    );
    // The RAW local id resolves too (exactly one provider owns it).
    assert!(host.materialize_dialog("picker", Duration::from_secs(5)).is_ok());
    // Unknown providers decline.
    assert!(host.materialize_dialog("nope", Duration::from_secs(1)).is_err());

    host.shutdown();
}

#[test]
fn dialog_materialize_is_hard_timeout_bounded() {
    let sandbox = common::sandbox("slowdialog");
    let entry = common::copy_fixture(&sandbox, "slowdialog.ts");
    let host = common::host_with(&[entry], &sandbox);
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready");

    let started = std::time::Instant::now();
    let outcome = host.materialize_dialog("slowdialog:slow", Duration::from_millis(400));
    let elapsed = started.elapsed();
    assert!(outcome.is_err(), "the never-settling build times out: {outcome:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "the hard timeout bounds the call (took {elapsed:?})"
    );
    // The host itself stays responsive afterwards.
    assert!(host.registrations().dialogs.iter().any(|dialog| dialog.id == "slowdialog:slow"));
    host.shutdown();
}
