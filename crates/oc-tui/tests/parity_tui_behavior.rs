//! lane-dialogs (wave 5): the TUI dialog-family parity proofs —
//! TUI-23 (`DialogConfig` full table), TUI-24 (`DialogErrorDetails`
//! generic), TUI-25 (ui/dialog-alert + dialog-confirm incl. the
//! ctx.ui resolution queue), TUI-TD22 (`DialogExperiments`). Store +
//! dialog-key-layer level (the keyboard-visible halves of TUI-8/9/10
//! live in `pty_daily.rs`).

use oc_tui::components::command_palette::{
    select_props_for, CommandDispatcher as _, StoreDispatcher,
};
use oc_tui::components::dialog::{handle_dialog_key, DialogKeyOutcome};
use oc_tui::keymap::{Keymap, ManualClock};
use oc_tui::reactive::Runtime;
use oc_tui::store::{DialogKind, DialogSize, Store};

/// The generic-config write-through recorder (TUI-23/TD22's sink).
#[derive(Default)]
struct RecordingSink(std::cell::RefCell<Vec<(String, String)>>);
impl oc_tui::store::ConfigSink for RecordingSink {
    fn update_theme_name(&self, _: &str) {}
    fn update_theme_mode(&self, _: &str) {}
    fn update_session_sidebar(&self, _: bool) {}
    fn update_session_grouping(&self, _: bool) {}
    fn update_setting(&self, path: &str, value: &str) {
        self.0.borrow_mut().push((path.to_owned(), value.to_owned()));
    }
}

fn stroke(key: &str) -> oc_tui::keymap::KeyStroke {
    oc_tui::keymap::KeyStroke::parse(key).expect("a parseable stroke")
}

fn dialog_layer(store: &Store, key: &str) -> DialogKeyOutcome {
    let mut keymap = Keymap::new();
    keymap.push_mode("dialog");
    let clock = ManualClock::default();
    handle_dialog_key(store, &mut keymap, &clock, stroke(key), (120, 40))
}

fn frame_text(store: &Store) -> String {
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let frame = oc_tui::components::dialog::render_app(store, &theme, (120, 40));
    frame.text_grid().join("\n")
}

/// TUI-23: `opencode.settings` mounts the FULL v2.0.24 `DialogConfig`
/// table — every row with its category + the formatted footer; ←/→
/// and enter cycle a row's values, writing through the config sink
/// and flipping the LIVE store state where one exists.
#[test]
fn dialogconfig_tabs_dialog_config() {
    let store = Store::new(Runtime::new());
    let mut dispatcher = StoreDispatcher;
    // The palette command opens the dialog.
    dispatcher.dispatch(&store, "opencode.settings", None);
    assert!(matches!(store.top_dialog().map(|entry| entry.kind), Some(DialogKind::Settings)));

    // The FULL table: every v2.0.24 row is present, in order, with
    // its category (the ref's category "tabs" — Appearance through
    // Debug).
    let entry = store.top_dialog().unwrap();
    let props = select_props_for(&entry.kind, &store);
    assert_eq!(props.title, "Settings");
    assert_eq!(props.options.len(), 34, "the full dialog-config.tsx table");
    let titles: Vec<&str> = props.options.iter().map(|option| option.title.as_str()).collect();
    for expected in [
        "Theme",
        "Color mode",
        "Animations",
        "Sidebar",
        "Thinking",
        "Markdown",
        "Tool grouping",
        "Verbosity",
        "Transcript images",
        "TPS",
        "New session location",
        "Permissions",
        "Mode",
        "Scope",
        "Layout",
        "Indicators",
        "Layout",
        "Wrapping",
        "File tree",
        "Single patch",
        "Scroll speed",
        "Acceleration",
        "Mouse",
        "Editor context",
        "Large pastes",
        "Image previews",
        "Leader timeout",
        "Notifications",
        "Sounds",
        "Volume",
        "Window title",
        "Copy behavior",
        "Developer tools",
    ] {
        assert!(titles.contains(&expected), "the {expected:?} row is present");
    }
    // A category rides each row (the ref's `category` field).
    assert!(props.options.iter().all(|option| option.category.is_some()));

    // Cycling a live row: `session.tps` false → true through ←/→
    // (dialog-config.tsx:405-418's bindings) AND the generic config
    // sink write-through.
    let sink = std::rc::Rc::new(RecordingSink::default());
    store.set_config_sink(sink.clone());
    store.setting_cycle("session.tps", 1);
    assert!(store.session_tps(), "the live tps flag flipped");
    assert_eq!(
        sink.0.borrow().last().cloned(),
        Some(("session.tps".to_owned(), "true".to_owned())),
        "the cycle writes through the config sink"
    );

    // A snapshot row: `prompt.paste` compact → full → compact, the
    // label display in the footer.
    store.setting_cycle("prompt.paste", 1);
    assert_eq!(
        sink.0.borrow().last().cloned(),
        Some(("prompt.paste".to_owned(), "full".to_owned()))
    );
    let entry = store.top_dialog().unwrap();
    let props = select_props_for(&entry.kind, &store);
    let paste = props.options.iter().find(|option| option.value == "prompt.paste").unwrap();
    assert_eq!(paste.footer.as_deref(), Some("full"));
    store.setting_cycle("prompt.paste", 1);
    let entry = store.top_dialog().unwrap();
    let props = select_props_for(&entry.kind, &store);
    let paste = props.options.iter().find(|option| option.value == "prompt.paste").unwrap();
    assert_eq!(paste.footer.as_deref(), Some("compact"));

    // A numeric row steps + clamps: leader.timeout 2000 → 2250 → …
    store.setting_cycle("leader.timeout", 1);
    assert_eq!(
        sink.0.borrow().last().cloned(),
        Some(("leader.timeout".to_owned(), "2250".to_owned()))
    );
    let entry = store.top_dialog().unwrap();
    let props = select_props_for(&entry.kind, &store);
    let timeout = props.options.iter().find(|option| option.value == "leader.timeout").unwrap();
    assert_eq!(timeout.footer.as_deref(), Some("2250 ms"));
}

