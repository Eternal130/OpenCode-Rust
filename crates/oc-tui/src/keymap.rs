//! Keymap engine — `@opentui/keymap` semantics for the Rust port:
//!
//! * **Layers/modes**: bindings live in named layers; a mode stack
//!   (`push_mode`/`pop_mode`, mirroring v2 `Keymap.mode.push`) resolves
//!   top-down, falling back to the base layer.
//! * **Leader sequences**: `ctrl+x` with a 2000 ms timeout; a pending
//!   sequence displays as `"ctrl+x n"` style strings and is consumed by
//!   the next stroke, expired by the clock, or cleared by `escape`
//!   (`registerEscapeClearsPendingSequence`).
//! * **Command registry**: the full v2 default binding table
//!   (`config/v1/keybind.ts` `Definitions` + `CommandMap`) as data — id,
//!   dot-command, defaults, description, palette visibility and group.
//! * **Lookup API**: [`Keymap::lookup`] returns the command (+args) or
//!   the pending display; escape/interrupt are special-cased exactly like
//!   the reference (escape clears a pending sequence first, dispatches
//!   `session.interrupt` only when nothing is pending).
//!
//! Foundation approximation: which layer a command activates in is
//! encoded per entry (`base` for the app-global table from tui-map §4;
//! `input`/`dialog`/`diff`/`autocomplete` for scoped layers). Wave 7.2
//! re-assigns as the real dialogs/composer land.

use std::cell::Cell;
use std::collections::HashMap;

/// Leader key (`LeaderDefault`) and timeout (`registerTimedLeader`
/// default 2000 ms).
pub const LEADER_DEFAULT: &str = "ctrl+x";
pub const LEADER_TIMEOUT_MS: u64 = 2000;

/// One key stroke: key name plus modifiers. Key names use the opentui
/// vocabulary (`return`, `escape`, `up`, `f2`, single letters).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyStroke {
    #[must_use]
    pub fn new(key: &str) -> Self {
        Self { key: key.to_owned(), ctrl: false, shift: false, alt: false }
    }

    #[must_use]
    pub fn ctrl(key: &str) -> Self {
        Self { ctrl: true, ..Self::new(key) }
    }

    #[must_use]
    pub fn alt(key: &str) -> Self {
        Self { alt: true, ..Self::new(key) }
    }

    #[must_use]
    pub fn shift(key: &str) -> Self {
        Self { shift: true, ..Self::new(key) }
    }

    /// Parse one stroke ("ctrl+x", "alt+down", "E", "return").
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut stroke = Self::new("");
        let tokens: Vec<&str> = spec.split('+').collect();
        for (index, token) in tokens.iter().enumerate() {
            let last = index + 1 == tokens.len();
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            if !last {
                match token.to_ascii_lowercase().as_str() {
                    "ctrl" => stroke.ctrl = true,
                    "shift" => stroke.shift = true,
                    "alt" | "meta" => stroke.alt = true,
                    // Recognized but unused by the default table.
                    "super" | "hyper" => {}
                    _ => return None,
                }
                continue;
            }
            if token.chars().count() == 1 {
                let ch = token.chars().next().expect("one char");
                if ch.is_ascii_uppercase() {
                    stroke.key = ch.to_ascii_lowercase().to_string();
                    stroke.shift = true;
                } else {
                    stroke.key = ch.to_string();
                }
            } else {
                stroke.key = token.to_ascii_lowercase();
            }
        }
        if stroke.key.is_empty() {
            None
        } else {
            Some(stroke)
        }
    }

    /// Display form (`stringifyKeyStroke`-compatible).
    #[must_use]
    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("ctrl".into());
        }
        if self.alt {
            parts.push("alt".into());
        }
        if self.shift {
            parts.push("shift".into());
        }
        parts.push(self.key.clone());
        parts.join("+")
    }
}

/// A binding chord: a sequence of strokes (`<leader>q` ⇒
/// `[ctrl+x, q]`; `>` also separates sequences).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub strokes: Vec<KeyStroke>,
}

impl Chord {
    /// Parse a binding spec: comma-separated alternatives are NOT handled
    /// here (the table splits them); one alternative per call. `<leader>`
    /// expands to the leader stroke and `>` separates sequence steps.
    #[must_use]
    pub fn parse(spec: &str, leader: &KeyStroke) -> Option<Self> {
        let segments: Vec<&str> =
            if spec.contains('>') { spec.split('>').collect() } else { vec![spec] };
        let mut strokes = Vec::new();
        for segment in segments {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            if segment.trim_matches(['<', '>']) == "leader" {
                strokes.push(leader.clone());
            } else {
                strokes.push(KeyStroke::parse(segment)?);
            }
        }
        if strokes.is_empty() {
            None
        } else {
            Some(Self { strokes })
        }
    }

    #[must_use]
    pub fn display(&self) -> String {
        self.strokes.iter().map(KeyStroke::display).collect::<Vec<_>>().join(" ")
    }
}

