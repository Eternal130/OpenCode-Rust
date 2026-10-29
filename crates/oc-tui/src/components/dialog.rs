//! The dialog framework (`ui/dialog.tsx`): the portal-style overlay
//! (backdrop + centered chrome box) painted on top of route content,
//! the modal key layer (escape and ctrl+c both close — ctrl+c clears
//! any filter/input text as part of the close, `ui/dialog.tsx:115-156`
//! — printable keys feed the focused filter), and the per-kind content
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
use crate::keymap::{Clock, KeyStroke, Keymap, Outcome};
use crate::render::Frame;
use crate::store::{DialogEntry, DialogKind, DialogSize, Focus, Route, Store};

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
    /// A dialog's own action resolved to a DOT COMMAND (TUI-25: the
    /// alert/confirm pill's `confirm_command` — the app layer
    /// dispatches it through the palette dispatcher after the close).
    CommandSubmitted(String),
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
    // The prompt-style inputs (not selects): rename, worktree-name,
    // the integration key/code entry.
    let is_prompt_kind = |kind: &DialogKind| {
        matches!(
            kind,
            DialogKind::SessionRename { .. }
                | DialogKind::WorktreeName
                | DialogKind::IntegrationKey { .. }
                | DialogKind::IntegrationCode { .. }
                | DialogKind::BtwPrompt
        )
    };
    let is_select = match &entry.kind {
        // The rename + worktree-name dialogs + the connect-flow
        // key/code prompts are the prompt-style inputs (not selects).
        // The prompt-style inputs + TUI-10/24/25's custom-key
        // families (image preview, error details, alert, confirm)
        // take per-kind actions, not the select pipeline. F15 fix
        // round: the pair dialog joins them — a CUSTOM (static)
        // dialog per ref dialog-pair.tsx; no filter, no option rows;
        // only the close bindings act (the modal layer's escape).
        DialogKind::SessionRename { .. }
        | DialogKind::WorktreeName
        | DialogKind::IntegrationKey { .. }
        | DialogKind::IntegrationCode { .. }
        | DialogKind::BtwPrompt
        | DialogKind::BtwAnswer { .. }
        | DialogKind::ImagePreview
        | DialogKind::ErrorDetails { .. }
        | DialogKind::Alert { .. }
        | DialogKind::Confirm { .. }
        | DialogKind::Pair => false,
        // A rows-only plugin dialog is static (like help: printables
        // pass through, only the close bindings act); the rest — the
        // palette, theme list, server lists, worktrees, the
        // queued-prompts manager (v2's DialogSelect) — are selects.
        DialogKind::Plugin { id } => {
            store.plugin_dialog_content(id).is_some_and(|content| content.is_select())
        }
        DialogKind::CommandPalette
        | DialogKind::QueuedPrompts
        | DialogKind::Update
        | DialogKind::ThemeList
        | DialogKind::Help
        | DialogKind::Plugins
        | DialogKind::TabMenu { .. }
        | DialogKind::Worktrees
        | DialogKind::WorktreeDeleteConfirm { .. }
        | DialogKind::MessageActions { .. }
        | DialogKind::WorkingDirectory { .. }
        | DialogKind::ServerList { .. }
        | DialogKind::ToolOutput { .. }
        | DialogKind::DiffSource
        | DialogKind::DiffHelp
        | DialogKind::Mcp
        | DialogKind::McpError { .. }
        | DialogKind::Status
        | DialogKind::Variant
        | DialogKind::Debug
        | DialogKind::Timeline { .. }
        | DialogKind::Integration
        | DialogKind::IntegrationAccounts { .. }
        | DialogKind::IntegrationMethod { .. }
        | DialogKind::IntegrationPending { .. }
        | DialogKind::Settings
        | DialogKind::ForkPicker { .. }
        | DialogKind::SessionExport { .. }
        | DialogKind::ExportResult { .. }
        // TUI-8/9/TD22: DialogSkill / DialogStash /
        // DialogExperiments are DialogSelects.
        | DialogKind::Skill
        | DialogKind::Stash
        | DialogKind::Experiments
        // TUI-5 (lane-tui2): DialogOpen + its worktrees view are
        // DialogSelects (dialog-open.tsx's DialogSelect mounts).
        | DialogKind::Open
        | DialogKind::OpenWorktrees { .. } => true,
    };
    let is_help = matches!(entry.kind, DialogKind::Help);
    let is_static = is_help
        || !is_select
            && !matches!(
                entry.kind,
                DialogKind::SessionRename { .. }
                    | DialogKind::WorktreeName
                    | DialogKind::IntegrationKey { .. }
                    | DialogKind::IntegrationCode { .. }
                    | DialogKind::BtwPrompt
            );

    // -- modal layer (dialog.tsx) -------------------------------------------
    // TUI-24: DialogErrorDetails binds escape to `onBack`
    // (dialog-error-details.tsx:89-96) — the plugins role re-seats
    // the plugin list (v2's `setDetail()` + `setSize("large")`).
    if let DialogKind::ErrorDetails { back_to_plugins, .. } = &entry.kind {
        if stroke.key == "escape" && !stroke.ctrl && !stroke.alt {
            if *back_to_plugins {
                store.replace_dialog(DialogKind::Plugins, DialogSize::Large);
            } else {
                store.close_dialog();
            }
            return DialogKeyOutcome::Closed;
        }
    }
    if stroke.key == "escape" && !stroke.ctrl && !stroke.alt {
        // Lane FINAL: closing an attempt view cancels it server-side
        // (`onCleanup`'s cancel, dialog-integration.tsx:654-661).
        if matches!(
            entry.kind,
            DialogKind::IntegrationPending { .. } | DialogKind::IntegrationCode { .. }
        ) {
            store.integration_cancel_active();
        }
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }
    // `DialogHelp` binds return/escape to "Close help" (dialog-help.tsx).
    if is_help && stroke.key == "return" && !stroke.ctrl && !stroke.alt {
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }
    if stroke.ctrl && stroke.key == "c" {
        // TUI-35: one ctrl+c CLOSES the modal — clearing any
        // filter/input text first (`ui/dialog.tsx:115-156`: "the modal
        // layer CLOSES the dialog (clearing any filter text first); it
        // does not exit" — the text clear is part of the close, not a
        // separate press). The clear-and-STAY shape swallowed the
        // standard exit chord (ctrl+c, ctrl+c, per keybind.ts:131
        // `app.exit` keeps ctrl+c/ctrl+d): with a dirty filter the
        // first press only cleared, the second closed, and no press
        // remained to exit — the app could never be torn down through
        // a dirty dialog (the session_filter_semantics 90 s wedge).
        let has_text =
            if is_select { !entry.select.filter.is_empty() } else { !entry.value.is_empty() };
        if has_text {
            store.update_dialog(|top| {
                if is_prompt_kind(&top.kind) {
                    top.value.clear();
                } else {
                    top.select.set_filter("", &props, usize::from(terminal.1));
                }
            });
            store.preview_theme_filter();
        }
        store.close_dialog();
        return DialogKeyOutcome::Closed;
    }

    // -- the focused input renderable ---------------------------------------
    if !is_static && !stroke.ctrl && !stroke.alt && stroke.key.chars().count() == 1 {
        if let Some(ch) = stroke.key.chars().next() {
            let text = if stroke.shift { ch.to_ascii_uppercase() } else { ch };
            store.update_dialog(|top| {
                if is_prompt_kind(&top.kind) {
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
            if is_prompt_kind(&top.kind) {
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
    // -- SURF-2: DialogMcp's toggle (dialog-mcp.tsx:145-153's action)
    // + DialogDebug's copy (dialog-debug.tsx:50-53) ---------------
    if let DialogKind::Mcp = entry.kind {
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        if keymap.stroke_triggers(&stroke, "dialog.mcp.toggle") {
            if let Some(server) = selected {
                store.mcp_toggle(&server);
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    // -- RE-02: DialogPlugins' toggle (plugins.tsx:116-138/201-209) --
    // enter (`plugins.toggle`, keybind table) on a REGISTERED row
    // flips that plugin's active state and the dialog STAYS OPEN
    // (v2's `onSelect` returns right after `toggle(entry)`); a
    // FAULTED row keeps the TUI-24 enter → error-details submit.
    if let DialogKind::Plugins = entry.kind {
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        if keymap.stroke_triggers(&stroke, "plugins.toggle") {
            if let Some(id) = selected.filter(|id| store.plugin_fault(id).is_none()) {
                store.plugin_toggle(&id);
                return DialogKeyOutcome::Consumed;
            }
        }
    }
    if let DialogKind::Debug = entry.kind {
        // `Keymap.createLayer`'s "Copy debug info" return binding
        // (dialog-debug.tsx:50-53): clipboard + the info toast.
        if stroke.key == "return" && !stroke.ctrl && !stroke.alt {
            let text = props
                .options
                .iter()
                .map(|option| option.title.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let _ = store.clipboard_write(&text);
            store.toast_show(crate::components::toast::ToastOptions {
                message: "Debug info copied to clipboard".to_owned(),
                variant: "info",
                ..crate::components::toast::ToastOptions::info("")
            });
            return DialogKeyOutcome::Consumed;
        }
    }
    if let DialogKind::QueuedPrompts = entry.kind {
        // The queued-prompts manager's action layer (v2
        // index.tsx:641-672): `queued_prompt.delete` (ctrl+d) removes
        // the row — the LAST removal closes the dialog
        // (`if (cancelled && last) dialog.clear()`); TUI-TD07's
        // `queued_prompt.undo` (ctrl+u) cancels the SELECTED row
        // server-side (`session.inbox.cancel`), refills the composer
        // (`appendPrompt` over the payload) and closes.
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        if keymap.stroke_triggers(&stroke, "queued_prompt.undo") {
            if let (Route::Session { session_id }, Some(id)) = (store.route(), selected.as_deref())
            {
                if let Some(payload) = store.queued_prompt_payload(&session_id, id) {
                    store.delete_queued_prompt(&session_id, id);
                    crate::components::command_palette::append_prompt_payload(
                        store,
                        &session_id,
                        &payload,
                    );
                    store.clear_dialogs();
                }
            }
            return DialogKeyOutcome::Consumed;
        }
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
    // -- Lane FINAL: the pending attempt's modal keys ------------------
    // `OAuthAuto`'s layer (dialog-integration.tsx:594-623): "o" opens
    // the authorization URL, "c" copies it (the confirmation-code
    // match falls back to the URL).
    if let DialogKind::IntegrationPending { .. } = entry.kind {
        if !stroke.ctrl && !stroke.alt && stroke.key.chars().count() == 1 {
            let url = store.integration_attempt().map(|attempt| attempt.url).unwrap_or_default();
            if stroke.key == "o" && !url.is_empty() {
                store.open_url(&url);
                return DialogKeyOutcome::Consumed;
            }
            if stroke.key == "c" && !url.is_empty() {
                if store.clipboard_write(&url).is_ok() {
                    store.toast_show(crate::components::toast::ToastOptions {
                        message: "Copied to clipboard".to_owned(),
                        variant: "info",
                        ..crate::components::toast::ToastOptions::info("")
                    });
                }
                return DialogKeyOutcome::Consumed;
            }
        }
        return DialogKeyOutcome::Consumed;
    }
    // -- Lane FINAL: DialogConfig's change bindings --------------------
    // footerHints' ←/→ (`bindings` left/right, dialog-config.tsx:
    // 405-418): cycle the moved-to setting's values.
    if let DialogKind::Settings = entry.kind {
        if stroke.key == "left" && !stroke.ctrl && !stroke.alt {
            if let Some(option) = entry.select.submit(&props) {
                store.setting_cycle(&option.value.clone(), -1);
            }
            return DialogKeyOutcome::Consumed;
        }
        if stroke.key == "right" && !stroke.ctrl && !stroke.alt {
            if let Some(option) = entry.select.submit(&props) {
                store.setting_cycle(&option.value.clone(), 1);
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    // -- Lane FINAL: the Sessions dialog's delete action ---------------
    // `session.delete` (dialog-session-list.tsx:255-279): two-press
    // ctrl+d — the first arms the row (the confirm title), the second
    // fires `session.remove`.
    if let DialogKind::ServerList { command: "session.list", .. } = entry.kind {
        if keymap.stroke_triggers(&stroke, "session.delete") {
            if let Some(option) = entry.select.submit(&props) {
                store.session_delete_pressed(&option.value.clone());
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    // -- Lane FINAL: the accounts view's delete action -----------------
    // `dialog.integration.delete` (dialog-integration.tsx:237-258):
    // two-press ctrl+d over a credential row.
    if let DialogKind::IntegrationAccounts { .. } = entry.kind {
        if keymap.stroke_triggers(&stroke, "dialog.integration.delete") {
            if let Some(option) = entry.select.submit(&props) {
                if option.value != "add" {
                    store.integration_account_delete(&option.value.clone());
                }
            }
            return DialogKeyOutcome::Consumed;
        }
    }

    // -- TUI-25: DialogAlert's return ("Confirm alert",
    // dialog-alert.tsx:18-27): onConfirm then dialog.clear. ------
    if let DialogKind::Alert { request_id, confirm_command, .. } = &entry.kind {
        if stroke.key == "return" && !stroke.ctrl && !stroke.alt {
            if let Some(id) = request_id {
                // v2's `done` resolves void; the settle-once guard —
                // clear the id BEFORE the close so the close hook
                // never double-resolves (api.tsx:318-325).
                store.enqueue_ui_dialog_resolution(*id, serde_json::Value::Null);
                store.update_dialog(|top| {
                    if let DialogKind::Alert { request_id, .. } = &mut top.kind {
                        *request_id = None;
                    }
                });
            }
            let command = confirm_command.clone();
            store.clear_dialogs();
            return match command {
                Some(command) => DialogKeyOutcome::CommandSubmitted(command),
                None => DialogKeyOutcome::Consumed,
            };
        }
        return DialogKeyOutcome::Consumed;
    }
    // -- TUI-25: DialogConfirm's layer (dialog-confirm.tsx:26-51):
    // return runs the ACTIVE pill, ←/→ swap it. --------------------
    if let DialogKind::Confirm {
        active_confirm, request_id, confirm_command, cancel_command, ..
    } = &entry.kind
    {
        if (stroke.key == "left" || stroke.key == "right") && !stroke.ctrl && !stroke.alt {
            store.confirm_toggle_active();
            return DialogKeyOutcome::Consumed;
        }
        if stroke.key == "return" && !stroke.ctrl && !stroke.alt {
            let confirm = *active_confirm;
            if let Some(id) = request_id {
                store.enqueue_ui_dialog_resolution(*id, serde_json::Value::Bool(confirm));
                store.update_dialog(|top| {
                    if let DialogKind::Confirm { request_id, .. } = &mut top.kind {
                        *request_id = None;
                    }
                });
            }
            let command = if confirm { confirm_command.clone() } else { cancel_command.clone() };
            store.clear_dialogs();
            return match command {
                Some(command) => DialogKeyOutcome::CommandSubmitted(command),
                None => DialogKeyOutcome::Consumed,
            };
        }
        return DialogKeyOutcome::Consumed;
    }
    // -- TUI-10: DialogImagePreview's ←/→ ("Previous image" /
    // "Next image", dialog-image-preview.tsx:38-41). ---------------
    if let DialogKind::ImagePreview = entry.kind {
        if stroke.key == "left" && !stroke.ctrl && !stroke.alt {
            store.image_preview_move(-1);
            return DialogKeyOutcome::Consumed;
        }
        if stroke.key == "right" && !stroke.ctrl && !stroke.alt {
            store.image_preview_move(1);
            return DialogKeyOutcome::Consumed;
        }
        return DialogKeyOutcome::Consumed;
    }
    // -- RE-13: the /btw Answer dialog's action layer (btw.tsx:105-
    // 125): `c` copies the answer, j/k/↑/↓ scroll the markdown
    // window, page/home/end the long jumps. -------------------------
    if let DialogKind::BtwAnswer { answer, .. } = &entry.kind {
        let max = btw_answer_scroll_max(store, answer, terminal);
        let (key, mods) = (stroke.key.as_str(), (stroke.ctrl, stroke.alt));
        match (key, mods) {
            ("up", (false, false)) | ("k", (false, false)) => {
                store.btw_scroll_by(-1, max);
                return DialogKeyOutcome::Consumed;
            }
            ("down", (false, false)) | ("j", (false, false)) => {
                store.btw_scroll_by(1, max);
                return DialogKeyOutcome::Consumed;
            }
            ("pageup", (false, false)) => {
                store.btw_scroll_by(-(btw_answer_window_rows(terminal) as i64), max);
                return DialogKeyOutcome::Consumed;
            }
            ("pagedown", (false, false)) => {
                store.btw_scroll_by(btw_answer_window_rows(terminal) as i64, max);
                return DialogKeyOutcome::Consumed;
            }
            ("home", (false, false)) => {
                store.btw_scroll_by(i64::MIN / 2, 0);
                return DialogKeyOutcome::Consumed;
            }
            ("end", (false, false)) => {
                store.btw_scroll_by(i64::MAX / 2, max);
                return DialogKeyOutcome::Consumed;
            }
            ("c", (false, false)) => {
                let text = answer.trim().to_owned();
                let _ = store.clipboard_write(&text);
                store.toast_show(crate::components::toast::ToastOptions {
                    message: "Copied to clipboard".to_owned(),
                    variant: "info",
                    ..crate::components::toast::ToastOptions::info("")
                });
                return DialogKeyOutcome::Consumed;
            }
            _ => return DialogKeyOutcome::Consumed,
        }
    }
    // -- TUI-24: DialogErrorDetails' action layer
    // (dialog-error-details.tsx:88-96 + the useKeyboard scroller). --
    if let DialogKind::ErrorDetails { error, context, .. } = &entry.kind {
        let max = error_details_scroll_max(store, error);
        let (key, mods) = (stroke.key.as_str(), (stroke.ctrl, stroke.alt));
        match (key, mods) {
            ("up", (false, false)) => {
                store.error_details_scroll_by(-1, max);
                return DialogKeyOutcome::Consumed;
            }
            ("down", (false, false)) => {
                store.error_details_scroll_by(1, max);
                return DialogKeyOutcome::Consumed;
            }
            ("pageup", (false, false)) => {
                store.error_details_scroll_by(-20, max);
                return DialogKeyOutcome::Consumed;
            }
            ("pagedown", (false, false)) => {
                store.error_details_scroll_by(20, max);
                return DialogKeyOutcome::Consumed;
            }
            ("home", (false, false)) => {
                store.error_details_scroll_by(i64::MIN / 2, 0);
                return DialogKeyOutcome::Consumed;
            }
            ("end", (false, false)) => {
                store.error_details_scroll_by(i64::MAX / 2, max);
                return DialogKeyOutcome::Consumed;
            }
            ("c", (false, false)) => {
                // `copy` — the errorDetails text onto the clipboard.
                let text = format!("{error}\n{context}");
                let _ = store.clipboard_write(text.trim());
                store.toast_show(crate::components::toast::ToastOptions {
                    message: "Copied to clipboard".to_owned(),
                    variant: "info",
                    ..crate::components::toast::ToastOptions::info("")
                });
                return DialogKeyOutcome::Consumed;
            }
            ("i", (false, false)) => {
                // `investigate` — navigate home with the error prompt
                // (errorDetails().prompt, util/error-details.ts).
                store.clear_dialogs();
                store.navigate(crate::store::Route::Home);
                let key = crate::app::HOME_PROMPT_KEY.to_owned();
                let mut composer = store.composer(&key);
                composer.set_text(&format!("Investigate the following error:\n\n{error}"));
                store.set_composer(&key, &composer);
                return DialogKeyOutcome::Consumed;
            }
            _ => return DialogKeyOutcome::Consumed,
        }
    }
    // -- TUI-9: DialogStash's action layer — `stash.delete` (ctrl+d,
    // the two-press remove with the confirm title flip). ------------
    if let DialogKind::Stash = entry.kind {
        let selected = entry.select.submit(&props).map(|option| option.value.clone());
        if keymap.stroke_triggers(&stroke, "stash.delete") {
            if let Some(value) = selected {
                if let Ok(index) = value.parse::<usize>() {
                    if let crate::store::DeleteStep::Confirmed = store.stash_delete_pressed(index) {
                        store.stash_remove(index);
                    }
                }
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    // -- TUI-TD22: DialogExperiments' ←/→ (the all-boolean toggle,
    // dialog-experiments.tsx:59-71). -------------------------------
    if let DialogKind::Experiments = entry.kind {
        if (stroke.key == "left" || stroke.key == "right") && !stroke.ctrl && !stroke.alt {
            if let Some(option) = entry.select.submit(&props) {
                let id = option.value.clone();
                store.experiment_toggle(&id);
            }
            return DialogKeyOutcome::Consumed;
        }
    }

    // -- TUI-5 (lane-tui2): DialogOpen's view bindings --------------
    // `right` on a project row swaps to the worktrees view
    // (`Show project worktrees`, dialog-open.tsx:384-406); the
    // worktrees view binds `left` to `back()` (:407-417) and ctrl+n
    // to the new-worktree prompt (DialogWorktreeName).
    if let DialogKind::Open = entry.kind {
        if stroke.key == "right" && !stroke.ctrl && !stroke.alt {
            if let Some(option) = entry.select.submit(&props) {
                if let Some(project_id) = option.value.strip_prefix("prj:") {
                    let name = match store.project_list() {
                        crate::store::ListState::Loaded(projects) => projects
                            .iter()
                            .find(|project| project.id == project_id)
                            .map(|project| {
                                crate::components::dialog_open::project_name(
                                    project.name.as_deref(),
                                    &project.canonical,
                                )
                            }),
                        _ => None,
                    };
                    store.open_menu_worktrees(
                        project_id,
                        &name.unwrap_or_else(|| "Project".to_owned()),
                    );
                }
            }
            return DialogKeyOutcome::Consumed;
        }
    }
    if let DialogKind::OpenWorktrees { .. } = &entry.kind {
        if stroke.key == "left" && !stroke.ctrl && !stroke.alt {
            store.open_menu_back();
            return DialogKeyOutcome::Consumed;
        }
        if stroke.ctrl && stroke.key == "n" {
            // `New worktree` seeds the name prompt (the worktree
            // state stays scoped to the view's project from the
            // right-arrow swap).
            store.replace_dialog(DialogKind::WorktreeName, DialogSize::Medium);
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
    // TUI-9: the stash dialog's `onMove` clears its armed row too
    // (dialog-stash.tsx:73-75).
    if matches!(entry.kind, DialogKind::Stash) {
        store.stash_delete_disarm();
    }
    // Lane FINAL: the same `onMove` contract for the sessions dialog's
    // `toDelete` (dialog-session-list.tsx:242) and the accounts view's
    // `deleting` (dialog-integration.tsx:173-176).
    if matches!(
        entry.kind,
        DialogKind::ServerList { command: "session.list", .. }
            | DialogKind::IntegrationAccounts { .. }
    ) {
        store.session_delete_disarm();
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
    // TUI-10/24: the custom families' sidecar — the preview state +
    // the error scroll (this path owns the store). F15: the pair
    // dialog's resource snapshot rides the same sidecar. RE-13: the
    // /btw answer's markdown rows + scroll.
    let sidecar = crate::components::dialog_solid::DialogSidecar {
        preview: store.image_preview_dialog(),
        error_scroll: store.error_details_scroll(),
        pair: store.pair_state(),
        btw_rows: btw_answer_rows(store, &entry.kind, terminal),
        btw_scroll: store.btw_scroll(),
    };
    let (rect, option_rows, button_hits) = render_overlay_with_options(
        base,
        &entry,
        &props,
        plugin.as_ref(),
        theme,
        terminal,
        &sidecar,
    );
    // TUI-10: paint the preview image's cells over the copied chrome
    // (the tree reserves the area; the pixels are the bridge's job —
    // blocks glyphs now, a graphics placement for kitty/sixel).
    if matches!(entry.kind, DialogKind::ImagePreview) {
        paint_preview_image(base, store, &entry, rect, theme, terminal);
    }
    // The backdrop's dismissal surface: mouseups outside this rect
    // close the dialog (`ui/dialog.tsx:34-63`).
    store.set_dialog_frame_rect(Some(rect));
    store.set_dialog_option_rows(option_rows);
    // The chrome buttons (auditA #6): the header close label + the
    // Help `ok` pill ride the same frame.
    store.set_dialog_button_hits(button_hits);
}

/// The overlay render's mouse surface bundle: the chrome rect, the
/// option-row map (flat index → absolute screen row) and the chrome
/// button hits (auditA #6).
pub type OverlayMouseSurface =
    ((usize, usize, usize, usize), Vec<Option<usize>>, Vec<crate::store::DialogButtonHit>);

/// [`render_overlay_with`] plus the option-row map (the mouse surface).
pub fn render_overlay_with_options(
    base: &mut Frame,
    entry: &DialogEntry,
    props: &crate::components::dialog_select::SelectProps,
    plugin: Option<&crate::store::PluginDialogContent>,
    theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
    sidecar: &crate::components::dialog_solid::DialogSidecar,
) -> OverlayMouseSurface {
    // Select dialogs record their option rows (absolute screen rows,
    // flat-index order); static content records none.
    let option_rows = if match &entry.kind {
        DialogKind::Update
        | DialogKind::CommandPalette
        | DialogKind::QueuedPrompts
        | DialogKind::ThemeList
        | DialogKind::Worktrees
        | DialogKind::WorktreeDeleteConfirm { .. }
        | DialogKind::ServerList { .. }
        | DialogKind::MessageActions { .. }
        | DialogKind::WorkingDirectory { .. }
        | DialogKind::ToolOutput { .. }
        | DialogKind::DiffSource
        | DialogKind::DiffHelp
        | DialogKind::Plugins
        | DialogKind::TabMenu { .. }
        | DialogKind::Mcp
        | DialogKind::McpError { .. }
        | DialogKind::Status
        | DialogKind::Variant
        | DialogKind::Debug
        | DialogKind::Timeline { .. }
        | DialogKind::Integration
        | DialogKind::IntegrationAccounts { .. }
        | DialogKind::IntegrationMethod { .. }
        | DialogKind::IntegrationPending { .. }
        | DialogKind::Settings
        | DialogKind::ForkPicker { .. }
        | DialogKind::SessionExport { .. }
        | DialogKind::ExportResult { .. }
        // TUI-8/9/TD22: the three new selects record rows too.
        | DialogKind::Skill
        | DialogKind::Stash
        | DialogKind::Experiments
        // TUI-5 (lane-tui2): the Open menu + its worktrees view.
        | DialogKind::Open
        | DialogKind::OpenWorktrees { .. } => true,

        // A plugin dialog records rows only when its content is a
        // select (rows-only dialogs render statically).
        DialogKind::Plugin { .. } => {
            plugin.is_some_and(crate::store::PluginDialogContent::is_select)
        }
        DialogKind::Help
        | DialogKind::SessionRename { .. }
        | DialogKind::WorktreeName
        | DialogKind::IntegrationKey { .. }
        | DialogKind::IntegrationCode { .. }
        // TUI-10/24/25: the custom-key families carry no option rows.
        | DialogKind::ImagePreview
        | DialogKind::ErrorDetails { .. }
        | DialogKind::Alert { .. }
        | DialogKind::Confirm { .. }
        // F15: the pair dialog's custom tree records none either.
        // RE-13: the /btw prompt input + Answer markdown dialog.
        | DialogKind::BtwPrompt
        | DialogKind::BtwAnswer { .. }
        | DialogKind::Pair => false,
    } {
        crate::components::dialog_select::option_dialog_rows(
            props,
            &entry.select,
            usize::from(terminal.1),
        )
    } else {
        Vec::new()
    };
    let (rect, button_hits) = crate::components::dialog_solid::render_overlay_solid(
        base, entry, props, plugin, theme, terminal, sidecar,
    );
    let (_box_x, box_y, _, _) = rect;
    let option_rows =
        option_rows.into_iter().map(|row| row.map(|content_row| box_y + 1 + content_row)).collect();
    (rect, option_rows, button_hits)
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
    let sidecar = crate::components::dialog_solid::DialogSidecar::default();
    // Lane M2 (retired): the dialog family renders through its
    // opentui-solid tree — the only renderer since the retirement
    // lane deleted the hand-rolled walk.
    crate::components::dialog_solid::render_overlay_solid(
        base, entry, props, plugin, theme, terminal, &sidecar,
    )
    .0
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

/// TUI-24: the error-details scroller's max offset — the error body
/// word-wrapped at the dialog's inner width minus the 20-row
/// scrollbox (`maxHeight={20}`, dialog-error-details.tsx:106-114).
fn error_details_scroll_max(store: &Store, error: &str) -> usize {
    let width = usize::from(store.terminal_size().0).max(1);
    let inner = width.min(60).saturating_sub(6).max(1);
    let lines = crate::wrap::wrap_label(error, inner).len();
    lines.saturating_sub(20)
}

/// RE-13: the `/btw` answer window's row budget (btw.tsx:101 —
/// `max(8, floor(height * 0.6))`).
fn btw_answer_window_rows(terminal: (u16, u16)) -> usize {
    (usize::from(terminal.1) * 3 / 5).max(8)
}

/// RE-13: the `/btw` answer window's scroll max — the markdown
/// render at the large dialog's inner width minus the window rows.
fn btw_answer_scroll_max(store: &Store, answer: &str, terminal: (u16, u16)) -> usize {
    let width = usize::from(terminal.0).max(1);
    let inner = width.min(crate::store::DialogSize::Large.width()).saturating_sub(6).max(1);
    let styles =
        crate::components::message::markdown::MarkdownStyles::from_theme(&store.theme().styles());
    let lines = crate::components::message::markdown::render_markdown(answer, inner, &styles);
    lines.len().saturating_sub(btw_answer_window_rows(terminal))
}

/// RE-13: the `/btw` answer's full markdown render — the styled rows
/// the Answer dialog's scroll window slices. Ratatui spans convert
/// to the dialog bridge's `StyledText` chunks (fg/bg RGB + the bold
/// bit).
fn btw_answer_rows(
    store: &Store,
    kind: &DialogKind,
    terminal: (u16, u16),
) -> Vec<opentui_core::styled_text::StyledText> {
    let DialogKind::BtwAnswer { answer, .. } = kind else { return Vec::new() };
    let width = usize::from(terminal.0).max(1);
    let inner = width.min(crate::store::DialogSize::Large.width()).saturating_sub(6).max(1);
    let styles =
        crate::components::message::markdown::MarkdownStyles::from_theme(&store.theme().styles());
    crate::components::message::markdown::render_markdown(answer, inner, &styles)
        .iter()
        .map(|line| {
            opentui_core::styled_text::StyledText::new(
                line.0
                    .iter()
                    .map(|span| {
                        let fg = match span.style.fg {
                            Some(ratatui::style::Color::Rgb(r, g, b)) => {
                                Some(opentui_core::Rgba::rgb(r, g, b))
                            }
                            _ => None,
                        };
                        let bg = match span.style.bg {
                            Some(ratatui::style::Color::Rgb(r, g, b)) => {
                                Some(opentui_core::Rgba::rgb(r, g, b))
                            }
                            _ => None,
                        };
                        let mut attributes = 0u32;
                        let modifiers = span.style.add_modifier;
                        if modifiers.contains(ratatui::style::Modifier::BOLD) {
                            attributes |= 1;
                        }
                        if modifiers.contains(ratatui::style::Modifier::ITALIC) {
                            attributes |= 2;
                        }
                        if modifiers.contains(ratatui::style::Modifier::UNDERLINED) {
                            attributes |= 4;
                        }
                        opentui_core::styled_text::TextChunk {
                            text: span.text.clone(),
                            fg,
                            bg,
                            attributes,
                            link: span.link.clone(),
                        }
                    })
                    .collect(),
            )
        })
        .collect()
}

/// TUI-10: paint the image preview's pixels over the copied chrome
/// tree (`<image fit="fit" protocol="auto">`, dialog-image-preview.
/// tsx:60-66). The tree reserved the area — the bridge fills it:
/// * a decodable payload on a DRAWABLE terminal → a graphics
///   placement (kitty/sixel) recorded onto the frame's image set;
/// * otherwise the BLOCKS renderer (`resolveImageRenderProtocol`'s
///   default) — aspect-fit, centered, one block glyph per cell.
fn paint_preview_image(
    base: &mut Frame,
    store: &Store,
    entry: &crate::store::DialogEntry,
    rect: (usize, usize, usize, usize),
    _theme: &crate::theme::ThemeState,
    terminal: (u16, u16),
) {
    let _ = entry;
    let (box_x, box_y, box_width, _box_height) = rect;
    let state = store.image_preview_dialog();
    let Some(item) = state.images.get(state.index) else { return };
    let Some(payload) = crate::components::composer::AttachmentPayload::from_data_uri(&item.uri)
    else {
        return;
    };
    let Some(bytes) = crate::clipboard::base64_decode(&payload.data) else { return };
    let data = crate::image::ImageData::Png(bytes);
    // The image area's geometry (the tree's `area` box): under the
    // chrome pad (1) + the header row (1) + the root gap (1), the
    // full inner width, `max(3, height - 8)` rows.
    let area_x = box_x + 2;
    let area_y = box_y + 3;
    let area_w = box_width.saturating_sub(4);
    let area_h = usize::from(terminal.1).saturating_sub(8).max(3);
    let protocol = store.graphics_protocol();
    if crate::image::drawable(protocol, &data) {
        let placement = crate::image::ImagePlacement {
            id: crate::image::image_id(&data),
            data,
            x: u16::try_from(area_x).unwrap_or(u16::MAX),
            y: u16::try_from(area_y).unwrap_or(u16::MAX),
            cols: u16::try_from(area_w).unwrap_or(u16::MAX),
            rows: u16::try_from(area_h).unwrap_or(u16::MAX),
        };
        let mut images = store.frame_images();
        images.push(placement);
        store.set_frame_images(images);
        return;
    }
    let crate::image::ImageData::Png(bytes) = &data else { return };
    let Some(image) = crate::image_blocks::decode_png(bytes) else { return };
    let (cols, rows) = crate::image_blocks::fit_cells(image.width, image.height, area_w, area_h);
    let cells = crate::image_blocks::blocks_cells(&image, cols, rows);
    let offset_x = area_x + area_w.saturating_sub(cols) / 2;
    let offset_y = area_y + area_h.saturating_sub(rows) / 2;
    for (line_index, line) in cells.iter().enumerate() {
        for (column, cell) in line.iter().enumerate() {
            if let Some(target) = base
                .cells
                .get_mut(offset_y + line_index)
                .and_then(|row| row.get_mut(offset_x + column))
            {
                target.ch = cell.glyph;
                target.fg = crate::render::CellColor::Rgb(cell.fg[0], cell.fg[1], cell.fg[2]);
                target.bg = crate::render::CellColor::Rgb(cell.bg[0], cell.bg[1], cell.bg[2]);
                target.attrs = 0;
            }
        }
    }
}

/// Convenience: render the full app frame — route content (home /
/// session skeletons) with the dialog overlay composited on top. This
/// is the frame source the keypress-replay harness compares.
#[must_use]
pub fn render_app(store: &Store, theme: &crate::theme::ThemeState, size: (u16, u16)) -> Frame {
    let styles = theme.styles();
    let mut frame = if store.diff_viewer_open() {
        // The `diff` plugin route (SURF-2, diff-viewer.tsx:193): the
        // viewer mounts FULLSCREEN over the app shell — body states,
        // scrolled patch column, tree pane.
        crate::components::diff_viewer_mount::render_diff_mount(
            store,
            &styles,
            usize::from(size.0),
            usize::from(size.1),
        )
    } else if store.stats_page_open() {
        // RE-14 — the `stats` plugin route (stats.tsx:156-178): the
        // page box paints edge-to-edge with the scrollbox-centered
        // poster (loading / error fallback states included).
        let phase = store.stats_page_state().phase;
        let poster_storage;
        let view = match &phase {
            crate::store::StatsPhase::Loading => {
                crate::components::stats_page_solid::StatsPhaseView::Loading
            }
            crate::store::StatsPhase::Failed(_) => {
                crate::components::stats_page_solid::StatsPhaseView::Failed
            }
            crate::store::StatsPhase::Loaded(info) => {
                // The poster model rebuilds per paint from the
                // immutable payload (the ref's createMemo over the
                // resource value). The calendar buckets by the LOCAL
                // day (the fetch already sends local_timezone_name).
                let timezone = crate::components::stats_page::local_timezone_name();
                poster_storage = crate::components::stats_page::stats_poster(info, size, &timezone);
                crate::components::stats_page_solid::StatsPhaseView::Loaded(&poster_storage)
            }
        };
        crate::components::stats_page_solid::render_stats_page_solid(view, &styles, size)
    } else {
        match store.route() {
            crate::store::Route::Home => crate::components::home::render_home(store, &styles, size),
            crate::store::Route::Session { session_id } => {
                crate::components::session::render_session(store, &session_id, &styles, size)
            }
        }
    };
    // G-07 — the devtools bar paints the bottom row BEFORE overlays
    // (dialogs render above it, ref's z-order); zero-op disabled.
    crate::components::devtools_solid::paint_devtools_bar_solid(
        &mut frame,
        store,
        &styles,
        (size.0, size.1),
    );
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
        // The update dialog's actions (`dialog-update.tsx`'s buttons):
        // skip dismisses + closes; confirm runs Update (the loop's
        // install spawn) or Restart (exit) per the live state.
        Some(DialogKind::Update) => {
            let action = value.to_owned();
            store.clear_dialogs();
            if action == "confirm" {
                match store.update_dialog_state() {
                    crate::components::update::UpdateDialogState::Available(version) => {
                        store.set_update_dialog(
                            crate::components::update::UpdateDialogState::Installing(
                                version.clone(),
                            ),
                        );
                        let _ = version;
                        // The loop intercepts this marker through its
                        // pending-command seam (a String payload rides
                        // the update dialog's state instead).
                        return crate::components::command_palette::Dispatch::ServerPending(
                            "update.install",
                        );
                    }
                    crate::components::update::UpdateDialogState::Installed(_) => {
                        return crate::components::command_palette::Dispatch::ServerPending(
                            "update.restart",
                        );
                    }
                    _ => {}
                }
            } else {
                store.update_dismiss();
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        // RD-17 — a palette setting row's activation (`onSelect`,
        // command-palette.tsx:42-44): `dialog.replace` with
        // DialogConfig opened at the setting — `current={settingID(
        // setting)}` seeds the row selection (`findIndex` clamped
        // to 0, dialog-config.tsx:329-332).
        Some(DialogKind::CommandPalette) if value.starts_with("setting:") => {
            let id = value.strip_prefix("setting:").unwrap_or(value);
            store.replace_dialog(DialogKind::Settings, DialogSize::Medium);
            let index = crate::components::dialog_config::settings_table()
                .into_iter()
                .position(|setting| setting.id() == id)
                .unwrap_or(0);
            store.update_dialog(|top| top.select.selected = index);
            return crate::components::command_palette::Dispatch::Handled;
        }
        Some(DialogKind::ThemeList) => {
            if !store.set_theme_by_name(command) {
                // ThemeErrorToast (auditB #9): an unresolvable theme
                // surfaces instead of silently keeping the current one.
                store.toast_show(crate::components::toast::ToastOptions {
                    title: Some("Failed to load theme".to_owned()),
                    message: format!("Theme \"{command}\" could not be resolved."),
                    variant: "error",
                    ..crate::components::toast::ToastOptions::info("")
                });
            }
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
        // TUI-5 (lane-tui2): DialogOpen's `onSelect`
        // (dialog-open.tsx:435-446): a session row navigates the
        // session route (the tab effect opens it); a project row
        // navigates home.
        Some(DialogKind::Open) => {
            let selection = value.to_owned();
            store.clear_dialogs();
            return crate::components::dialog_open::execute_open_selection(store, &selection);
        }
        // TUI-5: a worktrees-view row navigates home at the directory
        // (the port's home route keeps the process cwd — the row's
        // `dir:` value is intentionally not consumed here).
        Some(DialogKind::OpenWorktrees { .. }) => {
            let _ = value;
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The diff source select's `onSelect` (`props.onSwitchSource`,
        // diff-viewer.tsx:748-751): close + switch (a refetch rides
        // the store's diff sink).
        Some(DialogKind::DiffSource) => {
            let mode = crate::store::DiffSource::parse(value);
            store.clear_dialogs();
            store.switch_diff_source(mode);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The rows-only surfaces (diff shortcuts, status, pair,
        // debug): informational submits just close. The plugin
        // manager's rows open the ERROR DETAILS on a failed plugin
        // (TUI-24 — v2's `enter view error` footer over a faulted
        // row, plugins.tsx:250-270); RE-02 routes non-faulted rows
        // through `plugins.toggle` at the key layer (they never
        // reach this submit), so only faulted rows land here.
        Some(DialogKind::Plugins) => {
            if let Some(error) = store.plugin_fault(value) {
                let title = "TUI plugin error".to_owned();
                let context = format!("Plugin: {value}\nStatus: failed");
                store.clear_dialogs();
                store.open_error_details(&title, value, &error, &context, "", true);
            } else {
                store.clear_dialogs();
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        Some(
            DialogKind::DiffHelp
            | DialogKind::Status
            | DialogKind::Pair
            | DialogKind::Debug
            | DialogKind::ExportResult { .. },
        ) => {
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The tab context menu's actions (`TabContextMenu`'s `run`
        // closures, session-tabs.tsx:391-417 — lane KM): New session
        // (the add), Rename (the rename dialog), Copy session ID (the
        // clipboard + toast), Close, and the recently-closed reopen
        // rows. Status rows (no `run`) just close.
        Some(DialogKind::TabMenu { session_id, .. }) => {
            let session_id = session_id.clone();
            store.clear_dialogs();
            if let Some(id) = value.strip_prefix("tab:reopen:") {
                let _ = store.reopen_tab(Some(id));
            } else if value == "tab:new" {
                store.navigate(crate::store::Route::Home);
            } else if value == "tab:rename" {
                if let Some(session_id) = session_id {
                    store.replace_dialog(
                        DialogKind::SessionRename { session_id },
                        DialogSize::Medium,
                    );
                }
            } else if value == "tab:copy" {
                if let Some(session_id) = session_id.clone().or_else(|| match store.route() {
                    crate::store::Route::Session { session_id } => Some(session_id),
                    crate::store::Route::Home => None,
                }) {
                    if store.clipboard_write(&session_id).is_ok() {
                        store.toast_show(crate::components::toast::ToastOptions {
                            message: "Session ID copied to clipboard".to_owned(),
                            variant: "info",
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    }
                }
            } else if value == "tab:close" {
                if let Some(session_id) = session_id {
                    store.close_tab(Some(&session_id));
                }
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `DialogMcp`'s `select` (dialog-mcp.tsx:100-109): a failed /
        // needs-auth server opens its error details.
        Some(DialogKind::Mcp) => {
            let server = store.mcp_servers().into_iter().find(|server| server.name == value);
            if let Some(server) = server {
                if server.error.is_some() {
                    store.replace_dialog(
                        DialogKind::McpError { name: server.name },
                        DialogSize::Medium,
                    );
                    return crate::components::command_palette::Dispatch::Handled;
                }
            }
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The error details' back (dialog-mcp.tsx:171-173): return to
        // the server list at medium size.
        Some(DialogKind::McpError { .. }) => {
            store.replace_dialog(DialogKind::Mcp, DialogSize::Medium);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `DialogVariant`'s onSelect (dialog-variant.tsx:14-28):
        // clear + commit ("default" clears the variant).
        Some(DialogKind::Variant) => {
            let variant = value.to_owned();
            store.clear_dialogs();
            store.set_current_model_variant(Some(&variant));
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `DialogTimeline`'s onSelect (dialog-timeline.tsx:30-34):
        // replace with the message actions over that user message.
        Some(DialogKind::Timeline { session_id }) => {
            let session_id = session_id.clone();
            let message_id = value.to_owned();
            store.replace_dialog(
                DialogKind::MessageActions { session_id, message_id },
                DialogSize::Medium,
            );
            return crate::components::command_palette::Dispatch::Handled;
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
        // `onSelect` → `mutatePending("steer", …)`): TUI-TD07 — the
        // server-side PATCH (`session.inbox.update
        // {delivery: "steer"}`), NOT a local re-send; the row keeps
        // its place until the turn admits it.
        Some(DialogKind::QueuedPrompts) => {
            if let Route::Session { session_id } = store.route() {
                if store.queued_prompts(&session_id).iter().any(|item| item.id == value) {
                    store.steer_queued_prompt(&session_id, value);
                    store.clear_dialogs();
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
                    // v2's dialog-message.tsx: revert.stage + the
                    // projected prompt (`setPrompt(projectedPromptInput
                    // (value))` — dialog-fork.tsx's codec shared here).
                    store
                        .session_action_sink_revert_stage(session_id.as_str(), message_id.as_str());
                    if !text.is_empty() {
                        let mut composer = store.composer(session_id.as_str());
                        composer.set_text(&text);
                        store.set_composer(session_id.as_str(), &composer);
                    }
                }
                "message.copy" => {
                    let _ = store.clipboard_write(&text);
                }
                "session.fork" => {
                    // DialogFork with messageID forks immediately
                    // (dialog-fork.tsx:38-40 — no picker).
                    store.session_fork_submit(session_id.as_str(), Some(message_id.as_str()));
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
        // -- Lane FINAL: the connect flow's selects --------------------
        // `DialogIntegration`'s `onSelect` (dialog-integration.tsx:
        // 122-125): credentials → manage view; else method choice.
        Some(DialogKind::Integration) => {
            store.integration_select(value);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `manageConnections`' rows: "Add account" reopens the method
        // choice; a credential row activates (status rows re-run the
        // method flow).
        Some(DialogKind::IntegrationAccounts { integration_id }) => {
            if value == "add" {
                store.integration_open_methods(integration_id.as_str());
            } else {
                let row = store.integration_row(integration_id.as_str());
                match row.as_ref().map(|row| row.accounts.iter().find(|a| a.id == value)) {
                    Some(Some(account)) if account.sign_in_required => {
                        store.integration_open_methods(integration_id.as_str());
                    }
                    Some(Some(_)) => {
                        store.integration_account_activate(value);
                        store.clear_dialogs();
                    }
                    _ => store.clear_dialogs(),
                }
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `selectMethod`'s rows: open the picked method.
        Some(DialogKind::IntegrationMethod { integration_id }) => {
            if let Some(method) = store
                .integration_row(integration_id.as_str())
                .and_then(|row| row.methods.into_iter().find(|method| method.id == value))
            {
                store.integration_method_open(integration_id.as_str(), &method);
            } else {
                store.clear_dialogs();
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `DialogConfig`'s `onSelect` (dialog-config.tsx:388):
        // `change(1)` — cycle the row's values.
        Some(DialogKind::Settings) => {
            store.setting_cycle(value, 1);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // `DialogFork`'s rows (dialog-fork.tsx:69-82): "" = the full
        // session; a message id forks before it.
        Some(DialogKind::ForkPicker { session_id }) => {
            let before = if value.is_empty() { None } else { Some(value.to_owned()) };
            store.clear_dialogs();
            store.session_fork_submit(session_id.as_str(), before.as_deref());
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The export destinations: "format:destination".
        Some(DialogKind::SessionExport { session_id }) => {
            if let Some((format, destination)) = value.split_once(':') {
                store.session_export_submit(session_id.as_str(), format, destination);
            } else {
                store.clear_dialogs();
            }
            return crate::components::command_palette::Dispatch::Handled;
        }
        // The informational pending view: submits are inert.
        Some(DialogKind::IntegrationPending { .. }) => {
            return crate::components::command_palette::Dispatch::Handled;
        }
        // -- TUI-8: DialogSkill's `onSelect` (dialog-skill.tsx:36-41
        // + prompt/index.tsx:583-605): insert `@{skill} ` at the
        // composer's caret and close.
        Some(DialogKind::Skill) => {
            let skill = value.to_owned();
            store.clear_dialogs();
            let key = match store.route() {
                crate::store::Route::Session { session_id } => session_id,
                crate::store::Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
            };
            let mut composer = store.composer(&key);
            composer.insert_text(&format!("@{skill} "));
            store.set_composer(&key, &composer);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // -- TUI-9: DialogStash's `onSelect` (dialog-stash.tsx:77-87):
        // remove the entry, restore it into the focused composer
        // (`PromptRef.set`), close.
        Some(DialogKind::Stash) => {
            let index: usize = value.parse().unwrap_or(usize::MAX);
            if let Some(entry) = store.stash_remove(index) {
                let key = match store.route() {
                    crate::store::Route::Session { session_id } => session_id,
                    crate::store::Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
                };
                let mut composer = store.composer(&key);
                composer.set(entry.prompt);
                store.set_composer(&key, &composer);
            }
            store.clear_dialogs();
            return crate::components::command_palette::Dispatch::Handled;
        }
        // -- TUI-TD22: DialogExperiments' `onSelect`
        // (dialog-experiments.tsx:52): toggle the selected experiment.
        Some(DialogKind::Experiments) => {
            store.experiment_toggle(value);
            return crate::components::command_palette::Dispatch::Handled;
        }
        // A sessions-list submit navigates (the shared session.list
        // routing — including open.menu's large mount).
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
    use crate::components::dialog_select::DialogStyles;
    use crate::reactive::Runtime;
    use crate::render::CellColor;
    use crate::store::{DialogSize, PairState};
    use ratatui::style::Color;

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

    /// RE-02: the plugins dialog's enter is the ref's toggle —
    /// `plugins.toggle` (plugins.tsx:201-209's `onSelect` →
    /// `toggle(entry)`) flips the selected registered plugin's
    /// active state and the dialog STAYS OPEN (never just closes);
    /// a FAULTED row keeps the TUI-24 enter → error-details path.
    #[test]
    fn plugins_dialog_enter_toggles_and_stays_open() {
        let store = Store::new(Runtime::new());
        store.set_plugin_commands(vec![crate::store::PluginCommandEntry {
            command: "hw.hello".into(),
            title: "Say hello".into(),
            plugin_id: "helloworld".into(),
        }]);
        store.replace_dialog(DialogKind::Plugins, DialogSize::Medium);
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();

        // enter #1: disable — consumed, the dialog stays open.
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert_eq!(outcome, DialogKeyOutcome::Consumed);
        assert_eq!(store.dialogs().len(), 1, "the toggle never closes the dialog");
        assert!(!store.plugin_active("helloworld"), "enter disabled the plugin");

        // enter #2: re-enable — round trip through the key layer.
        handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert!(store.plugin_active("helloworld"));
        assert_eq!(store.dialogs().len(), 1, "still open after the round trip");

        // A FAULTED row keeps the TUI-24 behavior: enter opens the
        // error details (the plugins dialog is replaced).
        let mut faults = std::collections::HashMap::new();
        faults.insert("helloworld".to_owned(), "load failed".to_owned());
        store.set_plugin_faults(faults);
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("return").unwrap(),
            (80, 24),
        );
        assert_eq!(
            outcome,
            DialogKeyOutcome::Submitted("helloworld".to_owned()),
            "faulted rows submit into the error details"
        );
        let mut dispatcher = store_dispatcher();
        let dispatch = execute_submission(&store, &mut dispatcher, "helloworld");
        assert_eq!(dispatch, crate::components::command_palette::Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::ErrorDetails { .. }));
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

    /// F15 residual: the pair dialog's ≥96 TERMINAL-column horizontal
    /// split (dialog-pair.tsx:39 `dimensions().width >= 96`, :45 the
    /// fixed 29-cell text column). Below the gate the Large box's
    /// 84-cell walk keeps the whole intro on ONE row; past it the
    /// column is 29 cells — the intro wraps and the full link CANNOT
    /// render (it clips at 29). Rendering-field test: same store,
    /// same dialog, only the terminal width differs.
    #[test]
    fn pair_dialog_split_gate_is_the_terminal_width() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.set_pair_state(PairState::Loaded {
            links: vec!["http://127.0.0.1:4096/auth/connect/pair-code-123".to_owned()],
            localhost: Some("http://localhost:4096/auth/connect/pair-code-123".to_owned()),
            minutes: 10,
            loopback: false,
        });
        store.replace_dialog(DialogKind::Pair, DialogSize::Large);

        // 95 columns: stacked — the 65-char intro fits one 84-cell row.
        let mut frame = blank_base(95, 40);
        render_overlay(&mut frame, &store, &theme, (95, 40));
        let rows = frame.text_grid();
        assert!(
            rows.iter().any(|row| row.contains("expire in 10 minutes")),
            "below the gate the full intro is one row"
        );
        assert!(
            rows.iter().any(|row| row.contains("http://localhost:4096/auth/connect/pair-code-123")),
            "below the gate the full link renders"
        );

        // 96 columns: the split — the text column is 29 cells.
        let mut frame = blank_base(96, 40);
        render_overlay(&mut frame, &store, &theme, (96, 40));
        let rows = frame.text_grid();
        assert!(
            !rows.iter().any(|row| row.contains("expire in 10 minutes")),
            "past the gate the intro cannot fit one row"
        );
        assert!(
            rows.iter().any(|row| row.contains("expire in 10")),
            "the wrapped intro's middle row"
        );
        // f15-a: links WORD-WRAP inside the 29-cell column (ref
        // TextRenderable default wrapMode "word") — the unbroken URL
        // token hard-breaks across continuation rows instead of
        // clipping, and the fragments reassemble to the whole link.
        let joined = rows.join("");
        assert!(
            joined.contains("pair-code-123"),
            "the link wraps (fragments recoverable) inside the 29-cell column"
        );
        assert!(rows.iter().any(|row| row.contains("This device")), "the blocks survive");
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

// -- lane TAIL (auditB #8): the destructive confirm rows ---------

/// The worktree delete confirm's SELECTED row paints the
/// `action.destructive.focused` pair (not the primary selection).
#[test]
fn destructive_confirm_row_uses_the_destructive_pair() {
    let store = Store::new(crate::reactive::Runtime::new());
    store.open_dialog(
        crate::store::DialogKind::WorktreeDeleteConfirm { directory: "/w/dying".to_owned() },
        crate::store::DialogSize::Medium,
    );
    let theme = crate::theme::ThemeState::v2_dark();
    let frame = render_app(&store, &theme, (80, 24));
    let styles = theme.styles();
    // The selected ("yes") row: its cells carry the destructive bg.
    let found = frame.cells.iter().any(|row| {
        row.iter()
            .any(|cell| cell.bg == crate::render::CellColor::from(styles.destructive_focused_bg))
    });
    assert!(found, "the destructive selected pair painted");
}

// -- RE-13: the /btw Answer dialog --------------------------------------

#[cfg(test)]
mod btw_tests {
    use super::*;
    use crate::reactive::Runtime;

    /// One frame row as plain text (grid assertions).
    fn btw_frame_row(frame: &Frame, y: usize) -> String {
        frame.cells[y].iter().map(|cell| cell.ch).collect()
    }

    #[test]
    fn btw_answer_dialog_renders_markdown_and_copy_footer() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("S1".to_owned()));
        store.btw_ask("s1", "what did we do?");
        store.btw_answer("what did we do?", "You **ported** /btw.\n\n- one-shot answer");
        let mut frame = blank_base(100, 30);
        render_overlay(&mut frame, &store, &theme, (100, 30));
        // The header carries the /btw label + the collapsed question
        // (btw.tsx:130-138) and the action footer the copy hint
        // (btw.tsx:161-169).
        let rows: Vec<String> = (0..frame.height).map(|y| btw_frame_row(&frame, y)).collect();
        let header = rows.iter().find(|row| row.contains("/btw")).expect("the /btw header");
        assert!(header.contains("what did we do?"), "the question rides the header: {header}");
        assert!(header.contains("esc"), "the escape hint");
        assert!(rows.iter().any(|row| row.contains("ported")), "the markdown answer text");
        assert!(rows.iter().any(|row| row.contains("copy")), "the copy action");
    }

    #[test]
    fn btw_answer_dialog_scrolls_and_copies() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("S1".to_owned()));
        let answer = (0..40).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n\n");
        store.btw_ask("s1", "q");
        store.btw_answer("q", &answer);
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        let clock = crate::keymap::ManualClock::default();
        let scroll = store.btw_scroll();
        assert_eq!(scroll, 0, "the window starts at the top");
        // j scrolls one row (btw.tsx:119-120).
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("j").unwrap(),
            (100, 30),
        );
        assert_eq!(outcome, DialogKeyOutcome::Consumed);
        assert_eq!(store.btw_scroll(), 1);
        // end clamps to the max (btw.tsx:124).
        handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("end").unwrap(),
            (100, 30),
        );
        assert!(store.btw_scroll() > 1, "end jumps to the bottom");
        // c copies the answer (btw.tsx:105-110) — the toast confirms.
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("c").unwrap(),
            (100, 30),
        );
        assert_eq!(outcome, DialogKeyOutcome::Consumed);
        let toast = store.toasts().current().cloned().expect("the copy toast");
        assert_eq!(toast.message, "Copied to clipboard");
        // escape closes (the modal layer).
        let outcome = handle_dialog_key(
            &store,
            &mut keymap,
            &clock,
            crate::keymap::KeyStroke::parse("escape").unwrap(),
            (100, 30),
        );
        assert_eq!(outcome, DialogKeyOutcome::Closed);
        assert!(store.dialogs().is_empty());
    }

    fn blank_base(width: u16, height: u16) -> Frame {
        let cell = crate::render::Cell {
            ch: ' ',
            attrs: 0,
            fg: crate::render::CellColor::Rgb(255, 255, 255),
            bg: crate::render::CellColor::Rgb(10, 10, 10),
        };
        Frame {
            cells: vec![vec![cell; usize::from(width)]; usize::from(height)],
            width: usize::from(width),
            height: usize::from(height),
        }
    }
}
