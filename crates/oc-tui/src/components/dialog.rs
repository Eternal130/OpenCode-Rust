//! The dialog framework (`ui/dialog.tsx`): the portal-style overlay
//! (backdrop + centered chrome box) painted on top of route content,
//! the modal key layer (escape closes, ctrl+c clears the input first,
//! printable keys feed the focused filter), and the per-kind content
//! assembly (selects via [`crate::components::dialog_select`], the
//! rename prompt).
//!
//! Overlay geometry (pinned by the `command-palette` golden + the
//! parity rig's dialog corpus):
//! * backdrop: `RGBA(0,0,0,150)` alpha-composited over the whole frame
//!   — BOTH channels of every covered cell (a glyph keeps its shape,
//!   dimmed; whitespace fg stays at the terminal default), over the
//!   v2-dark base 10,10,10 → 4,4,4;
//! * the box: `dialogWidth(size)` columns (60/88/116, clamped to
//!   `width - 2`), horizontally centered, `paddingTop = height / 4`
//!   rounded HALF-UP (taffy: 30/4 = 7.5 renders the box at row 8 —
//!   rig-pinned at 120x30), background
//!   `surface("dialog").background.base` (20,20,20), one row of top
//!   padding before the content;
//! * per-family content roots (all rig-pinned against v2.0.24):
//!   `DialogSelect` — title block paddingLeft 4, root `gap 1
//!   paddingBottom 1`; `DialogPrompt` (rename / worktree name) and
//!   `DialogHelp` — root paddingLeft/right 2 `gap 1`, their own
//!   paddingBottom rows (dialog-prompt.tsx:99, dialog-help.tsx:20).

// Backdrop alpha compositing rounds f64 channel math to u8 — bounded
// terminal colors, no truncation risk.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use crate::components::command_palette::{
    select_props_for_entry, CommandDispatcher, StoreDispatcher,
};
use crate::components::dialog_select::DialogStyles;
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
    let props = select_props_for_entry(&entry, store);
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
        | DialogKind::MessageActions { .. }
        | DialogKind::WorkingDirectory { .. }
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
                    top.select.set_filter("", &props, usize::from(terminal.1));
                }
            });
            store.preview_theme_filter();
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
                    top.select.set_filter(&filter, &props, usize::from(terminal.1));
                }
            });
            store.preview_theme_filter();
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
                top.select.set_filter(&filter, &props, usize::from(terminal.1));
            }
        });
        store.preview_theme_filter();
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
    // The theme list PREVIEWS on every move (`onMove: (opt) =>
    // themes.set(opt.value)` — dialog-theme-list.tsx:28-30).
    if matches!(entry.kind, DialogKind::ThemeList) {
        store.preview_theme_move();
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
    let Some(entry) = store.top_dialog() else {
        store.set_dialog_frame_rect(None);
        return;
    };
    if store.focus() != Focus::Dialog {
        store.set_dialog_frame_rect(None);
        return;
    }
    let props = select_props_for_entry(&entry, store);
    let plugin = match &entry.kind {
        DialogKind::Plugin { id } => store.plugin_dialog_content(id),
        _ => None,
    };
    let (rect, option_rows) =
        render_overlay_with_options(base, &entry, &props, plugin.as_ref(), theme, terminal);
    // The backdrop's dismissal surface: mouseups outside this rect
    // close the dialog (`ui/dialog.tsx:34-63`).
    store.set_dialog_frame_rect(Some(rect));
    store.set_dialog_option_rows(option_rows);
}