/// Command palette group (`KeymapCommand.group` / `category`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGroup {
    Session,
    Agent,
    Tabs,
    Theme,
    Editor,
    Help,
    View,
    Prompt,
    System,
}

impl CommandGroup {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Session => "Session",
            Self::Agent => "Agent",
            Self::Tabs => "Tabs",
            Self::Theme => "Theme",
            Self::Editor => "Editor",
            Self::Help => "Help",
            Self::View => "View",
            Self::Prompt => "Prompt",
            Self::System => "System",
        }
    }
}

/// One registry entry: the v2 default table as data.
#[derive(Debug, Clone)]
pub struct CommandDef {
    /// `Definitions` key in `config/v1/keybind.ts`.
    pub bind_name: &'static str,
    /// Runtime dot command id (`CommandMap`).
    pub command: &'static str,
    /// Default binding specs (comma-separated alternatives inside).
    pub defaults: &'static str,
    pub description: &'static str,
    /// Layer the binding activates in.
    pub layer: &'static str,
    pub palette: bool,
    pub group: Option<CommandGroup>,
}

/// The FULL v2 default binding table (`config/v1/keybind.ts`,
/// `Definitions` + `CommandMap`; "none" ⇒ unbound). Palette/group flags
/// follow the reference's palette-visible app commands (tui-map §4);
/// scoped dialog/input commands stay out of the palette.
#[must_use]
pub fn default_table() -> &'static [CommandDef] {
    DEFAULT_TABLE
}

macro_rules! group_of {
    () => {
        None
    };
    ($g:ident) => {
        Some(CommandGroup::$g)
    };
}
macro_rules! palette_of {
    () => {
        false
    };
    ($g:ident) => {
        true
    };
}

macro_rules! defs {
    ($( $bind:literal => $cmd:literal, $default:literal, $desc:literal, $layer:literal $(, palette $group:ident)? $(,)? ; )+) => {
        &[ $( CommandDef {
            bind_name: $bind,
            command: $cmd,
            defaults: $default,
            description: $desc,
            layer: $layer,
            palette: palette_of!($($group)?),
            group: group_of!($($group)?),
        } ),+ ]
    };
}