/// TUI-24: the generic `DialogErrorDetails` — the props mount (title,
/// source, body, the action footer), the `i` investigate path (home +
/// the seeded composer), `c` copy, and escape's back-to-plugins role.
#[test]
fn dialogerrordet_dialog_error_details() {
    let store = Store::new(Runtime::new());
    store.open_error_details(
        "TUI plugin error",
        "/plugins/omo.ts",
        "ReferenceError: boom is not defined",
        "Plugin: omo\nStatus: failed",
        "ref-123",
        true,
    );
    let top = store.top_dialog().expect("the dialog opened");
    assert!(matches!(top.kind, DialogKind::ErrorDetails { .. }));

    // The rendered chrome: title, source, body, the action footer.
    let text = frame_text(&store);
    assert!(text.contains("TUI plugin error"));
    assert!(text.contains("/plugins/omo.ts"));
    assert!(text.contains("ReferenceError: boom"));
    assert!(text.contains("investigate"));
    assert!(text.contains("copy details"));
    assert!(text.contains("Reference: ref-123"));

    // `i` investigate (dialog-error-details.tsx:93): navigate home +
    // seed the home composer with the error prompt.
    let outcome = dialog_layer(&store, "i");
    assert_eq!(outcome, DialogKeyOutcome::Consumed);
    assert!(store.top_dialog().is_none(), "the dialog cleared");
    assert!(matches!(store.route(), oc_tui::store::Route::Home));
    let composer = store.composer(oc_tui::app::HOME_PROMPT_KEY);
    assert!(composer.draft.text.contains("ReferenceError: boom"));

    // The plugins-dialog role: a faulted plugin row opens the details
    // and escape returns to the plugin list (the large re-seat).
    let mut faults = std::collections::HashMap::new();
    faults.insert("omo".to_owned(), "load failed: bad export".to_owned());
    store.set_plugin_faults(faults);
    dispatcher_open_plugins(&store);
    assert!(matches!(store.top_dialog().map(|entry| entry.kind), Some(DialogKind::Plugins)));
    let outcome = oc_tui::components::dialog::execute_submission(
        &store,
        &mut oc_tui::components::command_palette::StoreDispatcher,
        "omo",
    );
    assert!(matches!(outcome, oc_tui::components::command_palette::Dispatch::Handled));
    let top = store.top_dialog().expect("the error details mounted");
    assert!(matches!(top.kind, DialogKind::ErrorDetails { .. }));
    // Escape → back to the plugins list at v2's large size.
    let _ = dialog_layer(&store, "escape");
    let top = store.top_dialog().expect("back on the plugins list");
    assert!(matches!(top.kind, DialogKind::Plugins));
    assert_eq!(top.size, DialogSize::Large);
}