/// [`render_overlay_with`] plus the option-row map (the mouse surface).
pub fn render_overlay_with_options(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &crate::components::dialog_select::SelectProps,
    plugin: Option<&crate::store::PluginDialogContent>,
    theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
) -> ((usize, usize, usize, usize), Vec<Option<usize>>) {
    // Select dialogs record their option rows (absolute screen rows,
    // flat-index order); static content records none.
    let option_rows = if match &entry.kind {
        DialogKind::CommandPalette
        | DialogKind::QueuedPrompts
        | DialogKind::ThemeList
        | DialogKind::Worktrees
        | DialogKind::WorktreeDeleteConfirm { .. }
        | DialogKind::ServerList { .. }
        | DialogKind::MessageActions { .. }
        | DialogKind::WorkingDirectory { .. } => true,
        // A plugin dialog records rows only when its content is a
        // select (rows-only dialogs render statically).
        DialogKind::Plugin { .. } => {
            plugin.is_some_and(crate::store::PluginDialogContent::is_select)
        }
        DialogKind::Help | DialogKind::SessionRename { .. } | DialogKind::WorktreeName => false,
    } {
        crate::components::dialog_select::option_dialog_rows(
            props,
            &entry.select,
            usize::from(terminal.1),
        )
    } else {
        Vec::new()
    };
    let rect = render_overlay_with(base, entry, props, plugin, theme, terminal);
    let (_box_x, box_y, _, _) = rect;
    let option_rows =
        option_rows.into_iter().map(|row| row.map(|content_row| box_y + 1 + content_row)).collect();
    (rect, option_rows)
}

/// The overlay compositor with explicit content props (the golden
/// fixtures mount custom command registries). `plugin` carries a
/// plugin dialog's materialized content (rows-only dialogs render it
/// statically; select dialogs' options already live in `props`).
/// Returns the chrome box's absolute rect — the outside-click
/// dismissal surface (`ui/dialog.tsx:34-63`).
pub fn render_overlay_with(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &crate::components::dialog_select::SelectProps,
    plugin: Option<&crate::store::PluginDialogContent>,
    theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
) -> (usize, usize, usize, usize) {
    // Lane M2 (retired): the dialog family renders through its
    // opentui-solid tree — the only renderer since the retirement
    // lane deleted the hand-rolled walk.
    crate::components::dialog_solid::render_overlay_solid(base, entry, props, plugin, theme, terminal)
}

/// Backdrop composite: `out = src·α + dst·(1-α)` with `src = black`,
/// applied to the cell's background ALWAYS and to its foreground only
/// where a glyph actually renders — the raster leaves whitespace fg at
/// the terminal default (rig-pinned: the composer placeholder's 'A'
/// dims idx8→idx236 under the dialog while the blank cells beside it
/// stay idx15; every covered bg dims). Shared by BOTH renderer paths
/// (the NEW-S2a semantics: the scrim is a bridge-side compositing
/// step because the raster cannot see the route frame beneath).
pub(crate) fn blend_backdrop(cell: &mut crate::render::Cell) {
    let dim = |value: u8| -> u8 { (f64::from(value) * (1.0 - BACKDROP_ALPHA)).round() as u8 };
    cell.bg = match cell.bg {
        crate::render::CellColor::Rgb(r, g, b) => {
            crate::render::CellColor::Rgb(dim(r), dim(g), dim(b))
        }
        // The raster's default bg is black: black over black stays black.
        crate::render::CellColor::Default => crate::render::CellColor::Rgb(0, 0, 0),
        crate::render::CellColor::Indexed(index) => crate::render::CellColor::Indexed(index),
    };
    if cell.ch != ' ' {
        cell.fg = match cell.fg {
            crate::render::CellColor::Rgb(r, g, b) => {
                crate::render::CellColor::Rgb(dim(r), dim(g), dim(b))
            }
            // defaultForeground() is white; the composite materializes it.
            crate::render::CellColor::Default => {
                crate::render::CellColor::Rgb(dim(255), dim(255), dim(255))
            }
            crate::render::CellColor::Indexed(index) => crate::render::CellColor::Indexed(index),
        };
    }
}