static DEFAULT_TABLE: &[CommandDef] = defs! {
    "leader" => "leader", "ctrl+x", "Leader key for keybind combinations", "base";
    "app_exit" => "app.exit", "ctrl+c,ctrl+d,<leader>q", "Exit the application", "base", palette System;
    "app_debug" => "app.debug", "none", "Toggle debug panel", "base";
    "app_console" => "app.console", "none", "Toggle console", "base";
    "app_heap_snapshot" => "app.heap_snapshot", "none", "Write heap snapshot", "base";
    "app_toggle_animations" => "app.toggle.animations", "none", "Toggle animations", "base";
    "app_toggle_file_context" => "app.toggle.file_context", "none", "Toggle file context", "base";
    "app_toggle_diffwrap" => "app.toggle.diffwrap", "none", "Toggle diff wrapping", "base";
    "app_toggle_paste_summary" => "app.toggle.paste_summary", "none", "Toggle paste summary", "base";
    "command_list" => "command.palette.show", "ctrl+p", "List available commands", "base", palette Help;
    "help_show" => "help.show", "none", "Open help dialog", "base", palette Help;
    "docs_open" => "docs.open", "none", "Open documentation", "base", palette Help;
    "diff_open" => "diff.open", "none", "Open diff viewer", "base", palette View;
    "diff_close" => "diff.close", "escape,q", "Close diff viewer", "diff";
    "diff_toggle" => "diff.toggle", "enter,space", "Toggle diff viewer item", "diff";
    "diff_expand" => "diff.expand", "right", "Expand diff viewer item", "diff";
    "diff_expand_all" => "diff.expand_all", "E", "Expand all diff viewer folders", "diff";
    "diff_collapse" => "diff.collapse", "left", "Collapse diff viewer item", "diff";
    "diff_switch_focus" => "diff.switch_focus", "tab", "Switch diff viewer focus", "diff";
    "diff_next_hunk" => "diff.next_hunk", "]", "Jump to next diff hunk", "diff";
    "diff_previous_hunk" => "diff.previous_hunk", "[", "Jump to previous diff hunk", "diff";
    "diff_next_file" => "diff.next_file", "n", "Jump to next diff file", "diff";
    "diff_previous_file" => "diff.previous_file", "p", "Jump to previous diff file", "diff";
    "diff_toggle_file_tree" => "diff.toggle_file_tree", "b", "Toggle diff viewer file tree", "diff";
    "diff_single_patch" => "diff.single_patch", "s", "Toggle single patch view", "diff";
    "diff_switch_source" => "diff.switch_source", "d", "Switch diff viewer source", "diff";
    "diff_toggle_view" => "diff.toggle_view", "v", "Toggle diff viewer split or unified view", "diff";
    "diff_help" => "diff.help", "?", "Show more diff viewer shortcuts", "diff";
    "editor_open" => "prompt.editor", "<leader>e", "Open external editor", "base", palette Editor;
    "theme_list" => "theme.switch", "<leader>t", "List available themes", "base", palette Theme;
    "theme_switch_mode" => "theme.switch_mode", "none", "Switch between light and dark theme mode", "base", palette Theme;
    "theme_mode_lock" => "theme.mode.lock", "none", "Lock or unlock theme mode", "base", palette Theme;
    "sidebar_toggle" => "session.sidebar.toggle", "<leader>b", "Toggle sidebar", "base", palette View;
    "scrollbar_toggle" => "session.toggle.scrollbar", "none", "Toggle session scrollbar", "base";
    "status_view" => "opencode.status", "<leader>s", "View status", "base", palette View;
    "debug_view" => "opencode.debug", "none", "View debug info", "base";
    "session_export" => "session.export", "<leader>x", "Export session to editor", "base", palette Session;
    "session_copy" => "session.copy", "none", "Copy session transcript", "base", palette Session;
    "session_move" => "session.move", "none", "Move session", "base", palette Session;
    "session_new" => "session.new", "<leader>n", "Create a new session", "base", palette Session;
    "session_list" => "session.list", "<leader>l", "List all sessions", "base", palette Session;
    "open_menu" => "open.menu", "ctrl+o", "Open recent sessions and projects", "base", palette Session;
    "session_tab_next" => "session.tab.next", "ctrl+tab,alt+down", "Switch to next open tab", "base";
    "session_tab_previous" => "session.tab.previous", "ctrl+shift+tab,alt+up", "Switch to previous open tab", "base";
    "session_tab_next_unread" => "session.tab.next_unread", "alt+shift+down", "Switch to next unread tab", "base";
    "session_tab_previous_unread" => "session.tab.previous_unread", "alt+shift+up", "Switch to previous unread tab", "base";
    "session_tab_close" => "session.tab.close", "<leader>w", "Close current tab", "base";
    "session_tab_reopen" => "session.tab.reopen", "ctrl+shift+t", "Reopen last closed tab", "base";
    "session_timeline" => "session.timeline", "<leader>g", "Show session timeline", "base", palette Session;
    "session_fork" => "session.fork", "none", "Fork session from message", "base", palette Session;
    "session_rename" => "session.rename", "ctrl+r", "Rename session", "base", palette Session;
    "session_delete" => "session.delete", "ctrl+d", "Delete session", "base", palette Session;
    "session_share" => "session.share", "none", "Share current session", "base", palette Session;
    "session_unshare" => "session.unshare", "none", "Unshare current session", "base", palette Session;
    "session_interrupt" => "session.interrupt", "escape", "Interrupt current session", "base";
    "session_background" => "session.background", "ctrl+b", "Background blocking session tools", "base";
    "session_compact" => "session.compact", "<leader>c", "Compact the session", "base", palette Session;
    "session_queued_prompts" => "session.queued_prompts", "<leader>q", "View queued prompts", "base", palette Prompt;
    "queued_prompt_delete" => "queued_prompt.delete", "ctrl+d", "Delete queued prompt", "dialog";
    "session_child_first" => "session.child.first", "down", "Toggle subagent picker", "base";
    "session_parent" => "session.parent", "up", "Go to parent session", "base";
    "session_pin_toggle" => "session.pin.toggle", "ctrl+f", "Pin or unpin session in the session list", "dialog";
    "session_quick_switch_1" => "session.quick_switch.1", "<leader>1", "Switch to session in quick slot 1", "base";
    "session_quick_switch_2" => "session.quick_switch.2", "<leader>2", "Switch to session in quick slot 2", "base";
    "session_quick_switch_3" => "session.quick_switch.3", "<leader>3", "Switch to session in quick slot 3", "base";
    "session_quick_switch_4" => "session.quick_switch.4", "<leader>4", "Switch to session in quick slot 4", "base";
    "session_quick_switch_5" => "session.quick_switch.5", "<leader>5", "Switch to session in quick slot 5", "base";
    "session_quick_switch_6" => "session.quick_switch.6", "<leader>6", "Switch to session in quick slot 6", "base";
    "session_quick_switch_7" => "session.quick_switch.7", "<leader>7", "Switch to session in quick slot 7", "base";
    "session_quick_switch_8" => "session.quick_switch.8", "<leader>8", "Switch to session in quick slot 8", "base";
    "session_quick_switch_9" => "session.quick_switch.9", "<leader>9", "Switch to session in quick slot 9", "base";
    "session_tab_select_1" => "session.tab.select.1", "<leader>1,ctrl+1", "Switch to tab 1", "base";
    "session_tab_select_2" => "session.tab.select.2", "<leader>2,ctrl+2", "Switch to tab 2", "base";
    "session_tab_select_3" => "session.tab.select.3", "<leader>3,ctrl+3", "Switch to tab 3", "base";
    "session_tab_select_4" => "session.tab.select.4", "<leader>4,ctrl+4", "Switch to tab 4", "base";
    "session_tab_select_5" => "session.tab.select.5", "<leader>5,ctrl+5", "Switch to tab 5", "base";
    "session_tab_select_6" => "session.tab.select.6", "<leader>6,ctrl+6", "Switch to tab 6", "base";
    "session_tab_select_7" => "session.tab.select.7", "<leader>7,ctrl+7", "Switch to tab 7", "base";
    "session_tab_select_8" => "session.tab.select.8", "<leader>8,ctrl+8", "Switch to tab 8", "base";
    "session_tab_select_9" => "session.tab.select.9", "<leader>9,ctrl+9", "Switch to tab 9", "base";
    "session_tab_select_10" => "session.tab.select.10", "<leader>0,ctrl+0", "Switch to tab 10", "base";
    "stash_delete" => "stash.delete", "ctrl+d", "Delete stash entry", "dialog";
    "model_provider_list" => "model.dialog.provider", "ctrl+a", "Open provider list from model dialog", "dialog";
    "model_favorite_toggle" => "model.dialog.favorite", "ctrl+f", "Toggle model favorite status", "dialog";
    "model_list" => "model.list", "<leader>m", "List available models", "base", palette Agent;
    "model_cycle_recent" => "model.cycle_recent", "f2", "Next recently used model", "base", palette Agent;
    "model_cycle_recent_reverse" => "model.cycle_recent_reverse", "shift+f2", "Previous recently used model", "base", palette Agent;
    "model_cycle_favorite" => "model.cycle_favorite", "none", "Next favorite model", "base";
    "model_cycle_favorite_reverse" => "model.cycle_favorite_reverse", "none", "Previous favorite model", "base";
    "mcp_list" => "mcp.list", "none", "List MCP servers", "base", palette Agent;
    "provider_connect" => "provider.connect", "none", "Connect an integration", "base", palette Agent;
    "agent_list" => "agent.list", "<leader>a", "List agents", "base", palette Agent;
    "agent_cycle" => "agent.cycle", "shift+tab", "Next agent", "base", palette Agent;
    "agent_cycle_reverse" => "agent.cycle.reverse", "none", "Previous agent", "base";
    "variant_cycle" => "variant.cycle", "ctrl+t", "Cycle model variants", "base", palette Agent;
    "variant_list" => "variant.list", "none", "List model variants", "base";
    "messages_page_up" => "session.page.up", "pageup,ctrl+alt+b", "Scroll messages up by one page", "base";
    "messages_page_down" => "session.page.down", "pagedown,ctrl+alt+f", "Scroll messages down by one page", "base";
    "messages_line_up" => "session.line.up", "ctrl+alt+y", "Scroll messages up by one line", "base";
    "messages_line_down" => "session.line.down", "ctrl+alt+e", "Scroll messages down by one line", "base";
    "messages_half_page_up" => "session.half.page.up", "ctrl+alt+u", "Scroll messages up by half page", "base";
    "messages_half_page_down" => "session.half.page.down", "ctrl+alt+d", "Scroll messages down by half page", "base";
    "messages_first" => "session.first", "ctrl+g,home,alt+home", "Navigate to first message", "base";
    "messages_last" => "session.last", "ctrl+alt+g,end", "Navigate to last message", "base";
    "messages_next" => "session.message.next", "none", "Navigate to next message", "base";
    "messages_previous" => "session.message.previous", "none", "Navigate to previous message", "base";
    "messages_next_user" => "session.message.user.next", "none", "Navigate to next user message", "base";
    "messages_previous_user" => "session.message.user.previous", "none", "Navigate to previous user message", "base";
    "messages_last_user" => "session.messages_last_user", "alt+end", "Navigate to last user message", "base";
    "messages_copy" => "messages.copy", "<leader>y", "Copy message", "base", palette Session;
    "messages_undo" => "session.undo", "<leader>u", "Undo message", "base", palette Session;
    "messages_redo" => "session.redo", "<leader>r", "Redo message", "base", palette Session;
    "display_thinking" => "session.toggle.thinking", "none", "Toggle thinking blocks visibility", "base";
    "prompt_submit" => "prompt.submit", "none", "Submit prompt", "input";
    "prompt_queue" => "prompt.queue", "<leader>return", "Queue prompt", "base", palette Prompt;
    "prompt_editor_context_clear" => "prompt.editor_context.clear", "none", "Clear editor context", "input";
    "prompt_images_view" => "prompt.images.view", "<leader>i", "View image attachments", "base", palette Prompt;
    "prompt_skills" => "prompt.skills", "none", "Open skill selector", "input";
    "prompt_stash" => "prompt.stash", "none", "Stash prompt", "input";
    "prompt_stash_pop" => "prompt.stash.pop", "none", "Pop stashed prompt", "input";
    "prompt_stash_list" => "prompt.stash.list", "none", "List stashed prompts", "input";
    "input_clear" => "prompt.clear", "ctrl+c", "Clear input field", "input";
    "input_paste" => "prompt.paste", "ctrl+v", "Paste from clipboard", "input";
    "input_submit" => "input.submit", "return", "Submit input", "input";
    "input_newline" => "input.newline", "shift+return,ctrl+return,alt+return,ctrl+j", "Insert newline in input", "input";
    "input_move_left" => "input.move.left", "left,ctrl+b", "Move cursor left in input", "input";
    "input_move_right" => "input.move.right", "right,ctrl+f", "Move cursor right in input", "input";
    "input_move_up" => "input.move.up", "up", "Move cursor up in input", "input";
    "input_move_down" => "input.move.down", "down", "Move cursor down in input", "input";
    "input_select_left" => "input.select.left", "shift+left", "Select left in input", "input";
    "input_select_right" => "input.select.right", "shift+right", "Select right in input", "input";
    "input_select_up" => "input.select.up", "shift+up", "Select up in input", "input";
    "input_select_down" => "input.select.down", "shift+down", "Select down in input", "input";
    "input_line_home" => "input.line.home", "ctrl+a", "Move to start of line in input", "input";
    "input_line_end" => "input.line.end", "ctrl+e", "Move to end of line in input", "input";
    "input_select_line_home" => "input.select.line.home", "ctrl+shift+a", "Select to start of line in input", "input";
    "input_select_line_end" => "input.select.line.end", "ctrl+shift+e", "Select to end of line in input", "input";
    "input_visual_line_home" => "input.visual.line.home", "alt+a", "Move to start of visual line in input", "input";
    "input_visual_line_end" => "input.visual.line.end", "alt+e", "Move to end of visual line in input", "input";
    "input_select_visual_line_home" => "input.select.visual.line.home", "alt+shift+a", "Select to start of visual line in input", "input";
    "input_select_visual_line_end" => "input.select.visual.line.end", "alt+shift+e", "Select to end of visual line in input", "input";
    "input_buffer_home" => "input.buffer.home", "none", "Move to start of buffer in input", "input";
    "input_buffer_end" => "input.buffer.end", "none", "Move to end of buffer in input", "input";
    "input_select_buffer_home" => "input.select.buffer.home", "shift+home", "Select to start of buffer in input", "input";
    "input_select_buffer_end" => "input.select.buffer.end", "shift+end", "Select to end of buffer in input", "input";
    "input_delete_line" => "input.delete.line", "ctrl+shift+d", "Delete line in input", "input";
    "input_delete_to_line_end" => "input.delete.to.line.end", "ctrl+k", "Delete to end of line in input", "input";
    "input_delete_to_line_start" => "input.delete.to.line.start", "ctrl+u", "Delete to start of line in input", "input";
    "input_backspace" => "input.backspace", "backspace,shift+backspace", "Backspace in input", "input";
    "input_delete" => "input.delete", "ctrl+d,delete,shift+delete", "Delete character in input", "input";
    "input_undo" => "input.undo", "ctrl+-,super+z", "Undo in input", "input";
    "input_redo" => "input.redo", "ctrl+.,super+shift+z", "Redo in input", "input";
    "input_word_forward" => "input.word.forward", "alt+f,alt+right,ctrl+right", "Move word forward in input", "input";
    "input_word_backward" => "input.word.backward", "alt+b,alt+left,ctrl+left", "Move word backward in input", "input";
    "input_select_word_forward" => "input.select.word.forward", "alt+shift+f,alt+shift+right", "Select word forward in input", "input";
    "input_select_word_backward" => "input.select.word.backward", "alt+shift+b,alt+shift+left", "Select word backward in input", "input";
    "input_delete_word_forward" => "input.delete.word.forward", "alt+d,alt+delete,ctrl+delete", "Delete word forward in input", "input";
    "input_delete_word_backward" => "input.delete.word.backward", "ctrl+w,ctrl+backspace,alt+backspace", "Delete word backward in input", "input";
    "input_select_all" => "input.select.all", "super+a", "Select all in input", "input";
    "history_previous" => "prompt.history.previous", "up", "Previous history item", "input";
    "history_next" => "prompt.history.next", "down", "Next history item", "input";
    "dialog.select.prev" => "dialog.select.prev", "up,ctrl+p", "Move to previous dialog item", "dialog";
    "dialog.select.next" => "dialog.select.next", "down,ctrl+n", "Move to next dialog item", "dialog";
    "dialog.select.page_up" => "dialog.select.page_up", "pageup", "Move up one page in dialog", "dialog";
    "dialog.select.page_down" => "dialog.select.page_down", "pagedown", "Move down one page in dialog", "dialog";
    "dialog.select.home" => "dialog.select.home", "home", "Move to first dialog item", "dialog";
    "dialog.select.end" => "dialog.select.end", "end", "Move to last dialog item", "dialog";
    "dialog.select.submit" => "dialog.select.submit", "return", "Submit selected dialog item", "dialog";
    "dialog.prompt.submit" => "dialog.prompt.submit", "return", "Submit dialog prompt", "dialog";
    "dialog.project_copy.generate" => "dialog.project_copy.generate", "tab", "Generate project copy name", "dialog";
    "dialog.move_session.new" => "dialog.move_session.new", "ctrl+m", "New project copy", "dialog";
    "dialog.move_session.delete" => "dialog.move_session.delete", "ctrl+d", "Delete project copy", "dialog";
    "dialog.move_session.refresh" => "dialog.move_session.refresh", "ctrl+r", "Refresh project copies", "dialog";
    "prompt.autocomplete.prev" => "prompt.autocomplete.prev", "up,ctrl+p", "Move to previous autocomplete item", "autocomplete";
    "prompt.autocomplete.next" => "prompt.autocomplete.next", "down,ctrl+n", "Move to next autocomplete item", "autocomplete";
    "prompt.autocomplete.hide" => "prompt.autocomplete.hide", "escape", "Hide autocomplete", "autocomplete";
    "prompt.autocomplete.select" => "prompt.autocomplete.select", "return", "Select autocomplete item", "autocomplete";
    "prompt.autocomplete.complete" => "prompt.autocomplete.complete", "tab", "Complete autocomplete item", "autocomplete";
    "permission.prompt.fullscreen" => "permission.prompt.fullscreen", "ctrl+f", "Toggle permission prompt fullscreen", "dialog";
    "plugins.toggle" => "plugins.toggle", "return", "Toggle plugin", "dialog";
    "dialog.mcp.toggle" => "dialog.mcp.toggle", "space", "Toggle MCP server", "dialog";
    "terminal_suspend" => "terminal.suspend", "ctrl+z", "Suspend terminal", "base", palette System;
    "terminal_title_toggle" => "terminal.title.toggle", "none", "Toggle terminal title", "base";
};

