//! The prompt autocomplete (`component/prompt/autocomplete.tsx`,
//! 953 LOC) — trigger detection (`/` commands, `@` files/agents/
//! skills), filtered popup reusing the dialog-select fuzzysort port,
//! selected-row navigation, and the accept/escape/dismiss flow.
//!
//! Golden scope (`composer-autocomplete-open` @80/40): typing `/re`
//! opens the slash-command list over the composer with `/redo`,
//! `/rename` on top — `/redo` selected (row 0), title column padded to
//! the registry's longest display + 2 (the v2 table's `/worktrees`
//! makes it 12 wide).

use crate::components::dialog_select::fuzzy;

/// Which trigger opened the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `/` — slash commands.
    Command,
    /// `@` — files, agents, skills, references.
    Reference,
}

/// One slash command (`commands()`): display `/name`, description =
/// the command's description or title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashEntry {
    pub display: String,
    pub description: String,
}

impl SlashEntry {
    fn new(name: &str, description: &str) -> Self {
        Self { display: format!("/{name}"), description: description.to_owned() }
    }
}

/// One rendered popup option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocompleteOption {
    pub display: String,
    pub description: Option<String>,
}

/// The registry the filter ranks (`keymapCommands()` — every command
/// with a slash name, sorted by display, padded to a uniform column).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlashRegistry {
    pub entries: Vec<SlashEntry>,
}

/// The v2 default slash table (`app.tsx`'s app commands + the prompt's
/// own /cd, /editor, /skills, /worktrees). Descriptions are the
/// commands' titles (app commands carry no separate description); the
/// golden's ordering depends on them exactly (`/rename`'s "Rename
/// session" description outranks `/resume`'s display-only match).
pub const V2_SLASH: &[(&str, &str)] = &[
    ("agents", "Switch agent"),
    ("cd", "Change working directory"),
    ("clear", "Clear session"),
    ("compact", "Compact session"),
    ("connect", "Connect an integration"),
    ("continue", "Switch session"),
    ("debug", "View debug info"),
    ("diff", "Open diff viewer"),
    ("editor", "Open editor"),
    ("effort", "Switch model variant"),
    ("exit", "Exit the app"),
    ("fork", "Fork session"),
    ("help", "Help"),
    ("mcps", "MCP servers"),
    ("models", "Switch model"),
    ("new", "New session"),
    ("open", "Open session or project"),
    ("pair", "Pair device"),
    ("plugins", "Plugins"),
    ("project", "Open session or project"),
    ("projects", "Open session or project"),
    ("q", "Exit the app"),
    ("quit", "Exit the app"),
    ("redo", "Redo"),
    ("reload", "Reload configuration"),
    ("rename", "Rename session"),
    ("restart", "Restart service"),
    ("resume", "Switch session"),
    ("sessions", "Switch session"),
    ("settings", "Open settings"),
    ("share", "Share session"),
    ("skills", "Skills"),
    ("status", "View status"),
    ("themes", "Switch theme"),
    ("thinking", "Switch model variant"),
    ("timeline", "Jump to message"),
    ("undo", "Undo previous message"),
    ("update", "Update OpenCode"),
    ("variants", "Switch model variant"),
    ("web", "Pair device"),
    ("worktrees", "Manage workspaces"),
];