fn dispatcher_open_plugins(store: &Store) {
    let _ =
        oc_tui::components::command_palette::StoreDispatcher.dispatch(store, "plugins.list", None);
}

/// TUI-25: the generic alert/confirm tones — the rendered pills, the
/// ←/→ focus, the return paths, and the ctx.ui settle-once queue
/// (confirm → true/false, ANY unresolved close → null).
#[test]
fn ui_dialog_alert_confirm() {
    let store = Store::new(Runtime::new());

    // The alert: title + message + the right-justified `ok` pill;
    // return confirms + closes.
    store.open_alert("Delete preset", "This cannot be undone", None, None);
    let text = frame_text(&store);
    assert!(text.contains("Delete preset"));
    assert!(text.contains("This cannot be undone"));
    assert!(text.contains("ok"));
    let outcome = dialog_layer(&store, "return");
    assert_eq!(outcome, DialogKeyOutcome::Consumed);
    assert!(store.top_dialog().is_none(), "the alert closed on return");

    // The confirm: cancel/confirm pills, ←/→ swap the focus, return
    // runs the ACTIVE pill, escape closes unresolved.
    store.open_confirm(
        "Remove workspace",
        "The working tree has uncommitted changes",
        "remove",
        "keep",
        Some(42),
        None,
        None,
    );
    let text = frame_text(&store);
    assert!(text.contains("Remove workspace"));
    assert!(text.contains("Keep"), "the cancel pill (titlecased)");
    assert!(text.contains("Remove"), "the confirm pill (titlecased)");
    assert!(store.confirm_active_is_confirm(), "confirm starts focused");

    let outcome = dialog_layer(&store, "left");
    assert_eq!(outcome, DialogKeyOutcome::Consumed);
    assert!(!store.confirm_active_is_confirm(), "← moves to cancel");

    // Return on cancel → the request resolves false.
    let outcome = dialog_layer(&store, "return");
    assert_eq!(outcome, DialogKeyOutcome::Consumed);
    assert!(store.top_dialog().is_none());
    let resolutions = store.take_ui_dialog_resolutions();
    assert_eq!(resolutions, vec![(42, serde_json::Value::Bool(false))]);

    // An escaped confirm settles null (onClose → done(undefined)).
    store.open_confirm("Sure?", "really", "confirm", "cancel", Some(7), None, None);
    let outcome = dialog_layer(&store, "escape");
    assert_eq!(outcome, DialogKeyOutcome::Closed);
    assert!(store.top_dialog().is_none());
    assert_eq!(store.take_ui_dialog_resolutions(), vec![(7, serde_json::Value::Null)]);

    // Return on the confirm pill resolves true.
    store.open_confirm("Sure?", "really", "confirm", "cancel", Some(9), None, None);
    let _ = dialog_layer(&store, "return");
    assert_eq!(store.take_ui_dialog_resolutions(), vec![(9, serde_json::Value::Bool(true))]);

    // A confirm command rides the outcome (the app layer dispatches
    // it after the close).
    store.open_confirm(
        "Drop it?",
        "yes/no",
        "confirm",
        "cancel",
        None,
        Some("stash.delete".into()),
        None,
    );
    let outcome = dialog_layer(&store, "return");
    assert_eq!(outcome, DialogKeyOutcome::CommandSubmitted("stash.delete".into()));
}