/// Clock abstraction so leader-timeout behavior is testable with a
/// [`ManualClock`].
pub trait Clock {
    fn now_ms(&self) -> u64;
}

/// Real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

/// Deterministic clock for tests: `advance(ms)` moves time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now: Cell<u64>,
}

impl ManualClock {
    pub fn advance(&self, ms: u64) {
        self.now.set(self.now.get() + ms);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
}

/// Result of feeding one stroke to the keymap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A command resolved (with optional invocation args).
    Command { command: String, args: Option<String> },
    /// The stroke extended/started a pending sequence; `display` feeds
    /// the pending-indicator UI.
    Pending { display: String },
    /// A pending sequence was cleared (escape or timeout).
    Cancelled,
    /// No binding matched; the stroke passes through.
    Unhandled,
}

struct Pending {
    strokes: Vec<KeyStroke>,
    expires_at: u64,
}

/// The keymap engine: default table + leader sequencing + mode stack.
pub struct Keymap {
    leader: KeyStroke,
    timeout_ms: u64,
    /// layer name → (chord, command) rules.
    layers: HashMap<&'static str, Vec<(Chord, &'static str)>>,
    /// registry by dot command (for palette + binding display).
    registry: HashMap<&'static str, &'static CommandDef>,
    modes: Vec<String>,
    pending: Option<Pending>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap {
    /// Keymap with the full v2 default table.
    #[must_use]
    pub fn new() -> Self {
        Self::with_table(default_table())
    }

    #[must_use]
    pub fn with_table(table: &'static [CommandDef]) -> Self {
        let leader = KeyStroke::parse(LEADER_DEFAULT).expect("leader parses");
        let mut layers: HashMap<&'static str, Vec<(Chord, &'static str)>> = HashMap::new();
        let mut registry = HashMap::new();
        for def in table {
            registry.insert(def.command, def);
            for spec in def.defaults.split(',') {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                if let Some(chord) = Chord::parse(spec, &leader) {
                    layers.entry(def.layer).or_default().push((chord, def.command));
                }
            }
        }
        Self {
            leader,
            timeout_ms: LEADER_TIMEOUT_MS,
            layers,
            registry,
            modes: Vec::new(),
            pending: None,
        }
    }

    /// Override the leader timeout (tests / user config `leader.timeout`).
    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }

    #[must_use]
    pub fn is_leader(&self, stroke: &KeyStroke) -> bool {
        *stroke == self.leader
    }

    /// Push an input mode (dialog, autocomplete, …); returns a guard id
    /// to pop with [`Keymap::pop_mode`].
    pub fn push_mode(&mut self, mode: &str) {
        self.modes.push(mode.to_owned());
        // Mode changes clear any half-typed sequence.
        self.pending = None;
    }

    /// Pop the most recently pushed input mode.
    pub fn pop_mode(&mut self) {
        self.modes.pop();
        self.pending = None;
    }

    #[must_use]
    pub fn current_mode(&self) -> &str {
        self.modes.last().map_or("base", String::as_str)
    }

    /// Pending sequence display ("ctrl+x"), if any.
    #[must_use]
    pub fn pending_display(&self) -> Option<String> {
        self.pending.as_ref().map(|p| Chord { strokes: p.strokes.clone() }.display())
    }

    /// Expire a stale pending sequence; `true` when one expired.
    pub fn expire(&mut self, clock: &dyn Clock) -> bool {
        if let Some(pending) = &self.pending {
            if clock.now_ms() >= pending.expires_at {
                self.pending = None;
                return true;
            }
        }
        false
    }

    /// Resolve one stroke. See the module docs for the escape and leader
    /// special-cases.
    pub fn lookup(&mut self, stroke: KeyStroke, clock: &dyn Clock) -> Outcome {
        self.expire(clock);
        // Escape clears a pending sequence first (registerEscapeClears-
        // PendingSequence); only an un-pended escape dispatches.
        if self.pending.is_some() && stroke.key == "escape" && !stroke.ctrl && !stroke.alt {
            self.pending = None;
            return Outcome::Cancelled;
        }
        if let Some(pending) = self.pending.take() {
            let mut candidate = pending.strokes.clone();
            candidate.push(stroke.clone());
            match self.match_chord(&candidate) {
                ChordMatch::Exact(command) => {
                    return Outcome::Command { command: command.to_owned(), args: None }
                }
                ChordMatch::Prefix => {
                    self.pending = Some(Pending {
                        expires_at: clock.now_ms() + self.timeout_ms,
                        strokes: candidate,
                    });
                    return Outcome::Pending { display: self.pending_display().expect("just set") };
                }
                ChordMatch::None => {
                    // Fall through: the stroke is evaluated fresh below.
                }
            }
        }
        if self.is_leader(&stroke) && self.leader_starts_sequence() {
            self.pending = Some(Pending {
                strokes: vec![stroke],
                expires_at: clock.now_ms() + self.timeout_ms,
            });
            return Outcome::Pending { display: self.pending_display().expect("just set") };
        }
        match self.match_chord(std::slice::from_ref(&stroke)) {
            ChordMatch::Exact(command) => {
                Outcome::Command { command: command.to_owned(), args: None }
            }
            _ => Outcome::Unhandled,
        }
    }

    /// Whether any registered chord begins with the leader.
    fn leader_starts_sequence(&self) -> bool {
        self.layers.values().flatten().any(|(chord, _)| chord.strokes.first() == Some(&self.leader))
    }

    fn match_chord(&self, strokes: &[KeyStroke]) -> ChordMatch<'_> {
        // Most recently pushed mode first, base last.
        let mut order: Vec<&str> = self.modes.iter().rev().map(String::as_str).collect();
        order.push("base");
        let mut prefix = false;
        for mode in order {
            let Some(rules) = self.layers.get(mode) else { continue };
            for (chord, command) in rules {
                if chord.strokes == strokes {
                    return ChordMatch::Exact(command);
                }
                if chord.strokes.len() > strokes.len()
                    && chord.strokes.starts_with(strokes)
                    && strokes.first() == Some(&self.leader)
                {
                    prefix = true;
                }
            }
        }
        if prefix {
            ChordMatch::Prefix
        } else {
            ChordMatch::None
        }
    }

    /// Palette-visible commands (grouped order: the registry order).
    #[must_use]
    pub fn palette_commands(&self) -> Vec<&'static CommandDef> {
        default_table().iter().filter(|def| def.palette).collect()
    }

    /// Display bindings for a command ("ctrl+x n" style), in table order.
    #[must_use]
    pub fn bindings_for(&self, command: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(def) = self.registry.get(command) {
            for spec in def.defaults.split(',') {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                if let Some(chord) = Chord::parse(spec, &self.leader) {
                    out.push(chord.display());
                }
            }
        }
        out
    }

    #[must_use]
    pub fn command_def(&self, command: &str) -> Option<&'static CommandDef> {
        self.registry.get(command).copied()
    }
}

