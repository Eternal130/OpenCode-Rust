//! PLUG-TUI-SLOT (host side): the v2 TUI-plugin slot/router/keymap
//! registration surface — `ctx.ui.slot` claims land in the
//! registration snapshot, materialize through the bounded
//! `slot.claim.render` call, click through `slot.claim.click`;
//! `ctx.ui.router.register` records the named route (materialized by
//! `route.render`, navigated by `tui.route.plugin` UI requests); and
//! `ctx.keymap.layer` commands land as native keymap entries whose
//! `run` dispatches through `keymap.command.run`.
//!
//! Hermetic: /tmp/opencode sandbox, no network (the marker files are
//! the observable effects).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-slot-claims-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slotclaims.ts"),
        dir.join("slotclaims.ts"),
    )
    .expect("fixture copy");
    dir
}

fn boot() -> Option<(Arc<oc_plugin_host::PluginHost>, PathBuf)> {
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect()
        .start_detached_with(config, &[sandbox.join("slotclaims.ts")]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());
    Some((Arc::new(host), sandbox))
}

fn wait_for(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The registration snapshot carries the slot claims, the route, and
/// BOTH keymap layers (the registration-time one and the one an
/// `app` claim's render installs at its first materialize).
#[test]
fn snapshot_carries_claims_routes_and_keymap_layers() {
    let Some((host, sandbox)) = boot() else { return };

    // The registration-time layer is immediate.
    let snapshot = host.registrations();
    assert!(
        snapshot
            .slot_claims
            .iter()
            .any(|claim| claim.kind == "append" && claim.target == "sidebar.content"),
        "the sidebar claim registered: {:?}",
        snapshot.slot_claims
    );
    assert!(
        snapshot.slot_claims.iter().any(|claim| claim.target == "app"),
        "the app claim registered: {:?}",
        snapshot.slot_claims
    );
    assert!(
        snapshot
            .routes
            .iter()
            .any(|route| route.name == "board" && route.plugin_id == "slotclaims"),
        "the route registered: {:?}",
        snapshot.routes
    );
    assert!(
        snapshot
            .keymap_commands
            .iter()
            .any(|command| command.id == "claims.open" && command.bind == "ctrl+x o"),
        "the registration-time layer: {:?}",
        snapshot.keymap_commands
    );

    // The app claim's render installs its layer DYNAMICALLY — run the
    // claim's materialize once, then the snapshot sees it.
    let app_claim =
        snapshot.slot_claims.iter().find(|claim| claim.target == "app").expect("the app claim");
    host.render_slot_claim("slotclaims", &app_claim.key, Duration::from_secs(5))
        .expect("app claim materializes");
    let snapshot = host.registrations();
    assert!(
        snapshot.keymap_commands.iter().any(|command| command.id == "claims.ping"
            && command.bind == "ctrl+x p"
            && command.palette),
        "the dynamic layer from the app claim's render: {:?}",
        snapshot.keymap_commands
    );

    host.shutdown();
    let _ = sandbox;
}

/// The claim materializes to the native section shape and its rows
/// click through the plugin's own `onMouseUp` (the marker file).
#[test]
fn claim_renders_and_clicks() {
    let Some((host, sandbox)) = boot() else { return };

    let snapshot = host.registrations();
    let claim = snapshot
        .slot_claims
        .iter()
        .find(|claim| claim.target == "sidebar.content")
        .expect("the sidebar claim");
    let spec =
        host.render_slot_claim("slotclaims", &claim.key, Duration::from_secs(5)).expect("render");
    assert_eq!(spec.title, "Claims");
    assert_eq!(spec.rows, ["target: board".to_owned(), "plain row".to_owned()]);
    assert_eq!(spec.clickable, [true, false], "row 0 carries the plugin's onMouseUp");

    // The click runs the plugin's handler (the marker file).
    assert!(
        host.click_slot_claim("slotclaims", &claim.key, 0, Duration::from_secs(5)).expect("click"),
        "the click dispatches"
    );
    assert!(
        wait_for(Duration::from_secs(5), || sandbox.join("claim-click.txt").exists()),
        "the plugin's onMouseUp ran"
    );
    assert!(
        !host
            .click_slot_claim("slotclaims", &claim.key, 1, Duration::from_secs(5))
            .unwrap_or(false),
        "the plain row has no handler"
    );

    host.shutdown();
}

/// The route materializes with its navigation data and the keymap
/// command's `run` dispatches guest-side.
#[test]
fn route_renders_and_keymap_command_runs() {
    let Some((host, sandbox)) = boot() else { return };

    let spec = host
        .render_route(
            "slotclaims",
            "board",
            Some(&serde_json::json!({ "story": "alpha" })),
            Duration::from_secs(5),
        )
        .expect("route render");
    assert_eq!(spec.title, "Board");
    assert_eq!(spec.rows, ["story: alpha".to_owned()]);

    assert!(
        host.run_keymap_command("slotclaims", "claims.open", Duration::from_secs(5))
            .expect("run dispatches"),
        "the layer command ran"
    );
    assert!(
        wait_for(Duration::from_secs(5), || sandbox.join("open-run.txt").exists()),
        "the run callback executed guest-side"
    );

    // The run's router.navigate ask lands on the UI-request queue as
    // a NavigateToPluginRoute (drained between frames).
    let requests = host.drain_ui_requests();
    assert!(
        requests.iter().any(|request| matches!(
            request,
            oc_plugin_host::UiRequest::NavigateToPluginRoute { plugin_id, name, data }
                if plugin_id == "slotclaims" && name == "board"
                    && data.as_ref().and_then(|d| d.get("story")).and_then(|s| s.as_str()) == Some("alpha")
        )),
        "the plugin-route navigation ask: {requests:?}"
    );

    host.shutdown();
}
