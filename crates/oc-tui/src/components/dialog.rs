//! The dialog framework (`ui/dialog.tsx`): the portal-style overlay
//! (backdrop + centered chrome box) painted on top of route content,
//! the modal key layer (escape closes, ctrl+c clears the input first,
//! printable keys feed the focused filter), and the per-kind content
//! assembly (selects via [`crate::components::dialog_select`], the
//! rename prompt).
//!
//! Overlay geometry (pinned by the `command-palette` golden):
//! * backdrop: `RGBA(0,0,0,150)` alpha-composited over the whole frame
//!   (over the v2-dark base 10,10,10 → 4,4,4);
//! * the box: `dialogWidth(size)` columns (60/88/116, clamped to
//!   `width - 2`), horizontally centered, `paddingTop = height / 4`,
//!   background `surface("dialog").background.base` (20,20,20), one
//!   row of top padding before the content.

// Backdrop alpha compositing rounds f64 channel math to u8 — bounded
// terminal colors, no truncation risk.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use crate::components::command_palette::{select_props_for, CommandDispatcher, StoreDispatcher};
use crate::components::dialog_select::{render_select, DialogStyles, SelectView};
use crate::keymap::{Clock, KeyStroke, Keymap, Outcome};
use crate::render::Frame;
use crate::store::{DialogEntry, DialogKind, DialogSize, Focus, Route, Store};
use ratatui::style::Color;

/// The backdrop's alpha (RGBA.fromInts(0, 0, 0, 150)).
const BACKDROP_ALPHA: f64 = 150.0 / 255.0;

/// What one stroke did to the dialog layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogKeyOutcome {
    /// The stroke was consumed by the dialog (filter typing, moves).
    Consumed,
    /// The top dialog closed (escape / ctrl+c).
    Closed,
    /// A select submitted: the selected option's value (the palette's
    /// command id, the theme-list value, …).
    Submitted(String),
    /// A prompt submitted: the typed value.
    PromptSubmitted(String),
    /// Nothing handled the stroke.
    Unhandled,
}

