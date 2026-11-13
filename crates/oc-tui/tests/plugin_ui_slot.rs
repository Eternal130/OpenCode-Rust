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

/// G-239/G-331 — the panelclaim fixture's sandbox twin.
fn sandbox_panel() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-panel-claim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../oc-plugin-host/tests/fixtures/panelclaim.ts"),
        dir.join("panelclaim.ts"),
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

/// G-239/G-331 — the panel boot twin (the panelclaim fixture through
/// the same harness).
fn boot_panel() -> Option<(
    oc_tui::plugin_boot::TuiPluginRuntime,
    Store,
    PathBuf,
    Vec<oc_tui::keymap::OwnedCommandDef>,
)> {
    let sandbox = sandbox_panel();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = Arc::new(
        oc_plugin_host::PluginHost::detect()
            .start_detached_with(config, &[sandbox.join("panelclaim.ts")]),
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

/// Pump helper: drain UI requests until `probe` holds (the deferred
/// open's resolution + the re-materialize both ride the pump).
fn pump_until(
    runtime: &oc_tui::plugin_boot::TuiPluginRuntime,
    store: &Store,
    timeout: Duration,
    mut probe: impl FnMut() -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        runtime.drain_ui_requests(store);
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
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
    // G-239/G-331 — `session.panel` is MOUNTED (the interactive
    // PanelInput API's native surface): its claims resolve into
    // `slotted` like every other mounted path.
    let panel = project_claims(&[oc_plugin_host::registrations::PluginSlotClaim {
        plugin_id: "slotclaims".into(),
        key: "slot#9".into(),
        kind: "append".into(),
        target: "session.panel".into(),
    }]);
    let panel = resolve_slots(&oc_tui::components::plugin_slots::mounted_paths(), &panel);
    assert!(panel.slotted.contains_key("session.panel"), "session.panel slotted");
    assert!(panel.suppressed.is_empty() && panel.degraded.is_empty());
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
    store.navigate(oc_tui::store::Route::session("ses_claims"));
    let frame = oc_tui::components::session::render_session_composed(
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
    let overlay = oc_tui::components::plugin_page_solid::plugin_page_solid_frame(
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

/// G-239/G-331 — the session.panel activation: the deferred open
/// resolves the route-check boolean, the mounted claim materializes
/// with its `PanelInput` props, the pane renders as the right leg, the
/// fullscreen takeover covers a narrow frame, and the owner-scoped
/// close tears it down.
#[test]
#[allow(clippy::too_many_lines)] // one phase per seam
fn panel_open_materializes_and_closes() {
    let Some((runtime, store, sandbox, owned)) = boot_panel() else { return };
    let host = Arc::clone(&runtime.booted.host);
    let theme = oc_tui::theme::ThemeState::v2_dark();

    // The open requires a session route (the route-check boolean).
    store.open_tab("ses_panel", Some("Panel session".to_owned()));
    store.navigate(oc_tui::store::Route::session("ses_panel"));
    // The app loop pushes the live size each frame (the props'
    // presentation input derives from it).
    store.set_terminal_size((160, 30));
    assert!(
        wait_for(Duration::from_secs(10), || sandbox.join("setup-done.txt").exists()),
        "the fixture setup ran"
    );
    runtime.drain_ui_requests(&store);

    // The layer bind is reachable on the owned surface
    // (`panelclaim::panel.open`).
    let snapshot = host.registrations();
    assert!(
        snapshot.keymap_commands.iter().any(|command| command.id == "panel.open"),
        "the panel layer landed"
    );
    assert!(!owned.is_empty(), "owned keymap entries landed");

    let runner = std::thread::spawn(move || {
        host.run_keymap_command("panelclaim", "panel.open", Duration::from_secs(10))
    });
    assert!(
        pump_until(&runtime, &store, Duration::from_secs(10), || {
            store.session_panel().is_some()
        }),
        "the open landed in the store"
    );
    assert!(runner.join().expect("run thread").expect("run dispatches"));
    assert!(
        wait_for(Duration::from_secs(5), || {
            std::fs::read_to_string(sandbox.join("panel-open.txt"))
                .is_ok_and(|text| text.trim() == "true")
        }),
        "the deferred open resolved true (on a session route)"
    );

    // The claim re-materialized with the PanelInput props (the pump's
    // panel fingerprint fired the bounded render).
    assert!(
        pump_until(&runtime, &store, Duration::from_secs(10), || {
            store
                .plugin_slot_sections("session.panel")
                .iter()
                .any(|section| section.rows.iter().any(|row| row.contains("name: board")))
        }),
        "the panel claim materialized"
    );
    let section = store
        .plugin_slot_sections("session.panel")
        .into_iter()
        .find(|section| section.plugin_id == "panelclaim")
        .expect("the panel section");
    assert!(section.rows.iter().any(|row| row.contains("focus: yes")));
    assert!(section.rows.iter().any(|row| row.contains("presentation: panel")));

    // (a) DOCKED at 160: the pane is the right leg (the equal split —
    // 80 cols) and the rows paint inside it.
    let frame = oc_tui::components::session::render_session_composed(
        &store,
        "ses_panel",
        &theme.styles(),
        (160, 30),
    );
    let grid = frame.text_grid();
    assert!(grid.iter().any(|row| row.contains("name: board")), "the claim row rendered: {grid:?}");
    assert!(store.panel_rect().is_some(), "the focus rect recorded");
    let rect = store.panel_rect().expect("rect");
    assert!(rect.x >= 78 && rect.x <= 82, "the docked pane sits at the split: {rect:?}");
    assert_eq!(store.panel_width(), 80, "the props' width input follows the pane");

    // The clickable row's hit rect rides the generic click sink.
    let hits = store.plugin_section_hits();
    assert_eq!(hits.rows.len(), 1, "the focus row is clickable");
    assert_eq!(hits.rows[0].plugin_id, "panelclaim");

    // (b) NARROW at 80: the fullscreen takeover (canSplit = width >
    // 80 — the session column hides, not an overlay).
    let frame = oc_tui::components::session::render_session_composed(
        &store,
        "ses_panel",
        &theme.styles(),
        (80, 30),
    );
    let grid = frame.text_grid();
    let panel_row = grid.iter().find(|row| row.contains("presentation: ")).expect("the props row");
    let col = panel_row.find("presentation: ").expect("col");
    assert!(col < 20, "fullscreen paints from the frame's left edge: {panel_row}");
    let rect = store.panel_rect().expect("rect");
    assert_eq!(rect.x, 0, "the fullscreen pane covers the frame");
    assert_eq!(rect.width, 80);

    // (c) The owner-scoped close (`ctx.ui.panel.close()`): current()
    // read the host-pushed view, then the panel dropped.
    let host = Arc::clone(&runtime.booted.host);
    let runner = std::thread::spawn(move || {
        host.run_keymap_command("panelclaim", "panel.close", Duration::from_secs(10))
    });
    assert!(
        pump_until(&runtime, &store, Duration::from_secs(10), || {
            store.session_panel().is_none()
        }),
        "the owner-scoped close landed"
    );
    assert!(runner.join().expect("run thread").expect("close dispatches"));
    assert!(
        wait_for(Duration::from_secs(5), || {
            std::fs::read_to_string(sandbox.join("panel-current.txt"))
                .is_ok_and(|text| text.contains("board") && text.contains("ses_panel"))
        }),
        "current() is plugin-filtered and session-carrying"
    );
    // The pane unmounted: the frame reflows and the rect clears.
    let frame = oc_tui::components::session::render_session_composed(
        &store,
        "ses_panel",
        &theme.styles(),
        (160, 30),
    );
    let grid = frame.text_grid();
    assert!(!grid.iter().any(|row| row.contains("name: board")));
    assert!(store.panel_rect().is_none());

    runtime.shutdown();
}