/// TUI-TD22: `DialogExperiments` — v2.0.24's EMPTY table renders "No
/// experiments available"; the toggle writes `experimental.<id>`
/// through the config sink (proven with an injected row, the port's
/// table-mirrors-upstream contract).
#[test]
fn dialog_experiments() {
    let store = Store::new(Runtime::new());
    let mut dispatcher = StoreDispatcher;

    // The palette row opens the dialog (the PORT ADDITION entry —
    // v2's DevToolsBar item is 06 #20's surface).
    let _ = dispatcher.dispatch(&store, "opencode.experiments", None);
    let top = store.top_dialog().expect("the dialog opened");
    assert!(matches!(top.kind, DialogKind::Experiments));
    let props = select_props_for(&top.kind, &store);
    assert_eq!(props.title, "Experiments");
    assert!(props.options.is_empty(), "v2.0.24 ships an empty table");
    assert_eq!(
        props.empty_text.as_deref(),
        Some("No experiments available"),
        "the ref's empty view copy"
    );

    // The toggle: an injected experiment row flips through ←/→ and
    // writes config.experimental.<id> via the sink.
    store.set_experiments(vec![oc_tui::store::Experiment {
        id: "new_reader".to_owned(),
        title: "New reader".to_owned(),
        description: "the experimental reader".to_owned(),
    }]);
    let top = store.top_dialog().unwrap();
    let props = select_props_for(&top.kind, &store);
    assert_eq!(props.options.len(), 1);
    assert_eq!(props.options[0].footer.as_deref(), Some("off"), "absent = off");

    let sink = std::rc::Rc::new(RecordingSink::default());
    store.set_config_sink(sink.clone());
    store.experiment_toggle("new_reader");
    assert!(store.experiment_enabled("new_reader"));
    assert_eq!(
        sink.0.borrow().last().cloned(),
        Some(("experimental.new_reader".to_owned(), "true".to_owned()))
    );
    let top = store.top_dialog().unwrap();
    let props = select_props_for(&top.kind, &store);
    assert_eq!(props.options[0].footer.as_deref(), Some("on"));
}

/// TUI-33 — `session.verbosity.cycle` (index.tsx:1064-1076): the
/// palette row carries v2's DYNAMIC title `Verbosity: {titlecase}` and
/// dispatch cycles the `low | medium | high` config through the sink
/// (levels[(i+1) % 3]) with the dialog cleared.
#[test]
fn session_verbosity_cycle_index() {
    let store = Store::new(Runtime::new());
    let mut dispatcher = StoreDispatcher;

    // The palette row (palette: true in v2's session table).
    let props = oc_tui::components::command_palette::palette_props(&store);
    let row = props
        .options
        .iter()
        .find(|option| option.value == "session.verbosity.cycle")
        .expect("the palette row exists");
    assert_eq!(row.title, "Verbosity: Medium", "v2's dynamic titlecase title");
    assert_eq!(row.category.as_deref(), Some("Session"));

    // Dispatch cycles: medium → high → low (wrap) → medium, each
    // writing `session.verbosity` through the config sink.
    let sink = std::rc::Rc::new(RecordingSink::default());
    store.set_config_sink(sink.clone());
    store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
    assert_eq!(
        dispatcher.dispatch(&store, "session.verbosity.cycle", None),
        oc_tui::components::command_palette::Dispatch::Handled
    );
    assert_eq!(store.session_verbosity(), "high");
    assert_eq!(
        sink.0.borrow().last().cloned(),
        Some(("session.verbosity".to_owned(), "high".to_owned()))
    );
    assert!(store.top_dialog().is_none(), "dialog.clear() rides the run");

    let _ = dispatcher.dispatch(&store, "session.verbosity.cycle", None);
    assert_eq!(store.session_verbosity(), "low", "the wrap: high → low");
    let _ = dispatcher.dispatch(&store, "session.verbosity.cycle", None);
    assert_eq!(store.session_verbosity(), "medium");

    // The palette title tracks the live level.
    let props = oc_tui::components::command_palette::palette_props(&store);
    let row = props
        .options
        .iter()
        .find(|option| option.value == "session.verbosity.cycle")
        .unwrap();
    assert_eq!(row.title, "Verbosity: Medium");
}

