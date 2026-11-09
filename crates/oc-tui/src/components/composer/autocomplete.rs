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
use std::fmt::Write as _; // format_push_string: write! into the existing String

/// Which trigger opened the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `/` — slash commands.
    Command,
    /// `@` — files, agents, skills, references.
    Reference,
    /// TUI-28 — the `/cd` argument trigger
    /// (`argumentAutocomplete: "directory"`, index.tsx:1957): the
    /// directory-recents list (empty query only — the live file
    /// listing arrives through G-127's async fetch sink,
    /// `directory_live_options` below).
    Directory,
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

/// TUI-28 — a row's two-press destructive action
/// (autocomplete.tsx:46: `{id, confirm, run}`): the recents rows' ctrl+d
/// delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destructive {
    /// The action's identity (the absolute directory) — the confirm
    /// state's key.
    pub id: String,
    /// The confirming row's replacement label ("Press ctrl+d to
    /// confirm").
    pub confirm: &'static str,
}

/// One rendered popup option.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutocompleteOption {
    pub display: String,
    pub description: Option<String>,
    /// RD-05 — the `@` rows' structured identity (`kind: "agent"` /
    /// `"skill"`): the accept path records the structured part
    /// instead of leaving plain text.
    pub reference: Option<Reference>,
    /// TUI-28 — the directory rows (`isDirectory`, autocomplete.tsx's
    /// complete/expand flow).
    pub is_directory: bool,
    /// The ABSOLUTE directory behind a recent row (the delete's id).
    pub absolute: Option<String>,
    /// TUI-28 — the two-press delete (the recents rows).
    pub destructive: Option<Destructive>,
    /// RE-03 — the file rows' relative path (autocomplete.tsx's
    /// `path` field on file options): the frecency lookup key for
    /// the reference-mode score multiplier, and the path the accept
    /// path records into the frecency store.
    pub path: Option<String>,
}

impl AutocompleteOption {
    /// A plain slash-command row (display + description).
    pub fn command(display: impl Into<String>, description: Option<String>) -> Self {
        Self { display: display.into(), description, ..Self::default() }
    }
}

/// RD-05 — one `@`-mention row's structured identity
/// (autocomplete.tsx:404-438): `kind: "agent"`/`"skill"`/`"reference"`
/// options carry the attachment value the accept path records
/// (`PromptInput`'s `agents`/`skills`/`files`), not just display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// `@<agent.id>` — the attachment value is `{name: agent.id}`.
    Agent { name: String },
    /// `@<skill.id>` — the attachment value is `{id: skill.id}`.
    Skill { id: String },
    /// G-6 — `@<reference.name>` (autocomplete.tsx:449-460, `kind:
    /// "reference"`): the accept path records a FILE part
    /// (`{type: "file", path: reference.name}`) — the wire `files`
    /// attachment.
    Source { name: String },
}

/// One `@`-mention agent source row (autocomplete.tsx:404-418: the
/// non-hidden, non-`primary` agents).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceAgent {
    pub id: String,
}

/// One `@`-mention skill source row (autocomplete.tsx:424-437).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSkill {
    pub id: String,
    pub description: Option<String>,
}

/// G-6 — one `@`-mention reference-source row (autocomplete.tsx:
/// 449-460, `referenceAliases`): the configured reference registry's
/// non-hidden entries. `detail` is the description's source text —
/// the git `repository` or the local `path` (ref:
/// `source.type === "git" ? source.repository : source.path`; the
/// server projects the same value onto the wire `path` field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSourceRow {
    pub name: String,
    pub detail: String,
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
    ("btw", "Ask a side question"),
    ("cd", "Change working directory"),
    ("clear", "Clear session"),
    ("compact", "Compact session"),
    ("connect", "Connect an integration"),
    ("continue", "Switch session"),
    ("copy", "Copy session transcript"),
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
    ("stats", "Usage statistics"),
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
    ("btw", "session.aside"),
    ("clear", "session.clear"),
    ("compact", "session.compact"),
    ("connect", "provider.connect"),
    ("continue", "session.list"),
    ("copy", "session.copy"),
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
    // RE-14 — `/stats` (stats.tsx:199): the usage poster page.
    ("stats", "stats.open"),
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
pub const ARGUMENT_SLASHES: &[(&str, &str)] =
    &[("cd", "session.cd"), ("rename", "session.rename"), ("btw", "session.aside")];

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

    /// G-186 — merge the server-provided commands
    /// (`data.location.command.list(location)`, autocomplete.tsx:
    /// 486-494): every server command appears as a `/{name}` row;
    /// names already covered by the static builtin table are SKIPPED
    /// (the mini footer's `hidden` dedupe, footer.prompt.tsx:542-544
    /// — the builtin entry wins), and the merged table stays sorted
    /// by display. The static entries remain when the server is
    /// unreachable (the merge is a no-op over an empty list).
    pub fn merge_server(&mut self, server: &[(String, Option<String>)]) {
        let known: std::collections::BTreeSet<String> =
            self.entries.iter().map(|entry| entry.display.clone()).collect();
        for (name, description) in server {
            let display = format!("/{name}");
            if known.contains(&display) {
                continue;
            }
            self.entries
                .push(SlashEntry { display, description: description.clone().unwrap_or_default() });
        }
        self.entries.sort_by(|a, b| a.display.cmp(&b.display));
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
    /// TUI-28 — the armed destructive confirm (`confirming`,
    /// autocomplete.tsx:629-638): the ctrl+d id whose SECOND press
    /// runs the delete.
    confirming: Option<String>,
    /// G-287⑨ — the accept path re-reads the trigger query (its
    /// `#N-N` line range reattaches to the accepted file chip).
    pub(super) last_query: Option<String>,
    /// RE-03 — the `@file` frecency store (frecency.tsx's provider on
    /// the popup): ranks the reference rows (score × (1 + frecency))
    /// and records accepts of path-carrying options.
    pub frecency: Frecency,
    /// G-127 — the last `fs.find` rows keyed by their query (the
    /// `createResource` `latest` cache, autocomplete.tsx:401-405:
    /// a refetch KEEPS the previous rows visible while loading; a
    /// new result REPLACES the cache and re-attaches).
    pub file_rows: Option<(String, Vec<AutocompleteOption>)>,
    /// G-127 — the last `directoryAutocomplete` rows keyed by their
    /// query (the same `latest` cache on the `/cd` path).
    pub dir_rows: Option<(String, Vec<AutocompleteOption>)>,
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
    /// prev/next commands). Moving off a row clears the armed
    /// destructive confirm (`setConfirming(undefined)`,
    /// autocomplete.tsx:613).
    pub fn move_selection(&mut self, delta: i8) {
        let count = self.filtered_len.max(self.options.len());
        if count == 0 {
            return;
        }
        let index = self.selected.cast_signed() + isize::from(delta);
        self.selected = index.rem_euclid(count.cast_signed()).max(0).cast_unsigned();
        self.confirming = None;
    }

    /// The armed destructive confirm (the confirming row renders the
    /// action's `confirm` label instead of its display).
    #[must_use]
    pub fn confirming(&self) -> Option<&str> {
        self.confirming.as_deref()
    }

    /// TUI-28 — the directory mode's live query (the caller attaches
    /// the recents rows only while it is EMPTY — index.tsx:1962).
    #[must_use]
    pub fn directory_query(&self) -> Option<&str> {
        if self.visible == Some(Mode::Directory) {
            Some(self.last_query.as_deref().unwrap_or(""))
        } else {
            None
        }
    }

    /// `triggerDestructive` (autocomplete.tsx:629-638): the FIRST
    /// ctrl+d arms the confirm (returns `None` — nothing ran); the
    /// SECOND runs the action — returns the removed entry's id, clamps
    /// the selection over the shortened list, and clears the confirm.
    /// `None` when the selected row carries no action.
    pub fn trigger_destructive(&mut self) -> Option<String> {
        let action = self.selected_option().and_then(|option| option.destructive.clone())?;
        if self.confirming.as_deref() != Some(action.id.as_str()) {
            self.confirming = Some(action.id.clone());
            return None;
        }
        let count = self.filtered_len.max(self.options.len());
        self.selected = self.selected.min(count.saturating_sub(2));
        self.confirming = None;
        Some(action.id)
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
        // TUI-28 — the `/cd` argument trigger
        // (`slashArgumentAutocomplete`, directory-completion.ts:42-65):
        // the head + a trailing space with the caret after it opens
        // the DIRECTORY list (the recents ride an empty query).
        if let Some((index, query)) = directory_trigger(text, caret) {
            let query_changed = self.last_query.as_deref() != Some(query.as_str());
            self.visible = Some(Mode::Directory);
            self.trigger_index = index;
            if query_changed {
                self.selected = 0;
                self.confirming = None;
                self.last_query = Some(query);
            }
            self.options = Vec::new();
            self.filtered_len = 0;
            return;
        }
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
                        self.confirming = None;
                        self.last_query = Some(query);
                    }
                }
            }
            (None, Some((mode, index))) => {
                self.visible = Some(mode);
                self.trigger_index = index;
                self.selected = 0;
                self.confirming = None;
                self.last_query = Some(query_between(text, index, caret));
            }
            _ => self.close(),
        }
        self.sync_options(text, caret);
    }

    /// Recompute `options` for the visible mode (command mode filters
    /// the registry; reference mode's live `fs.find` source — G-127's
    /// async half, `FileFindRow`/`merge_file_rows` below — merges in
    /// through the fetch sink, so the sync path itself stays empty).
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