/// Display one binding spec alternative with `<leader>` expanded
/// (shared with the opentui-solid path).
pub(crate) fn spec_display(spec: &str) -> String {
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
// The per-kind submission table grew with the mouse-opened dialogs
// (message actions + working directory); same shape as the key layer.
#[allow(clippy::too_many_lines)]
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
            // `confirmed = true` before `dialog.clear()` — the clear's
            // unmount must NOT revert the preview (dialog-theme-list.
            // tsx:31-35).
            store.confirm_theme_dialog();
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
        // `DialogMessage`'s actions (`routes/session/dialog-message.
        // tsx:23-85`): Jump to clears; Revert seeds the composer with
        // the message text (the server's `revert.stage` POST is the
        // 7.3 seam — not yet wired); Copy writes the clipboard;
        // Fork needs the fork dialog (7.3 — closes for now).
        Some(DialogKind::MessageActions { session_id, message_id }) => {
            use crate::components::message::model::{Message, Part};
            let text = store
                .messages(session_id.as_str())
                .iter()
                .find(|message| message.id() == message_id.as_str())
                .map(|message| match message {
                    Message::User { text, .. } => text.clone(),
                    Message::Assistant { parts, .. } => parts
                        .iter()
                        .filter_map(|part| match part {
                            Part::Text { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                })
                .unwrap_or_default();
            match command {
                "session.revert" => {
                    // `setPrompt(projectedPromptInput(value))`: the
                    // message text lands in the composer (draft +
                    // editing state kept in sync by the setter).
                    if !text.is_empty() {
                        let mut composer = store.composer(session_id.as_str());
                        composer.set_text(&text);
                        store.set_composer(session_id.as_str(), &composer);
                    }
                }
                "message.copy" => {
                    let _ = store.clipboard_write(&text);
                }
                _ => {}
            }
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The working-directory actions (`ui/working-directory-actions.
        // tsx:26-67`): Copy path writes the clipboard and toasts;
        // Open folder shells out to the system opener (a spawn
        // failure surfaces the error toast); Workspaces opens the
        // Worktrees dialog (`move.open()`).
        Some(DialogKind::WorkingDirectory { directory }) => {
            match command {
                "location.copy" => {
                    let _ = store.clipboard_write(directory.as_str());
                    store.toast_show(crate::components::toast::ToastOptions::info(
                        "Path copied to clipboard",
                    ));
                    store.clear_dialogs();
                }
                "location.open" => match open_path(directory.as_str()) {
                    Ok(()) => store.clear_dialogs(),
                    Err(error) => {
                        store.clear_dialogs();
                        store.toast_show(crate::components::toast::ToastOptions {
                            message: error,
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    }
                },
                "session.move" => {
                    let project = store.worktree_dialog().project_id;
                    if !project.is_empty() {
                        store.worktree_sink_refresh(&project);
                    }
                    store.replace_dialog(DialogKind::Worktrees, DialogSize::Medium);
                }
                _ => store.clear_dialogs(),
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        _ => {}
    }
    store.clear_dialogs();
    dispatcher.dispatch(store, command, None)
}

/// `openPath` (`util/open.ts`): hand a path to the system opener —
/// `xdg-open` on Linux, detached (no wait); the spawn error message
/// feeds the error toast (`toast.error`).
fn open_path(directory: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("open {directory}: {error}"))
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

    /// The overlay geometry from the command-palette golden + the rig's
    /// dialog corpus: backdrop (4,4,4) over the base 10 with the FG
    /// dimmed on glyph cells only, box (20,20,20) at `round(height/4)`
    /// (half-up), centered, 60 wide with a 78-column clamp.
    #[test]
    fn overlay_backdrop_and_box_geometry() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        // Backdrop over the base 10 → (4,4,4) everywhere outside the box;
        // the blank cell's fg (white) stays UNdimmed (whitespace fg keeps
        // the terminal default under the overlay — rig-pinned idx15).
        assert_eq!(rgb(frame.cell(0, 0).bg), (4, 4, 4));
        assert_eq!(rgb(frame.cell(23, 79).bg), (4, 4, 4));
        assert_eq!(rgb(frame.cell(0, 0).fg), (255, 255, 255));
        // A GLYPH cell's fg dims by the same 150/255 black (the base's
        // 'X' at (1,1): 255 → 105 — rig-pinned 'A' idx8 → idx236).
        frame.cells[1][1].ch = 'X';
        blend_backdrop(&mut frame.cells[1][1]);
        assert_eq!(rgb(frame.cell(1, 1).fg), (105, 105, 105));
        // Box: 60 wide centered at x=10, from y = round(24/4) = 6.
        assert_eq!(rgb(frame.cell(6, 10).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(6, 69).bg), (20, 20, 20));
        assert_eq!(rgb(frame.cell(6, 70).bg), (4, 4, 4));
        // Content row: the title at box_x + 4, the input row over the
        // dialog formfield background.
        assert_eq!(frame.cell(7, 14).ch, 'C');
        assert_eq!(rgb(frame.cell(9, 14).bg), (20, 20, 20));
    }

    /// paddingTop = height/4 rounds HALF-UP (taffy): 30/4 = 7.5 lands
    /// the box at row 8 — the 120x30 rig runs pinned ref row 8 vs our
    /// old floor(7).
    #[test]
    fn overlay_box_y_rounds_half_up() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.replace_dialog(DialogKind::Help, DialogSize::Medium);
        let mut frame = blank_base(120, 30);
        render_overlay(&mut frame, &store, &theme, (120, 30));
        assert_eq!(rgb(frame.cell(7, 30).bg), (4, 4, 4), "row 7 stays backdrop");
        assert_eq!(rgb(frame.cell(8, 30).bg), (20, 20, 20), "box top at row 8");
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

    /// The rename prompt: prompt-family geometry (paddingLeft 2, no
    /// formfield background, `enter submit` footer + paddingBottom),
    /// seeded with the session's current title.
    #[test]
    fn rename_prompt_renders_title_input_and_footer() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Parity fake says hello.".to_owned()));
        store.replace_dialog(
            DialogKind::SessionRename { session_id: "s1".to_owned() },
            DialogSize::Medium,
        );
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let rows: Vec<String> = frame.text_grid();
        let styles = DialogStyles::from_theme(&theme);
        let surface = match styles.surface_bg {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => panic!("surface must be RGB"),
        };
        // Title row at box_y + 1, x = box_x + 2 (prompt-family padding).
        assert!(rows[7].contains("Rename session"), "{}", rows[7]);
        assert_eq!(rows[7].find("Rename session"), Some(12), "paddingLeft 2");
        // The input row keeps the box surface (NO formfield bg) and
        // shows the placeholder when the value is empty.
        assert!(rows[9].contains("Session title"), "{}", rows[9]);
        assert_eq!(rgb(frame.cell(9, 30).bg), surface, "no formfield background");
        // The `enter submit` footer + its paddingBottom close the box
        // at row 12 (7 rows total: chrome + 6 content).
        assert!(rows[11].contains("enter submit"), "{}", rows[11]);
        assert_eq!(rows[11].find("enter"), Some(12), "footer at paddingLeft 2");
        assert_eq!(rgb(frame.cell(12, 30).bg), surface, "footer paddingBottom row");
        assert_eq!(rgb(frame.cell(13, 30).bg), (4, 4, 4), "box ends at row 12");
        // The footer's inner space ("enter " is one styled text node)
        // carries text.base like the `enter` glyphs (rig-pinned
        // idx255, not the ambient default).
        let text_fg = match styles.text {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => panic!("text token must be RGB"),
        };
        assert_eq!(rgb(frame.cell(11, 17).fg), text_fg, "frame x=17 is the styled space");
        assert_eq!(rgb(frame.cell(11, 12).fg), text_fg, "frame x=12 is `enter`");
    }

    /// `session.rename` seeds the prompt with the tab's live title
    /// (`value={currentTitle}`, dialog-session-rename.tsx:16).
    #[test]
    fn rename_dialog_seeds_the_current_title() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Parity fake says hello.".to_owned()));
        store.navigate(Route::Session { session_id: "s1".to_owned() });
        let mut dispatcher = crate::components::dialog::store_dispatcher();
        dispatcher.dispatch(&store, "session.rename", None);
        let entry = store.top_dialog().expect("dialog opened");
        assert!(matches!(entry.kind, DialogKind::SessionRename { .. }));
        assert_eq!(entry.value, "Parity fake says hello.");
    }

    /// `DialogHelp` (D-05 + the geometry wave): the prompt-family root
    /// (paddingLeft/right 2, gap 1), the palette hint WORD-WRAPPED at
    /// the content width, the hint box's paddingBottom, the gap, the
    /// right-justified `ok` button (padding fg = ambient default) and
    /// the button row's paddingBottom — 9 box rows at 80 cols
    /// (rig-pinned). return and escape both close; printables pass
    /// through; NO keybinding table.
    #[test]
    fn help_dialog_renders_the_prompt_family_geometry() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.replace_dialog(DialogKind::Help, DialogSize::Medium);
        let mut frame = blank_base(80, 24);
        render_overlay(&mut frame, &store, &theme, (80, 24));
        let rows: Vec<String> = frame.text_grid();
        let styles = DialogStyles::from_theme(&theme);
        let surface = match styles.surface_bg {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => panic!("surface must be RGB"),
        };

        // Title row at box_y + 1 with paddingLeft 2 (col 12, not 14).
        assert!(rows[7].contains("Help"), "title row: {:?}", rows[7]);
        assert_eq!(rows[7].find("Help"), Some(12), "prompt-family paddingLeft 2");
        assert!(rows[7].contains("esc/enter"), "close hint: {:?}", rows[7]);
        assert_eq!(
            rows[7].find("esc/enter"),
            Some(59),
            "hint ends at paddingRight 2: {:?}",
            rows[7]
        );

        // The palette hint wraps at the 56-column content width:
        // "…commands" then "in any context." (rig-pinned 80-col rows).
        assert_eq!(
            rows[9].find("Press ctrl+p to see all available actions and commands"),
            Some(12),
            "hint line 1 at paddingLeft 2: {:?}",
            rows[9]
        );
        assert!(!rows[9].contains(" in "), "no spill past the wrap: {:?}", rows[9]);
        assert_eq!(rows[10].trim(), "in any context.", "hint line 2: {:?}", rows[10]);

        // The hint box's paddingBottom (row 11) + the root gap (row 12).
        assert_eq!(rgb(frame.cell(11, 30).bg), surface);
        assert_eq!(rgb(frame.cell(12, 30).bg), surface);

        // The right-justified `ok` button on row 13: 8 cells ending at
        // the paddingRight-2 edge (cols 60..68 — `ok` at 63), padding
        // cells keep the ambient default fg, label uses the focused
        // primary action pair.
        let ok_row = rows[13].trim_end();
        assert!(ok_row.ends_with("ok"), "ok button right-justified: {ok_row:?}");
        assert_eq!(rows[13].find("ok"), Some(63), "button at the content edge");
        let expected = match (styles.selected_fg, styles.selected_bg) {
            (Color::Rgb(r, g, b), Color::Rgb(br, bg_, bb)) => ((r, g, b), (br, bg_, bb)),
            _ => panic!("focused action tokens must be RGB"),
        };
        let button_cells = &frame.cells[13];
        let fg = match button_cells[63].fg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        let bg = match button_cells[63].bg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        assert_eq!((fg, bg), expected, "ok text uses the focused primary action pair");
        let pad_bg = match button_cells[60].bg {
            CellColor::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        assert_eq!(pad_bg, expected.1, "button padding carries the focused bg");
        assert_eq!(
            rgb(button_cells[60].fg),
            (255, 255, 255),
            "button padding keeps the default fg (rig-pinned idx15)"
        );

        // The button row's paddingBottom closes the box at row 14 (9
        // rows total: chrome + 8 content).
        assert_eq!(rgb(frame.cell(14, 30).bg), surface);
        assert_eq!(rgb(frame.cell(15, 30).bg), (4, 4, 4), "box ends at row 14");

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