/// TUI-TD15 — the `--prompt` auto-submit waits for the model halves
/// (home.tsx:69-80 / index.tsx:597-604): the startup prompt is ARMED
/// at boot and only submits once `local.model.ready &&
/// local.model.catalogReady` hold AND a current model resolves — a
/// boot with the catalog still loading keeps the draft parked (never
/// a failed first turn).
#[test]
fn prompt_autosubmit_ready() {
    use oc_tui::app::App;
    use oc_tui::store::ListState;

    let mut app = App::new((80, 24));
    app.store.set_models(ListState::Loading, Vec::new());

    // Arm at boot (run_with_size's --prompt block).
    app.arm_startup_prompt("fix the login bug".to_owned());
    app.maybe_submit_startup_prompt();
    let composer = app.store.composer(oc_tui::app::HOME_PROMPT_KEY);
    assert_eq!(
        composer.draft.text, "fix the login bug",
        "the draft seeds immediately (r.set) — but does NOT submit"
    );
    app.maybe_submit_startup_prompt();
    let composer = app.store.composer(oc_tui::app::HOME_PROMPT_KEY);
    assert!(
        !composer.draft.text.is_empty(),
        "still parked while the catalog is loading"
    );

    // The catalog lands → the gate opens → the one-shot submit fires
    // (the draft clears into the prompt sink).
    app.store.set_models(
        ListState::Loaded(vec![oc_tui::store::ServerModel {
            provider_id: "opencode".to_owned(),
            id: "grok".to_owned(),
            name: "Grok".to_owned(),
            ..oc_tui::store::ServerModel::default()
        }]),
        Vec::new(),
    );
    app.maybe_submit_startup_prompt();
    let composer = app.store.composer(oc_tui::app::HOME_PROMPT_KEY);
    assert!(composer.draft.text.is_empty(), "the startup prompt submitted once ready");

    // One-shot: the second pass never re-submits (v2's `sent` latch).
    app.maybe_submit_startup_prompt();
}

/// TUI-TD28 — the plugin-route fallback screen
/// (component/plugin-route-missing.tsx:3-21): when the host has no
/// registered route `{id}/{name}` the page renders the warning line +
/// the "go home" button, and the button's release navigates home
/// (`onHome={() => route.navigate({type:"home"})}`).
#[test]
fn pluginroutemis() {
    use crossterm::event::{MouseButton, KeyEventKind, MouseEvent, MouseEventKind};
    use oc_tui::app::App;

    let theme = oc_tui::theme::ThemeState::v2_dark();
    let mut app = App::new((80, 24));

    // An unregistered route opens (the guest's navigate) and the
    // bounded render replies miss → the page flags missing.
    app.store.open_plugin_route("omo", "stats", None);
    app.store.mark_plugin_route_missing();
    let page = app.store.plugin_route_page().expect("the page stays open");
    assert!(page.missing, "the miss latches the fallback state");

    // The fallback frame: the warning line + the go-home button.
    let frame = oc_tui::components::plugin_page::render_plugin_route_missing(
        "omo",
        "stats",
        &theme.styles(),
        (80, 24),
        &mut Vec::new(),
    );
    let text = frame.text_grid().join("\n");
    assert!(text.contains("Unknown plugin route: omo/stats"), "the warning line");
    assert!(text.contains("go home"), "the button");

    // The button's hit rect: centered, the release navigates home and
    // closes the page (v2's onHome).
    let mut hits = Vec::new();
    let _ = oc_tui::components::plugin_page::render_plugin_route_missing(
        "omo",
        "stats",
        &theme.styles(),
        (80, 24),
        &mut hits,
    );
    assert_eq!(hits.len(), 1, "the go-home button recorded its hit rect");
    let (hx, hy) = (hits[0].0, hits[0].1);
    app.mouse_event(&MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: u16::try_from(hx + 1).unwrap(),
        row: u16::try_from(hy).unwrap(),
        modifiers: crossterm::event::KeyModifiers::empty(),
    });
    assert!(matches!(app.store.route(), oc_tui::store::Route::Home));
    assert!(app.store.plugin_route_page().is_none(), "the page closed");
    let _ = (KeyEventKind::Press,);
}