/// One grapheme cluster's prompt-offset width (`promptOffsetWidth`'s
/// per-cluster term, display.ts:8-10): newlines count ONE (textarea
/// offsets), everything else its display width
/// ([`crate::wrap::display_width`]).
fn cluster_offset_width(cluster: &str) -> usize {
    if cluster == "\n" {
        1
    } else {
        crate::wrap::display_width(cluster)
    }
}

/// G-144 — `promptOffsetWidth` (prompt/display.ts:5-11): the
/// composer's offset coordinate space — DISPLAY columns with
/// newlines as one position.
#[must_use]
pub fn prompt_offset_width(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    text.graphemes(true).map(cluster_offset_width).sum()
}

/// G-144 — `displayOffsetIndex` (display.ts:13-25): the first
/// grapheme whose cumulative width would pass `offset` (the char
/// index of the display-column slice boundary).
fn display_char_index(text: &str, offset: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation as _;
    let mut width = 0usize;
    let mut chars = 0usize;
    for cluster in text.graphemes(true) {
        let next = width + cluster_offset_width(cluster);
        if next > offset {
            return chars;
        }
        width = next;
        chars += cluster.chars().count();
    }
    chars
}

/// G-144 — `displaySlice` (display.ts:27-31) as a char range.
fn display_char_range(text: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    display_char_index(text, start)..display_char_index(text, end)
}

/// G-144 — `mentionTriggerIndex` (display.ts:41-50): the last `@` in
/// the display slice before `offset`, valid when the preceding char
/// is start-or-whitespace and the query carries no whitespace.
/// Returns the anchor's DISPLAY column.
#[must_use]
pub fn mention_trigger_index(text: &str, offset: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let range = display_char_range(text, 0, offset);
    let slice = &chars[range];
    let index = slice.iter().rposition(|ch| *ch == '@')?;
    let before = if index == 0 { None } else { Some(slice[index - 1]) };
    let query: String = slice[index..].iter().collect();
    let before_ok = before.is_none_or(char::is_whitespace);
    if before_ok && !query.chars().any(char::is_whitespace) {
        let prefix: String = slice[..index].iter().collect();
        Some(prompt_offset_width(&prefix))
    } else {
        None
    }
}

/// G-144 — `slashTriggerIndex` (display.ts:52-61): scan `/` backwards
/// through the display slice; skip anchors with a non-whitespace
/// predecessor; the query must carry no whitespace and no second
/// `/`. Returns the anchor's DISPLAY column.
#[must_use]
pub fn slash_trigger_index(text: &str, offset: usize) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let range = display_char_range(text, 0, offset);
    let slice = &chars[range];
    let mut search = slice.len();
    while let Some(index) = slice[..search].iter().rposition(|ch| *ch == '/') {
        let before = if index == 0 { None } else { Some(slice[index - 1]) };
        let query: String = slice[index..].iter().collect();
        if before.is_some_and(|ch| !ch.is_whitespace()) {
            // `continue` — retry the next earlier `/`.
            search = index;
            continue;
        }
        let query_chars: Vec<char> = query.chars().collect();
        if query_chars.iter().any(|ch| ch.is_whitespace()) || query_chars[1..].contains(&'/') {
            return None;
        }
        let prefix: String = slice[..index].iter().collect();
        return Some(prompt_offset_width(&prefix));
    }
    None
}

/// `slashTriggerIndex` / `mentionTriggerIndex` over the composer's
/// char-index caret: the display-column anchors (G-144) mapped back
/// to char indices for the internal popup seam. The RIGHTMOST anchor
/// wins (the ref's two probes each scan from the caret backward).
fn trigger_for(text: &str, caret: usize) -> Option<(Mode, usize)> {
    let prefix: String = text.chars().take(caret).collect();
    let offset = prompt_offset_width(&prefix);
    let candidates = [
        slash_trigger_index(text, offset).map(|column| (Mode::Command, column)),
        mention_trigger_index(text, offset).map(|column| (Mode::Reference, column)),
    ];
    // Char index of the grapheme starting at `column`.
    let column_to_char = |column: usize| -> usize {
        use unicode_segmentation::UnicodeSegmentation as _;
        let mut width = 0usize;
        let mut chars = 0usize;
        for cluster in prefix.graphemes(true) {
            if width >= column {
                break;
            }
            width += cluster_offset_width(cluster);
            chars += cluster.chars().count();
        }
        chars
    };
    candidates
        .into_iter()
        .flatten()
        .max_by_key(|(_, column)| *column)
        .map(|(mode, column)| (mode, column_to_char(column)))
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

/// TUI-28 — `slashArgumentAutocomplete` (directory-completion.ts:
/// 42-65): the draft's `/cd` head + at least one trailing char with
/// the caret after it (v2: `head.end !== beforeCursor.length` — the
/// cursor sits past the head's separator). Returns the trigger index
/// (right after the head, where the popup anchors) + the query (the
/// text between the head's separator and the caret).
#[must_use]
pub fn directory_trigger(text: &str, caret: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let end = caret.min(chars.len());
    let head: String = chars.iter().take(end).collect();
    let name = head.strip_prefix('/')?;
    let Some(at) = name.find(char::is_whitespace) else {
        return None; // no trailing separator: the command popup owns it
    };
    let (head_name, rest) = (&name[..at], &name[at..]);
    // `argumentAutocomplete: (command) => command.id === "session.cd"
    // ? "directory" : undefined` (index.tsx:1957): ONLY `/cd` carries
    // the directory list (`/rename`'s argument popup is undefined).
    if head_name != "cd" {
        return None;
    }
    let index = 1 + head_name.chars().count(); // right after `/cd`
    Some((index, rest[1..].to_owned()))
}

/// TUI-28 — one recent-directory entry (`RecentDirectory`,
/// directory-recents.ts:3-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDirectory {
    pub directory: String,
    /// `usedAt` (ms epoch) — the list's descending sort key.
    pub used_at: u64,
}

