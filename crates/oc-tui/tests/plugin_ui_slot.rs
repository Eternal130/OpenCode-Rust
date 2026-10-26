//! PLUG-TUI-SLOT — the TUI-side integration (the ledger's
//! `slot_renders_and_clicks`): the slotclaims fixture loads through
//! the real [`PluginHost`]; [`oc_tui::plugin_boot::TuiPluginRuntime`]
//! resolves its `ctx.ui.slot` claims against the TUI's mounted paths
//! (the ported slot router) and materializes the sidebar claim into
//! the store; the claim's row clicks through the plugin's own
//! `onMouseUp`; the `ctx.keymap.layer` binds resolve on the owned
//! keymap surface and dispatch into the guest's `run`; and the
//! router surface (`ctx.ui.router.register` + `navigate`) opens the
//! plugin route page with its materialized content.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use oc_tui::components::plugin_slots::{project_claims, resolve_slots};
use oc_tui::keymap::{KeyStroke, Keymap, ManualClock, Outcome};
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-slot-claim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../oc-plugin-host/tests/fixtures/slotclaims.ts"),
        dir.join("slotclaims.ts"),
    )
    .expect("fixture copy");
    dir
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

/// Boot the host + runtime + store with every seam attached (the
/// app's assembly — attach resolves + materializes the claims and
/// projects the owned keymap set).
fn boot() -> Option<(
    oc_tui::plugin_boot::TuiPluginRuntime,
    Store,
    PathBuf,
    Vec<oc_tui::keymap::OwnedCommandDef>,
)> {
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = Arc::new(
        oc_plugin_host::PluginHost::detect()
            .start_detached_with(config, &[sandbox.join("slotclaims.ts")]),
    );
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());
    let booted = oc_tui::plugin_boot::BootedPluginHost {
        host,
        resolution: oc_plugin_host::resolve(&[], None),
        ready: true,
    };
    let runtime = oc_tui::plugin_boot::TuiPluginRuntime::new(booted);
    let store = Store::new(Runtime::new());
    let projection = runtime.attach(&store);
    Some((runtime, store, sandbox, projection.owned))
}

/// The pure router resolves the fixture's claims onto the mounted
/// paths (sidebar + app) — the ref's structure.ts policy.
#[test]
fn router_resolves_fixture_claims() {
    let claims = project_claims(&[
        oc_plugin_host::registrations::PluginSlotClaim {
            plugin_id: "slotclaims".into(),
            key: "slot#0".into(),
            kind: "append".into(),
            target: "sidebar.content".into(),
        },
        oc_plugin_host::registrations::PluginSlotClaim {
            plugin_id: "slotclaims".into(),
            key: "slot#1".into(),
            kind: "append".into(),
            target: "app".into(),
        },
    ]);
    let resolution = resolve_slots(&oc_tui::components::plugin_slots::mounted_paths(), &claims);
    assert!(resolution.slotted.contains_key("sidebar.content"));
    assert!(resolution.slotted.contains_key("app"));
    assert!(resolution.suppressed.is_empty(), "no suppression: {:?}", resolution.suppressed);
}

/// G-04 — the footer-face paths are mounted: claims targeting
/// `home.footer(.status)`, `prompt.footer(.status/.file)` and
/// `sidebar.footer` resolve into their own buckets (no more
/// "unmounted" suppression/degradation diagnostics).
#[test]
fn footer_face_claims_resolve_onto_mounted_paths() {
    let claims = project_claims(
        &[
            "home.footer",
            "home.footer.status",
            "prompt.footer",
            "prompt.footer.status",
            "prompt.footer.file",
            "sidebar.footer",
        ]
        .iter()
        .map(|target| oc_plugin_host::registrations::PluginSlotClaim {
            plugin_id: "slotclaims".into(),
            key: "slot#0".into(),
            kind: "append".into(),
            target: (*target).to_owned(),
        })
        .collect::<Vec<_>>(),
    );
    let resolution = resolve_slots(&oc_tui::components::plugin_slots::mounted_paths(), &claims);
    for path in [
        "home.footer",
        "home.footer.status",
        "prompt.footer",
        "prompt.footer.status",
        "prompt.footer.file",
        "sidebar.footer",
    ] {
        assert!(resolution.slotted.contains_key(path), "{path} slotted");
    }
    assert!(resolution.suppressed.is_empty(), "no suppression: {:?}", resolution.suppressed);
    assert!(resolution.degraded.is_empty(), "no degradation: {:?}", resolution.degraded);
    // `session.panel` stays unmounted (interactive API — documented
    // exemption): its claims keep the pre-G-04 degradation.
    let panel = project_claims(&[oc_plugin_host::registrations::PluginSlotClaim {
        plugin_id: "slotclaims".into(),
        key: "slot#9".into(),
        kind: "append".into(),
        target: "session.panel".into(),
    }]);
    let panel = resolve_slots(&oc_tui::components::plugin_slots::mounted_paths(), &panel);
    assert!(panel.slotted.is_empty());
    assert_eq!(panel.suppressed.len(), 1, "session.panel stays unmounted");
}

