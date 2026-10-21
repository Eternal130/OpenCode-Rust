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