/// TUI-28 — `useDirectoryRecents` (prompt/directory-recents.ts): the
/// per-project recent-directories store (`useStorage().store(
/// "directory-recents")` shape): `touch` upserts at the head with a
/// fresh `usedAt` (dedup + the 10-entry cap), `remove` drops, `list`
/// returns `usedAt`-descending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryRecents {
    projects: std::collections::HashMap<String, Vec<RecentDirectory>>,
}

impl DirectoryRecents {
    /// `list(projectID)`: `usedAt`-descending.
    #[must_use]
    pub fn list(&self, project_id: &str) -> Vec<RecentDirectory> {
        let mut entries = self.projects.get(project_id).cloned().unwrap_or_default();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.used_at));
        entries
    }

    /// `touch(projectID, directory)` with an explicit clock (the
    /// production caller stamps `Date.now()`): head-insert with the
    /// old entry dropped, capped at 10.
    pub fn touch(&mut self, project_id: &str, directory: &str, used_at: u64) {
        let entries = self.projects.entry(project_id.to_owned()).or_default();
        entries.retain(|item| item.directory != directory);
        entries.insert(0, RecentDirectory { directory: directory.to_owned(), used_at });
        entries.truncate(10);
    }

    /// `remove(projectID, directory)`.
    pub fn remove(&mut self, project_id: &str, directory: &str) {
        if let Some(entries) = self.projects.get_mut(project_id) {
            entries.retain(|item| item.directory != directory);
        }
    }
}

/// RE-03 — one frecency record (`FrecencyEntry`, frecency.tsx:8):
/// `{path, frequency, lastOpen}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrecencyEntry {
    pub frequency: u64,
    /// `lastOpen` (ms epoch).
    pub last_open: i64,
}

/// RE-03 — the `@file` frecency store (`prompt/frecency.tsx`): an
/// in-memory `path → {frequency, lastOpen}` map loaded from (and
/// appended back to) the state root's `frecency.jsonl`
/// (storage.rs's `parse_frecency` is the boot read side). When
/// `file` is attached (production boot), every `update` APPENDS one
/// JSONL line (the ref's fire-and-forget `appendText`) and the
/// over-cap pass REWRITES the compacted top-1000 (`writeText`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frecency {
    pub data: std::collections::HashMap<String, FrecencyEntry>,
    /// `<state-root>/frecency.jsonl` — the persistence sink (`None`
    /// in unit tests / headless composers).
    pub file: Option<std::path::PathBuf>,
}

/// `path.resolve(paths.cwd, filePath)` (frecency.tsx:58/75): the
/// store keys files by their ABSOLUTE path — relative inputs join
/// the process cwd, then `.`/`..` normalize lexically.
#[must_use]
pub fn resolve_frecency_path(cwd: &std::path::Path, file: &str) -> String {
    use std::path::Component;
    let joined = if std::path::Path::new(file).is_absolute() {
        std::path::PathBuf::from(file)
    } else {
        cwd.join(file)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

/// Best-effort JSONL append (`appendText`): create-on-first-write.
fn append_line(file: &std::path::Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut handle = std::fs::OpenOptions::new().create(true).append(true).open(file)?;
    handle.write_all(line.as_bytes())
}

/// Atomic rewrite (`writeText` — tmp + rename like storage.rs's heal).
fn rewrite_file(file: &std::path::Path, body: &str) -> std::io::Result<()> {
    let temporary = file.with_extension(format!("frecency.{}.tmp", std::process::id()));
    std::fs::write(&temporary, body).and_then(|()| std::fs::rename(&temporary, file))
}

impl Frecency {
    /// The store over parsed `frecency.jsonl` lines (the v2 provider's
    /// onMount load — `parseFrecency` output keyed by `path`).
    #[must_use]
    pub fn from_parsed(entries: &[serde_json::Value]) -> Self {
        let data = entries
            .iter()
            .filter_map(|entry| {
                let path = entry.get("path")?.as_str()?.to_owned();
                let frequency = entry.get("frequency")?.as_u64()?;
                let last_open = entry.get("lastOpen")?.as_i64()?;
                Some((path, FrecencyEntry { frequency, last_open }))
            })
            .collect();
        Self { data, file: None }
    }

    /// The runtime store (frecency.tsx:43-53's onMount): load
    /// `<state-root>/frecency.jsonl` and attach it as the update
    /// sink (the boot heal already rewrote the compacted file).
    #[must_use]
    pub fn open(state_root: &std::path::Path) -> Self {
        let file = state_root.join(crate::storage::FILE_FRECENCY);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let mut store = Self::from_parsed(&crate::storage::parse_frecency(&text));
        store.file = Some(file);
        store
    }

    /// `calculateFrecency` (frecency.tsx:33-36):
    /// `frequency / (1 + Δdays)` where Δdays is the days since
    /// `lastOpen`; an absent path scores 0.
    #[must_use]
    pub fn get(&self, path: &str, now_ms: i64) -> f64 {
        let Some(entry) = self.data.get(path) else { return 0.0 };
        let delta_days = (now_ms - entry.last_open) as f64 / 86_400_000.0;
        entry.frequency as f64 / (1.0 + delta_days)
    }

    /// `updateFrecency` (frecency.tsx:57-72): the ABSOLUTE path
    /// (`path.resolve(cwd, …)`) gets frequency+1 with a fresh
    /// `lastOpen`; with a sink attached, one JSONL line is appended
    /// (fire-and-forget) and a store over
    /// [`MAX_FRECENCY_ENTRIES`][crate::storage::MAX_FRECENCY_ENTRIES]
    /// is compacted by rewriting the `lastOpen`-desc top-1000.
    pub fn update(&mut self, path: &str, now_ms: i64) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
        let key = resolve_frecency_path(&cwd, path);
        let frequency = self.data.get(&key).map_or(0, |entry| entry.frequency) + 1;
        self.data.insert(key.clone(), FrecencyEntry { frequency, last_open: now_ms });
        if let Some(file) = self.file.clone() {
            let line = format!(
                "{{\"path\":{},\"frequency\":{frequency},\"lastOpen\":{now_ms}}}\n",
                serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into())
            );
            let _ = append_line(&file, &line);
        }
        if self.data.len() > crate::storage::MAX_FRECENCY_ENTRIES {
            let mut sorted: Vec<(String, FrecencyEntry)> =
                self.data.iter().map(|(k, v)| (k.clone(), *v)).collect();
            sorted.sort_by_key(|(_, entry)| std::cmp::Reverse(entry.last_open));
            sorted.truncate(crate::storage::MAX_FRECENCY_ENTRIES);
            self.data = sorted.iter().cloned().collect();
            if let Some(file) = self.file.clone() {
                let body = sorted
                    .iter()
                    .map(|(path, entry)| {
                        format!(
                            "{{\"path\":{},\"frequency\":{},\"lastOpen\":{}}}\n",
                            serde_json::to_string(path).unwrap_or_else(|_| "\"\"".into()),
                            entry.frequency,
                            entry.last_open
                        )
                    })
                    .collect::<String>();
                let _ = rewrite_file(&file, &body);
            }
        }
    }

    /// The JSONL rewrite (`writeText`, frecency.tsx:68-71): one
    /// `{path, frequency, lastOpen}` line per entry in insertion
    /// order.
    #[must_use]
    pub fn encode_jsonl(&self) -> String {
        let mut text = String::new();
        for (path, entry) in &self.data {
            let _ = write!(
                text,
                "{{\"path\":{},\"frequency\":{},\"lastOpen\":{}}}\n",
                serde_json::to_string(path).unwrap_or_else(|_| "\"\"".into()),
                entry.frequency,
                entry.last_open
            );
        }
        text
    }
}