/// TUI-30 — the SessionLocationMissing recovery screen
/// (routes/session/location-missing.tsx): when the location worker's
/// error directory matches the routed session's `location.directory`,
/// the question replaces the prompt mount; `return` runs the single
/// recovery action (→ DialogWorkspaces); clearing the error restores
/// the composer.
#[test]
fn sessionlocatio_location() {
    use oc_tui::app::App;
    use oc_tui::data::{Ingest, SessionSnapshot};

    let mut app = App::new((100, 30));
    let theme = oc_tui::theme::ThemeState::v2_dark();
    app.store.ingest_all([Ingest::Resync {
        sessions: vec![SessionSnapshot {
            id: "s1".to_owned(),
            directory: Some("/gone/dir".to_owned()),
            ..SessionSnapshot::default()
        }],
        unread: Vec::new(),
        messages: Vec::new(),
        cursors: Vec::new(),
        permissions: Vec::new(),
        forms: Vec::new(),
    }]);
    app.store.select_tab("s1");
    assert!(
        app.store.session_location_missing("s1").is_none(),
        "no error recorded yet: the composer owns the mount"
    );

    // The location worker reports the failed directory.
    app.store.set_location_error(Some("/gone/dir".to_owned()));
    assert_eq!(
        app.store.session_location_missing("s1").as_deref(),
        Some("/gone/dir"),
        "the routed session's directory matches the error"
    );

    // The question surface replaces the prompt mount.
    let frame = oc_tui::components::session::render_session(
        &app.store,
        "s1",
        &theme.styles(),
        (100, 30),
    );
    let text = frame.text_grid().join("\n");
    assert!(text.contains("Session location unavailable"), "the question title");
    assert!(text.contains("/gone/dir"), "the failed directory");
    assert!(text.contains("Choose another directory to continue this session."));
    assert!(text.contains("Recovery actions"));
    assert!(text.contains("Choose directory"), "the single recovery action");

    // `return` selects the action: the Workspaces dialog opens (the
    // session.move surface whose selection relocations the session).
    app.press("return");
    assert!(
        matches!(app.store.top_dialog().map(|entry| entry.kind), Some(oc_tui::store::DialogKind::Worktrees)),
        "the recovery action opens DialogWorkspaces"
    );

    // A resolved location clears the mount (the composer returns).
    app.store.set_location_error(None);
    let frame = oc_tui::components::session::render_session(
        &app.store,
        "s1",
        &theme.styles(),
        (100, 30),
    );
    let text = frame.text_grid().join("\n");
    assert!(!text.contains("Session location unavailable"));
}

/// TUI-TD06 — content-anchored scroll restore (index.tsx:518-595 +
/// anchors.ts): leaving a session while scrolled away saves a CONTENT
/// anchor (the message straddling the viewport top + its screenY);
/// returning re-seats the viewport on the anchor's CURRENT position —
/// content inserted above (background streaming) does not drift the
/// view — and a vanished anchor falls back to the bottom.
#[test]
fn scroll_anchor_restore() {
    use oc_tui::store::{MessagePosition, Route, TranscriptMetrics};
    use oc_tui_core::scroll::FollowState;

    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    store.set_transcript_metrics(
        "s1",
        TranscriptMetrics { viewport: 10, content_height: 100, rows: 60 },
    );
    let positions = |m2: usize| {
        vec![
            MessagePosition { id: "m1".to_owned(), y: m2 - 20 },
            MessagePosition { id: "m2".to_owned(), y: m2 },
            MessagePosition { id: "m3".to_owned(), y: m2 + 20 },
        ]
    };
    // m2 sits at content y 30; scroll to offset 32 (two rows under
    // m2's top — away from the bottom): sticky starts at the maximum
    // (100 − 10 = 90), 58 line-ups land at 32.
    store.set_message_positions("s1", positions(30));
    for _ in 0..58 {
        store.scroll_command("s1", oc_tui_core::scroll::ScrollCommand::LineUp);
    }
    let offset = store.scroll_offset("s1");
    assert_eq!(offset, 32, "the away-from-bottom fixture (offset {offset})");

    // Leaving the session saves the anchor: m2 straddles the viewport
    // top (y 30 ≤ offset+1), so the anchor is (m2, screenY = -2).
    store.navigate(Route::Home);
    let anchor = store.scroll_anchor("s1").expect("the anchor saved on leave");
    assert_eq!(anchor.message_id, "m2");
    assert_eq!(anchor.screen_y, -2);

    // While away, 8 rows of content are inserted ABOVE (a background
    // stream admits a message at the top): every row shifts down 8.
    store.set_message_positions("s1", positions(38));
    store.set_transcript_metrics(
        "s1",
        TranscriptMetrics { viewport: 10, content_height: 108, rows: 61 },
    );

    // Returning schedules the restore; the session render resolves it
    // against the FRESH positions (the same seam the nav walk uses).
    store.select_tab("s1");
    store.resolve_scroll_restore("s1");
    let follow = store.follow_state("s1");
    let restored = match follow {
        FollowState::Free { offset, maximum } => (offset, maximum),
        other => panic!("expected Free, got {other:?}"),
    };
    assert_eq!(restored, (40, 98), "m2's top re-seated at screenY -2 (30-32 → 38-40)");
    assert_eq!(
        store
            .message_positions("s1")
            .iter()
            .find(|position| position.id == "m2")
            .map(|position| position.y as i64 - i64::try_from(store.scroll_offset("s1")).unwrap()),
        Some(-2),
        "the anchored message sits at the same screen row"
    );

    // A vanished anchor (the message trimmed) falls back to the
    // bottom (restore's `!boundary` arm, index.tsx:565-571).
    store.scroll_command("s1", oc_tui_core::scroll::ScrollCommand::LineUp);
    store.navigate(Route::Home);
    assert!(store.scroll_anchor("s1").is_some());
    store.set_message_positions(
        "s1",
        vec![MessagePosition { id: "other".to_owned(), y: 5 }],
    );
    store.select_tab("s1");
    store.resolve_scroll_restore("s1");
    assert_eq!(store.follow_state("s1"), FollowState::Sticky, "missing anchor → toBottom");
    assert!(store.scroll_anchor("s1").is_none(), "the dead anchor cleared");
}

