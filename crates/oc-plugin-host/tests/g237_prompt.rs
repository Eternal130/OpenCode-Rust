//! G-237 — `ctx.ui.prompt` end-to-end against a REAL bun guest: the
//! fixture's `tui.poke` handler awaits a prompt; the deferred request
//! carries the guest's jsonrpc id (the promise's settle key) plus the
//! ref's option fields (api.tsx:367-386 → DialogPromptOptions,
//! context.ts:348-353). The embedder's `resolve_ui_request` settles
//! the promise — the typed string on submit, null on dismiss.

mod common;

use std::time::Duration;

use oc_plugin_host::UiRequest;

/// Poll the UI-request queue until one prompt lands (the drain
/// consumes; the found request is captured, never dropped).
fn wait_for_prompt(host: &oc_plugin_host::PluginHost, label: &str) -> UiRequest {
    let found = std::cell::RefCell::new(None);
    let ok = common::wait_for(Duration::from_secs(10), || {
        if found.borrow().is_some() {
            return true;
        }
        for request in host.drain_ui_requests() {
            if matches!(request, UiRequest::Prompt { .. }) {
                *found.borrow_mut() = Some(request);
                return true;
            }
        }
        false
    });
    found.into_inner().unwrap_or_else(|| panic!("{label}"))
}

#[test]
fn g237_prompt_resolves_typed_value_and_null_on_dismiss() {
    let sandbox = common::sandbox("g237prompt");
    let entry = common::copy_fixture(&sandbox, "g237prompt.ts");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)), "host ready");

    // Round 1: the prompt queues with the full option shape.
    assert!(host.tap().publish(oc_plugin_host::TapEvent::new(
        "tui.poke",
        serde_json::json!({ "round": 1 })
    )));
    let request = wait_for_prompt(&host, "the prompt request queued");
    let request_id = match &request {
        UiRequest::Prompt { request_id, plugin_id, title, description, placeholder, value } => {
            assert_eq!(plugin_id, "g237prompt");
            assert_eq!(title, "Name the preset");
            assert_eq!(description.as_deref(), Some("One line of description"));
            assert_eq!(placeholder.as_deref(), Some("preset name"));
            assert_eq!(value.as_deref(), Some("seed-value"));
            *request_id
        }
        other => panic!("unexpected request {other:?}"),
    };

    // The TUI's submit: the typed string resolves the guest's promise.
    host.resolve_ui_request(request_id, serde_json::json!("typed value"));
    assert!(
        common::wait_for(Duration::from_secs(10), || {
            common::read(&sandbox.join("prompt-1.json")) == "\"typed value\""
        }),
        "the guest promise resolved with the typed value"
    );

    // Round 2: the dismissed prompt settles null (any unresolved close).
    assert!(host.tap().publish(oc_plugin_host::TapEvent::new(
        "tui.poke",
        serde_json::json!({ "round": 2 })
    )));
    let request = wait_for_prompt(&host, "the second prompt request queued");
    let request_id = match &request {
        UiRequest::Prompt { request_id, .. } => *request_id,
        other => panic!("unexpected request {other:?}"),
    };
    assert_ne!(request_id, 0, "each prompt carries its own deferred id");
    host.resolve_ui_request(request_id, serde_json::Value::Null);
    assert!(
        common::wait_for(Duration::from_secs(10), || {
            common::read(&sandbox.join("prompt-2.json")) == "null"
        }),
        "the dismissed prompt resolved null"
    );
}