/// RE-03 — the reference-mode scoreFn's frecency factor
/// (autocomplete.tsx:569-570): `score * (1 + getFrecency(path))`,
/// an identity when the option carries no path (commands/agents/
/// skills).
#[must_use]
pub fn frecency_weighted(
    score: f64,
    path: Option<&str>,
    frecency: Option<&Frecency>,
    now_ms: i64,
) -> f64 {
    match (path, frecency) {
        (Some(path), Some(frecency)) => score * (1.0 + frecency.get(path, now_ms)),
        _ => score,
    }
}

/// TUI-28 — `directoryRecentValue` (directory-completion.ts:92-98):
/// the `~`-relative display (empty relative = `~`, a clean relative
/// beneath home = `~/…`, anything else absolute).
#[must_use]
pub fn directory_recent_value(directory: &str, home: &str) -> String {
    if home.is_empty() || !directory.starts_with(home) {
        return directory.to_owned();
    }
    let relative = &directory[home.len()..];
    if relative.is_empty() {
        return "~".to_owned();
    }
    let relative = relative.strip_prefix('/').unwrap_or(relative);
    if relative.starts_with("..") || relative.is_empty() {
        return directory.to_owned();
    }
    format!("~/{relative}")
}

/// TUI-28 — the recents as popup options (`directoryOptions`,
/// index.tsx:1962-1985): ONE row per recent — the `~`-relative
/// display, the `recent` description, the directory flags, and the
/// two-press destructive delete (`run: directoryRecents.remove`).
/// Empty-query only (the caller checks; the live file-list half is
/// the async `fs.find` source — G-127, landed below as `FileFindRow`/
/// `file_find_options`/`merge_file_rows`).
#[must_use]
pub fn directory_recents_options(
    recents: &[RecentDirectory],
    home: &str,
) -> Vec<AutocompleteOption> {
    recents
        .iter()
        .map(|item| {
            let value = directory_recent_value(&item.directory, home);
            AutocompleteOption {
                display: value.clone(),
                description: Some("recent".to_owned()),
                reference: None,
                is_directory: true,
                absolute: Some(item.directory.clone()),
                path: None,
                destructive: Some(Destructive {
                    id: item.directory.clone(),
                    confirm: "Press ctrl+d to confirm",
                }),
            }
        })
        .collect()
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
                ..Default::default()
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
            ..Default::default()
        })
        .collect()
}

/// RD-05 — the `@` reference rows (autocomplete.tsx:404-460 + the
/// `options` memo :535-592): skills first, then the reference sources
/// (`referenceAliases`), then the agents — the ref ordering
/// `[...skillOptions, ...referenceAliases, ...agents]`. An empty
/// query lists everything; otherwise a case-insensitive containment
/// filter over `@<id>` with a prefix boost, capped at the ref's popup
/// limit (10).
#[must_use]
pub fn filter_references(
    agents: &[ReferenceAgent],
    skills: &[ReferenceSkill],
    references: &[ReferenceSourceRow],
    query: &str,
) -> Vec<AutocompleteOption> {
    filter_references_ranked(agents, skills, references, query, None, 0)
}

/// RE-03 — the ranked form of [`filter_references`]: the score
/// (prefix boost) is multiplied by `(1 + frecency)` for options
/// carrying a `path` — the ref scoreFn's factor
/// (autocomplete.tsx:569-570: `score * (1 + frecencyScore)`).
#[must_use]
pub fn filter_references_ranked(
    agents: &[ReferenceAgent],
    skills: &[ReferenceSkill],
    references: &[ReferenceSourceRow],
    query: &str,
    frecency: Option<&Frecency>,
    now_ms: i64,
) -> Vec<AutocompleteOption> {
    let skill_rows = skills.iter().map(|skill| AutocompleteOption {
        display: format!("@{}", skill.id),
        description: skill.description.clone(),
        reference: Some(Reference::Skill { id: skill.id.clone() }),
        ..AutocompleteOption::default()
    });
    // G-6 — `referenceAliases` (autocomplete.tsx:449-460): the
    // non-hidden entries, display `@<name>`, description ` <detail>`
    // (the LEADING SPACE is part of the ref's description string).
    // The rows carry no `path` (only file rows do) — the frecency
    // factor is an identity here.
    let reference_rows = references.iter().map(|reference| AutocompleteOption {
        display: format!("@{}", reference.name),
        description: Some(format!(" {}", reference.detail)),
        reference: Some(Reference::Source { name: reference.name.clone() }),
        ..AutocompleteOption::default()
    });
    let agent_rows = agents.iter().map(|agent| AutocompleteOption {
        display: format!("@{}", agent.id),
        description: None,
        reference: Some(Reference::Agent { name: agent.id.clone() }),
        ..AutocompleteOption::default()
    });
    let all: Vec<AutocompleteOption> = skill_rows.chain(reference_rows).chain(agent_rows).collect();
    rank_reference_options(all, query, frecency, now_ms)
}

/// RE-03 — rank pre-built `@` reference rows (the ref scoreFn,
/// autocomplete.tsx:553-571, over the non-file option set): the
/// containment filter with the `@{query}` prefix boost, the score
/// multiplied by `(1 + getFrecency(path))` when the option carries
/// a `path` (files/references), sliced to 10, ties by display.
#[must_use]
pub fn rank_reference_options(
    options: Vec<AutocompleteOption>,
    query: &str,
    frecency: Option<&Frecency>,
    now_ms: i64,
) -> Vec<AutocompleteOption> {
    if query.trim().is_empty() {
        return options;
    }
    let needle = query.to_lowercase();
    let prefix = format!("@{needle}");
    let mut ranked: Vec<(f64, AutocompleteOption)> = options
        .into_iter()
        .filter_map(|option| {
            let target = option.display.to_lowercase();
            if !target.contains(&needle) {
                return None;
            }
            // Base score 1 for a containment match (+1 more on the
            // `@{query}` prefix), so the frecency factor
            // (autocomplete.tsx:569-570) can reorder matched rows.
            let boost = 1.0 + f64::from(u8::from(target.starts_with(&prefix)));
            Some((frecency_weighted(boost, option.path.as_deref(), frecency, now_ms), option))
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.display.cmp(&b.1.display))
    });
    ranked.into_iter().map(|(_, option)| option).take(10).collect()
}

/// Wall-clock epoch ms (`Date.now()`) — the frecency store's clock.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

