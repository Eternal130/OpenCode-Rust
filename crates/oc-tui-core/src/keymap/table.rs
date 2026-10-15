//! The keymap DATA layer: strokes, chords, the full v2 default
//! binding table (`config/keybind.ts` `Definitions`), user-override
//! parsing, the config-error sink, clocks — everything the engine
//! ([`super::engine`]) and the [`super::Keymap`] facade consume.
//!
//! Wave A lane note: this module moved verbatim from oc-tui's
//! `keymap.rs` into the oc-tui-core interaction-contract crate.

use std::cell::Cell;

/// Leader key (`LeaderDefault`) and timeout (`registerTimedLeader`
/// default 2000 ms).
pub const LEADER_DEFAULT: &str = "ctrl+x";
pub const LEADER_TIMEOUT_MS: u64 = 2000;

/// One key stroke: key name plus modifiers. Key names use the opentui
/// vocabulary (`return`, `escape`, `up`, `f2`, single letters).
#[allow(clippy::struct_excessive_bools)] // modifier flags mirror the wire shape
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Super/hyper modifier (the `super+a`-style bindings): tracked so
    /// `super+a` ≠ plain `a` (the reference's `input.select.all`).
    pub super_key: bool,
}

impl KeyStroke {
    #[must_use]
    pub fn new(key: &str) -> Self {
        Self { key: key.to_owned(), ctrl: false, shift: false, alt: false, super_key: false }
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

    /// Parse one stroke ("ctrl+x", "alt+down", "E", "return"). The
    /// final key token keeps interior meaning: a single space is a
    /// valid key, so only the split segments are trimmed around the
    /// `+` separators.
    #[must_use]
    pub fn parse(spec: &str) -> Option<Self> {
        let mut stroke = Self::new("");
        // NB: the spec is NOT trimmed whole — a lone space is a valid
        // key; only the `+`-separated segments trim.
        let tokens: Vec<&str> = spec.split('+').collect();
        for (index, token) in tokens.iter().enumerate() {
            let last = index + 1 == tokens.len();
            let token = token.trim();
            if !last && token.is_empty() {
                return None;
            }
            if !last {
                match token.to_ascii_lowercase().as_str() {
                    "ctrl" => stroke.ctrl = true,
                    "shift" => stroke.shift = true,
                    "alt" | "meta" => stroke.alt = true,
                    "super" | "hyper" => stroke.super_key = true,
                    _ => return None,
                }
                continue;
            }
            // The key itself: an untrimmed single character (space is a
            // key) or a trimmed name.
            let token = if tokens[index].chars().count() == 1 { tokens[index] } else { token };
            if token.is_empty() {
                return None;
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
        if self.super_key {
            parts.push("super".into());
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

/// Command palette group (`KeymapCommand.group` / `category`). The
/// [`CommandGroup::Plugin`] variant marks plugin-registered commands
/// (the owned keymap layer's entries — wave 9.1).
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
    Plugin,
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
            Self::Plugin => "Plugin",
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
    /// The default binding's `preventDefault` (binding-object form,
    /// `config/keybind.ts:14-29`): `true` — the keymap consumes the
    /// matched key event itself; `false` — the layer leaves
    /// suppression to the command's handler (v2's `prompt.paste`
    /// calls `event?.preventDefault()` + `stopPropagation()` only
    /// when an event is present — prompt/index.tsx:470-492; a
    /// programmatic `keymap.dispatch` has none, :1787).
    pub prevent_default: bool,
}

/// An OWNED registry entry for plugin-registered commands (wave 9.1
/// seam): everything [`CommandDef`] carries, but heap-allocated —
/// plugin ids/descriptions/bindings arrive over IPC at registration
/// time and cannot be `&'static str`s. Owned entries live in a
/// separate layer of the [`Keymap`] and are resolved **after** the
/// static table: a plugin binding for `ctrl+p` never shadows the
/// built-in palette command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedCommandDef {
    /// Runtime dot command id (`hw.hello`).
    pub command: String,
    pub description: String,
    /// Binding specs (`"ctrl+x h"`; comma alternatives allowed) — may
    /// be empty (palette-only command).
    pub defaults: Vec<String>,
    /// Layer the binding activates in (`"base"`).
    pub layer: String,
    pub palette: bool,
    /// Owning plugin id (display provenance; the palette marks the
    /// group "Plugin").
    pub plugin: String,
}

/// One user keybind override (`cli.json` → `keybinds`): the v2 config
/// schema keys overrides by the dot command (`"app.exit"`); the v1
/// config used the bind name (`"app_exit"`) — both are accepted, the
/// dot id first.
///
/// Values follow `BindingValueSchema`: a comma-separated alternatives
/// string (`"ctrl+q,<leader>x"`), an array of those, or an unset
/// (`false` / `"none"` / empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindOverride {
    /// The override key as written (`app.exit` or `app_exit`).
    pub name: String,
    /// Parsed alternatives; empty = unset (removes the binding).
    pub specs: Vec<String>,
}

impl KeybindOverride {
    /// Parse one override value (`BindingValueSchema` subset): string
    /// (comma alternatives), array of strings/`{key}` objects, `false`
    /// or `"none"` (unset). `None` = unparseable (logged + skipped by
    /// the caller).
    #[must_use]
    pub fn parse_value(name: &str, value: &serde_json::Value) -> Option<Self> {
        let specs = match value {
            serde_json::Value::String(text) => parse_binding_string(text),
            serde_json::Value::Array(items) => {
                let mut specs = Vec::new();
                for item in items {
                    match item {
                        serde_json::Value::String(text) => specs.extend(parse_binding_string(text)),
                        serde_json::Value::Object(object) => {
                            // BindingObject: `{ key: "…" | {name,…} }`.
                            let text = match object.get("key")? {
                                serde_json::Value::String(text) => text.clone(),
                                serde_json::Value::Object(stroke) => {
                                    let key = stroke.get("name")?.as_str()?.to_owned();
                                    let mut parts = Vec::new();
                                    for modifier in
                                        ["ctrl", "shift", "alt", "meta", "super", "hyper"]
                                    {
                                        if stroke.get(modifier).and_then(serde_json::Value::as_bool)
                                            == Some(true)
                                        {
                                            parts.push(modifier.to_owned());
                                        }
                                    }
                                    parts.push(key);
                                    parts.join("+")
                                }
                                _ => return None,
                            };
                            specs.extend(parse_binding_string(&text));
                        }
                        _ => return None,
                    }
                }
                specs
            }
            serde_json::Value::Bool(false) => Vec::new(),
            _ => return None,
        };
        Some(Self { name: name.to_owned(), specs })
    }
}

/// Split a comma-form binding string into alternatives; `"none"`
/// unsets.
fn parse_binding_string(text: &str) -> Vec<String> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Vec::new();
    }
    text.split(',').map(str::trim).filter(|spec| !spec.is_empty()).map(str::to_owned).collect()
}

/// Parse the whole `keybinds` config object into overrides. Unknown
/// keys and unparseable values are logged and skipped (the reference's
/// `parse()` throws; the Rust port must never brick the TUI on a bad
/// user config — the entry is dropped, the default stands).
#[must_use]
pub fn parse_keybind_overrides(value: &serde_json::Value) -> Vec<KeybindOverride> {
    let Some(object) = value.as_object() else { return Vec::new() };
    let mut out = Vec::new();
    for (name, raw) in object {
        match KeybindOverride::parse_value(name, raw) {
            Some(entry) => out.push(entry),
            None => log_skip(&format!("keybind {name:?}: unparseable value {raw} (skipped)")),
        }
    }
    out
}

/// Config-load skip logging — **F5**: the skip keeps its stderr line
/// AND lands in the pending sink so the app pump can surface it as a
/// toast (the reference routes theme failures through `ThemeErrorToast`
/// and config errors through `toast.error`; the port must never brick
/// the TUI on a bad user config, so the entry is dropped, the default
/// stands, and the user is told why).
pub fn log_skip(reason: &str) {
    eprintln!("opencode tui: {reason}");
    record_config_error(reason);
}

/// The pending config errors (thread-safe; the pump drains them on its
/// next tick — `app::run_with_size`'s drain seam:
/// `for toast in keymap::drain_config_error_toasts() {
///     store.toast_show(toast) }`).
static CONFIG_ERRORS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The sink's bound (a pathological config must not grow it unbounded).
const CONFIG_ERRORS_MAX: usize = 32;

fn record_config_error(reason: &str) {
    let Ok(mut errors) = CONFIG_ERRORS.lock() else { return };
    if errors.len() < CONFIG_ERRORS_MAX {
        errors.push(reason.to_owned());
    }
}

/// Drain the pending config errors (the app pump's toast drain).
#[must_use]
pub fn take_config_errors() -> Vec<String> {
    let Ok(mut errors) = CONFIG_ERRORS.lock() else { return Vec::new() };
    std::mem::take(&mut *errors)
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
/// `preventDefault` of one entry's DEFAULT binding: v2's `keybind()`
/// takes a string (binding objects may set `preventDefault: false` —
/// the keymap then leaves suppression to the command's handler). Only
/// `prompt.paste` ships `false` (config/keybind.ts:205).
macro_rules! prevent_default_of {
    () => {
        true
    };
    (false) => {
        false
    };
    (true) => {
        true
    };
}

macro_rules! defs {
    ($( $bind:literal => $cmd:literal, $default:literal, $desc:literal, $layer:literal $(, palette $group:ident)? $(, prevent_default $pd:tt)? $(,)? ; )+) => {
        &[ $( CommandDef {
            bind_name: $bind,
            command: $cmd,
            defaults: $default,
            description: $desc,
            layer: $layer,
            palette: palette_of!($($group)?),
            group: group_of!($($group)?),
            prevent_default: prevent_default_of!($($pd)?),
        } ),+ ]
    };
}

static DEFAULT_TABLE: &[CommandDef] = defs! {
    "leader" => "leader", "ctrl+x", "Leader key for keybind combinations", "base";
    // <leader>q decollided (audit §1b / keybind.ts:131): the chord belongs to
    // session.queued_prompts; app.exit keeps ctrl+c/ctrl+d.
    "app_exit" => "app.exit", "ctrl+c,ctrl+d", "Exit the application", "base", palette System;
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
    // v2 default is "none" (config/keybind.ts:92) — `<leader>t` is the
    // terminal pane toggle there; the port previously drifted this to
    // `<leader>t`.
    "theme_list" => "theme.switch", "none", "List available themes", "base", palette Theme;
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
    // keybind.ts:137-138 — table-only defaults (v2 registers no
    // command handler for either; the input layer shadows the raw
    // arrows while a textarea is focused, matching v2's inert pair).
    "session_child_next" => "session.child.next", "right", "Go to next child session", "base";
    "session_child_previous" => "session.child.previous", "left", "Go to previous child session", "base";
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
    "input_paste" => "prompt.paste", "ctrl+v", "Paste from clipboard", "input", prevent_default false;
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
    // The move_session trio follows the OPERATIVE v2 keybind table
    // (`config/keybind.ts`) — the v1 defaults differ (ctrl+m); the
    // dialog-select golden's footer labels ("new ctrl+a") pin the v2
    // values at runtime.
    "dialog.move_session.new" => "dialog.move_session.new", "ctrl+a", "New worktree", "dialog";
    "dialog.move_session.move" => "dialog.move_session.move", "ctrl+m", "Move session to worktree", "dialog";
    "dialog.move_session.delete" => "dialog.move_session.delete", "ctrl+d", "Delete worktree", "dialog";
    "dialog.move_session.refresh" => "dialog.move_session.refresh", "ctrl+r", "Refresh worktrees", "dialog";
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
    // The composer panel's keybind family (config/keybind.ts:245-255
    // + the `bind` props on the layer commands — composer/index.tsx:72-82,
    // composer/subagents-tab.tsx:136-194). The mode is pushed while
    // the picker is open (`Keymap.mode.push("composer")`).
    "composer_subagent_up" => "composer.subagent.up", "up", "Previous subagent", "composer";
    "composer_subagent_down" => "composer.subagent.down", "down", "Next subagent", "composer";
    "composer_subagent_select" => "composer.subagent.select", "return", "Navigate to subagent", "composer";
    "composer_subagent_toggle_activity" => "composer.subagent.toggle-activity", "ctrl+a", "Toggle active subagents", "composer";
    "composer_subagent_interrupt" => "composer.subagent.interrupt", "ctrl+d", "Interrupt subagent", "composer";
    "composer_shell_up" => "composer.shell.up", "up", "Previous shell", "composer";
    "composer_shell_down" => "composer.shell.down", "down", "Next shell", "composer";
    "composer_shell_select" => "composer.shell.select", "return", "View shell output", "composer";
    "composer_shell_kill" => "composer.shell.kill", "ctrl+d", "Kill shell command", "composer";
    "composer_close" => "composer.close", "escape,ctrl+c", "Close composer", "composer";
    "composer_tab_prev" => "composer.tab.prev", "left", "Previous composer tab", "composer";
    "composer_tab_next" => "composer.tab.next", "right", "Next composer tab", "composer";
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

    /// The current reading (tests).
    #[must_use]
    pub fn now_ms_value(&self) -> u64 {
        self.now.get()
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
}