/// The modal key layer (`ui/dialog.tsx` + the select's dialog
/// commands): route one stroke while a dialog holds focus.
// The modal layer is inherently a key-by-key walk (escape → ctrl+c →
// input → per-kind actions → the select commands); the R-05 worktree
// actions pushed it past the lint budget.
#[allow(clippy::too_many_lines)]
pub fn handle_dialog_key(
    store: &Store,
    keymap: &mut Keymap,
    clock: &dyn Clock,
    stroke: KeyStroke,
    terminal: (u16, u16),
) -> DialogKeyOutcome {
    let Some(entry) = store.top_dialog() else { return DialogKeyOutcome::Unhandled };
    let props = select_props_for(&entry.kind, store);
    let is_select = match &entry.kind {
        // The rename + worktree-name dialogs are the prompt-style
        // inputs (not selects).
        DialogKind::SessionRename { .. } | DialogKind::WorktreeName => false,
        // A rows-only plugin dialog is static (like help: printables
        // pass through, only the close bindings act); the rest — the
        // palette, theme list, server lists, worktrees, the
        // queued-prompts manager (v2's DialogSelect) — are selects.
        DialogKind::Plugin { id } => {
            store.plugin_dialog_content(id).is_some_and(|content| content.is_select())
        }
        DialogKind::CommandPalette
        | DialogKind::QueuedPrompts
        | DialogKind::ThemeList
        | DialogKind::Help
        | DialogKind::Worktrees
        | DialogKind::WorktreeDeleteConfirm { .. }
        | DialogKind::ServerList { .. } => true,
    };
    let is_help = matches!(entry.kind, DialogKind::Help);
    let is_static = is_help
        || !is_select
            && !matches!(entry.kind, DialogKind::SessionRename { .. } | DialogKind::WorktreeName);

    // -- modal layer (dialog.tsx) -------------------------------------------
    if stroke.key == "escape" && !stroke.ctrl && !stroke.alt {
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }
    // `DialogHelp` binds return/escape to "Close help" (dialog-help.tsx).
    if is_help && stroke.key == "return" && !stroke.ctrl && !stroke.alt {
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }
    if stroke.ctrl && stroke.key == "c" {
        // ctrl+c clears a non-empty filter/input before closing.
        let has_text =
            if is_select { !entry.select.filter.is_empty() } else { !entry.value.is_empty() };
        if has_text {
            store.update_dialog(|top| {
                if matches!(top.kind, DialogKind::SessionRename { .. }) {
                    top.value.clear();
                } else {
                    top.select.set_filter("");
                }
            });
            return DialogKeyOutcome::Consumed;
        }
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }

    // -- the focused input renderable ---------------------------------------
    if !is_static && !stroke.ctrl && !stroke.alt && stroke.key.chars().count() == 1 {
        if let Some(ch) = stroke.key.chars().next() {
            let text = if stroke.shift { ch.to_ascii_uppercase() } else { ch };
            store.update_dialog(|top| {
                if matches!(top.kind, DialogKind::SessionRename { .. } | DialogKind::WorktreeName) {
                    top.value.push(text);
                } else {
                    let mut filter = std::mem::take(&mut top.select.filter);
                    filter.push(text);
                    top.select.set_filter(&filter);
                }
            });
            return DialogKeyOutcome::Consumed;
        }
    }
    if !is_static && stroke.key == "backspace" && !stroke.ctrl && !stroke.alt && !stroke.shift {
        store.update_dialog(|top| {
            if matches!(top.kind, DialogKind::SessionRename { .. } | DialogKind::WorktreeName) {
                top.value.pop();
            } else if !top.select.filter.is_empty() {
                let mut filter = std::mem::take(&mut top.select.filter);
                filter.pop();
                top.select.set_filter(&filter);
            }
        });
        return DialogKeyOutcome::Consumed;
    }

    // -- the Worktrees dialog actions (R-05) --------------------------------
    // v2 wires these as DialogSelect `actions` through the keymap
    // (`dialog.move_session.*`, config/keybind.ts:268-271); the name
    // dialog's tab is `dialog.worktree.generate` (:267).
    if let DialogKind::QueuedPrompts = entry.kind {
        // The queued-prompts manager's action layer (v2
        // index.tsx:641-672): `queued_prompt.delete` (ctrl+d) removes
        // the row — the LAST removal closes the dialog
        // (`if (cancelled && last) dialog.clear()`).
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        if keymap.stroke_triggers(&stroke, "queued_prompt.delete") {
            if let (Route::Session { session_id }, Some(id)) = (store.route(), selected.as_deref())
            {
                store.delete_queued_prompt(&session_id, id);
                if store.queued_prompts(&session_id).is_empty() {
                    // The last removal closes the manager
                    // (`if (cancelled && last) dialog.clear()`).
                    store.clear_dialogs();
                } else {
                    // Clamp the selection into the shrunken list
                    // (`reconcileSelection`).
                    let count = store.queued_prompts(&session_id).len();
                    store.update_dialog(|top| {
                        top.select.selected = crate::components::dialog_select::reconcile_selection(
                            top.select.selected,
                            count,
                        );
                    });
                }
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    if let DialogKind::Worktrees = entry.kind {
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        // The DialogSelect action layer: these resolve by their OWN
        // effective bindings (a same-layer table conflict —
        // `queued_prompt.delete` also claims ctrl+d — must not shadow
        // the dialog's actions; v2 binds them in a higher-priority
        // keymap layer).
        let triggers =
            |command: &'static str, keymap: &Keymap| keymap.stroke_triggers(&stroke, command);
        if triggers("dialog.move_session.new", keymap) {
            // v2 REPLACES the workspaces dialog with the name
            // dialog (`DialogWorktreeName.show` → `dialog.replace`)
            // at the default medium size.
            store.replace_dialog(DialogKind::WorktreeName, DialogSize::Medium);
            return DialogKeyOutcome::Consumed;
        }
        if triggers("dialog.move_session.refresh", keymap) {
            let project = store.worktree_dialog().project_id;
            if !project.is_empty() {
                store.worktree_sink_refresh(&project);
            }
            return DialogKeyOutcome::Consumed;
        }
        if triggers("dialog.move_session.delete", keymap) {
            if let Some(directory) = selected {
                if let crate::store::DeleteStep::Confirmed =
                    store.worktree_delete_pressed(&directory)
                {
                    let project = store.worktree_dialog().project_id;
                    store.worktree_sink_remove(&project, &directory, false);
                }
            }
            return DialogKeyOutcome::Consumed;
        }
        if triggers("dialog.move_session.move", keymap) {
            if let crate::store::Route::Session { session_id } = store.route() {
                if let Some(directory) = selected {
                    store.move_session_to_worktree(&session_id, &directory);
                }
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    if let DialogKind::WorktreeName = entry.kind {
        // tab = `dialog.worktree.generate` (Slug.create into the
        // input, caret to the end — the dialog's own keybind).
        if stroke.key == "tab" && !stroke.ctrl && !stroke.alt {
            store.update_dialog(|top| {
                let seed =
                    top.value.chars().map(|ch| u64::from(ch as u32)).fold(0u64, u64::wrapping_add);
                top.value = crate::components::dialog_worktrees::generate_slug(seed);
            });
            return DialogKeyOutcome::Consumed;
        }
    }

    // -- the dialog keymap layer (dialog.select.*) ---------------------------
    let Outcome::Command { command, .. } = keymap.lookup(stroke, clock) else {
        return DialogKeyOutcome::Unhandled;
    };
    let mut state = entry.select.clone();
    match command.as_str() {
        "dialog.select.prev" => state.move_selection(&props, -1),
        "dialog.select.next" => state.move_selection(&props, 1),
        "dialog.select.page_up" => state.move_selection(&props, -10),
        "dialog.select.page_down" => state.move_selection(&props, 10),
        "dialog.select.home" => state.move_to_edge(&props, false),
        "dialog.select.end" => state.move_to_edge(&props, true),
        "dialog.select.submit" | "dialog.prompt.submit" => {
            if is_select {
                return match state.submit(&props) {
                    Some(option) => DialogKeyOutcome::Submitted(option.value.clone()),
                    None => DialogKeyOutcome::Consumed,
                };
            }
            let value = entry.value.trim().to_owned();
            return DialogKeyOutcome::PromptSubmitted(value);
        }
        _ => return DialogKeyOutcome::Unhandled,
    }
    state.reveal(&props, usize::from(terminal.1));
    store.update_dialog(|top| top.select = state);
    // v2's DialogSelect clears a pending destructive action on any
    // selection move (`onMove: () => setToDelete(undefined)`).
    if matches!(entry.kind, DialogKind::Worktrees) {
        store.clear_worktree_delete();
    }
    DialogKeyOutcome::Consumed
}

/// Composite the top dialog over a rendered route frame (the portal
/// overlay): backdrop, chrome box, content.
pub fn render_overlay(
    base: &mut Frame,
    store: &Store,
    theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
) {
    let Some(entry) = store.top_dialog() else { return };
    if store.focus() != Focus::Dialog {
        return;
    }
    let props = select_props_for(&entry.kind, store);
    let plugin = match &entry.kind {
        DialogKind::Plugin { id } => store.plugin_dialog_content(id),
        _ => None,
    };
    render_overlay_with(base, &entry, &props, plugin.as_ref(), theme, terminal);
}

/// The overlay compositor with explicit content props (the golden
/// fixtures mount custom command registries). `plugin` carries a
/// plugin dialog's materialized content (rows-only dialogs render it
/// statically; select dialogs' options already live in `props`).
pub fn render_overlay_with(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &crate::components::dialog_select::SelectProps,
    plugin: Option<&crate::store::PluginDialogContent>,
    theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
) {
    let styles = DialogStyles::from_theme(theme);
    let dialog_bg = styles.surface_bg;

    // Backdrop: alpha-composite black@150 over every cell's bg.
    for row in &mut base.cells {
        for cell in row {
            cell.bg = blend_backdrop(cell.bg);
        }
    }

    // Chrome box.
    let width = usize::from(terminal.0);
    let height = usize::from(terminal.1);
    let box_width = entry.size.width().min(width.saturating_sub(2));
    let box_x = (width - box_width) / 2;
    let box_y = height / 4;

    let content = match &entry.kind {
        DialogKind::SessionRename { .. } => {
            render_rename_prompt(entry, &styles, dialog_bg, box_width)
        }
        DialogKind::WorktreeName => {
            render_worktree_name_prompt(entry, &styles, dialog_bg, box_width)
        }
        DialogKind::Help => render_help_content(&styles, dialog_bg, box_width),
        DialogKind::Plugin { .. } if plugin.is_some_and(|content| !content.is_select()) => {
            render_plugin_rows(
                plugin.unwrap_or(&crate::store::PluginDialogContent::default()),
                &styles,
                dialog_bg,
                box_width,
            )
        }
        _ => render_select(
            props,
            &entry.select,
            &styles,
            dialog_bg,
            box_width,
            usize::from(terminal.1),
            None,
        ),
    };
    let box_height = 1 + content.rows.len();
    for y in box_y..(box_y + box_height).min(height) {
        for x in box_x..(box_x + box_width).min(width) {
            if let Some(cell) = base.cells.get_mut(y).and_then(|row| row.get_mut(x)) {
                *cell = crate_render_cell(' ', 0, styles.default_fg, dialog_bg);
            }
        }
    }

    // Content rows over the box padding.
    for (offset, row) in content.rows.iter().enumerate() {
        let y = box_y + 1 + offset;
        if y >= height {
            break;
        }
        for (x, cell) in row.iter().enumerate().take(box_width) {
            if let Some(target) = base.cells.get_mut(y).and_then(|line| line.get_mut(box_x + x)) {
                *target = *cell;
            }
        }
    }
}

/// Backdrop composite: `out = src·α + dst·(1-α)` with `src = black`.
fn blend_backdrop(bg: crate::render::CellColor) -> crate::render::CellColor {
    match bg {
        crate::render::CellColor::Rgb(r, g, b) => {
            let keep = 1.0 - BACKDROP_ALPHA;
            crate::render::CellColor::Rgb(
                (f64::from(r) * keep).round() as u8,
                (f64::from(g) * keep).round() as u8,
                (f64::from(b) * keep).round() as u8,
            )
        }
        other => other,
    }
}

/// A plugin dialog's static content (the delegation payload's
/// rows-only shape): the bold title row with the `esc` hint, a blank
/// row, then every row verbatim (muted) — the same chrome geometry as
/// the select's title block.
fn render_plugin_rows(
    content: &crate::store::PluginDialogContent,
    styles: &DialogStyles,
    surface: Color,
    width: usize,
) -> SelectView {
    let default = styles.default_fg;
    let mut rows: Vec<Vec<crate::render::Cell>> = Vec::new();

    let mut title_row = blank_row(width, default, surface);
    let title = crate::wrap::take_width(&content.title, width.saturating_sub(12).max(8));
    put_text(&mut title_row, 4, &title, styles.text, surface, 1);
    put_text_end(&mut title_row, width - 4, "esc", styles.muted, surface, 0);
    rows.push(title_row);
    rows.push(blank_row(width, default, surface));

    for line in &content.rows {
        let mut row = blank_row(width, default, surface);
        let text = crate::wrap::take_width(line, width.saturating_sub(8).max(8));
        put_text(&mut row, 4, &text, styles.muted, surface, 0);
        rows.push(row);
    }
    rows.push(blank_row(width, default, surface));

    SelectView { rows, width }
}

/// `DialogPrompt` (rename): title row + input row with the session
/// title placeholder (`dialog-session-rename.tsx` mounts title
/// "Rename session", placeholder "Session title").
fn render_rename_prompt(
    entry: &DialogEntry,
    styles: &DialogStyles,
    surface: Color,
    width: usize,
) -> SelectView {
    let default = styles.default_fg;
    let mut rows: Vec<Vec<crate::render::Cell>> = Vec::new();

    // Title row (same geometry as the select's).
    let mut title_row = blank_row(width, default, surface);
    put_text(&mut title_row, 4, "Rename session", styles.text, surface, 1);
    put_text_end(&mut title_row, width - 4, "esc", styles.muted, surface, 0);
    rows.push(title_row);

    rows.push(blank_row(width, default, surface));

    // Input row over the formfield background.
    let mut input = blank_row(width, default, surface);
    let input_width = width.saturating_sub(8);
    for slot in input.iter_mut().skip(4).take(input_width) {
        *slot = crate_render_cell(' ', 0, default, styles.formfield_bg);
    }
    if entry.value.is_empty() {
        put_text(&mut input, 4, "Session title", styles.muted, styles.formfield_bg, 0);
    } else {
        let end =
            put_text(&mut input, 4, &entry.value, styles.formfield_fg, styles.formfield_bg, 0);
        if end < 4 + input_width {
            input[end] = crate_render_cell(' ', 0, surface, styles.formfield_fg);
        }
    }
    rows.push(input);

    rows.push(blank_row(width, default, surface));
    rows.push(blank_row(width, default, surface));

    SelectView { rows, width }
}

/// `DialogWorktreeName` (`component/dialog-worktree-name.tsx`): the
/// bold "Name worktree" title with the `esc` hint, the input over
/// the formfield background ("Worktree name" placeholder), and the
/// `enter submit` / `tab generate one` footer hints.
fn render_worktree_name_prompt(
    entry: &DialogEntry,
    styles: &DialogStyles,
    surface: Color,
    width: usize,
) -> SelectView {
    let default = styles.default_fg;
    let mut rows: Vec<Vec<crate::render::Cell>> = Vec::new();

    let mut title_row = blank_row(width, default, surface);
    put_text(&mut title_row, 4, "Name worktree", styles.text, surface, 1);
    put_text_end(&mut title_row, width - 4, "esc", styles.muted, surface, 0);
    rows.push(title_row);

    rows.push(blank_row(width, default, surface));

    let mut input = blank_row(width, default, surface);
    let input_width = width.saturating_sub(8);
    for slot in input.iter_mut().skip(4).take(input_width) {
        *slot = crate_render_cell(' ', 0, default, styles.formfield_bg);
    }
    if entry.value.is_empty() {
        put_text(&mut input, 4, "Worktree name", styles.muted, styles.formfield_bg, 0);
    } else {
        let end =
            put_text(&mut input, 4, &entry.value, styles.formfield_fg, styles.formfield_bg, 0);
        if end < 4 + input_width {
            input[end] = crate_render_cell(' ', 0, surface, styles.formfield_fg);
        }
    }
    rows.push(input);

    rows.push(blank_row(width, default, surface));

    // The footer hints (enter submit · tab generate one).
    let mut footer = blank_row(width, default, surface);
    let end = put_text(&mut footer, 4, "enter", styles.text, surface, 0);
    put_text(&mut footer, end + 1, "submit", styles.muted, surface, 0);
    let end = put_text(&mut footer, 16, "tab", styles.text, surface, 0);
    put_text(&mut footer, end + 1, "generate one", styles.muted, surface, 0);
    rows.push(footer);

    SelectView { rows, width }
}

/// `DialogHelp` (`ui/dialog-help.tsx`) — v2's help is NOT a keybind
/// table: the bold "Help" title with the `esc/enter` close hint, the
/// command-palette hint line ("Press ctrl+p to see all available
/// actions and commands in any context."), and a right-justified
/// `ok` button (`paddingHorizontal 3` over
/// `background.action.primary.focused` /
/// `text.action.primary.focused`). D-05: the port's earlier
/// keybinding table (and its 22-column binding clamp) is gone — v2
/// renders no list here.
fn render_help_content(styles: &DialogStyles, surface: Color, width: usize) -> SelectView {
    let default = styles.default_fg;
    let mut rows: Vec<Vec<crate::render::Cell>> = Vec::new();

    // Header: bold "Help", right-aligned "esc/enter".
    let mut header = blank_row(width, default, surface);
    put_text(&mut header, 4, "Help", styles.text, surface, 1);
    put_text_end(&mut header, width - 4, "esc/enter", styles.muted, surface, 0);
    rows.push(header);
    rows.push(blank_row(width, default, surface));

    // The palette hint (dialog-help.tsx's body copy; the binding is
    // the palette command's default).
    let mut hint = blank_row(width, default, surface);
    let palette = crate::keymap::default_table()
        .iter()
        .find(|def| def.command == "command.palette.show")
        .map_or_else(
            || "ctrl+p".to_owned(),
            |def| spec_display(def.defaults.split(',').next().unwrap_or("none")),
        );
    put_text(
        &mut hint,
        4,
        &format!("Press {palette} to see all available actions and commands in any context."),
        styles.muted,
        surface,
        0,
    );
    rows.push(hint);
    rows.push(blank_row(width, default, surface));

    // The right-justified `ok` button: paddingLeft/Right 3 over the
    // focused primary action pair, text `text.action.primary.focused`.
    let mut button = blank_row(width, default, surface);
    let button_width = 3 + 2 + 3;
    let start = width.saturating_sub(4 + button_width).max(4);
    for cell in &mut button[start..start + button_width] {
        cell.bg = to_cell_color(styles.selected_bg);
        cell.fg = to_cell_color(styles.selected_fg);
    }
    put_text(&mut button, start + 3, "ok", styles.selected_fg, styles.selected_bg, 0);
    rows.push(button);

    SelectView { rows, width }
}

/// Display one binding spec alternative with `<leader>` expanded.
fn spec_display(spec: &str) -> String {
    let leader =
        crate::keymap::KeyStroke::parse("ctrl+x").unwrap_or_else(|| crate::keymap::KeyStroke {
            key: "x".into(),
            ctrl: true,
            alt: false,
            shift: false,
            super_key: false,
        });
    crate::keymap::Chord::parse(spec, &leader)
        .map_or_else(|| spec.to_owned(), |chord| chord.display())
}

// -- small paint helpers (cell-level, shared with the prompt) -----------------

type CellRow = Vec<crate::render::Cell>;

fn blank_row(width: usize, fg: Color, bg: Color) -> CellRow {
    vec![crate_render_cell(' ', 0, fg, bg); width]
}

fn crate_render_cell(ch: char, attrs: u16, fg: Color, bg: Color) -> crate::render::Cell {
    crate::render::Cell { ch, attrs, fg: to_cell_color(fg), bg: to_cell_color(bg) }
}

fn to_cell_color(color: Color) -> crate::render::CellColor {
    match color {
        Color::Rgb(r, g, b) => crate::render::CellColor::Rgb(r, g, b),
        Color::Indexed(index) => crate::render::CellColor::Indexed(index),
        _ => crate::render::CellColor::Default,
    }
}

fn put_text(row: &mut CellRow, x: usize, text: &str, fg: Color, bg: Color, attrs: u16) -> usize {
    let mut at = x;
    for atom in crate::wrap::flatten(text, ratatui::style::Style::new().fg(fg).bg(bg)) {
        if at >= row.len() {
            break;
        }
        row[at] = crate_render_cell(atom.text.chars().next().unwrap_or(' '), attrs, fg, bg);
        at += atom.width.max(1);
    }
    at
}

fn put_text_end(row: &mut CellRow, end: usize, text: &str, fg: Color, bg: Color, attrs: u16) {
    let start = end.saturating_sub(crate::wrap::display_width(text));
    put_text(row, start, text, fg, bg, attrs);
}

/// Convenience: render the full app frame — route content (home /
/// session skeletons) with the dialog overlay composited on top. This
/// is the frame source the keypress-replay harness compares.
#[must_use]
pub fn render_app(store: &Store, theme: &crate::theme::ThemeState, size: (u16, u16)) -> Frame {
    let styles = theme.styles();
    let mut frame = match store.route() {
        crate::store::Route::Home => crate::components::home::render_home(store, &styles, size),
        crate::store::Route::Session { session_id } => {
            crate::components::session::render_session(store, &session_id, &styles, size)
        }
    };
    render_overlay(&mut frame, store, theme, size);
    frame
}

/// Execute a submitted select value through a dispatcher (the palette's
/// `onSelect`: clear, then run).
pub fn execute_submission(
    store: &Store,
    dispatcher: &mut dyn CommandDispatcher,
    value: &str,
) -> crate::components::command_palette::Dispatch {
    let command = crate::components::command_palette::command_of_value(value);
    // Theme selections resolve through the registry (the dialog's
    // values are registry names); server-list rows route by the
    // picker's own command (model/agent/session selections).
    match store.top_dialog().map(|entry| entry.kind) {
        Some(DialogKind::ThemeList) => {
            store.set_theme_by_name(command);
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        Some(DialogKind::ServerList { command: picker, .. }) => {
            let selection = value.to_owned();
            store.clear_dialogs();
            return crate::components::command_palette::execute_server_list_selection(
                store, picker, &selection,
            );
        }
        // A Worktrees row submit: v2's `onSelect` → `selectWorkspace` —
        // close + navigate home at the directory.
        Some(DialogKind::Worktrees) => {
            let directory = value.to_owned();
            store.select_worktree(&directory);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The forced-delete confirm: "yes" re-fires the remove with
        // `force: true`; anything else just closes (v2's
        // `DialogWorkspaceFileChanges` yes/no).
        Some(DialogKind::WorktreeDeleteConfirm { directory }) => {
            let forced = value == "yes";
            if forced {
                let project = store.worktree_dialog().project_id;
                store.worktree_sink_remove(&project, &directory, true);
            }
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The queued-prompts manager's steer (v2 index.tsx:636-640:
        // `onSelect` → `mutatePending("steer", …)`): the selected
        // prompt delivers NOW (the local serial queue's head-jump).
        Some(DialogKind::QueuedPrompts) => {
            if let Route::Session { session_id } = store.route() {
                let queued = store.queued_prompts(&session_id);
                if let Some(prompt) = queued.iter().find(|item| item.id == value) {
                    let payload = crate::components::composer::SubmittedPrompt {
                        text: prompt.text.clone(),
                        ..crate::components::composer::SubmittedPrompt::default()
                    };
                    store.delete_queued_prompt(&session_id, value);
                    store.clear_dialogs();
                    store.forward_submission(&session_id, &payload);
                    return crate::components::command_palette::Dispatch::Handled;
                }
            }
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        _ => {}
    }
    store.clear_dialogs();
    dispatcher.dispatch(store, command, None)
}

/// Default dispatcher accessor for embedders that just want store
/// behavior.
#[must_use]
pub fn store_dispatcher() -> StoreDispatcher {
    StoreDispatcher
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::render::CellColor;
    use crate::store::DialogSize;

    fn rgb(color: CellColor) -> (u8, u8, u8) {
        match color {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        }
    }

    /// The overlay geometry from the command-palette golden: backdrop
    /// (4,4,4) over the base 10, box (20,20,20) at `height / 4`,
    /// centered, 60 wide with a 78-column clamp.
    #[test]
    fn overlay_backdrop_and_box_geometry() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        // Backdrop over the base 10 → (4,4,4) everywhere outside the box.
        assert_eq!(rgb(frame.cell(0, 0).bg), (4, 4, 4));
        assert_eq!(rgb(frame.cell(23, 79).bg), (4, 4, 4));
        // Box: 60 wide centered at x=10, from y = 24/4 = 6.
        assert_eq!(rgb(frame.cell(6, 10).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(6, 69).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(6, 70).bg), (4, 4, 4));
        // Content row: the title at box_x + 4, the input row over the
        // dialog formfield background.
        assert_eq!(frame.cell(7, 14).ch, 'C');
        assert_eq!(rgb(frame.cell(9, 14).bg), (20, 20, 20));
    }

    /// The clamp: the box never exceeds `width - 2`.
    #[test]
    fn overlay_box_clamps_to_width_minus_two() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Xlarge);
        let mut frame = blank_base(40, 24);
        render_overlay(&mut frame, &store, &theme, (40, 24));
        assert_eq!(rgb(frame.cell(12, 0).bg), (4, 4, 4), "1-col margin each side");
        assert_eq!(rgb(frame.cell(12, 1).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(12, 38).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(12, 39).bg), (4, 4, 4));
    }

    fn blank_base(width: u16, height: u16) -> Frame {
        let cell = crate::render::Cell {
            ch: ' ',
            attrs: 0,
            fg: CellColor::Rgb(255, 255, 255),
            bg: CellColor::Rgb(10, 10, 10),
        };
        Frame {
            cells: vec![vec![cell; usize::from(width)]; usize::from(height)],
            width: usize::from(width),
            height: usize::from(height),
        }
    }

    /// The modal layer through the pump-shaped path: escape closes,
    /// printable keys type, the dialog.* layer moves.
    #[test]
    fn dialog_key_layer_routes_moves_and_input() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("S1".to_owned()));
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();

        // Typing filters and resets the selection.
        for key in ["r", "e", "n", "a", "m", "e"] {
            let outcome = handle_dialog_key(
                &store,
                &mut keymap,
                &clock,
                crate::keymap::KeyStroke::parse(key).unwrap(),
                (80, 24),
            );
            assert_eq!(outcome, DialogKeyOutcome::Consumed);
        }
        let entry = store.top_dialog().unwrap();
        assert_eq!(entry.select.filter, "rename");
        assert_eq!(entry.select.selected, 0);

        // Submit runs the selected command: rename replaces the dialog.
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert_eq!(
            outcome,
            DialogKeyOutcome::Submitted("session.rename".to_owned()),
            "the filtered top row submits its command id"
        );

        // Moves exercise the dialog.* layer (down = dialog.select.next).
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
        let before = store.top_dialog().unwrap().select.selected;
        handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("down").unwrap(),
            (80, 24),
        );
        assert_ne!(store.top_dialog().unwrap().select.selected, before);
        // Escape closes (the modal layer, before any base binding).
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("escape").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Closed);
        assert_eq!(store.dialogs().len(), 0);
    }

    /// The rename prompt renders its title and placeholder.
    #[test]
    fn rename_prompt_renders_title_and_input() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.replace_dialog(
            DialogKind::SessionRename { session_id: "s1".to_owned() },
            DialogSize::Medium,
        );
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let row: String = frame.cells[7].iter().map(|cell| cell.ch).collect();
        assert!(row.contains("Rename session"), "{row}");
        let input: String = frame.cells[9].iter().map(|cell| cell.ch).collect();
        assert!(input.contains("Session title"), "{input}");
    }

    /// `DialogHelp` (D-05): v2's help is the small surface — the bold
    /// "Help" + `esc/enter` header, the palette hint line, and the
    /// right-justified `ok` button. NO keybinding table (the port's
    /// clamped table is removed); return and escape both close.
    #[test]
    fn help_dialog_lists_bindings_and_closes_on_return() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.replace_dialog(DialogKind::Help, DialogSize::Medium);
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let rows: Vec<String> = frame.text_grid();
        assert!(rows[7].contains("Help"), "title row: {:?}", rows[7]);
        assert!(rows[7].contains("esc/enter"), "close hint: {:?}", rows[7]);
        // The palette hint (dialog-help.tsx's body copy).
        assert!(
            rows[9].contains("Press ctrl+p to see all available actions and commands"),
            "hint row: {:?}",
            rows[9]
        );
        // The right-justified `ok` button (padding 3 each side over
        // the focused primary action pair).
        let ok_row = rows[11].trim_end();
        assert!(ok_row.ends_with("ok"), "ok button right-justified: {ok_row:?}");
        assert!(
            ok_row.ends_with("   ok"),
            "three cells of focused-bg padding left of the label: {ok_row:?}"
        );
        let styles = DialogStyles::from_theme(&theme);
        let expected = match (styles.selected_fg, styles.selected_bg) {
            (Color::Rgb(r, g, b), Color::Rgb(br, bg_, bb)) => ((r, g, b), (br, bg_, bb)),
            _ => panic!("focused action tokens must be RGB"),
        };
        let button_cells = &frame.cells[11];
        let ok_pos = rows[11].find("ok").expect("ok text");
        let fg = match button_cells[ok_pos].fg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        let bg = match button_cells[ok_pos].bg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        assert_eq!((fg, bg), expected, "ok text uses the focused primary action pair");
        let pad_bg = match button_cells[ok_pos - 3].bg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        assert_eq!(pad_bg, expected.1, "button padding carries the focused bg");
        // No keybinding table: no mode sections, no binding rows, no
        // clamp artifacts.
        let text = rows.join("\n");
        assert!(!text.contains("base"), "no mode sections in v2 help");
        assert!(!text.contains("ctrl+x q"), "no binding table");
        assert!(!text.contains("Exit the application"), "no descriptions");

        // return closes (dialog-help binds return + escape).
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Closed);
        assert_eq!(store.dialogs().len(), 0);

        // Typing in the help dialog neither filters nor moves anything.
        store.replace_dialog(DialogKind::Help, DialogSize::Medium);
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("x").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Unhandled, "printables pass through help");
        assert!(store.top_dialog().unwrap().select.filter.is_empty(), "no filter state on help");
    }

    /// Wave 3 — `DialogKind::Plugin` with rows-only content: the
    /// title + `esc` hint header, the rows verbatim, printables
    /// passing through (static, like help), escape closing.
    #[test]
    fn plugin_dialog_renders_rows_and_closes_on_escape() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_plugin_dialog_with(
            "plugin:omos:about",
            crate::store::PluginDialogContent {
                title: "About omos".into(),
                rows: vec!["version 3.0.3".into(), "preset: zhipuai".into()],
                options: vec![],
            },
        );
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let rows = frame.text_grid();
        assert!(rows[7].contains("About omos"), "the title row: {:?}", rows[7]);
        assert!(rows[7].contains("esc"), "the close hint: {:?}", rows[7]);
        assert!(rows[9].contains("version 3.0.3"), "row 1: {:?}", rows[9]);
        assert!(rows[10].contains("preset: zhipuai"), "row 2: {:?}", rows[10]);

        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("x").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Unhandled, "printables pass through");
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("escape").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Closed);
        assert_eq!(store.dialogs().len(), 0);
    }

    /// Wave 3 — a plugin dialog WITH options is a select: the native
    /// pipeline (typing filters, submit returns the option's value —
    /// dispatchable through the palette dispatcher).
    #[test]
    fn plugin_dialog_with_options_selects_and_submits() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_plugin_dialog_with(
            "plugin:omos:preset",
            crate::store::PluginDialogContent {
                title: "Switch preset".into(),
                rows: vec![],
                options: vec![
                    crate::store::PluginDialogOption {
                        title: "zhipuai".into(),
                        value: "preset.zhipuai".into(),
                        description: "own/glm".into(),
                    },
                    crate::store::PluginDialogOption {
                        title: "openai".into(),
                        value: "preset.openai".into(),
                        description: String::new(),
                    },
                ],
            },
        );
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let rows = frame.text_grid();
        assert!(rows[7].contains("Switch preset"), "the title: {:?}", rows[7]);
        assert!(rows[9].contains("Search"), "the filter input: {:?}", rows[9]);
        assert!(rows[11].contains("Plugin"), "the category group: {:?}", rows[11]);
        assert!(rows[12].contains("zhipuai"), "option 1: {:?}", rows[12]);
        assert!(rows[13].contains("openai"), "option 2: {:?}", rows[13]);

        // Typing filters; submit returns the option's value.
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();
        for key in ["o", "p", "e"] {
            let outcome = handle_dialog_key(
                &store,
                &mut keymap,
                &clock,
                crate::keymap::KeyStroke::parse(key).unwrap(),
                (80, 24),
            );
            assert_eq!(outcome, DialogKeyOutcome::Consumed);
        }
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Submitted("preset.openai".to_owned()));
    }

    /// The delegation seam: opening by id materializes through the
    /// attached source (once per open — the hard timeout is the
    /// source's contract); unknown ids no-op.
    #[test]
    fn open_plugin_dialog_materializes_through_the_source() {
        struct Source;
        impl crate::store::PluginDialogSource for Source {
            fn content(&self, id: &str) -> Option<crate::store::PluginDialogContent> {
                (id == "plugin:omos:preset").then(|| crate::store::PluginDialogContent {
                    title: "Switch preset".into(),
                    rows: vec!["materialized".into()],
                    options: vec![],
                })
            }
        }
        let store = Store::new(Runtime::new());
        store.set_plugin_dialog_source(std::rc::Rc::new(Source));
        assert!(store.open_plugin_dialog("plugin:omos:preset"));
        let top = store.top_dialog().expect("opened");
        assert!(matches!(top.kind, DialogKind::Plugin { .. }));
        let content = store.plugin_dialog_content("plugin:omos:preset").expect("cached");
        assert_eq!(content.rows, ["materialized".to_owned()]);
        assert!(!store.open_plugin_dialog("plugin:missing"), "unknown ids no-op");
    }
}