/// The popup geometry (`height()`): `min(10, count, max(1, anchor_y))`.
#[must_use]
pub fn popup_height(count: usize, anchor_y: usize) -> usize {
    10.min(count.max(1)).min(anchor_y.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G-186 — the server command list merges into the candidates
    /// (`location.command.list`, autocomplete.tsx:486-494): a custom
    /// `/deploy` row appears beside the builtins, a name already in
    /// the static table is NOT duplicated (the builtin wins), and an
    /// EMPTY server list (unreachable server) leaves the builtins
    /// alone.
    #[test]
    fn server_commands_merge_into_the_slash_candidates() {
        let mut registry = SlashRegistry::v2();
        let builtin_len = registry.entries.len();
        registry.merge_server(&[
            ("deploy".to_owned(), Some("Run the deploy playbook".to_owned())),
            ("new".to_owned(), Some("shadowed by the builtin".to_owned())),
            ("zz-custom".to_owned(), None),
        ]);
        assert_eq!(registry.entries.len(), builtin_len + 2, "only the unknown names append");
        let displays: Vec<&str> =
            registry.entries.iter().map(|entry| entry.display.as_str()).collect();
        assert!(displays.contains(&"/deploy"), "the custom command appears");
        assert!(displays.contains(&"/zz-custom"), "a description-less command appears too");
        assert_eq!(displays.iter().filter(|name| **name == "/new").count(), 1, "the builtin wins");
        let deploy = registry.entries.iter().find(|entry| entry.display == "/deploy").unwrap();
        assert_eq!(deploy.description, "Run the deploy playbook");
        // Sorted by display.
        assert!(registry.entries.windows(2).all(|pair| pair[0].display <= pair[1].display));
        // The custom entry is INSERTED on accept, never run as a dot
        // command (no builtin mapping).
        assert_eq!(SlashRegistry::command_for("/deploy"), None);
        // filter_commands surfaces it under a query.
        let options = filter_commands(&registry, "deploy");
        assert!(
            options.iter().any(|option| option.display.trim() == "/deploy"),
            "the custom command is a candidate: {options:?}"
        );

        // Unreachable server: the merge over nothing changes nothing.
        let mut fallback = SlashRegistry::v2();
        let before = fallback.clone();
        fallback.merge_server(&[]);
        assert_eq!(fallback, before, "builtins only when the server is unreachable");
    }

    #[test]
    fn slash_and_mention_anchors_follow_the_display_ts_rules() {
        // G-144 — display.ts:41-61 semantics: `/` anchors after ANY
        // whitespace (not just a line head — the ref's
        // `before !== undefined && !/\s/.test(before)` skip), `@`
        // requires a start-or-whitespace predecessor, and either
        // query closes on whitespace.
        assert_eq!(trigger_for("/re", 3), Some((Mode::Command, 0)));
        assert_eq!(trigger_for("a /re", 5), Some((Mode::Command, 2)));
        assert_eq!(trigger_for("a/re", 4), None, "no whitespace before the slash");
        assert_eq!(trigger_for("@fil", 4), Some((Mode::Reference, 0)));
        assert_eq!(trigger_for("hey @fil", 8), Some((Mode::Reference, 4)));
        assert_eq!(trigger_for("hey@fil", 7), None, "no whitespace before the @");
        assert_eq!(trigger_for("/re na", 6), None, "space closes the token");
        assert_eq!(trigger_for("/re/na", 6), None, "a second slash closes the token");
    }

    #[test]
    fn wide_char_mixed_anchors_match_display_columns() {
        // G-144 — the anchors are display columns: a wide char before
        // the trigger shifts the column (not the char count), and the
        // char-index mapping back lands on the trigger itself.
        assert_eq!(prompt_offset_width("你a"), 3);
        assert_eq!(mention_trigger_index("你 @fil", prompt_offset_width("你 @fil")), Some(3));
        assert_eq!(slash_trigger_index("你 /re", prompt_offset_width("你 /re")), Some(3));
        // The display COLUMN is 3 (the wide glyph shifts it past the
        // char count); the internal seam maps back to the trigger's
        // CHAR index (2).
        assert_eq!(trigger_for("你 @fil", 6), Some((Mode::Reference, 2)));
        assert_eq!(trigger_for("你 /re", 5), Some((Mode::Command, 2)));
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
            AutocompleteOption { display: "/a".into(), description: None, ..Default::default() },
            AutocompleteOption { display: "/b".into(), description: None, ..Default::default() },
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

    /// RD-05 — the `@` rows: skills before the reference sources
    /// before agents (the ref ordering), an empty query lists
    /// everything, and a query filters `@<id>` case-insensitively
    /// with the structured identity attached.
    #[test]
    fn filter_references_orders_and_filters() {
        let agents =
            vec![ReferenceAgent { id: "build".into() }, ReferenceAgent { id: "plan".into() }];
        let skills = vec![ReferenceSkill { id: "refactor".into(), description: Some("d".into()) }];
        let references =
            vec![ReferenceSourceRow { name: "docs".into(), detail: "/tmp/docs".into() }];
        let all = filter_references(&agents, &skills, &references, "");
        assert_eq!(
            all.iter().map(|option| option.display.clone()).collect::<Vec<_>>(),
            vec!["@refactor", "@docs", "@build", "@plan"]
        );
        assert_eq!(all[0].reference, Some(Reference::Skill { id: "refactor".into() }));
        // G-6 — the reference row: `kind: "reference"`, the detail as
        // the description WITH the ref's leading space.
        assert_eq!(all[1].reference, Some(Reference::Source { name: "docs".into() }));
        assert_eq!(all[1].description.as_deref(), Some(" /tmp/docs"));
        assert_eq!(all[2].reference, Some(Reference::Agent { name: "build".into() }));

        let filtered = filter_references(&agents, &skills, &references, "BU");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display, "@build");
    }

    /// RE-03 — `calculateFrecency` (frecency.tsx:33-36):
    /// `frequency / (1 + Δdays)`, 0 for an unknown path.
    #[test]
    fn frecency_decays_frequency_over_days() {
        let day = 86_400_000i64;
        let mut frecency = Frecency::default();
        frecency
            .data
            .insert("src/main.rs".to_owned(), FrecencyEntry { frequency: 9, last_open: 100 * day });
        assert_eq!(frecency.get("src/main.rs", 100 * day), 9.0, "same day: no decay");
        assert!((frecency.get("src/main.rs", 103 * day) - 9.0 / 4.0).abs() < 1e-9, "3 days later");
        assert_eq!(frecency.get("other.rs", 100 * day), 0.0, "unknown path scores 0");
        // The scoreFn factor: score × (1 + frecency) (autocomplete.
        // tsx:569-570); an identity without a path.
        assert!(
            (frecency_weighted(2.0, Some("src/main.rs"), Some(&frecency), 100 * day) - 20.0).abs()
                < 1e-9
        );
        assert_eq!(frecency_weighted(2.0, None, Some(&frecency), 0), 2.0);
    }

    /// RE-03 — a frequent recent file outranks a rare old one at the
    /// same base score (the `(1 + frecency)` factor reorders rows).
    #[test]
    fn frecency_ranks_frequent_recent_files_first() {
        let day = 86_400_000i64;
        let now = 100 * day;
        let mut frecency = Frecency::default();
        frecency.data.insert(
            "old/rare.rs".to_owned(),
            FrecencyEntry { frequency: 1, last_open: now - 10 * day },
        );
        frecency
            .data
            .insert("src/hot.rs".to_owned(), FrecencyEntry { frequency: 20, last_open: now });
        let file = |display: &str, path: &str| AutocompleteOption {
            display: format!("@{display}"),
            path: Some(path.to_owned()),
            ..AutocompleteOption::default()
        };
        let ranked = rank_reference_options(
            vec![file("readme", "old/rare.rs"), file("readme", "src/hot.rs")],
            "readme",
            Some(&frecency),
            now,
        );
        assert_eq!(
            ranked[0].path.as_deref(),
            Some("src/hot.rs"),
            "hot file's (1+20) beats (1+1/11)"
        );
        assert_eq!(ranked[1].path.as_deref(), Some("old/rare.rs"));
    }

    /// RE-03 — `updateFrecency` (frecency.tsx:57-72): frequency+1
    /// with a fresh `lastOpen`, and the parsed-JSONL load.
    #[test]
    fn frecency_update_increments_and_reloads() {
        let mut frecency = Frecency::default();
        let key = resolve_frecency_path(&std::env::current_dir().unwrap_or_default(), "a.rs");
        frecency.update("a.rs", 1_000);
        assert_eq!(frecency.data[&key], FrecencyEntry { frequency: 1, last_open: 1_000 });
        frecency.update("a.rs", 2_000);
        assert_eq!(frecency.data[&key], FrecencyEntry { frequency: 2, last_open: 2_000 });

        let parsed = crate::storage::parse_frecency(
            "{\"path\":\"/abs/a.rs\",\"frequency\":5,\"lastOpen\":9000}\n{\"path\":\"/abs/a.rs\",\"frequency\":6,\"lastOpen\":9500}\n",
        );
        let reloaded = Frecency::from_parsed(&parsed);
        assert_eq!(
            reloaded.data["/abs/a.rs"],
            FrecencyEntry { frequency: 6, last_open: 9500 },
            "last line wins"
        );
        assert!(reloaded.encode_jsonl().contains("\"frequency\":6"));
    }

    /// RE-03 — keys are ABSOLUTE (`path.resolve(cwd, filePath)`,
    /// frecency.tsx:58/75): relative joins the cwd, `.`/`..`
    /// normalize, absolute stays.
    #[test]
    fn frecency_keys_are_absolute() {
        assert_eq!(
            resolve_frecency_path(std::path::Path::new("/proj"), "src/main.rs"),
            "/proj/src/main.rs"
        );
        assert_eq!(
            resolve_frecency_path(std::path::Path::new("/proj"), "./src/../lib/a.rs"),
            "/proj/lib/a.rs"
        );
        assert_eq!(
            resolve_frecency_path(std::path::Path::new("/proj"), "/elsewhere/a.rs"),
            "/elsewhere/a.rs"
        );
        let mut frecency = Frecency::default();
        frecency.update("src/main.rs", 5_000);
        let key = resolve_frecency_path(&std::env::current_dir().unwrap(), "src/main.rs");
        assert_eq!(frecency.data.get(&key).map(|entry| entry.frequency), Some(1));
    }

    /// RE-03 — persistence: `update` APPENDS one JSONL line per
    /// update (the ref's fire-and-forget `appendText`,
    /// frecency.tsx:61) with the absolute key, and `open` reloads it.
    #[test]
    fn frecency_update_appends_and_reloads_across_restart() {
        let dir =
            std::env::temp_dir().join(format!("oc-tui-frecency-append-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut frecency = Frecency::open(&dir);
        frecency.update("src/hot.rs", 1_000);
        frecency.update("src/hot.rs", 2_000);
        let file = dir.join(crate::storage::FILE_FRECENCY);
        let text = std::fs::read_to_string(&file).expect("appended frecency.jsonl");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one append per update");
        let key = serde_json::to_string(&resolve_frecency_path(
            &std::env::current_dir().unwrap(),
            "src/hot.rs",
        ))
        .unwrap();
        assert!(lines[0].contains(&key), "absolute key: {}", lines[0]);
        assert!(lines[0].ends_with("\"frequency\":1,\"lastOpen\":1000}"));
        assert!(lines[1].ends_with("\"frequency\":2,\"lastOpen\":2000}"));

        // Restart: the store reloads (last line wins per path).
        let reloaded = Frecency::open(&dir);
        let hot = resolve_frecency_path(&std::env::current_dir().unwrap(), "src/hot.rs");
        assert_eq!(
            reloaded.data.get(&hot),
            Some(&FrecencyEntry { frequency: 2, last_open: 2_000 }),
            "restart keeps the recorded frecency"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RE-03 — compaction at >1000 (frecency.tsx:63-71): the 1001st
    /// update rewrites the file with the `lastOpen`-descending top
    /// 1000 (the oldest entry dropped).
    #[test]
    fn frecency_compacts_the_file_at_1001() {
        let dir = std::env::temp_dir().join(format!("oc-tui-frecency-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        let mut frecency = Frecency::open(&dir);
        for index in 0..crate::storage::MAX_FRECENCY_ENTRIES {
            frecency.update(&format!("/old/file-{index}.rs"), 1_000 + index as i64);
        }
        let file = dir.join(crate::storage::FILE_FRECENCY);
        let before = std::fs::read_to_string(&file).expect("appends");
        assert_eq!(before.lines().count(), crate::storage::MAX_FRECENCY_ENTRIES);

        // The 1001st distinct path pushes the store over the cap →
        // the file is REWRITTEN compacted (newest 1000 kept).
        frecency.update("/fresh/new.rs", 5_000_000);
        let after = std::fs::read_to_string(&file).expect("compacted frecency.jsonl");
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.len(), crate::storage::MAX_FRECENCY_ENTRIES, "compacted to the cap");
        assert!(after.contains("/fresh/new.rs"), "the newest survives");
        assert!(!after.contains("/old/file-0.rs"), "the oldest drops");
        assert_eq!(frecency.data.len(), crate::storage::MAX_FRECENCY_ENTRIES);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod btw_tests {
    use super::*;

    /// RE-13: `/btw` takes arguments (btw.tsx:57 — `slash: { name:
    /// "btw", arguments: true }`): the head intercepts the submit
    /// with the remaining text as `input`.
    #[test]
    fn btw_is_an_argument_slash() {
        assert_eq!(argument_slash("/btw what is this?"), Some(("session.aside", " what is this?")));
        assert_eq!(argument_slash("/btw"), Some(("session.aside", "")));
        // The registry + display table carry the row.
        assert!(SLASH_COMMANDS.contains(&("btw", "session.aside")));
        assert!(V2_SLASH.contains(&("btw", "Ask a side question")));
    }

    /// RE-14: `/stats` is a plain (no-argument) slash command
    /// (stats.tsx:199 — `slash: { name: "stats" }`): the head
    /// dispatches `stats.open`, the draft never routes into the
    /// composer.
    #[test]
    fn stats_slash_routes_to_the_stats_page() {
        assert!(SLASH_COMMANDS.contains(&("stats", "stats.open")));
        assert!(V2_SLASH.contains(&("stats", "Usage statistics")), "the display row");
        assert!(!ARGUMENT_SLASHES.contains(&("stats", "stats.open")), "no argument");
    }
}

// -- G-127: the async halves (@-file find + /cd live listing) --------------

/// G-127 — one `fs.find` result row (`FileSystemEntry`): the
/// location-relative `path` + the entry kind (autocomplete.tsx:
/// 361-385 consumes `result.data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFindRow {
    pub path: String,
    pub is_directory: bool,
}

/// G-127 — strip a `path#12-34` line-range suffix
/// (`parseFileLineRange(...).base`, prompt/parse.ts): the fuzzy-find
/// query is the path half only. G-287⑨ — the full parse port: the
/// `#N`/`#N-N`/`#N-` suffix validates against
/// `/^(\d+)(?:-(\d*))?$/` (an invalid suffix keeps the whole input
/// as the base — the hash only strips on a MATCH, parse.ts:1-13),
/// and `end` only survives when `start < end` (the `#5-3` collapse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileLineRange {
    pub start_line: u32,
    pub end_line: Option<u32>,
}

/// G-287⑨ — `parseFileLineRange` (prompt/parse.ts:1-13).
#[must_use]
pub fn parse_file_line_range(input: &str) -> (String, Option<FileLineRange>) {
    let Some(hash) = input.rfind('#') else {
        return (input.to_owned(), None);
    };
    let base = input[..hash].to_owned();
    let suffix = &input[hash + 1..];
    let Some((start, end)) = suffix.split_once('-') else {
        // `#N` alone.
        return match start_end_line(suffix, None) {
            Some(range) => (base, Some(range)),
            None => (base, None),
        };
    };
    let range = start_end_line(start, Some(end));
    match range {
        Some(range) => (base, Some(range)),
        None => (base, None),
    }
}

/// The validated `{startLine, endLine}` pair: digits only; `end`
/// survives just when it parsed AND `start < end` (parse.ts:7-8).
fn start_end_line(start: &str, end: Option<&str>) -> Option<FileLineRange> {
    let start_line = parse_line_number(start)?;
    let end_line = match end {
        // `#N-` — the open-ended `\d*` empty match.
        None | Some("") => None,
        Some(text) => {
            let end = parse_line_number(text)?;
            (start_line < end).then_some(end)
        }
    };
    Some(FileLineRange { start_line, end_line })
}

fn parse_line_number(text: &str) -> Option<u32> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten()
}

/// G-127 — the query base (see [`parse_file_line_range`]).
#[must_use]
pub fn file_query_base(query: &str) -> &str {
    match query.rfind('#') {
        Some(at) => &query[..at],
        None => query,
    }
}

/// G-127 — the `fs.find` rows as popup options (autocomplete.tsx:
/// 364-385: `createFilePart(item, path.join(location.directory,
/// item.path))`): display = the relative path (files arrive from fff
/// already fuzzy-ranked — NO additional fuzzysort pass, the ref's
/// comment at :536-538), the directory flag, and the structured
/// file-part identity so accept records the wire `files` attachment
/// and bumps frecency on `part.path` (:262).
#[must_use]
pub fn file_find_options(rows: &[FileFindRow]) -> Vec<AutocompleteOption> {
    rows.iter()
        .map(|row| AutocompleteOption {
            display: row.path.clone(),
            description: None,
            reference: Some(Reference::Source { name: row.path.clone() }),
            is_directory: row.is_directory,
            absolute: None,
            path: Some(row.path.clone()),
            destructive: None,
        })
        .collect()
}

/// G-127 — merge the live file rows behind the static `@` rows (the
/// ref's options memo, autocomplete.tsx:536-591:
/// `[...fuzziedNonFiles, ...fileOptions]`): the file rows ride AFTER
/// the ranked non-file set, and a non-empty query caps the merged
/// list at the popup's 10 (`slice(0, 10)`).
#[must_use]
pub fn merge_file_rows(
    non_file: Vec<AutocompleteOption>,
    file_rows: &[AutocompleteOption],
    query: &str,
) -> Vec<AutocompleteOption> {
    let mut options = non_file;
    options.extend(file_rows.iter().cloned());
    if query.trim().is_empty() {
        options
    } else {
        options.truncate(10);
        options
    }
}

/// G-127 — `directoryAutocompleteSearch` (directory-completion.ts:
/// 67-82): split the typed query into the directory to list, the
/// display prefix (`~/`, `/`, `../…/`), and the basename filter.
#[must_use]
pub fn directory_autocomplete_search(query: &str, directory: &str, home: &str) -> DirectorySearch {
    if query == "~" {
        return DirectorySearch {
            directory: home.to_owned(),
            prefix: "~/".to_owned(),
            query: String::new(),
        };
    }
    if let Some(rest) = query.strip_prefix("~/") {
        return directory_search(rest, home, "~/");
    }
    // `/^(?:\.\.\/)*\.\.$/` — a bare `..` chain resolves against the
    // current directory and filters nothing.
    if !query.is_empty() && query.split('/').all(|part| part == "..") {
        return DirectorySearch {
            directory: resolve_dots(directory, query),
            prefix: format!("{query}/"),
            query: String::new(),
        };
    }
    if let Some(rest) = query.strip_prefix('/') {
        let root = directory.split('/').next().unwrap_or("/");
        return directory_search(rest, &format!("{root}/"), "/");
    }
    directory_search(query, directory, "")
}

fn directory_search(query: &str, root: &str, prefix: &str) -> DirectorySearch {
    match query.rfind('/') {
        None => DirectorySearch {
            directory: root.to_owned(),
            prefix: prefix.to_owned(),
            query: query.to_owned(),
        },
        Some(at) => {
            let parent = &query[..=at];
            DirectorySearch {
                directory: resolve_dots(root, parent),
                prefix: format!("{prefix}{parent}"),
                query: query[at + 1..].to_owned(),
            }
        }
    }
}

/// Lexical `path.resolve` for `..`/`.` components (no filesystem
/// access — the worker already lists a real directory).
fn resolve_dots(root: &str, relative: &str) -> String {
    use std::path::Component;
    let joined = std::path::Path::new(root).join(relative);
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

/// G-127 — the search triple (directory-completion.ts:62-66).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySearch {
    pub directory: String,
    pub prefix: String,
    pub query: String,
}

/// G-127 — `directoryAutocompleteMatches` (directory-completion.ts:
/// 87-91): case-insensitive prefix match; an empty filter hides
/// dotfiles.
#[must_use]
pub fn directory_autocomplete_matches(basename: &str, query: &str) -> bool {
    let value = basename.trim_start_matches(['\\', '/']);
    if query.is_empty() && value.starts_with('.') {
        return false;
    }
    value.to_lowercase().starts_with(&query.to_lowercase())
}

/// G-127 — the `/cd` live rows (directory-completion.ts:10-28):
/// `fs.list(search.directory)` → the type-`directory` entries →
/// absolute-resolved → basename-filtered → `{value: prefix +
/// basename + "/", absolute}` popup options. The exact-prefix row
/// (`directoryAutocompleteExactValue`, :83-85) rides FIRST.
#[must_use]
pub fn directory_live_options(
    entries: &[FileFindRow],
    location_directory: &str,
    search: &DirectorySearch,
) -> Vec<AutocompleteOption> {
    let mut options = Vec::new();
    // The exact row: only when a prefix exists and no basename filter
    // is typed, the queried directory itself completes first.
    if !search.prefix.is_empty() && search.query.is_empty() {
        options.push(AutocompleteOption {
            display: search.prefix.clone(),
            description: None,
            reference: None,
            is_directory: true,
            absolute: Some(search.directory.clone()),
            path: None,
            destructive: None,
        });
    }
    for entry in entries.iter().filter(|entry| entry.is_directory) {
        let absolute = resolve_dots(location_directory, &entry.path);
        let basename = absolute.rsplit('/').next().unwrap_or(&absolute).to_owned();
        if !directory_autocomplete_matches(&basename, &search.query) {
            continue;
        }
        let value = format!("{}{}/", search.prefix, basename.trim_start_matches(['\\', '/']));
        options.push(AutocompleteOption {
            display: value,
            description: None,
            reference: None,
            is_directory: true,
            absolute: Some(absolute),
            path: None,
            destructive: None,
        });
    }
    options
}

/// G-127 — the async-fetch seam (`client.api.file.find` /
/// `client.api.file.list`, autocomplete.tsx:336/361): the store
/// holds the sink; the production run loop wires the HTTP impl to
/// the loop fetcher, unit tests keep the no-op default.
pub trait AutocompleteFetchSink {
    /// `fs.find({query, limit: 20, location})` (autocomplete.tsx:361).
    fn file_find(&self, query: &str, directory: Option<String>);
    /// `fs.list` for `directoryAutocomplete` (autocomplete.tsx:336 —
    /// the search triple's directory + the display prefix/query).
    fn directory_list(&self, query: &str, directory: String, home: String);
}

/// The no-op sink (no loop fetcher attached — headless/unit tests).
pub struct NoopAutocompleteFetchSink;
impl AutocompleteFetchSink for NoopAutocompleteFetchSink {
    fn file_find(&self, _query: &str, _directory: Option<String>) {}
    fn directory_list(&self, _query: &str, _directory: String, _home: String) {}
}

// -- G-127 ---------------------------------------------------------

#[test]
fn file_query_base_strips_the_line_range() {
    assert_eq!(file_query_base("src/lib.rs#12-40"), "src/lib.rs");
    assert_eq!(file_query_base("src/lib.rs"), "src/lib.rs");
    assert_eq!(file_query_base(""), "");
}

/// G-287⑨ — `parseFileLineRange` (prompt/parse.ts:1-13): the
/// suffix validates as `#N` / `#N-N` / `#N-`; `end` survives only
/// when `start < end` (the `#5-3` collapse); a non-matching
/// suffix keeps the hash-stripped base with NO range.
#[test]
fn line_ranges_parse_and_validate() {
    let (base, range) = parse_file_line_range("src/lib.rs#12-40");
    assert_eq!(base, "src/lib.rs");
    assert_eq!(range, Some(FileLineRange { start_line: 12, end_line: Some(40) }));
    // Single line.
    let (_, range) = parse_file_line_range("a.rs#7");
    assert_eq!(range, Some(FileLineRange { start_line: 7, end_line: None }));
    // Open-ended.
    let (_, range) = parse_file_line_range("a.rs#7-");
    assert_eq!(range, Some(FileLineRange { start_line: 7, end_line: None }));
    // Inverted range collapses to start-only.
    let (_, range) = parse_file_line_range("a.rs#5-3");
    assert_eq!(range, Some(FileLineRange { start_line: 5, end_line: None }));
    // Non-numeric / non-suffix hashes: no range, base still strips.
    let (base, range) = parse_file_line_range("src#main");
    assert_eq!(base, "src");
    assert_eq!(range, None);
    let (base, range) = parse_file_line_range("no-hash");
    assert_eq!(base, "no-hash");
    assert_eq!(range, None);
}

#[test]
fn file_find_options_carry_the_file_part_identity() {
    let rows = [
        FileFindRow { path: "src/main.rs".to_owned(), is_directory: false },
        FileFindRow { path: "docs".to_owned(), is_directory: true },
    ];
    let options = file_find_options(&rows);
    assert_eq!(options.len(), 2);
    assert_eq!(options[0].display, "src/main.rs");
    assert!(!options[0].is_directory);
    assert_eq!(options[0].path.as_deref(), Some("src/main.rs"));
    // The structured file-part identity (createFilePart → the
    // wire `files` attachment + frecency on `part.path`).
    assert!(
        matches!(options[0].reference, Some(Reference::Source { ref name }) if name == "src/main.rs")
    );
    assert!(options[1].is_directory);
}

#[test]
fn merge_file_rows_ride_behind_the_static_set() {
    let non_file = vec![AutocompleteOption::command("@build", None)];
    let file = (0..12)
        .map(|index| AutocompleteOption {
            display: format!("file{index}.rs"),
            ..AutocompleteOption::default()
        })
        .collect::<Vec<_>>();
    // Empty query: no slice (the ref returns early, :548-549).
    let merged = merge_file_rows(non_file.clone(), &file, "");
    assert_eq!(merged.len(), 13);
    // A typed query caps the merged list at the popup's 10.
    let merged = merge_file_rows(non_file, &file, "file");
    assert_eq!(merged.len(), 10);
    assert_eq!(merged[0].display, "@build");
    assert_eq!(merged[1].display, "file0.rs");
}

#[test]
fn directory_search_splits_prefix_and_filter() {
    let search = directory_autocomplete_search("src/ma", "/w/repo", "/h");
    assert_eq!(search.directory, "/w/repo/src");
    assert_eq!(search.prefix, "src/");
    assert_eq!(search.query, "ma");
    // `~` and `~/…` resolve against home.
    let search = directory_autocomplete_search("~", "/w/repo", "/h");
    assert_eq!(
        (search.directory.as_str(), search.prefix.as_str(), search.query.as_str()),
        ("/h", "~/", "")
    );
    let search = directory_autocomplete_search("~/pro", "/w", "/h");
    assert_eq!(
        (search.directory.as_str(), search.prefix.as_str(), search.query.as_str()),
        ("/h", "~/", "pro")
    );
    // A bare `..` chain resolves against the current directory.
    let search = directory_autocomplete_search("../..", "/w/repo/sub", "/h");
    assert_eq!(
        (search.directory.as_str(), search.prefix.as_str(), search.query.as_str()),
        ("/w", "../../", "")
    );
    // Absolute queries resolve from the root.
    let search = directory_autocomplete_search("/us", "/w/repo", "/h");
    assert_eq!(
        (search.directory.as_str(), search.prefix.as_str(), search.query.as_str()),
        ("/", "/", "us")
    );
}

#[test]
fn directory_matches_hide_dotfiles_on_empty_query() {
    assert!(directory_autocomplete_matches("src", ""));
    assert!(!directory_autocomplete_matches(".git", ""));
    assert!(directory_autocomplete_matches(".git", ".g"));
    assert!(directory_autocomplete_matches("Repo", "rep"));
}

#[test]
fn directory_live_options_filter_and_prefix() {
    let search = DirectorySearch {
        directory: "/w/repo/src".to_owned(),
        prefix: "src/".to_owned(),
        query: "ma".to_owned(),
    };
    let entries = [
        FileFindRow { path: "main.rs".to_owned(), is_directory: false },
        FileFindRow { path: "markdown".to_owned(), is_directory: true },
        FileFindRow { path: "math".to_owned(), is_directory: true },
        FileFindRow { path: "zed".to_owned(), is_directory: true },
    ];
    let options = directory_live_options(&entries, "/w/repo/src", &search);
    let displays: Vec<&str> = options.iter().map(|option| option.display.as_str()).collect();
    assert_eq!(displays, ["src/markdown/", "src/math/"]);
    assert_eq!(options[0].absolute.as_deref(), Some("/w/repo/src/markdown"));
    assert!(options[0].is_directory);
    // No basename filter + a prefix: the queried directory itself
    // completes first (directoryAutocompleteExactValue).
    let search = DirectorySearch {
        directory: "/w/repo/src".to_owned(),
        prefix: "src/".to_owned(),
        query: String::new(),
    };
    let options = directory_live_options(&entries, "/w/repo/src", &search);
    assert_eq!(options[0].display, "src/");
    assert_eq!(options[0].absolute.as_deref(), Some("/w/repo/src"));
}