/// Slash name/alias → the dot command v2's registry runs on select
/// (`onSelect: command.run`, `autocomplete.tsx:481`; the command ids
/// come from `app.tsx`'s `slash: { name, aliases }` entries, the
/// prompt's own `/editor` `/skills` `/worktrees`, and the system
/// plugin's `/plugins` — feature-plugins/system/plugins.tsx:337-347,
/// SA-F6: the live-typed command opens the Plugins dialog on Enter).
/// `/cd` takes arguments (`slash.arguments: true`) and stays an
/// INSERT; the skills dialog is not ported (no dispatcher entry — the
/// run is a no-op there, matching v2's "runs but the surface is
/// absent").
pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("agents", "agent.list"),
    ("clear", "session.clear"),
    ("compact", "session.compact"),
    ("connect", "provider.connect"),
    ("continue", "session.list"),
    ("debug", "opencode.debug"),
    ("diff", "diff.open"),
    ("editor", "prompt.editor"),
    ("effort", "variant.list"),
    ("exit", "app.exit"),
    ("fork", "session.fork"),
    ("help", "help.show"),
    ("mcps", "mcp.list"),
    ("models", "model.list"),
    ("new", "session.new"),
    ("open", "open.menu"),
    ("pair", "server.pair"),
    ("plugins", "plugins.list"),
    ("project", "open.menu"),
    ("projects", "open.menu"),
    ("q", "app.exit"),
    ("quit", "app.exit"),
    ("redo", "session.redo"),
    ("reload", "location.reload"),
    ("restart", "service.restart"),
    ("resume", "session.list"),
    ("sessions", "session.list"),
    ("settings", "opencode.settings"),
    ("share", "session.share"),
    ("skills", "prompt.skills"),
    ("status", "opencode.status"),
    ("themes", "theme.switch"),
    ("thinking", "variant.list"),
    ("timeline", "session.timeline"),
    ("undo", "session.undo"),
    ("update", "opencode.update"),
    ("variants", "variant.list"),
    ("web", "server.pair"),
    ("worktrees", "session.move"),
];

/// The argument-taking slash commands (`slash: { arguments: true }`)
/// — the popup INSERTS the token (`insertSlash`, autocomplete.tsx:
/// 481) and the SUBMIT path intercepts the draft instead of sending
/// it (`argumentSlash`, prompt/index.tsx:173-183 + 1133-1146: head
/// `/name`, run with the remaining text as `input`). v2's table:
/// `/cd` (prompt/index.tsx:269) and `/rename`
/// (routes/session/index.tsx:899).
pub const ARGUMENT_SLASHES: &[(&str, &str)] = &[("cd", "session.cd"), ("rename", "session.rename")];

/// `argumentSlash(input, keymapCommands())` (prompt/index.tsx:173-
/// 183): the draft's `/name` head against the argument-taking table.
/// Returns the dot command and the argument text (`Some("")` when the
/// head carries NO argument — `/rename` alone still intercepts and
/// runs with an empty input, matching v2's `head.arguments`).
#[must_use]
pub fn argument_slash(text: &str) -> Option<(&'static str, &str)> {
    let name = text.strip_prefix('/')?;
    if name.is_empty() {
        return None;
    }
    let (head, args) = match name.find(char::is_whitespace) {
        Some(at) => (&name[..at], &name[at..]),
        None => (name, ""),
    };
    ARGUMENT_SLASHES.iter().find(|(slash, _)| *slash == head).map(|(_, command)| (*command, args))
}

impl SlashRegistry {
    /// The full v2 table (sorted by display like `commands()`; the
    /// `padEnd(max + 2)` title column width derives from it).
    #[must_use]
    pub fn v2() -> Self {
        let mut entries: Vec<SlashEntry> =
            V2_SLASH.iter().map(|(name, description)| SlashEntry::new(name, description)).collect();
        entries.sort_by(|a, b| a.display.cmp(&b.display));
        Self { entries }
    }

