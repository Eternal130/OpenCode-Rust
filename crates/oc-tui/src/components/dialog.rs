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
use crate::store::{DialogEntry, DialogKind, Focus, Store};
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
pub fn handle_dialog_key(
    store: &Store,
    keymap: &mut Keymap,
    clock: &dyn Clock,
    stroke: KeyStroke,
    terminal: (u16, u16),
) -> DialogKeyOutcome {
    let Some(entry) = store.top_dialog() else { return DialogKeyOutcome::Unhandled };
    let props = select_props_for(&entry.kind, store);
    let is_select = !matches!(entry.kind, DialogKind::SessionRename { .. });
    let is_help = matches!(entry.kind, DialogKind::Help);

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
    if !is_help && !stroke.ctrl && !stroke.alt && stroke.key.chars().count() == 1 {
        if let Some(ch) = stroke.key.chars().next() {
            let text = if stroke.shift { ch.to_ascii_uppercase() } else { ch };
            store.update_dialog(|top| {
                if matches!(top.kind, DialogKind::SessionRename { .. }) {
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
    if !is_help && stroke.key == "backspace" && !stroke.ctrl && !stroke.alt && !stroke.shift {
        store.update_dialog(|top| {
            if matches!(top.kind, DialogKind::SessionRename { .. }) {
                top.value.pop();
            } else if !top.select.filter.is_empty() {
                let mut filter = std::mem::take(&mut top.select.filter);
                filter.pop();
                top.select.set_filter(&filter);
            }
        });
        return DialogKeyOutcome::Consumed;
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
    render_overlay_with(base, &entry, &props, theme, terminal);
}

/// The overlay compositor with explicit content props (the golden
/// fixtures mount custom command registries).
pub fn render_overlay_with(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &crate::components::dialog_select::SelectProps,
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
        DialogKind::Help => render_help_content(&styles, dialog_bg, box_width),
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

/// `DialogHelp` (`ui/dialog-help.tsx`): the bold "Help" title with
/// the `esc/enter` close hint, the command-palette hint line
/// ("Press ctrl+p to see all available actions and commands in any
/// context."), then the keybinding table grouped by mode — built from
/// the keymap's default table (bound entries only; "none" rows stay
/// out). The binding column shows every alternative, `<leader>`
/// expanded (`ctrl+x m`).
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

    // The binding table grouped by layer (mode), bound entries only.
    let layers = ["base", "input", "dialog", "diff", "autocomplete"];
    let leader =
        crate::keymap::KeyStroke::parse("ctrl+x").unwrap_or_else(|| crate::keymap::KeyStroke {
            key: "x".into(),
            ctrl: true,
            alt: false,
            shift: false,
            super_key: false,
        });
    for layer in layers {
        let entries: Vec<&crate::keymap::CommandDef> = crate::keymap::default_table()
            .iter()
            .filter(|def| def.layer == layer && def.defaults != "none" && def.command != "leader")
            .collect();
        if entries.is_empty() {
            continue;
        }
        let binding_width = entries
            .iter()
            .map(|def| display_width(&def.bindings_display(&leader)))
            .max()
            .unwrap_or(8)
            .min(22);
        let mut section = blank_row(width, default, surface);
        put_text(&mut section, 4, layer, styles.text, surface, 1);
        rows.push(section);
        for def in entries {
            let mut row = blank_row(width, default, surface);
            let binding = def.bindings_display(&leader);
            let at = 4 + binding_width + 2;
            put_text(&mut row, 4, &binding, styles.formfield_fg, surface, 0);
            let description =
                crate::wrap::take_width(def.description, width.saturating_sub(at + 2).max(8));
            put_text(&mut row, at, &description, styles.muted, surface, 0);
            rows.push(row);
        }
        rows.push(blank_row(width, default, surface));
    }

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

fn display_width(text: &str) -> usize {
    crate::wrap::display_width(text)
}

impl crate::keymap::CommandDef {
    /// Every default alternative's display, comma-joined
    /// (`shortcuts.all`).
    fn bindings_display(&self, leader: &crate::keymap::KeyStroke) -> String {
        self.defaults
            .split(',')
            .map(str::trim)
            .filter(|spec| !spec.is_empty() && *spec != "none")
            .map(|spec| {
                crate::keymap::Chord::parse(spec, leader)
                    .map_or_else(|| spec.to_owned(), |chord| chord.display())
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
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

    /// `DialogHelp`: the v2 header (bold "Help" + `esc/enter` close
    /// hint, the palette hint line) plus the keybinding table grouped
    /// by mode; return and escape both close.
    #[test]
    fn help_dialog_lists_bindings_and_closes_on_return() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.replace_dialog(DialogKind::Help, DialogSize::Large);
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
        // The binding table: grouped by mode, `<leader>` expanded,
        // bound entries with their descriptions. (The overlay clamps
        // to the terminal height — a 24-row terminal shows the head
        // of the list; the early rows pin the shape.)
        let text = rows.join("\n");
        assert!(text.contains("base"), "mode sections present");
        assert!(
            text.contains("ctrl+x q"),
            "leader binding expanded (app.exit's third alternative)"
        );
        assert!(text.contains("Exit the application"), "descriptions present");
        assert!(!text.contains("app.console"), "unbound (none) entries stay out");
        assert!(!text.contains("Leader key for"), "the leader definition row stays out");

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
        store.replace_dialog(DialogKind::Help, DialogSize::Large);
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
}