enum ChordMatch<'a> {
    Exact(&'a str),
    Prefix,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(spec: &str) -> KeyStroke {
        KeyStroke::parse(spec).expect("test stroke parses")
    }

    #[test]
    fn default_table_is_complete_and_parses() {
        let table = default_table();
        // Spot anchors from tui-map §4 + the full table shape.
        assert!(table.len() > 150, "full v2 table, got {}", table.len());
        let keymap = Keymap::new();
        for def in table {
            assert!(!def.bind_name.is_empty() && !def.command.is_empty());
            for spec in def.defaults.split(',') {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                assert!(
                    Chord::parse(spec, &KeyStroke::parse("ctrl+x").unwrap()).is_some(),
                    "{spec} parses"
                );
            }
            assert_eq!(
                keymap.command_def(def.command).map(|found| found.command),
                Some(def.command),
                "{} registered",
                def.command
            );
        }
        // Dot-command spot checks from CommandMap.
        assert_eq!(keymap.command_def("command.palette.show").unwrap().bind_name, "command_list");
        assert_eq!(
            keymap.command_def("session.tab.select.10").unwrap().defaults,
            "<leader>0,ctrl+0"
        );
    }

    #[test]
    fn top_bindings_resolve() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        let cases: &[(&str, &str)] = &[
            ("ctrl+p", "command.palette.show"),
            ("ctrl+o", "open.menu"),
            ("ctrl+tab", "session.tab.next"),
            ("alt+down", "session.tab.next"),
            ("ctrl+shift+tab", "session.tab.previous"),
            ("alt+up", "session.tab.previous"),
            ("ctrl+shift+t", "session.tab.reopen"),
            ("ctrl+r", "session.rename"),
            ("ctrl+b", "session.background"),
            ("ctrl+t", "variant.cycle"),
            ("f2", "model.cycle_recent"),
            ("shift+f2", "model.cycle_recent_reverse"),
            ("escape", "session.interrupt"),
            ("ctrl+z", "terminal.suspend"),
            ("ctrl+1", "session.tab.select.1"),
            ("ctrl+0", "session.tab.select.10"),
            ("pageup", "session.page.up"),
            ("alt+shift+down", "session.tab.next_unread"),
        ];
        for (spec, command) in cases {
            assert_eq!(
                keymap.lookup(stroke(spec), &clock),
                Outcome::Command { command: (*command).to_owned(), args: None },
                "{spec} → {command}"
            );
        }
    }

    #[test]
    fn leader_chords_resolve() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        // q completes app_exit via <leader>q.
        assert_eq!(
            keymap.lookup(stroke("ctrl+x"), &clock),
            Outcome::Pending { display: "ctrl+x".to_owned() }
        );
        assert_eq!(
            keymap.lookup(stroke("q"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None }
        );
        // n → session.new; e → prompt.editor; m → model.list; w → tab close.
        for (chord_tail, command) in [
            ("n", "session.new"),
            ("e", "prompt.editor"),
            ("m", "model.list"),
            ("w", "session.tab.close"),
        ] {
            assert_eq!(
                keymap.lookup(stroke("ctrl+x"), &clock),
                Outcome::Pending { display: "ctrl+x".into() }
            );
            assert_eq!(
                keymap.lookup(stroke(chord_tail), &clock),
                Outcome::Command { command: command.to_owned(), args: None },
                "ctrl+x {chord_tail}"
            );
        }
        // The pending display extends for multi-stroke prefixes.
        assert_eq!(
            keymap.lookup(stroke("ctrl+x"), &clock),
            Outcome::Pending { display: "ctrl+x".into() }
        );
    }

    #[test]
    fn leader_timeout_expires_pending() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        clock.advance(LEADER_TIMEOUT_MS);
        assert!(keymap.expire(&clock), "pending expired at the timeout");
        assert_eq!(keymap.pending_display(), None);
        // After expiry a plain stroke resolves normally (no stale prefix).
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
        // One ms before the timeout it is still live.
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        clock.advance(LEADER_TIMEOUT_MS - 1);
        assert!(!keymap.expire(&clock));
    }

    #[test]
    fn escape_clears_pending_then_interrupts_when_idle() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(keymap.lookup(stroke("escape"), &clock), Outcome::Cancelled);
        // With nothing pending, escape interrupts the session.
        assert_eq!(
            keymap.lookup(stroke("escape"), &clock),
            Outcome::Command { command: "session.interrupt".to_owned(), args: None }
        );
    }

    #[test]
    fn unmatched_continuation_falls_through() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        // ctrl+x z has no chord; the z is then evaluated fresh — nothing.
        assert_eq!(keymap.lookup(stroke("z"), &clock), Outcome::Unhandled);
        assert_eq!(keymap.pending_display(), None);
    }

    #[test]
    fn mode_stack_overrides_base() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        // Base: down → subagent picker toggle.
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
        keymap.push_mode("input");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "input.move.down".to_owned(), args: None }
        );
        assert_eq!(
            keymap.lookup(stroke("return"), &clock),
            Outcome::Command { command: "input.submit".to_owned(), args: None }
        );
        keymap.push_mode("autocomplete");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "prompt.autocomplete.next".to_owned(), args: None }
        );
        keymap.pop_mode();
        keymap.pop_mode();
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
    }

    #[test]
    fn dialog_layer_shadows_base_conflicts() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.push_mode("dialog");
        // ctrl+p inside a dialog moves the selection, not the palette.
        assert_eq!(
            keymap.lookup(stroke("ctrl+p"), &clock),
            Outcome::Command { command: "dialog.select.prev".to_owned(), args: None }
        );
    }

    #[test]
    fn binding_displays_use_leaders() {
        let keymap = Keymap::new();
        assert_eq!(keymap.bindings_for("session.new"), vec!["ctrl+x n".to_owned()]);
        assert_eq!(
            keymap.bindings_for("session.tab.select.10"),
            vec!["ctrl+x 0".to_owned(), "ctrl+0".to_owned()]
        );
        assert!(keymap.bindings_for("app.exit").contains(&"ctrl+c".to_owned()));
    }

    #[test]
    fn palette_lists_grouped_commands() {
        let keymap = Keymap::new();
        let palette = keymap.palette_commands();
        assert!(palette
            .iter()
            .any(|def| def.command == "session.new" && def.group == Some(CommandGroup::Session)));
        assert!(palette
            .iter()
            .any(|def| def.command == "model.list" && def.group == Some(CommandGroup::Agent)));
        assert!(palette.iter().all(|def| def.palette));
    }
}
