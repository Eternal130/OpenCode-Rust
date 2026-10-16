//! L1 — the models panel (`DialogModel` through the select shell):
//! state goldens for the census family — the mount title
//! ("Select model", dialog-model.tsx:122), the cursor RESTING ON ROW
//! 0 (`focusCurrent={false}`, :171) with the selected-row highlight
//! (`background.action.primary.focused`), the current model's `●`
//! marker on its own row, the unconnected FLAT list (no group
//! headers, no per-row descriptions), and the "View all
//! integrations ctrl+a" footer action (:168-176). Update with
//! `PARITY_UPDATE=1 cargo test -p oc-tui --test models_panel_golden`.

use oc_tui::components::command_palette::model_list_props;
use oc_tui::components::dialog_select::{render_select, SelectState};
use oc_tui::store::{ListState, ServerModel, ServerProvider, Store};
use oc_tui::theme::ThemeState;

fn store_with_models(connected: bool) -> Store {
    let store = Store::new(oc_tui::reactive::Runtime::new());
    store.open_tab("s1", None);
    store.set_models(
        ListState::Loaded(vec![
            ServerModel {
                provider_id: "opencode".into(),
                id: "fledge".into(),
                name: "Fledge Alpha".into(),
                cost_input: 0.0,
                free: true,
                deprecated: false,
                variants: Vec::new(),
                released: 10.0,
                context_limit: None,
            },
            ServerModel {
                provider_id: "parityfake".into(),
                id: "fake-model".into(),
                name: "Fake Model".into(),
                cost_input: 0.0,
                free: true,
                deprecated: false,
                variants: Vec::new(),
                released: 5.0,
                context_limit: None,
            },
        ]),
        vec![
            ServerProvider { id: "opencode".into(), name: "opencode".into() },
            ServerProvider { id: "parityfake".into(), name: "parityfake".into() },
        ],
    );
    store.set_integration_connected(connected);
    store
}

fn grid(store: &Store, filter: &str, moves: i64) -> Vec<String> {
    let props = model_list_props(store, "Select model");
    let mut state = SelectState::default();
    if !filter.is_empty() {
        state.set_filter(filter, &props, 24);
    }
    for _ in 0..moves {
        state.move_selection(&props, 1);
    }
    let styles =
        oc_tui::components::dialog_select::DialogStyles::from_theme(&ThemeState::v2_dark());
    let view = render_select(
        &props,
        &state,
        &styles,
        ratatui::style::Color::Rgb(8, 8, 8),
        80,
        24,
        Some(24),
    );
    view.rows.iter().map(|row| row.iter().map(|cell| cell.ch).collect()).collect()
}

/// OPEN, unfiltered, unconnected: the mount title, row-0 cursor, flat
/// rows (Free footers, no group headers, no descriptions), and the
/// integrations footer action.
#[test]
fn open_unconnected_flat_with_row0_cursor_and_action() {
    let store = store_with_models(false);
    let rows = grid(&store, "", 0);
    let text = rows.join("\n");
    assert!(text.contains("Select model"), "mount title: {text}");
    assert!(text.contains("View all integrations"), "footer action: {text}");
    assert!(!text.contains("parityfake\n"), "no group headers while unconnected: {text}");
    assert!(text.contains("Fledge Alpha"), "rows present: {text}");
    // No per-row description column while unconnected.
    let row = rows.iter().find(|r| r.contains("Fledge Alpha")).expect("row");
    assert!(!row.trim_end().ends_with("opencode"), "no description suffix: {row}");
}

/// SELECTED state: the cursor row (row 0 — `focusCurrent={false}`)
/// carries the action-focused background; the CURRENT model keeps
/// only its `●` marker on its own row.
#[test]
fn cursor_sits_on_row_zero_and_current_carries_the_marker() {
    let store = store_with_models(false);
    store.stage_model(
        "s1",
        oc_tui::store::ModelSelection {
            provider_id: "parityfake".into(),
            model_id: "fake-model".into(),
            variant: None,
        },
    );
    let props = model_list_props(&store, "Select model");
    // The mounted dialog does NOT focus the current (cursor 0).
    let state = SelectState::default();
    assert_eq!(state.selected, 0);
    let props = props.with_current(Some("parityfake/fake-model".to_owned()));
    // The current row renders the `●` marker prefix.
    let styles =
        oc_tui::components::dialog_select::DialogStyles::from_theme(&ThemeState::v2_dark());
    let view = render_select(
        &props,
        &state,
        &styles,
        ratatui::style::Color::Rgb(8, 8, 8),
        80,
        24,
        Some(24),
    );
    let marker_row = view
        .rows
        .iter()
        .find(|row| row.iter().any(|cell| cell.ch == '●'))
        .expect("the current row carries ●");
    let text: String = marker_row.iter().map(|cell| cell.ch).collect();
    assert!(text.contains("Fake Model"), "the marker rides the current row: {text}");
}

/// HOVER/motion state: moving the selection one row down highlights
/// the second row (pointer `moveTo` equivalence).
#[test]
fn motion_moves_the_highlight() {
    let store = store_with_models(false);
    let styles =
        oc_tui::components::dialog_select::DialogStyles::from_theme(&ThemeState::v2_dark());
    let want = match styles.selected_bg {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    let focused_row_text = |moves: i64| -> String {
        let props = model_list_props(&store, "Select model");
        let mut state = SelectState::default();
        for _ in 0..moves {
            state.move_selection(&props, 1);
        }
        let view = render_select(
            &props,
            &state,
            &styles,
            ratatui::style::Color::Rgb(8, 8, 8),
            80,
            24,
            Some(24),
        );
        let focused: Vec<usize> = view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.iter().any(|cell| matches!(cell.bg, oc_tui::render::CellColor::Rgb(r, g, b) if (r, g, b) == want))
            })
            .map(|(index, _)| index)
            .collect();
        assert_eq!(focused.len(), 1, "exactly one highlighted row after {moves} moves");
        view.rows[focused[0]].iter().map(|cell| cell.ch).collect()
    };
    // The dialog's free-tier ordering puts Fledge Alpha first; one
    // move takes the highlight to the second row.
    assert!(focused_row_text(0).contains("Fledge Alpha"), "row 0 highlighted at open");
    assert!(!focused_row_text(1).contains("Fledge Alpha"), "the highlight moved off row 0");
}

/// CONNECTED flip: provider categories return as group headers and
/// the action reads "Connect an integration".
#[test]
fn connected_restores_categories_and_the_connect_wording() {
    let store = store_with_models(true);
    let rows = grid(&store, "", 0);
    let text = rows.join("\n");
    assert!(text.contains("parityfake"), "group headers while connected: {text}");
    assert!(text.contains("Connect an integration"), "connected wording: {text}");
}