    /// The slash registry's `onSelect` (`autocomplete.tsx:481`):
    /// `slash.arguments ? insertSlash(name) : command.run` — every
    /// built-in WITHOUT arguments RUNS on select; only
    /// argument-taking commands (`/cd`) insert. Returns the dot
    /// command id a display (`/name` or `/alias`) executes.
    #[must_use]
    pub fn command_for(display: &str) -> Option<&'static str> {
        let name = display.strip_prefix('/')?;
        SLASH_COMMANDS.iter().find(|(slash, _)| *slash == name).map(|(_, command)| *command)
    }

    /// The golden harness's mount: the v2 table plus the three
    /// session-scoped commands the fixture registers (rename, redo,
    /// compact — deduped by display).
    #[must_use]
    pub fn golden_harness() -> Self {
        let mut registry = Self::v2();
        for (name, description) in
            [("rename", "Rename session"), ("redo", "Redo"), ("compact", "Compact session")]
        {
            let entry = SlashEntry::new(name, description);
            if let Some(existing) =
                registry.entries.iter_mut().find(|item| item.display == entry.display)
            {
                *existing = entry;
            } else {
                registry.entries.push(entry);
            }
        }
        registry.entries.sort_by(|a, b| a.display.cmp(&b.display));
        registry
    }

    /// The title-column width: longest display + 2 (`padEnd(max + 2)`).
    #[must_use]
    pub fn display_width(&self) -> usize {
        self.entries
            .iter()
            .map(|entry| crate::wrap::display_width(&entry.display))
            .max()
            .map_or(2, |width| width + 2)
    }
}

/// The live popup state (`AutocompleteRef`'s store).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutocompleteState {
    pub visible: Option<Mode>,
    /// Char offset of the trigger character in the draft.
    pub trigger_index: usize,
    pub selected: usize,
    pub registry: SlashRegistry,
    /// The filtered options the renderer shows (attached by
    /// [`Self::with_options`] after each edit).
    pub options: Vec<AutocompleteOption>,
    /// The option count the selection wrap navigates (kept in sync by
    /// [`Self::with_options`]).
    pub filtered_len: usize,
    last_query: Option<String>,
}

impl AutocompleteState {
    /// A state over a specific registry (the golden harness mounts the
    /// v2 table plus its session commands).
    #[must_use]
    pub fn with_registry(registry: SlashRegistry) -> Self {
        Self { registry, ..Self::default() }
    }

    /// Close without touching the draft (`hide()`).
    pub fn close(&mut self) {
        self.visible = None;
    }

    /// `hide()` from the escape binding — the dismissed value that
    /// suppresses re-opening until the text changes is the caller's
    /// (the composer tracks it via the draft text itself).
    pub fn dismiss(&mut self) {
        self.visible = None;
    }

    /// `moveSelection` with the wrap policy (the reference layer's
    /// prev/next commands).
    pub fn move_selection(&mut self, delta: i8) {
        let count = self.filtered_len.max(self.options.len());
        if count == 0 {
            return;
        }
        let index = self.selected.cast_signed() + isize::from(delta);
        self.selected = index.rem_euclid(count.cast_signed()).max(0).cast_unsigned();
    }

    #[must_use]
    pub fn selected_option(&self) -> Option<&AutocompleteOption> {
        self.options.get(self.selected.min(self.options.len().saturating_sub(1)))
    }

    /// `onInput`: recompute the trigger from the text + caret, closing
    /// when the cursor leaves the token (a space between the trigger
    /// and the caret, or the cursor moving before the trigger). The
    /// filtered options reattach on every edit (the live popup and
    /// `accept_autocomplete` read them without further plumbing).
    pub fn on_input(&mut self, text: &str, caret: usize) {
        let chars: Vec<char> = text.chars().collect();
        let trigger = trigger_for(text, caret);
        match (self.visible, trigger) {
            (Some(mode), Some((next_mode, index))) => {
                let between = &chars[index..caret.min(chars.len())];
                let closed = between.iter().any(|ch| ch.is_whitespace())
                    || caret <= index
                    || next_mode != mode;
                if closed {
                    self.close();
                } else {
                    self.trigger_index = index;
                    let query = query_between(text, index, caret);
                    if self.last_query.as_deref() != Some(query.as_str()) {
                        // The filter effect resets the selection whenever
                        // the options change.
                        self.selected = 0;
                        self.last_query = Some(query);
                    }
                }
            }
            (None, Some((mode, index))) => {
                self.visible = Some(mode);
                self.trigger_index = index;
                self.selected = 0;
                self.last_query = Some(query_between(text, index, caret));
            }
            _ => self.close(),
        }
        self.sync_options(text, caret);
    }