#[test]
#[allow(clippy::too_many_lines)] // one phase per seam
fn slot_renders_and_clicks() {
    let Some((runtime, store, sandbox, owned)) = boot() else { return };
    let host = &runtime.booted.host;

    // -- the sidebar claim materialized into the store -----------------
    let sections = store.plugin_sidebar_sections();
    let section = sections
        .iter()
        .find(|section| section.plugin_id == "slotclaims")
        .expect("the claim's section landed");
    assert_eq!(section.title, "Claims");
    assert_eq!(section.rows, ["target: board".to_owned(), "plain row".to_owned()]);
    assert_eq!(section.clickable, vec![true, false]);

    // -- G-04: the footer faces materialized onto their surfaces ------
    assert_eq!(
        store.plugin_slot_rows("prompt.footer"),
        ["prompt footer: normal".to_owned()],
        "prompt.footer rendered with the PromptFooterInput props"
    );
    assert_eq!(store.plugin_slot_rows("home.footer"), ["home footer row".to_owned()]);
    let sidebar_footer = store.plugin_slot_sections("sidebar.footer");
    assert_eq!(sidebar_footer.len(), 1);
    assert_eq!(sidebar_footer[0].title, "Side footer");
    assert_eq!(sidebar_footer[0].rows, ["sidebar footer row".to_owned()]);

    // The LIVE route renders it in the docked sidebar pane (the
    // plugin_slots.rs proof shape: docked at 160 cols, the section
    // sits under the session title).
    let theme = oc_tui::theme::ThemeState::v2_dark();
    store.open_tab("ses_claims", Some("Claims session".to_owned()));
    store.navigate(oc_tui::store::Route::Session { session_id: "ses_claims".into() });
    let frame = oc_tui::components::session::render_session(
        &store,
        "ses_claims",
        &theme.styles(),
        (160, 30),
    );
    let grid = frame.text_grid();
    let sidebar_pane: Vec<String> =
        grid.iter().map(|row| row.chars().skip(118).collect()).collect();
    assert!(sidebar_pane[3].contains("Claims"), "the claim section title: {:?}", sidebar_pane[3]);
    assert!(sidebar_pane[4].contains("target: board"), "the clickable row: {:?}", sidebar_pane[4]);
    assert!(sidebar_pane[5].contains("plain row"), "the plain row: {:?}", sidebar_pane[5]);
    // -- the click runs the plugin's onMouseUp --------------------------
    // (the click sink's claim-keyed path — the section's row 0).
    assert!(
        host.click_slot_claim("slotclaims", "slot#0", 0, Duration::from_secs(5)).expect("click"),
        "the claim click dispatches"
    );
    assert!(
        wait_for(Duration::from_secs(5), || sandbox.join("claim-click.txt").exists()),
        "the plugin's onMouseUp ran"
    );

    // -- the keymap layer resolves on the owned surface -----------------
    let snapshot = host.registrations();
    assert!(
        snapshot.keymap_commands.iter().any(|command| command.id == "claims.open"),
        "the registration-time layer landed"
    );
    let mut keymap = Keymap::new();
    keymap.push_mode("base");
    for def in &owned {
        keymap.register_owned(def.clone());
    }
    let clock = ManualClock::default();
    keymap.lookup(KeyStroke::parse("ctrl+x").unwrap(), &clock);
    match keymap.lookup(KeyStroke::parse("o").unwrap(), &clock) {
        Outcome::Command { command, .. } => assert_eq!(command, "slotclaims::claims.open"),
        other => panic!("the layer bind resolves the plugin command: {other:?}"),
    }
    // The dynamic layer (installed by the app claim's render) rides
    // the same surface.
    keymap.lookup(KeyStroke::parse("ctrl+x").unwrap(), &clock);
    match keymap.lookup(KeyStroke::parse("p").unwrap(), &clock) {
        Outcome::Command { command, .. } => assert_eq!(command, "slotclaims::claims.ping"),
        other => panic!("the dynamic layer bind resolves: {other:?}"),
    }

    // -- the layer command's run dispatches into the guest --------------
    assert!(
        host.run_keymap_command("slotclaims", "claims.open", Duration::from_secs(5))
            .expect("run dispatches"),
        "the run executed"
    );
    assert!(
        wait_for(Duration::from_secs(5), || sandbox.join("open-run.txt").exists()),
        "the run callback executed guest-side"
    );

    // -- the router surface: the navigate ask opens the plugin page -----
    for _ in 0..50 {
        let before = store.plugin_route_page().is_some();
        runtime.drain_ui_requests(&store);
        if before && store.plugin_route_page().is_some_and(|page| !page.rows.is_empty()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let page = store.plugin_route_page().expect("the plugin route opened");
    assert_eq!(page.plugin_id, "slotclaims");
    assert_eq!(page.name, "board");
    assert_eq!(page.rows, ["story: alpha".to_owned()], "the materialized content landed");

    // The page paints over the app (App::frame's overlay seam — the
    // pure page renderer here).
    let overlay = oc_tui::components::plugin_page_solid::render_plugin_page_solid(
        &page.title,
        &page.rows,
        &theme.styles(),
        (80, 24),
    );
    let grid = overlay.text_grid();
    assert!(grid[1].contains("Board"));
    assert!(grid[3].contains("story: alpha"));

    // A session navigation closes the page (the router replaces the
    // visible route).
    store.navigate(oc_tui::store::Route::Home);
    assert!(store.plugin_route_page().is_none(), "the plugin page closed");

    runtime.shutdown();
}