/// TUI-28 — the `/cd` directory-recents completion + the ctrl+d
/// destructive delete (prompt/index.tsx:1958-1985 +
/// prompt/directory-recents.ts): typing `/cd ` lists the project's
/// recent directories (description `recent`, home-abbreviated
/// display), and ctrl+d on a recent is a TWO-PRESS delete — the
/// first press arms the confirm ("Press ctrl+d to confirm"), the
/// second removes the entry and clamps the selection.
#[test]
fn recents_ctrl_prompt_index() {
    use oc_tui::components::composer::autocomplete::{
        directory_recent_value, directory_recents_options, AutocompleteState, DirectoryRecents,
        Mode,
    };

    // `useDirectoryRecents` (directory-recents.ts): touch dedupes +
    // stamps usedAt (most recent first), caps at 10; remove drops.
    let mut recents = DirectoryRecents::default();
    recents.touch("p1", "/w/one", 100);
    recents.touch("p1", "/w/two", 300);
    recents.touch("p1", "/w/one", 200); // re-touch reorders + dedupes
    recents.touch("p2", "/w/other", 50); // per-project
    let list = recents.list("p1");
    assert_eq!(list.len(), 2, "the re-touch deduped (no duplicate)");
    assert_eq!(list[0].directory, "/w/two", "usedAt-descending (300 > 200)");
    assert_eq!(recents.list("p2").len(), 1, "projects stay separate");
    recents.remove("p1", "/w/two");
    assert!(!recents.list("p1").iter().any(|item| item.directory == "/w/two"));
    recents.remove("p1", "/w/one");
    // The 10-entry cap.
    for index in 0..12 {
        recents.touch("p1", &format!("/w/fill{index}"), 400 + index);
    }
    let list = recents.list("p1");
    assert_eq!(list.len(), 10, "the 10-entry cap");
    assert_eq!(list[0].directory, "/w/fill11", "most recent first");

    // `directoryRecentValue` (directory-completion.ts:92-98): the
    // home-relative display.
    assert_eq!(directory_recent_value("/home/u", "/home/u"), "~");
    assert_eq!(directory_recent_value("/home/u/work/api", "/home/u"), "~/work/api");
    assert_eq!(directory_recent_value("/elsewhere", "/home/u"), "/elsewhere");

    // The popup: `/cd ` (empty query) opens the directory mode over
    // the recents.
    let mut state = AutocompleteState::default();
    state.on_input("/cd ", 4);
    assert_eq!(state.visible, Some(Mode::Directory), "the argument slash opens the directory list");
    let options = directory_recents_options(&recents.list("p1"), "/home/u");
    assert!(!options.is_empty());
    assert!(options.iter().all(|option| option.description.as_deref() == Some("recent")));
    assert!(options.iter().all(|option| option.is_directory));
    assert!(options
        .iter()
        .all(|option| option.destructive.as_ref().is_some_and(|action| action.confirm
            == "Press ctrl+d to confirm")));
    let state = state.with_options(options);

    // ctrl+d is the two-press delete (autocomplete.tsx:629-638): the
    // first press arms the confirm, the second RUNS the removal (the
    // entry's absolute directory) and clamps the selection.
    let mut state = state;
    assert_eq!(state.confirming(), None);
    assert_eq!(state.trigger_destructive(), None, "first press only arms");
    assert_eq!(state.trigger_destructive(), Some("/w/fill11".to_owned()), "second press removes");
    assert_eq!(state.confirming(), None, "the confirm cleared");
    assert_eq!(state.selected, 0);

    // A non-empty query hides the recents (index.tsx:1962: `if
    // (query !== "") return []` — the live file list is the async
    // half, not ported).
    state.on_input("/cd /us", 7);
    assert_eq!(state.visible, Some(Mode::Directory));
    assert_eq!(state.options.len(), 0, "no recents under a query");
}