    /// Recompute `options` for the visible mode (command mode filters
    /// the registry; reference mode has no live file source yet — an
    /// empty list renders no popup, matching the port's scope).
    pub fn sync_options(&mut self, text: &str, caret: usize) {
        self.options = match self.visible {
            Some(Mode::Command) => {
                let query = query_between(text, self.trigger_index, caret);
                filter_commands(&self.registry, &query)
            }
            _ => Vec::new(),
        };
        self.filtered_len = self.options.len();
        if self.selected >= self.filtered_len {
            self.selected = 0;
        }
    }

    /// Attach the filtered options (rendering + selection clamp).
    #[must_use]
    pub fn with_options(mut self, options: Vec<AutocompleteOption>) -> Self {
        self.options = options;
        self.filtered_len = self.options.len();
        if self.selected >= self.filtered_len {
            self.selected = 0;
        }
        self
    }
}

// -- trigger detection ---------------------------------------------------------

/// `slashTriggerIndex` / `mentionTriggerIndex`: the nearest trigger
/// character before the caret with no whitespace between it and the
/// caret. `/` triggers only at a line head; `@` anywhere.
fn trigger_for(text: &str, caret: usize) -> Option<(Mode, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let end = caret.min(chars.len());
    let mut index = end;
    while index > 0 {
        let ch = chars[index - 1];
        if ch.is_whitespace() {
            return None;
        }
        if ch == '@' {
            return Some((Mode::Reference, index - 1));
        }
        if ch == '/' && (index - 1 == 0 || chars[index - 2] == '\n') {
            return Some((Mode::Command, index - 1));
        }
        index -= 1;
    }
    None
}

/// The popup's query: the text after the trigger through the caret.
#[must_use]
pub fn query_between(text: &str, trigger: usize, caret: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let end = caret.min(chars.len());
    if trigger >= end {
        return String::new();
    }
    chars[trigger + 1..end].iter().collect()
}

/// Filter the registry for a command-mode query — the dialog's
/// fuzzysort port with the autocomplete's keys (display + description
/// + aliases) and scoreFn:
///
/// * the score is the key sum, doubled when the display starts with
///   `/{query}`;
/// * the result is sliced to 10;
/// * ties keep registry (alphabetical) order — the golden's `/redo`
///   before `/rename` depends on it.
#[must_use]
pub fn filter_commands(registry: &SlashRegistry, query: &str) -> Vec<AutocompleteOption> {
    let needle = query.to_lowercase();
    if needle.trim().is_empty() {
        // No query: the full padded list in registry order.
        return registry
            .entries
            .iter()
            .map(|entry| AutocompleteOption {
                display: entry.display.clone(),
                description: Some(entry.description.clone()),
            })
            .collect();
    }
    let prefix = format!("/{needle}");
    let ranked = fuzzy::go_keys_with(
        &needle,
        &registry.entries,
        &[
            |entry: &SlashEntry| Some(entry.display.clone()),
            |entry: &SlashEntry| Some(entry.description.clone()),
            |_: &SlashEntry| Some(String::new()),
        ],
        {
            let prefix = prefix.clone();
            move |entry, scores| {
                let mut score: f64 = scores.iter().sum();
                if entry.display.starts_with(&prefix) {
                    score *= 2.0;
                }
                score
            }
        },
        None,
    );
    ranked
        .into_iter()
        .take(10)
        .map(|ranked| AutocompleteOption {
            display: ranked.target.display.clone(),
            description: Some(ranked.target.description.clone()),
        })
        .collect()
}

