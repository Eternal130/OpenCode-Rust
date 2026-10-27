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
    /// TUI-28 — the `/cd` argument trigger
    /// (`argumentAutocomplete: "directory"`, index.tsx:1957): the
    /// directory-recents list (empty query only — the live file
    /// listing is the async half, not ported).
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
}

impl AutocompleteOption {
    /// A plain slash-command row (display + description).
    pub fn command(display: impl Into<String>, description: Option<String>) -> Self {
        Self { display: display.into(), description, ..Self::default() }
    }
}

/// RD-05 — one `@`-mention row's structured identity
/// (autocomplete.tsx:404-438): `kind: "agent"`/`"skill"` options carry
/// the attachment value the accept path records (`PromptInput`'s
/// `agents`/`skills`), not just display text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// `@<agent.id>` — the attachment value is `{name: agent.id}`.
    Agent { name: String },
    /// `@<skill.id>` — the attachment value is `{id: skill.id}`.
    Skill { id: String },
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
    /// TUI-28 — the armed destructive confirm (`confirming`,
    /// autocomplete.tsx:629-638): the ctrl+d id whose SECOND press
    /// runs the delete.
    confirming: Option<String>,
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
/// the async source, not ported).
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

/// RD-05 — the `@` reference rows (autocomplete.tsx:404-438 + the
/// `options` memo :535-592): skills first, then agents (the ref
/// ordering `[...skillOptions, ...referenceAliases, ...agents]` — no
/// live file/reference source in the port yet). An empty query lists
/// everything; otherwise a case-insensitive containment filter over
/// `@<id>` with a prefix boost, capped at the ref's popup limit (10).
#[must_use]
pub fn filter_references(
    agents: &[ReferenceAgent],
    skills: &[ReferenceSkill],
    query: &str,
) -> Vec<AutocompleteOption> {
    let skill_rows = skills.iter().map(|skill| AutocompleteOption {
        display: format!("@{}", skill.id),
        description: skill.description.clone(),
        reference: Some(Reference::Skill { id: skill.id.clone() }),
        ..AutocompleteOption::default()
    });
    let agent_rows = agents.iter().map(|agent| AutocompleteOption {
        display: format!("@{}", agent.id),
        description: None,
        reference: Some(Reference::Agent { name: agent.id.clone() }),
        ..AutocompleteOption::default()
    });
    let all: Vec<AutocompleteOption> = skill_rows.chain(agent_rows).collect();
    if query.trim().is_empty() {
        return all;
    }
    let needle = query.to_lowercase();
    let prefix = format!("@{needle}");
    let mut ranked: Vec<(u8, AutocompleteOption)> = all
        .into_iter()
        .filter_map(|option| {
            let target = option.display.to_lowercase();
            let at = target.find(&needle)?;
            let boost = u8::from(target.starts_with(&prefix) && at == 0);
            Some((boost, option))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.display.cmp(&b.1.display)));
    ranked.into_iter().map(|(_, option)| option).take(10).collect()
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

    /// RD-05 — the `@` rows: skills before agents (the ref ordering),
    /// an empty query lists everything, and a query filters `@<id>`
    /// case-insensitively with the structured identity attached.
    #[test]
    fn filter_references_orders_and_filters() {
        let agents =
            vec![ReferenceAgent { id: "build".into() }, ReferenceAgent { id: "plan".into() }];
        let skills = vec![ReferenceSkill { id: "refactor".into(), description: Some("d".into()) }];
        let all = filter_references(&agents, &skills, "");
        assert_eq!(
            all.iter().map(|option| option.display.clone()).collect::<Vec<_>>(),
            vec!["@refactor", "@build", "@plan"]
        );
        assert_eq!(all[0].reference, Some(Reference::Skill { id: "refactor".into() }));
        assert_eq!(all[1].reference, Some(Reference::Agent { name: "build".into() }));

        let filtered = filter_references(&agents, &skills, "BU");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].display, "@build");
    }
}