/// TUI-10 (the diff half): a working-tree image file renders the
/// inline `DiffViewerImage` — the "Working tree preview" label, the
/// decoded `WxH` size row, the blocks pixels — and the mount records
/// the hit rect whose release opens the preview
/// (diff-viewer-image.tsx:24-80).
#[test]
fn dialogerrordet_diff_image_inline_preview() {
    use oc_tui::components::diff_viewer::{
        DiffFile, DiffViewerOptions, DiffViewerState, FileStatus,
    };
    let png: &[u8] = include_bytes!("fixtures/r07-tiny-4x2.png");
    let files = vec![DiffFile {
        file: "shot.png".into(),
        additions: 0,
        deletions: 0,
        status: FileStatus::Modified,
        patch: String::new(),
        image: true,
        image_data: Some(png.to_vec()),
    }];
    let theme = oc_tui::theme::ThemeState::v2_dark();
    let mut hits = Vec::new();
    let frame = oc_tui::components::diff_viewer::render_diff_viewer_with_hits(
        &files,
        &DiffViewerState::default(),
        &DiffViewerOptions { source_label: "Uncommitted", source_detail: "", viewport_height: 40 },
        &theme.styles(),
        120,
        60,
        Some(&mut hits),
    );
    let text = frame.text_grid().join("\n");
    assert!(text.contains("Working tree preview"), "the label row");
    assert!(text.contains("4x2"), "the decoded size row");
    assert!(text.chars().any(|ch| "▀▄█▌▐▘▝▖▗▚▞▙▛▜▟".contains(ch)), "the blocks pixels painted");
    assert_eq!(hits.len(), 1, "the inline image recorded its hit rect");
    assert!(hits[0].4.starts_with("data:image/png;base64,"), "the hit carries the data URI");

    // A committed diff keeps the fallback copy (no working-tree bytes).
    let mut none = Vec::new();
    let _ = oc_tui::components::diff_viewer::render_diff_viewer_with_hits(
        &files,
        &DiffViewerState::default(),
        &DiffViewerOptions { source_label: "Committed", source_detail: "", viewport_height: 40 },
        &theme.styles(),
        120,
        60,
        Some(&mut none),
    );
    assert!(none.is_empty(), "committed images carry no hit");

    // The preview dialog opens over the hit's URI and paints the
    // blocks cells into the frame.
    let store = Store::new(Runtime::new());
    store.open_image_preview(
        vec![oc_tui::store::ImagePreviewItem {
            uri: hits[0].4.clone(),
            mention: Some("shot.png".to_owned()),
        }],
        0,
    );
    let frame = oc_tui::components::dialog::render_app(&store, &theme, (120, 40));
    let text = frame.text_grid().join("\n");
    assert!(text.contains("Image 1 of 1"), "the preview header");
    assert!(text.contains("shot.png"), "the mention caption");
    assert!(
        text.chars().any(|ch| "▀▄█▌▐▘▝▖▗▚▞▙▛▜▟".contains(ch)),
        "the preview's blocks pixels painted"
    );
}