/// The popup geometry (`height()`): `min(10, count, max(1, anchor_y))`.
#[must_use]
pub fn popup_height(count: usize, anchor_y: usize) -> usize {
    10.min(count.max(1)).min(anchor_y.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_triggers_only_at_line_head() {
        assert_eq!(trigger_for("/re", 3), Some((Mode::Command, 0)));
        assert_eq!(trigger_for("a /re", 5), None, "mid-line slash is not a trigger");
        assert_eq!(trigger_for("@fil", 4), Some((Mode::Reference, 0)));
        assert_eq!(trigger_for("hey @fil", 8), Some((Mode::Reference, 4)));
        assert_eq!(trigger_for("/re na", 6), None, "space closes the token");
    }

    #[test]
    fn slash_registry_pads_to_the_longest_display() {
        let registry = SlashRegistry::golden_harness();
        assert_eq!(registry.display_width(), 12, "'/worktrees' (10) + 2");
        assert!(registry.entries.iter().any(|entry| entry.display == "/redo"));
    }

    /// SA-F6: `/plugins` is a live slash command (the system plugin's
    /// `plugins.list` registration — feature-plugins/system/
    /// plugins.tsx:337-347): typing it opens the autocomplete popup,
    /// Enter selects and RUNS the Plugins dialog command.
    #[test]
    fn slash_registry_has_the_live_plugins_command() {
        let registry = SlashRegistry::v2();
        assert!(
            registry.entries.iter().any(|entry| entry.display == "/plugins"),
            "the v2 registry lists /plugins"
        );
        assert_eq!(
            SlashRegistry::command_for("/plugins"),
            Some("plugins.list"),
            "Enter on /plugins dispatches plugins.list (the Plugins dialog)"
        );
        // The filter narrows to exactly the typed command.
        let options = filter_commands(&registry, "plugins");
        assert!(
            options.iter().all(|option| option.display == "/plugins"),
            "typing /plugins leaves only the plugins entry: {:?}",
            options.iter().map(|o| &o.display).collect::<Vec<_>>()
        );
    }

    /// The golden's filter: `/re` ranks `/redo` then `/rename` (the
    /// prefix bonus ties; registry order breaks it).
    #[test]
    fn filter_re_orders_redo_then_rename() {
        let registry = SlashRegistry::golden_harness();
        let options = filter_commands(&registry, "re");
        assert!(
            options.len() >= 2,
            "matches: {:?}",
            options.iter().map(|o| &o.display).collect::<Vec<_>>()
        );
        assert_eq!(options[0].display, "/redo");
        assert_eq!(options[0].description.as_deref(), Some("Redo"));
        assert_eq!(options[1].display, "/rename");
        assert!(options.iter().all(|option| option.display != "/compact"));
    }

    #[test]
    fn typing_opens_and_space_closes() {
        let mut state = AutocompleteState::default();
        state.on_input("/", 1);
        assert_eq!(state.visible, Some(Mode::Command));
        state.on_input("/r", 2);
        state.on_input("/re", 3);
        assert_eq!(state.visible, Some(Mode::Command));
        assert_eq!(state.trigger_index, 0);
        // Space between trigger and cursor closes.
        state.on_input("/re ", 4);
        assert_eq!(state.visible, None);
        // The cursor before the trigger closes.
        let mut state = AutocompleteState::default();
        state.on_input("@file", 5);
        assert_eq!(state.visible, Some(Mode::Reference));
        state.on_input("@file", 0);
        assert_eq!(state.visible, None);
    }

    #[test]
    fn selection_wraps_and_clamps() {
        let options = vec![
            AutocompleteOption { display: "/a".into(), description: None },
            AutocompleteOption { display: "/b".into(), description: None },
        ];
        let mut state = AutocompleteState::default().with_options(options);
        state.move_selection(1);
        assert_eq!(state.selected, 1);
        state.move_selection(1);
        assert_eq!(state.selected, 0, "wrap policy");
        state.move_selection(-1);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn popup_height_clamps_to_the_anchor() {
        assert_eq!(popup_height(2, 2), 2);
        assert_eq!(popup_height(20, 2), 2);
        assert_eq!(popup_height(2, 30), 2);
        assert_eq!(popup_height(20, 30), 10);
    }
}
