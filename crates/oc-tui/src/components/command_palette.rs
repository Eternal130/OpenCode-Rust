//! Command palette (`component/command-palette.tsx`): the palette-
//! visible command registry (`id` / title / group / bindings, from the
//! reference's app + session-route command tables), the fuzzy options
//! it renders through [`crate::components::dialog_select`], and the
//! [`CommandDispatcher`] seam that executes a selected command against
//! the store.
//!
//! Registry notes (reference parity):
//! * titles/categories follow `app.tsx` `appCommands` +
//!   `routes/session/index.tsx` command tables (NOT the v1 keybind
//!   descriptions);
//! * bindings follow the operative v2 keybind defaults
//!   (`config/keybind.ts`) — e.g. `theme.switch` is unbound there,
//!   `dialog.move_session.new` is `ctrl+a`;
//! * `searchText = [id, description].filter(Boolean).join(" ")` and
//!   the app commands carry no description, so search text is the id;
//! * commands registered `palette: undefined` (tab switching, quick
//!   slots, toggles) stay out;
//! * `suggested` commands surface first (a "Suggested" section) while
//!   no filter is active — wave 1 keeps the static flags
//!   (`model.list`, `opencode.settings`, plus `session.new` on a
//!   session route and `provider.connect` while disconnected), which
//!   the replay corpus never distinguishes.

use crate::components::dialog_select::{SelectAction, SelectOption, SelectProps, SelectState};
use crate::store::{
    DialogKind, DialogSize, DiffSource, ListState, ModelSelection, Route, ServerAgent, Store,
};
use oc_tui_core::scroll::ScrollCommand;

/// One palette-visible command (`KeymapCommand` from the app tables).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommand {
    /// Runtime dot-command id (`value`; what dispatch executes).
    pub command: &'static str,
    pub title: &'static str,
    pub group: &'static str,
    /// Binding displays (v2 defaults), comma-joined for the footer.
    pub bindings: &'static [&'static str],
}

impl PaletteCommand {
    /// The row footer while unfiltered (`shortcuts.all(id)` —
    /// `", "`-joined binding displays).
    #[must_use]
    pub fn footer(&self) -> String {
        self.bindings.join(", ")
    }

    /// The row footer while filtering (`searchFooter`:
    /// `group · bindings`).
    #[must_use]
    pub fn search_footer(&self) -> String {
        if self.bindings.is_empty() {
            self.group.to_owned()
        } else {
            format!("{} · {}", self.group, self.footer())
        }
    }

    /// The suggested flag (static subset; see the module notes).
    #[must_use]
    pub fn suggested(&self, store: &Store) -> bool {
        match self.command {
            "model.list" | "opencode.settings" => true,
            "session.new" => matches!(store.route(), Route::Session { .. }),
            // `suggested: !connected()` (app.tsx:948) — v2's
            // `connected` is INTEGRATION-backed (`hasConnectedProvider`
            // over `integration.list`, use-connected.tsx), not the
            // server connection: with no connected integration the
            // palette suggests "Connect an integration" between
            // "Switch model" and "Open settings".
            "provider.connect" => !store.server_lists().connected,
            _ => false,
        }
    }

    /// v2's dynamic `enabled` gate (a disabled command is filtered
    /// from the palette, never a dead row). The dynamic cases in the
    /// registry: `session.redo` only with a staged revert (index.
    /// tsx:1005), `session.queued_prompts` only with queued prompts
    /// (:1268), `prompt.images.view` only with image attachments
    /// (prompt/index.tsx:495), `prompt.editor_context.clear` only
    /// with a live editor context (prompt/index.tsx:462).
    #[must_use]
    pub fn enabled(&self, store: &Store) -> bool {
        match self.command {
            "session.redo" => {
                return match store.route() {
                    Route::Session { session_id } => store.revert(&session_id).is_some(),
                    Route::Home => false,
                };
            }
            "session.queued_prompts" => {
                return match store.route() {
                    Route::Session { session_id } => !store.queued_prompts(&session_id).is_empty(),
                    Route::Home => false,
                };
            }
            "prompt.images.view" => {
                let key = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => return false,
                };
                return store.composer(&key).draft.files.iter().any(|file| {
                    file.payload.as_ref().is_some_and(|payload| payload.mime.starts_with("image/"))
                });
            }
            "prompt.editor_context.clear" => return store.editor_context().is_some(),
            _ => {}
        }
        true
    }
}

/// The production palette registry in registration order (app
/// commands, then the session-route commands). Server-backed commands
/// dispatch to [`Dispatch::ServerPending`] stubs until the wave-3
/// oc-client wiring lands.
pub static PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        command: "session.list",
        title: "Switch session",
        group: "Session",
        bindings: &["ctrl+x l"],
    },
    PaletteCommand {
        command: "session.new",
        title: "New session",
        group: "Session",
        bindings: &["ctrl+x n"],
    },
    PaletteCommand {
        command: "session.clear",
        title: "Clear session",
        group: "Session",
        bindings: &[],
    },
    PaletteCommand {
        command: "open.menu",
        title: "Open session or project",
        group: "Session",
        bindings: &["ctrl+o"],
    },
    PaletteCommand {
        command: "session.tab.close",
        title: "Close tab",
        group: "Session",
        bindings: &["ctrl+x w"],
    },
    PaletteCommand {
        command: "session.tab.reopen",
        title: "Reopen closed tab",
        group: "Session",
        bindings: &["ctrl+shift+t"],
    },
    PaletteCommand {
        command: "session.share",
        title: "Share session",
        group: "Session",
        bindings: &[],
    },
    PaletteCommand {
        command: "session.copy.id",
        title: "Copy session ID",
        group: "Session",
        bindings: &[],
    },
    // TUI-34 — `messages.copy` (index.tsx:1128-1167, <leader>y).
    PaletteCommand {
        command: "messages.copy",
        title: "Copy last assistant message",
        group: "Session",
        bindings: &["<leader>y"],
    },
    // TUI-TD13 — `session.copy` (index.tsx:1180-1200, slash `/copy`).
    PaletteCommand {
        command: "session.copy",
        title: "Copy session transcript",
        group: "Session",
        bindings: &[],
    },
    PaletteCommand {
        command: "session.cd",
        title: "Change working directory",
        group: "Session",
        bindings: &[],
    },
    // RE-13 — `session.aside` (`/btw`, feature-plugins/prompt/btw.
    // tsx:51-56): one-shot side answer, palette-listed in Session.
    PaletteCommand {
        command: "session.aside",
        title: "Ask a side question",
        group: "Session",
        bindings: &[],
    },
    // RE-14 — `stats.open` (`/stats`, feature-plugins/system/
    // stats.tsx:196-209): the usage poster page, palette-listed in
    // System.
    PaletteCommand {
        command: "stats.open",
        title: "Usage statistics",
        group: "System",
        bindings: &[],
    },
    PaletteCommand {
        command: "session.rename",
        title: "Rename session",
        group: "Session",
        bindings: &["ctrl+r"],
    },
    PaletteCommand {
        command: "session.timeline",
        title: "Jump to message",
        group: "Session",
        bindings: &["ctrl+x g"],
    },
    PaletteCommand {
        command: "session.fork",
        title: "Fork session",
        group: "Session",
        bindings: &[],
    },
    PaletteCommand {
        command: "session.compact",
        title: "Compact session",
        group: "Session",
        bindings: &["ctrl+x c"],
    },
    PaletteCommand {
        command: "session.undo",
        title: "Undo previous message",
        group: "Session",
        bindings: &["ctrl+x u"],
    },
    PaletteCommand {
        command: "session.redo",
        title: "Redo",
        group: "Session",
        bindings: &["ctrl+x r"],
    },
    // TUI-33 — `session.verbosity.cycle` (index.tsx:1064-1076): the
    // palette row's title is DYNAMIC (`Verbosity: ${titlecase}` —
    // `palette_props` special-cases it against the live level; the
    // registry's static title is the fallback shape).
    PaletteCommand {
        command: "session.verbosity.cycle",
        title: "Verbosity",
        group: "Session",
        bindings: &[],
    },
    // Lane FINAL: `session.export` (index.tsx:1201-1244 — palette-
    // listed with slash `/export`, bound `<leader>x`).
    PaletteCommand {
        command: "session.export",
        title: "Export session transcript",
        group: "Session",
        bindings: &["ctrl+x x"],
    },
    PaletteCommand {
        command: "session.move",
        title: "Manage workspaces",
        group: "Session",
        bindings: &[],
    },
    PaletteCommand {
        command: "model.list",
        title: "Switch model",
        group: "Agent",
        bindings: &["ctrl+x m"],
    },
    PaletteCommand {
        command: "agent.list",
        title: "Switch agent",
        group: "Agent",
        bindings: &["ctrl+x a"],
    },
    PaletteCommand { command: "mcp.list", title: "MCP servers", group: "Agent", bindings: &[] },
    PaletteCommand { command: "plugins.list", title: "Plugins", group: "System", bindings: &[] },
    // TUI-34 — `session.child.first` (index.tsx:1255-1263, bind
    // `down`): the subagent picker's toggle.
    PaletteCommand {
        command: "session.child.first",
        title: "Toggle subagent picker",
        group: "Session",
        bindings: &["down"],
    },
    // TUI-34 — `session.queued_prompts` (index.tsx:1265-1271, group
    // "Prompt"): `enabled: queuedPrompts().length > 0` — the enabled
    // gate below filters the dead row.
    PaletteCommand {
        command: "session.queued_prompts",
        title: "View queued prompts",
        group: "Prompt",
        bindings: &[],
    },
    PaletteCommand {
        command: "toast.activate",
        title: "Activate the current toast action",
        group: "Prompt",
        bindings: &["ctrl+alt+t"],
    },
    // TUI-9: the stash family (prompt/index.tsx:874-920 — v2's
    // palette rows, category Prompt).
    PaletteCommand {
        command: "prompt.stash",
        title: "Stash prompt",
        group: "Prompt",
        bindings: &[],
    },
    // TUI-34 — `prompt.queue` (prompt/index.tsx:421-436, <leader>
    // return): the queue submit.
    PaletteCommand {
        command: "prompt.queue",
        title: "Queue prompt",
        group: "Prompt",
        bindings: &["<leader>return"],
    },
    // TUI-34 — `prompt.editor` (prompt/index.tsx:567-605, <leader>e,
    // slash /editor).
    PaletteCommand {
        command: "prompt.editor",
        title: "Open editor",
        group: "Session",
        bindings: &["<leader>e"],
    },
    // TUI-34 — `prompt.skills` (prompt/index.tsx:607-640, slash
    // /skills).
    PaletteCommand { command: "prompt.skills", title: "Skills", group: "Prompt", bindings: &[] },
    // TUI-34 — `prompt.images.view` (prompt/index.tsx:493-498,
    // <leader>i): `enabled: imageAttachments().length > 0` — the
    // enabled gate below filters the dead row.
    PaletteCommand {
        command: "prompt.images.view",
        title: "View image attachments",
        group: "Prompt",
        bindings: &["<leader>i"],
    },
    PaletteCommand {
        command: "prompt.stash.pop",
        title: "Stash pop",
        group: "Prompt",
        bindings: &[],
    },
    PaletteCommand {
        command: "prompt.stash.list",
        title: "Stash list",
        group: "Prompt",
        bindings: &[],
    },
    // TUI-15: the editor context's clear (index.tsx:459-466 — "Remove
    // editor context", bound to `none` by default: palette-only).
    PaletteCommand {
        command: "prompt.editor_context.clear",
        title: "Remove editor context",
        group: "Prompt",
        bindings: &[],
    },
    PaletteCommand {
        command: "variant.cycle",
        title: "Variant cycle",
        group: "Agent",
        bindings: &["ctrl+t"],
    },
    PaletteCommand {
        command: "variant.list",
        title: "Switch model variant",
        group: "Agent",
        bindings: &[],
    },
    PaletteCommand {
        command: "provider.connect",
        title: "Connect an integration",
        group: "Integration",
        bindings: &[],
    },
    PaletteCommand {
        command: "opencode.settings",
        title: "Open settings",
        group: "System",
        bindings: &[],
    },
    // TUI-TD22 (PORT ADDITION — the toast.activate precedent): the
    // palette row for DialogExperiments. v2's entry is the DevToolsBar
    // item (devtools-bar.tsx:429-437); the port has no devtools bar
    // (06 #20 KNOWN — separate ledger row), so the palette carries the
    // reachable surface.
    PaletteCommand {
        command: "opencode.experiments",
        title: "Experiments",
        group: "System",
        bindings: &[],
    },
    // G-07 — the devtools bar's palette rows (ref devtools-bar.tsx):
    // the bar itself is `debug.devtools`-gated; the rows toggle/show
    // live (the settings row persists the gate).
    PaletteCommand {
        command: "devtools.toggle",
        title: "Toggle devtools bar",
        group: "System",
        bindings: &["ctrl+x d"],
    },
    PaletteCommand {
        command: "devtools.panel",
        title: "Switch devtools panel",
        group: "System",
        bindings: &["ctrl+x ]"],
    },
    PaletteCommand {
        command: "devtools.dump",
        title: "Dump devtools state",
        group: "System",
        bindings: &["ctrl+x j"],
    },
    PaletteCommand {
        command: "opencode.status",
        title: "View status",
        group: "System",
        bindings: &["ctrl+x s"],
    },
    PaletteCommand { command: "server.pair", title: "Pair device", group: "System", bindings: &[] },
    // Lane TAIL (auditB #10): `/update` — the DialogUpdate mount.
    PaletteCommand {
        command: "update.check",
        title: "Check for updates",
        group: "System",
        bindings: &[],
    },
    PaletteCommand {
        command: "location.reload",
        title: "Reload configuration",
        group: "System",
        bindings: &[],
    },
    PaletteCommand {
        command: "opencode.debug",
        title: "View debug info",
        group: "System",
        bindings: &[],
    },
    PaletteCommand {
        command: "theme.switch",
        title: "Switch theme",
        group: "System",
        bindings: &[],
    },
    // v2 registers it `palette: true` in the session frame's layer
    // (session-frame.tsx:187-195, group "Session" — our View group).
    PaletteCommand {
        command: "session.sidebar.toggle",
        title: "Toggle sidebar",
        group: "View",
        bindings: &["ctrl+x b"],
    },
    PaletteCommand { command: "help.show", title: "Help", group: "System", bindings: &[] },
    PaletteCommand { command: "docs.open", title: "Open docs", group: "System", bindings: &[] },
    // The diff viewer's plugin command (`diff.open`, group "VCS",
    // slash `/diff` — diff-viewer.tsx:1198-1204).
    PaletteCommand { command: "diff.open", title: "Open diff viewer", group: "VCS", bindings: &[] },
    PaletteCommand {
        command: "app.exit",
        title: "Exit the app",
        group: "System",
        bindings: &["ctrl+c", "ctrl+d", "ctrl+x q"],
    },
    PaletteCommand {
        command: "app.console",
        title: "Toggle console",
        group: "System",
        bindings: &[],
    },
];

/// Look up one registry entry.
#[must_use]
pub fn palette_command(command: &str) -> Option<&'static PaletteCommand> {
    PALETTE_COMMANDS.iter().find(|entry| entry.command == command)
}

/// A row's run-time title (v2's dynamic `title` thunks over the live
/// state). The one dynamic case in the registry:
/// `session.verbosity.cycle` renders `Verbosity: ${Locale.titlecase(
/// verbosity())}` (index.tsx:1064-1067).
#[must_use]
pub fn palette_title(entry: &PaletteCommand, store: &Store) -> String {
    if entry.command == "session.verbosity.cycle" {
        let level = store.session_verbosity();
        let mut chars = level.chars();
        let titlecased = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        return format!("Verbosity: {titlecased}");
    }
    entry.title.to_owned()
}

/// The palette options for a registry slice (`options()`): title,
/// group category, `id` search text, both footers. The command-palette
/// fixture (golden) registers two custom commands — tests pass their
/// own slice.
#[must_use]
pub fn palette_options(commands: &[PaletteCommand]) -> Vec<SelectOption> {
    commands
        .iter()
        .map(|entry| {
            SelectOption::titled(entry.title, entry.command)
                .categorized(entry.group)
                .with_search(entry.command)
                .with_footer(entry.footer())
                .with_search_footer(entry.search_footer())
        })
        .collect()
}

/// [`palette_options`] over borrowed entries (the conditional slice).
#[must_use]
fn palette_options_slice(commands: &[&PaletteCommand], store: &Store) -> Vec<SelectOption> {
    commands
        .iter()
        .map(|entry| {
            SelectOption::titled(palette_title(entry, store), entry.command)
                .categorized(entry.group)
                .with_search(entry.command)
                .with_footer(entry.footer())
                .with_search_footer(entry.search_footer())
        })
        .collect()
}

/// The full palette props for the current store state: the suggested
/// section first (a duplicate set under "Suggested") when no filter is
/// active, then every registry command — `list()` in
/// `command-palette.tsx` — then the plugin-registered commands (the
/// oc-plugin-host registration snapshot's palette entries, group
/// "Plugin"). RD-17 (`command-palette.tsx:49-60`): the FILTERED list
/// swaps the suggested section for the per-setting rows — ref
/// `list()` returns `[...options(), ...settingOptions]` under a
/// filter and `suggested + options()` without one. The palette mounts
/// flat with a `0.7` threshold.
#[must_use]
pub fn palette_props(store: &Store) -> SelectProps {
    palette_props_filtered(store, "")
}

#[must_use]
pub fn palette_props_filtered(store: &Store, filter: &str) -> SelectProps {
    let mut options = Vec::new();
    if filter.is_empty() {
        let suggested: Vec<SelectOption> = PALETTE_COMMANDS
            .iter()
            .filter(|entry| entry.suggested(store))
            .map(|entry| {
                SelectOption::titled(entry.title, format!("suggested:{}", entry.command))
                    .categorized("Suggested")
                    .with_search(entry.command)
                    .with_footer(entry.footer())
                    .with_search_footer(entry.search_footer())
            })
            .collect();
        options.extend(suggested);
    }
    // v2's conditional palette entries (`palette: undefined` — e.g.
    // variant.list without variants, app.tsx:923): a command whose
    // surface cannot open is REMOVED from the list, never advertised
    // as a dead row.
    let advertised: Vec<&PaletteCommand> = PALETTE_COMMANDS
        .iter()
        .filter(|entry| {
            !(entry.command == "variant.list" && store.current_model_variants().is_empty())
                && entry.enabled(store)
        })
        .collect();
    options.extend(palette_options_slice(&advertised, store));
    // The plugin layer (registration-time snapshot): id search text,
    // the plugin id beside the group, no bindings display. RE-02:
    // an INACTIVE plugin's rows drop (v2's deactivate clears the
    // registration's contributions, context.tsx:144-148/213).
    for entry in store.plugin_commands() {
        if !store.plugin_active(&entry.plugin_id) {
            continue;
        }
        options.push(
            SelectOption::titled(entry.title.clone(), entry.command.clone())
                .categorized("Plugin")
                .with_search(format!("{} {}", entry.command, entry.plugin_id))
                .with_footer(format!("plugin:{}", entry.plugin_id))
                .with_search_footer(format!("Plugin · {}", entry.plugin_id)),
        );
    }
    // F14 — duplicate-entry dedup by value: a plugin (or future
    // registration source) that claims a builtin command id renders
    // ONE row (the registry's own), never two rows dispatching the
    // same command. The `suggested:` prefixed mirrors stay (v2's
    // suggested section is deliberately a second copy,
    // command-palette.tsx:53-59).
    let mut seen = std::collections::HashSet::new();
    options.retain(|option| seen.insert(option.value.clone()));
    // RD-17 — the per-setting rows ride the FILTERED list only
    // (`if (ref?.filter) return [...options(), ...settingOptions]`):
    // dedup above never sees them (their `setting:` values are
    // unique), so they append after.
    if !filter.is_empty() {
        options.extend(palette_setting_options());
    }
    SelectProps::new("Commands", options).flat(true).threshold(0.7)
}

/// RD-17 — the per-setting search rows (`settingOptions`,
/// command-palette.tsx:36-45): one row per `settings[]` entry with the
/// setting's title + category, the keywords as search text, a
/// `Settings · {category}` search footer, and the `setting:{id}`
/// value whose activation replaces the palette with `DialogConfig`
/// opened at that setting.
#[must_use]
pub fn palette_setting_options() -> Vec<SelectOption> {
    crate::components::dialog_config::settings_table()
        .into_iter()
        .map(|setting| {
            let mut option =
                SelectOption::titled(setting.title, format!("setting:{}", setting.id()))
                    .categorized(setting.category);
            if !setting.keywords.is_empty() {
                option = option.with_search(setting.keywords);
            }
            option.with_search_footer(format!("Settings · {}", setting.category))
        })
        .collect()
}

/// The select props for one select-style dialog kind (the dialog
/// renderer's content source).
#[must_use]
// The per-kind props table (one arm per dialog kind).
#[allow(clippy::too_many_lines)]
pub fn select_props_for(kind: &DialogKind, store: &Store) -> SelectProps {
    match kind {
        DialogKind::CommandPalette => palette_props(store),
        DialogKind::ThemeList => theme_list_props(store),
        DialogKind::Help
        | DialogKind::SessionRename { .. }
        | DialogKind::WorktreeName
        | DialogKind::BtwPrompt
        | DialogKind::BtwAnswer { .. } => SelectProps::new("", Vec::new()),
        DialogKind::Plugin { id } => {
            // The delegation payload: plugin-supplied options at open
            // time. Rows-only content returns empty props (the dialog
            // renders its static rows; keys pass through except the
            // close bindings).
            match store.plugin_dialog_content(id) {
                Some(content) if content.is_select() => {
                    let options = content
                        .options
                        .iter()
                        .map(|option| {
                            let mut built =
                                SelectOption::titled(option.title.clone(), option.value.clone())
                                    .categorized("Plugin");
                            if !option.description.is_empty() {
                                built = built.with_footer(option.description.clone());
                            }
                            built
                        })
                        .collect();
                    SelectProps::new(content.title, options).flat(true).threshold(0.7)
                }
                _ => SelectProps::new("", Vec::new()),
            }
        }
        // The queued-prompts manager (v2 index.tsx:627-635): one row
        // per queued prompt, the `N of M` footer, enter steers.
        DialogKind::QueuedPrompts => {
            let session = match store.route() {
                Route::Session { session_id } => session_id,
                Route::Home => {
                    return SelectProps::new("Queued prompts", Vec::new());
                }
            };
            let queued = store.queued_prompts(&session);
            let total = queued.len();
            let options = queued
                .iter()
                .enumerate()
                .map(|(index, prompt)| {
                    SelectOption::titled(prompt.text.clone(), prompt.id.clone())
                        .categorized("Queued")
                        .with_footer(format!("{} of {}", index + 1, total))
                })
                .collect();
            SelectProps::new("Queued prompts", options)
        }
        // SURF-1: the tool detail viewer (`DialogExecute` for
        // execute — dialog-execute.tsx; `DialogShellOutput` for
        // background shells — dialog-shell-output.tsx): one row per
        // line of the part's detail — the shell output (with the
        // `(N earlier lines omitted)` notice when the tail truncated)
        // or the execute input/metadata rows.
        DialogKind::ToolOutput { session_id, message_id, part_id } => {
            const SHELL_WINDOW: usize = 512;
            let part = store
                .messages(session_id)
                .iter()
                .find(|message| message.id() == message_id.as_str())
                .and_then(|message| {
                    crate::components::message::resolve_part(message.parts(), part_id).cloned()
                });
            let Some(crate::components::message::model::Part::Tool { name, state, .. }) = part
            else {
                return SelectProps::new("Tool", Vec::new());
            };
            let canonical =
                crate::components::message::model::canonical_tool_name(&name).to_owned();
            if canonical == "execute" {
                // DialogExecute: the input/metadata views.
                let mut options = Vec::new();
                for (key, value) in &state.input {
                    options.push(
                        SelectOption::titled(format!("{key}: {value}"), format!("input:{key}"))
                            .categorized("Input"),
                    );
                }
                for (key, value) in &state.metadata {
                    options.push(
                        SelectOption::titled(format!("{key}: {value}"), format!("meta:{key}"))
                            .categorized("Metadata"),
                    );
                }
                if let Some(error) = &state.error {
                    options.push(SelectOption::titled(error.clone(), "error").categorized("Error"));
                }
                if let Some(output) = &state.output {
                    for (index, line) in output.lines().enumerate() {
                        options.push(
                            SelectOption::titled(line.to_owned(), format!("line:{index}"))
                                .categorized("Output"),
                        );
                    }
                }
                return SelectProps::new("Execute", options).flat(true);
            }
            // DialogShellOutput: the tail preview + the omitted-lines
            // notice (PAGE_BYTES windowing approximated per line here;
            // the full viewer scrolls the same rows).
            let output = state.output.clone().unwrap_or_default();
            let lines: Vec<&str> = output.lines().collect();
            let (omitted, visible) = if lines.len() > SHELL_WINDOW {
                (Some(lines.len() - SHELL_WINDOW), &lines[lines.len() - SHELL_WINDOW..])
            } else {
                (None, &lines[..])
            };
            let mut options = Vec::new();
            if let Some(count) = omitted {
                options.push(
                    SelectOption::titled(format!("({count} earlier lines omitted)"), "omitted")
                        .muted_placeholder(),
                );
            }
            if let Some(error) = &state.error {
                options.push(SelectOption::titled(error.clone(), "error").categorized("Error"));
            }
            for (index, line) in visible.iter().enumerate() {
                options.push(SelectOption::titled((*line).to_owned(), format!("line:{index}")));
            }
            SelectProps::new("Shell output", options).flat(true)
        }
        DialogKind::ServerList { command: "model.list", title } => {
            model_list_props(store, title).with_current(model_current_value(store))
        }
        // The Worktrees dialog (R-05): the store's worktree state
        // builds the rows; the move action rides a session route.
        DialogKind::Worktrees => crate::components::dialog_worktrees::worktree_props(
            &store.worktree_dialog(),
            matches!(store.route(), crate::store::Route::Session { .. }),
            &home_directory(),
        ),
        // The forced-delete confirm: v2's file-changes dialog in its
        // yes/no role (`DialogWorkspaceFileChanges.show`).
        DialogKind::WorktreeDeleteConfirm { .. } => SelectProps::new(
            "Delete worktree?",
            vec![
                SelectOption::titled("This worktree has file changes.", "yes")
                    .described("Do you want to delete it anyway? Yes deletes it."),
                SelectOption::titled("Keep the worktree", "no").described("Cancels the delete."),
            ],
        )
        .destructive(),
        DialogKind::ServerList { command: "agent.list", title } => agent_list_props(store, title),
        DialogKind::ServerList { command: "session.list", title } => {
            // The session list dialog renders the HISTORY (v2's
            // `data.session.list()` — every session the resync
            // reported), NOT the open tabs; selecting a row navigates
            // and the route effect opens its tab. TUI-6 (lane-tui2):
            // the rows carry v2's shapes — date-group categories
            // ("Today" / the day string), the timestamped fallback
            // title, the outside-project footer basename, and the
            // armed two-press delete's confirm title
            // (dialog-session-list.tsx:141-192).
            let options = crate::components::dialog_open::session_list_options(store);
            // SE-F4 + TUI-7: the ref's filter semantics — trimmed,
            // case-folded, byte-exact substring (whitespace-only = the
            // full list); the empty + no-match copy is the dialog's
            // `searchState` (:110-121).
            let mut props = SelectProps::new(*title, options)
                .substring_filter()
                .with_empty_texts("No sessions available", "No sessions found");
            // The current session row carries the `●` focus mark
            // (`current={currentSessionID()}`, :211).
            if let crate::store::Route::Session { session_id } = store.route() {
                props = props.with_current(Some(session_id));
            }
            props = props
                .with_action(crate::components::dialog_select::SelectAction::new(
                    "session.pin.toggle",
                    "pin/unpin",
                    "ctrl+f",
                ))
                .with_action(crate::components::dialog_select::SelectAction::new(
                    "session.delete",
                    "delete",
                    "ctrl+d",
                ));
            props
        }
        // TUI-5 (lane-tui2): `DialogOpen` — the Sessions recents +
        // Projects sections over the store's project registry
        // (dialog-open.tsx:197-274).
        DialogKind::Open => crate::components::dialog_open::open_menu_props(store),
        // TUI-5: the `{Project} / Worktrees` view (dialog-open.tsx
        // :276-322).
        DialogKind::OpenWorktrees { project_id, project_name } => {
            crate::components::dialog_open::open_worktrees_props(
                store,
                project_id,
                project_name,
                &home_directory(),
            )
        }
        // -- Lane FINAL: the integration connect surface ---------------
        // `DialogIntegration` (dialog-integration.tsx:102-145): one row
        // per integration — name, Popular/Services/MCP category, the
        // connected-✓ gutter folded into the footer (summary /
        // "Sign in required →" warning), "Environment only" when no
        // connect method, the muted empty view.
        DialogKind::Integration => {
            let options = match store.integration_list() {
                crate::store::IntegrationListState::Loading => {
                    vec![status_option("Loading integrations…")]
                }
                crate::store::IntegrationListState::Failed(error) => {
                    vec![status_option(&format!("Integrations unavailable: {error}"))]
                }
                crate::store::IntegrationListState::Loaded(rows) => {
                    if rows.is_empty() {
                        vec![status_option("No integrations available")]
                    } else {
                        rows.iter()
                            .map(|row| {
                                let mut option =
                                    SelectOption::titled(row.name.clone(), row.id.clone())
                                        .categorized(row.category);
                                if row.methods.is_empty() && row.accounts.is_empty() {
                                    option = option.described("Environment only");
                                }
                                if row.sign_in_required {
                                    option = option.with_footer("Sign in required →");
                                } else if !row.summary.is_empty() {
                                    option = option.with_footer(format!("✓ {}", row.summary));
                                }
                                option
                            })
                            .collect()
                    }
                }
            };
            SelectProps::new("Connect an integration", options)
        }
        // `manageConnections` (dialog-integration.tsx:148-263): "Add
        // account" + the credential rows (the armed delete row flips
        // its title to the confirm prompt).
        DialogKind::IntegrationAccounts { integration_id } => {
            let mut options = Vec::new();
            if let Some(row) = store.integration_row(integration_id) {
                if !row.methods.is_empty() {
                    options.push(SelectOption::titled("Add account", "add"));
                }
                let deleting = store.integration_deleting();
                for account in &row.accounts {
                    let armed = deleting.as_deref() == Some(account.id.as_str());
                    let title = if armed {
                        "Press ctrl+d again to confirm".to_owned()
                    } else {
                        account.label.clone()
                    };
                    let mut option = SelectOption::titled(title, account.id.clone())
                        .categorized("Connected accounts");
                    if account.sign_in_required {
                        option = option.with_footer("Sign in required →");
                    }
                    options.push(option);
                }
            }
            let title = store
                .integration_row(integration_id)
                .map_or_else(|| "Connect an integration".to_owned(), |row| row.name);
            SelectProps::new(title, options)
        }
        // `selectMethod` (dialog-integration.tsx:265-283): one row per
        // non-env method (key first — the rows arrive pre-sorted).
        DialogKind::IntegrationMethod { integration_id } => {
            let options = store
                .integration_row(integration_id)
                .map(|row| row.methods.clone())
                .unwrap_or_default()
                .into_iter()
                .map(|method| {
                    let title = if method.kind == "key" && method.label.is_empty() {
                        "API key".to_owned()
                    } else {
                        method.label.clone()
                    };
                    SelectOption::titled(title, method.id)
                })
                .collect();
            let name =
                store.integration_row(integration_id).map_or_else(String::new, |row| row.name);
            SelectProps::new(format!("Connect {name}"), options)
        }
        // The prompt-style inputs render their own chrome (the props
        // stay empty like SessionRename — the solid renderer draws the
        // input; the code dialog's URL/instructions rows render in the
        // description block).
        DialogKind::IntegrationKey { title, .. } => {
            let mut props = SelectProps::new(title.clone(), Vec::new());
            "API key".clone_into(&mut props.placeholder);
            if let Some(error) = store.integration_error() {
                props.hints.push(error);
            }
            props
        }
        DialogKind::IntegrationCode { .. } => {
            let title = store
                .integration_attempt()
                .map_or_else(|| "Connect".to_owned(), |attempt| attempt.title);
            let mut props = SelectProps::new(title, Vec::new());
            "Authorization code".clone_into(&mut props.placeholder);
            if let Some(attempt) = store.integration_attempt() {
                if !attempt.url.is_empty() {
                    props.hints.push(attempt.url);
                }
                if !attempt.instructions.is_empty() {
                    props.hints.push(attempt.instructions);
                }
            }
            if let Some(error) = store.integration_error() {
                props.hints.push(error);
            }
            props
        }
        // The pending view (`OAuthAuto`/`CommandPending`): the URL +
        // instructions + waiting message as rows, the o/c action hints
        // in the footer.
        DialogKind::IntegrationPending { .. } => {
            let mut options = Vec::new();
            if let Some(attempt) = store.integration_attempt() {
                if !attempt.url.is_empty() {
                    options.push(SelectOption::titled(attempt.url.clone(), "url"));
                }
                if !attempt.instructions.is_empty() {
                    options.push(
                        SelectOption::titled(attempt.instructions.clone(), "instructions")
                            .muted_placeholder(),
                    );
                }
            }
            let title = store
                .integration_attempt()
                .map_or_else(|| "Waiting".to_owned(), |attempt| attempt.title);
            SelectProps::new(title, options)
        }
        // (TUI-23: DialogConfig's props moved to the FULL table arm
        // below — every v2.0.24 settings row through
        // `crate::components::dialog_config`.)
        // `DialogFork` (dialog-fork.tsx:69-82): "Full session" + the
        // user messages newest-first (text single-lined, the created
        // time in the footer).
        DialogKind::ForkPicker { session_id } => {
            let mut options =
                vec![SelectOption::titled("Full session", "")
                    .described("fork at the latest message")];
            for message in store.messages(session_id).iter().rev() {
                if let crate::components::message::model::Message::User {
                    id, text, created, ..
                } = message
                {
                    options.push(
                        SelectOption::titled(text.replace('\n', " "), id.clone())
                            .with_footer(short_time(*created / 1000)),
                    );
                }
            }
            SelectProps::new("Fork session", options)
        }
        // The export destinations (DialogExportOptions' format ×
        // action rows collapsed to four).
        DialogKind::SessionExport { .. } => SelectProps::new(
            "Export session",
            vec![
                SelectOption::titled("Markdown → clipboard", "markdown:clipboard")
                    .described("thinking off, tool calls on"),
                SelectOption::titled("Markdown → file", "markdown:file")
                    .described("thinking off, tool calls on"),
                SelectOption::titled("JSON → clipboard", "json:clipboard").described("unsanitized"),
                SelectOption::titled("JSON → file", "json:file").described("unsanitized"),
            ],
        ),
        // `DialogExportResult` (dialog-export-result.tsx:5-27): the
        // written path row; return/esc closes.
        DialogKind::ExportResult { path } => {
            SelectProps::new("Session exported", vec![SelectOption::titled(path.clone(), "path")])
        }
        // -- TUI-8: DialogSkill (dialog-skill.tsx:31-45): the padded
        // name column + the description; loading / error / empty
        // render the muted status rows; "No skills found" on a miss.
        DialogKind::Skill => {
            let options = match store.skills() {
                crate::store::ListState::Loading => vec![status_option("Loading skills…")],
                crate::store::ListState::Failed(error) => {
                    vec![status_option(&format!("Could not load skills: {error}"))]
                }
                crate::store::ListState::Loaded(skills) => {
                    if skills.is_empty() {
                        vec![status_option("No skills available")]
                    } else {
                        let max_width = skills
                            .iter()
                            .map(|skill| skill.name.chars().count())
                            .max()
                            .unwrap_or(0);
                        skills
                            .iter()
                            .map(|skill| {
                                let title = format!("{:width$}", skill.name, width = max_width);
                                let mut option = SelectOption::titled(title, skill.id.clone());
                                if let Some(description) = &skill.description {
                                    let squashed = description
                                        .split_whitespace()
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    option = option.described(squashed);
                                }
                                option
                            })
                            .collect()
                    }
                }
            };
            SelectProps::new("Skills", options)
        }
        // -- TUI-9: DialogStash (dialog-stash.tsx:38-65): the entries
        // most-recent first, the relative-time description, the line
        // count footer, the armed row's confirm title.
        DialogKind::Stash => {
            let entries = store.stash_entries();
            let armed = store.stash_delete_armed();
            let options: Vec<SelectOption> = entries
                .iter()
                .enumerate()
                .rev()
                .map(|(index, entry)| {
                    // `value` = the STASH index (v2's `value: index`).
                    let reverse_position = entries.len() - 1 - index;
                    let is_deleting = armed == Some(index);
                    let title = if is_deleting {
                        "Press ctrl+d again to confirm".to_owned()
                    } else {
                        // getStashPreview: the first line, ≤50 chars.
                        let first = entry.prompt.text.split('\n').next().unwrap_or("").trim();
                        first.chars().take(50).collect::<String>()
                    };
                    let mut option = SelectOption::titled(title, index.to_string());
                    option = option.described(relative_time(entry.timestamp));
                    let lines = entry.prompt.text.matches('\n').count() + 1;
                    if lines > 1 {
                        option = option.with_footer(format!("~{lines} lines"));
                    }
                    let _ = reverse_position;
                    option
                })
                .collect();
            let mut props = SelectProps::new("Stash", options);
            if armed.is_some() {
                props = props.destructive();
            }
            props
        }
        // -- TUI-23: the full DialogConfig table (dialog-config.tsx)
        // — every row with its category, the formatted footer, the
        // keywords as search text.
        DialogKind::Settings => {
            let options = crate::components::dialog_config::settings_table()
                .into_iter()
                .map(|setting| {
                    let value = crate::components::dialog_config::current_value(store, &setting);
                    let footer = if setting.path == "theme.name" {
                        store.theme_name()
                    } else {
                        setting.display(&value)
                    };
                    let mut option = SelectOption::titled(setting.title, setting.path)
                        .categorized(setting.category);
                    if !setting.keywords.is_empty() {
                        option = option.with_search(setting.keywords);
                    }
                    option.with_footer(footer)
                })
                .collect();
            SelectProps::new("Settings", options)
        }
        // -- TUI-TD22: DialogExperiments — v2.0.24's table is EMPTY
        // (`No experiments available`); rows carry the on/off footer.
        DialogKind::Experiments => {
            let experiments = store.experiments();
            if experiments.is_empty() {
                SelectProps::new("Experiments", Vec::new())
                    .with_empty_texts("No experiments available", "No experiments available")
            } else {
                let options = experiments
                    .iter()
                    .map(|experiment| {
                        SelectOption::titled(experiment.title.clone(), experiment.id.clone())
                            .with_search(experiment.description.clone())
                            .with_footer(
                                if store.experiment_enabled(&experiment.id) { "on" } else { "off" }
                                    .to_owned(),
                            )
                    })
                    .collect();
                SelectProps::new("Experiments", options)
            }
        }
        // -- TUI-10/24/25: the custom-key families render their own
        // chrome (no select pipeline).
        DialogKind::ImagePreview
        | DialogKind::ErrorDetails { .. }
        | DialogKind::Alert { .. }
        | DialogKind::Confirm { .. } => SelectProps::new("", Vec::new()),
        // SE-F9: the stub row is DELISTED — only model/agent/session
        // listings ever mount a ServerList now; an unknown picker
        // falls to the shared empty state ("No items available"),
        // never placeholder copy.
        DialogKind::ServerList { title, .. } => SelectProps::new(*title, Vec::new()),
        // SURF-2 `DialogMcp` (`/mcps`, dialog-mcp.tsx:70-81): the
        // name-sorted server rows with the status footer text
        // (`Status`, :20-34).
        // `plugins.list` (lane KM): the plugin manager select — one
        // row per registered plugin id (commands + home actions),
        // empty → the shared select empty state. SA-F6 fidelity with
        // the ref's PluginsDialog (feature-plugins/system/
        // plugins.tsx:354-368): a DialogSelect titled "Plugins" whose
        // empty options render the shared "No items available"
        // placeholder (ui/dialog-select.tsx:723), plus the
        // `ctrl+a show internal` footerHints pair riding the footer
        // bar (a FooterHint renders `title label` — display-only; the
        // ref's ctrl+a TOGGLE of internal plugins has no ported
        // surface, so the pair carries a surfaceless command id).
        DialogKind::Plugins => {
            let mut ids: Vec<String> = store
                .plugin_commands()
                .into_iter()
                .map(|entry| entry.plugin_id)
                .chain(store.plugin_home_actions().into_iter().map(|action| action.plugin_id))
                .collect();
            ids.sort();
            ids.dedup();
            let options = if ids.is_empty() {
                Vec::new()
            } else {
                // RE-02 (ref parity, plugins.tsx:54/308-318): the
                // footer carries the status only when NOT active
                // ("inactive"); an active row shows no status footer.
                ids.iter()
                    .map(|id| {
                        let option = SelectOption::titled(id.clone(), id.clone());
                        if store.plugin_active(id) {
                            option
                        } else {
                            option.with_footer("inactive".to_owned())
                        }
                    })
                    .collect()
            };
            SelectProps::new("Plugins", options)
                .with_action(SelectAction::new("dialog.plugins.check", "ctrl+a", "show internal"))
                // RE-02: the `plugins.toggle` action (plugins.tsx:
                // 231-237, side right) — enter flips the selected
                // registered plugin's active state.
                .with_action(SelectAction::new("plugins.toggle", "return", "toggle"))
        }
        // The tab context menu (lane KM — `TabContextMenu`'s actions,
        // session-tabs.tsx:391-417): a tab-scoped menu carries New
        // session/Rename/Copy session ID/Close; the strip-scoped one
        // (no session) lists the recently-closed tabs (≤ 10) with
        // reopen actions, else the muted empty row. Values carry the
        // action verb for the submit path.
        DialogKind::TabMenu { session_id, .. } => {
            // A tab-scoped menu: New session / Rename / Copy session
            // ID / Close. A strip-scoped one (no session): the
            // recently-closed list (≤ 10) with reopen actions, else
            // the muted empty row. Values carry the action verb for
            // the submit path.
            if session_id.is_some() {
                let options = vec![
                    SelectOption::titled(
                        crate::store::NEW_SESSION_TAB_TITLE.to_owned(),
                        "tab:new".to_owned(),
                    ),
                    SelectOption::titled("Rename".to_owned(), "tab:rename".to_owned()),
                    SelectOption::titled("Copy session ID".to_owned(), "tab:copy".to_owned()),
                    SelectOption::titled("Close".to_owned(), "tab:close".to_owned()),
                ];
                SelectProps::new("", options).flat(true)
            } else {
                let closed = store.tabs().closed;
                let mut options = vec![status_option("Recently closed tabs")];
                if closed.is_empty() {
                    options.push(status_option("No recently closed tabs"));
                } else {
                    for entry in closed.iter().rev().take(10) {
                        let title = entry
                            .tab
                            .title
                            .clone()
                            .unwrap_or_else(|| "Untitled session".to_owned());
                        options.push(SelectOption::titled(
                            title,
                            format!("tab:reopen:{}", entry.tab.session_id),
                        ));
                    }
                }
                SelectProps::new("", options).flat(true)
            }
        }
        DialogKind::Mcp => {
            let mut servers = store.mcp_servers();
            // RD-18 — the ref dialog renders `mcp.servers()`' order
            // (core localeCompare); the belt-and-braces re-sort uses
            // the same collation.
            servers.sort_by(|left, right| oc_util::localecmp::locale_cmp(&left.name, &right.name));
            let options = servers
                .iter()
                .map(|server| {
                    let footer = match server.status {
                        "connected" => "Connected ✓".to_owned(),
                        "pending" => "Connecting …".to_owned(),
                        "failed" => "Failed !".to_owned(),
                        "needs_auth" => "Sign in required →".to_owned(),
                        _ => "Disabled ○".to_owned(),
                    };
                    let toggle = match server.status {
                        "connected" => "disconnect",
                        "failed" => "retry",
                        "needs_auth" => "sign in",
                        _ => "connect",
                    };
                    SelectOption::titled(server.name.clone(), server.name.clone())
                        .with_footer(footer)
                        .with_search(toggle.to_owned())
                })
                .collect();
            let mut props = SelectProps::new("MCP servers", options);
            if store.mcp_servers().is_empty() {
                props = SelectProps::new(
                    "MCP servers",
                    vec![SelectOption::titled("No MCP servers", "status").muted_placeholder()],
                );
            }
            // The toggle action (`dialog.mcp.toggle` = space,
            // dialog-mcp.tsx:145-153).
            props = props.flat(true).with_actions(vec![SelectAction::new(
                "dialog.mcp.toggle",
                "toggle",
                "space",
            )]);
            props
        }
        // The MCP error details (dialog-mcp.tsx:163-174 — the
        // `DialogErrorDetails` role: error + context rows).
        DialogKind::McpError { name } => {
            let row = store.mcp_servers().into_iter().find(|server| server.name == *name);
            let mut options = Vec::new();
            if let Some(row) = row {
                options.push(SelectOption::titled(
                    row.error.clone().unwrap_or_else(|| "Unknown MCP connection error".to_owned()),
                    "error",
                ));
                options.push(
                    SelectOption::titled(format!("Status: {}", row.status), "context")
                        .muted_placeholder(),
                );
                options.push(
                    SelectOption::titled(
                        format!("Configuration: mcp.servers.{}", row.name),
                        "config",
                    )
                    .muted_placeholder(),
                );
            }
            SelectProps::new(format!("MCP server: {name}"), options).flat(true)
        }
        // Lane TAIL `DialogUpdate` (`/update`, dialog-update.tsx):
        // the live state row (title + detail) + the Skip/Update/
        // Restart actions; enter confirms.
        DialogKind::Update => {
            let state = store.update_dialog_state();
            let (title, detail) = crate::components::update::dialog_rows(&state);
            let mut options = vec![SelectOption::titled(title, detail).muted_placeholder()];
            options.push(SelectOption::titled("Skip this version", "skip"));
            if let Some(confirm) = crate::components::update::dialog_confirm(&state) {
                options.push(SelectOption::titled(confirm, "confirm"));
            }
            SelectProps::new("Update opencode", options).flat(true)
        }
        // SURF-2 `DialogStatus` (`/status`, dialog-status.tsx:29-56):
        // the count line + per-server bullet rows, or the empty
        // fallback.
        DialogKind::Status => {
            let servers = store.mcp_servers();
            let mut options = Vec::new();
            if servers.is_empty() {
                options.push(SelectOption::titled("No MCP servers", "status").muted_placeholder());
            } else {
                options.push(SelectOption::titled(
                    format!(
                        "{} MCP server{}",
                        servers.len(),
                        if servers.len() == 1 { "" } else { "s" }
                    ),
                    "count",
                ));
                for server in &servers {
                    let status_text = match server.status {
                        "connected" => "Connected".to_owned(),
                        "failed" => server.error.clone().unwrap_or_else(|| "failed".to_owned()),
                        "disabled" => "Disabled in configuration".to_owned(),
                        "needs_auth" => format!(
                            "Needs authentication: {}",
                            server.error.clone().unwrap_or_default()
                        ),
                        _ => server.status.to_owned(),
                    };
                    options.push(SelectOption::titled(
                        format!("• {} {}", server.name, status_text),
                        server.name.clone(),
                    ));
                }
            }
            SelectProps::new("Status", options).flat(true)
        }
        // SURF-2 `DialogVariant` (`/variants`, dialog-variant.tsx:
        // 10-29): "Default" + the current model's variants.
        DialogKind::Variant => {
            let mut options = vec![SelectOption::titled("Default", "default")];
            for variant in store.current_model_variants() {
                options.push(SelectOption::titled(variant.clone(), variant));
            }
            let current = match store.route() {
                Route::Session { session_id } => store.selection_for(&session_id).model,
                Route::Home => store.selection_for(crate::app::HOME_PROMPT_KEY).model,
            }
            .and_then(|model| model.variant)
            .unwrap_or_else(|| "default".to_owned());
            SelectProps::new("Select variant", options).flat(true).with_current(Some(current))
        }
        // F15 fix round: `DialogPair` (`/pair`, dialog-pair.tsx) is a
        // CUSTOM dialog, NOT a DialogSelect — the word-wrapped intro
        // (+ link blocks, loopback hint, loading/error states) renders
        // through `dialog_pair::rows` + `dialog_solid::Family::Pair`.
        // The props are unused by that tree (the key layer still asks
        // for them unconditionally); the QR block stays REGISTERED
        // (dialog_pair.rs header).
        DialogKind::Pair => SelectProps::new("Pair", Vec::new()),
        // SURF-2 `DialogDebug` (`/debug`, dialog-debug.tsx:25-35): the
        // env/config entries + the share hint (enter copies).
        DialogKind::Debug => {
            let model = match store.route() {
                Route::Session { session_id } => store.effective_model(&session_id),
                Route::Home => store.effective_model(crate::app::HOME_PROMPT_KEY),
            };
            let session = match store.route() {
                Route::Session { session_id } => session_id,
                Route::Home => "n/a".to_owned(),
            };
            let terminal = std::env::var("TERM").unwrap_or_default();
            let entries = [
                ("Version", format!("{} (local)", env!("CARGO_PKG_VERSION"))),
                ("Date", chrono_like_now()),
                ("OS", std::env::consts::OS.to_owned()),
                ("Terminal", terminal),
                ("Session ID", session),
                (
                    "Model",
                    model.map_or_else(
                        || "n/a".to_owned(),
                        |model| format!("{}/{}", model.provider_id, model.model_id),
                    ),
                ),
            ];
            let options = entries
                .iter()
                .map(|(label, value)| {
                    SelectOption::titled(format!("{label:<10}{value}"), format!("debug:{label}"))
                })
                .collect();
            SelectProps::new("Debug", options).flat(true).with_actions(vec![SelectAction::new(
                "dialog.debug.copy",
                "copy",
                "return",
            )])
        }
        // SURF-2 `DialogTimeline` (`session.timeline`,
        // dialog-timeline.tsx:21-39): the user messages newest-first
        // with the short-time footer.
        DialogKind::Timeline { session_id } => {
            let mut options = Vec::new();
            for message in store.messages(session_id) {
                if let crate::components::message::model::Message::User {
                    id, text, created, ..
                } = &message
                {
                    options.push(
                        SelectOption::titled(text.replace('\n', " "), id.clone())
                            .with_footer(short_time(*created)),
                    );
                }
            }
            options.reverse();
            SelectProps::new("Timeline", options)
        }
        // The diff source select (`openSwitchDiffDialog`,
        // diff-viewer.tsx:735-767): one row per available source, the
        // current one marked; "turn" only from a session route.
        DialogKind::DiffSource => {
            let live = store.diff_viewer();
            let mut options = Vec::new();
            for mode in [DiffSource::Branch, DiffSource::Committed, DiffSource::Working] {
                options.push(
                    SelectOption::titled(mode.label(), mode.vcs_mode())
                        .described(mode.description())
                        .with_search(mode.description().to_owned()),
                );
            }
            if live.session_id.is_some() {
                options.push(
                    SelectOption::titled(DiffSource::Turn.label(), "turn")
                        .described(DiffSource::Turn.description()),
                );
            }
            SelectProps::new("Diff source", options)
                .flat(true)
                .with_current(Some(live.mode.vcs_mode().to_owned()))
        }
        // The diff shortcuts dialog (`DiffViewerHelpDialog`,
        // diff-viewer.tsx:1100-1192): the viewer's key groups with the
        // binding displays (`shortcut()` reads the table's defaults).
        DialogKind::DiffHelp => {
            let shortcut = |commands: &str| -> String {
                let shortcuts: Vec<String> = commands
                    .split('|')
                    .map(|command| {
                        crate::keymap::default_table()
                            .iter()
                            .find(|def| def.command == command)
                            .map_or_else(String::new, |def| {
                                def.defaults
                                    .split(',')
                                    .map(str::trim)
                                    .filter(|spec| !spec.is_empty() && *spec != "none")
                                    .map(crate::components::dialog::spec_display)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                    })
                    .filter(|text| !text.is_empty())
                    .collect();
                if shortcuts.is_empty() {
                    "unbound".to_owned()
                } else {
                    shortcuts.join(" / ")
                }
            };
            let mut options = Vec::new();
            let mut group = |title: &str, rows: &[(&str, &str)]| {
                for (commands, label) in rows {
                    let shortcut_text = shortcut(commands);
                    let value = format!("help:{}", label.replace('/', "-"));
                    options.push(
                        SelectOption::titled(format!("{shortcut_text:<17}{label}"), value)
                            .categorized(title),
                    );
                }
            };
            group(
                "Files",
                &[
                    ("diff.next_file|diff.previous_file", "Next / previous file"),
                    ("diff.next_hunk|diff.previous_hunk", "Next / previous hunk"),
                    ("diff.mark_reviewed", "Mark reviewed"),
                ],
            );
            group(
                "Scroll",
                &[
                    ("diff.down|diff.up", "Down / up"),
                    ("diff.half_page.down|diff.half_page.up", "Half page down / up"),
                    ("diff.page.down|diff.page.up", "Page down / up"),
                    ("diff.first|diff.last", "First / last"),
                ],
            );
            group(
                "View",
                &[
                    ("diff.toggle_view", "Split / unified"),
                    ("diff.single_patch", "All files / single file"),
                    ("diff.toggle_file_tree", "Show / hide file tree"),
                    ("diff.switch_source", "Switch diff source"),
                    ("diff.close", "Close diff viewer"),
                ],
            );
            SelectProps::new("Diff shortcuts", options).flat(true)
        }
        // `DialogMessage` (`routes/session/dialog-message.tsx`): the
        // four fixed actions over the clicked user message.
        DialogKind::MessageActions { .. } => SelectProps::new(
            "Message Actions",
            vec![
                SelectOption::titled("Jump to", "message.jump")
                    .described("view message in session"),
                SelectOption::titled("Revert", "session.revert")
                    .described("undo messages and file changes"),
                SelectOption::titled("Copy", "message.copy").described("message text to clipboard"),
                SelectOption::titled("Fork", "session.fork").described("create a new session"),
            ],
        ),
        // `ui/working-directory-actions.tsx`: the three fixed actions
        // over the footer's `{dir}:{branch}` label.
        DialogKind::WorkingDirectory { directory } => SelectProps::new(
            "Working directory",
            vec![
                SelectOption::titled("Copy path", "location.copy").described(directory.clone()),
                SelectOption::titled("Open folder", "location.open")
                    .described("in system file manager"),
                SelectOption::titled("Workspaces", "session.move")
                    .described("to another working directory"),
            ],
        ),
    }
}

/// The entry-aware props table: the theme dialog's `current` keys on
/// the ENTRY's captured initial (the mount-time theme), everything
/// else falls through to [`select_props_for`]. Every site that holds
/// the open [`crate::store::DialogEntry`] (the key layer, the render
/// overlay, the store's focus/preview helpers) must go through here —
/// a live-theme `current` re-marks the previewed theme under a filter.
#[must_use]
pub fn select_props_for_entry(entry: &crate::store::DialogEntry, store: &Store) -> SelectProps {
    match &entry.kind {
        DialogKind::ThemeList => {
            theme_list_props_with_current(store, entry.theme_initial.as_deref())
        }
        // RD-17 — the palette's list tracks the live filter text
        // (ref `list()` reads `ref?.filter`): suggested rows without
        // one, the setting rows under one.
        DialogKind::CommandPalette => palette_props_filtered(store, &entry.select.filter),
        kind => select_props_for(kind, store),
    }
}

/// The home directory for `abbreviateHome` (the v2 TUI reads it from
/// the runtime paths at boot; the port resolves `$HOME` at props
/// time — tests inject explicit homes through the worktree props
/// builder).
fn home_directory() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// The model picker's props from the store's `model.list` state
/// (`component/dialog-model.tsx`): non-deprecated rows, the "Free"
/// footer (`cost.length > 0 && every input zero`), `providerID/modelID`
/// values. While UNCONNECTED the rows render FLAT — no provider
/// categories (group headers) and no per-row description
/// (`category: connected() ? provider?.name : undefined`,
/// `description: favorite ? "(Favorite)" : undefined`,
/// dialog-model.tsx:77-97) — and the footer action reads
/// "View all integrations" (`:168-176`, the `ctrl+a` keybind at
/// config/keybind.ts:162); connected flips both (categories +
/// grouped sort + "Connect an integration"). Loading/error show a
/// muted status row (the reference's empty state); an empty list
/// renders the select's "No items available".
#[must_use]
pub fn model_list_props(store: &Store, title: &'static str) -> SelectProps {
    let lists = store.server_lists();
    let connected = lists.connected;
    let provider_name = |id: &str| -> String {
        lists
            .providers
            .iter()
            .find(|provider| provider.id == id)
            .map_or_else(|| id.to_owned(), |provider| provider.name.clone())
    };
    let options = match &lists.models {
        ListState::Loading => vec![status_option("Loading models…")],
        ListState::Failed(error) => vec![status_option(&format!("Models unavailable: {error}"))],
        ListState::Loaded(models) => {
            let mut rows: Vec<&crate::store::ServerModel> =
                models.iter().filter(|model| !model.deprecated).collect();
            // The dialog's browse mode sorts its full option list with
            // `sortModelOptions(…, connected())` — connected → grouped.
            sort_model_options(&mut rows, connected, &provider_name);
            rows.iter()
                .map(|model| {
                    let title =
                        if model.name.is_empty() { model.id.clone() } else { model.name.clone() };
                    let value = format!("{}/{}", model.provider_id, model.id);
                    let mut option = SelectOption::titled(title, value)
                        .with_search(format!("{} {}", model.provider_id, model.id));
                    // Categories exist only while connected — the
                    // unconnected dialog renders one flat list.
                    if connected {
                        option = option.categorized(provider_name(&model.provider_id));
                    }
                    if model.free {
                        option = option.with_footer("Free");
                    }
                    option
                })
                .collect()
        }
    };
    let mut props = SelectProps::new(title, options);
    props = props.flat(true);
    // The footer action (`actions[0]`, dialog-model.tsx:167-183):
    // unconnected → "View all integrations", connected →
    // "Connect an integration"; both ride `ctrl+a`
    // (`model.dialog.provider`, keybind.ts:162).
    props = props.with_actions(vec![SelectAction::new(
        "model.dialog.provider",
        if connected { "Connect an integration" } else { "View all integrations" },
        "ctrl+a",
    )]);
    props
}

/// `localeCompare` approximation — the repo convention (ICU primary
/// is case-insensitive; tertiary puts lowercase first on ties).
/// RD-18: the shared `oc-util` collation carries the non-ASCII
/// folds; ASCII behavior is unchanged.
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    oc_util::localecmp::locale_cmp(a, b)
}

/// TUI-9: `getRelativeTime` (dialog-stash.ts:11-24) — "just now" /
/// "Nm ago" / "Nh ago" / "Nd ago" against the wall clock
/// (`Date.now()`; the stash's timestamps are epoch ms).
fn relative_time(timestamp_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
    let seconds = now.saturating_sub(timestamp_ms) / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if seconds < 60 {
        "just now".to_owned()
    } else if minutes < 60 {
        format!("{minutes}m ago")
    } else if hours < 24 {
        format!("{hours}h ago")
    } else if days < 7 {
        format!("{days}d ago")
    } else {
        // `Locale.datetime` — the fixed-locale date form.
        let days_since_epoch = timestamp_ms / 1000 / 86_400;
        let z = i64::try_from(days_since_epoch).unwrap_or(0) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// v2 `sortModelOptions` (`dialog-model.tsx:185-211`), verbatim key
/// order: grouped → `opencode-go` providers first, then `opencode`,
/// then provider-name collation; then Free-first; then release date
/// DESCENDING; then title collation. `grouped = false` (the dialog's
/// search path) skips the two provider keys — Free/release/title
/// ordering stays. Ties keep the incoming order (both sorts stable).
pub fn sort_model_options(
    rows: &mut [&crate::store::ServerModel],
    grouped: bool,
    provider_name: &dyn Fn(&str) -> String,
) {
    let title = |model: &crate::store::ServerModel| -> String {
        if model.name.is_empty() {
            model.id.clone()
        } else {
            model.name.clone()
        }
    };
    rows.sort_by(|a, b| {
        let provider = if grouped {
            usize::from(b.provider_id == "opencode-go")
                .cmp(&usize::from(a.provider_id == "opencode-go"))
                .then_with(|| {
                    usize::from(b.provider_id == "opencode")
                        .cmp(&usize::from(a.provider_id == "opencode"))
                })
                .then_with(|| {
                    locale_cmp(&provider_name(&a.provider_id), &provider_name(&b.provider_id))
                })
        } else {
            std::cmp::Ordering::Equal
        };
        provider
            .then_with(|| usize::from(b.free).cmp(&usize::from(a.free)))
            .then_with(|| b.released.total_cmp(&a.released))
            .then_with(|| locale_cmp(&title(a), &title(b)))
    });
}

/// The agent picker's props from the store's `agent.list` state
/// (`component/dialog-agent.tsx`): non-hidden non-subagent rows, the
/// id as title, the description beside it, `current` = the active
/// session's selection.
#[must_use]
pub fn agent_list_props(store: &Store, title: &'static str) -> SelectProps {
    let lists = store.server_lists();
    let current = active_selection(store).agent;
    let options = match &lists.agents {
        ListState::Loading => vec![status_option("Loading agents…")],
        ListState::Failed(error) => vec![status_option(&format!("Agents unavailable: {error}"))],
        ListState::Loaded(agents) => agents
            .iter()
            .filter(|agent| agent.mode != "subagent" && !agent.hidden)
            .map(|agent| {
                let mut option = SelectOption::titled(agent.id.clone(), agent.id.clone());
                if let Some(description) = &agent.description {
                    option = option.described(description.clone());
                }
                option
            })
            .collect(),
    };
    SelectProps::new(title, options).with_current(current)
}

/// The active session's model selection as a picker `current` value
/// (`providerID/modelID`) — the RESOLVED selection (the chain's
/// fallbacks included; an unset session still marks the model a
/// submit would commit as current).
fn model_current_value(store: &Store) -> Option<String> {
    let key = match store.route() {
        Route::Session { session_id } => session_id,
        Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
    };
    store.effective_model(&key).map(|model| format!("{}/{}", model.provider_id, model.model_id))
}

/// The selection state of the focused route's session (home carries
/// the next-created session's draft).
fn active_selection(store: &Store) -> crate::store::SessionSelection {
    let key = match store.route() {
        Route::Session { session_id } => session_id,
        Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
    };
    store.selection_for(&key)
}

/// A muted status row (loading/error) — the placeholder surface.
fn status_option(text: &str) -> SelectOption {
    SelectOption::titled(text, "status").muted_placeholder()
}

/// `Locale.time` (`util/locale.ts:7-10` — `toLocaleTimeString` at
/// `timeStyle: "short"`): the local HH:MM form. The port renders the
/// 24-hour short form (a fixed locale, like every other ported
/// locale surface).
fn short_time(epoch_seconds: u64) -> String {
    let seconds_of_day = epoch_seconds % 86_400;
    format!("{:02}:{:02}", seconds_of_day / 3_600, (seconds_of_day % 3_600) / 60)
}

/// `new Date().toISOString()` (dialog-debug.tsx:29): the UTC stamp.
fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let date = now / 86_400;
    let seconds_of_day = now % 86_400;
    // Days since epoch → y/m/d (civil-from-days, Howard Hinnant).
    let z = i64::try_from(date).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    )
}

impl SelectOption {
    /// Marker helper for status/stub rows.
    #[must_use]
    pub fn muted_placeholder(mut self) -> Self {
        self.description = Some("(server-backed list — fetch in flight or unavailable)".into());
        self
    }
}

/// Execute a submitted server-list row by the picker's command:
/// model/agent selections stage + commit through the selection sink,
/// session rows switch tabs (`DialogModel.onSelect` /
/// `DialogAgent.onSelect` / the session list's navigate).
pub fn execute_server_list_selection(
    store: &Store,
    command: &str,
    value: &str,
) -> crate::components::command_palette::Dispatch {
    match command {
        "model.list" => {
            let Some((provider_id, model_id)) = value.split_once('/') else {
                return Dispatch::Handled;
            };
            let model = ModelSelection {
                provider_id: provider_id.to_owned(),
                model_id: model_id.to_owned(),
                variant: None,
            };
            match store.route() {
                Route::Session { session_id } => store.select_model(&session_id, &model),
                Route::Home => store.stage_model(crate::app::HOME_PROMPT_KEY, model),
            }
            Dispatch::Handled
        }
        "agent.list" => {
            // The reference validates against the agent list and warns
            // on unknown ids (`local.agent.set`).
            let known = match store.server_lists().agents {
                ListState::Loaded(agents) => {
                    agents.iter().any(|agent: &ServerAgent| agent.id == value)
                }
                _ => true, // list unavailable: trust the row we rendered
            };
            if !known {
                store.toast_show(crate::components::toast::ToastOptions {
                    message: format!("Agent not found: {value}"),
                    variant: "warning",
                    ..crate::components::toast::ToastOptions::info("")
                });
                return Dispatch::Handled;
            }
            match store.route() {
                Route::Session { session_id } => store.select_agent(&session_id, value),
                Route::Home => store.stage_agent(crate::app::HOME_PROMPT_KEY, value),
            }
            Dispatch::Handled
        }
        "session.list" => {
            store.select_tab(value);
            Dispatch::Handled
        }
        _ => Dispatch::Handled,
    }
}

/// The `messages.copy` payload (`routes/session/index.tsx:1129`):
/// the LAST assistant message's text parts joined `\n` and trimmed.
/// `Err` carries the exact v2 toast message for the miss case.
pub fn copy_last_assistant_text(store: &Store, session_id: &str) -> Result<String, &'static str> {
    use crate::components::message::model::Message;
    let messages = store.messages(session_id);
    let Some(Message::Assistant { parts, .. }) =
        messages.iter().rev().find(|message| matches!(message, Message::Assistant { .. }))
    else {
        return Err("No assistant messages found");
    };
    let texts: Vec<&str> = parts
        .iter()
        .filter(|part| part.kind() == "text")
        .map(crate::components::message::model::Part::text)
        .collect();
    if texts.is_empty() {
        return Err("No text parts found in last assistant message");
    }
    let text = texts.join("\n").trim().to_owned();
    if text.is_empty() {
        return Err("No text content found in last assistant message");
    }
    Ok(text)
}

/// Run `messages.copy` end to end: resolve the text (exact v2 toast
/// messages on each miss), write it through the clipboard sink (OSC 52
/// in production), toast the outcome.
pub fn copy_message_to_clipboard(store: &Store) {
    let miss = |message: &str| {
        store.toast_show(crate::components::toast::ToastOptions {
            message: message.to_owned(),
            variant: "error",
            ..crate::components::toast::ToastOptions::info("")
        });
    };
    let hit = |message: &str| {
        store.toast_show(crate::components::toast::ToastOptions {
            message: message.to_owned(),
            variant: "success",
            ..crate::components::toast::ToastOptions::info("")
        });
    };
    let session_id = match store.route() {
        Route::Session { session_id } => session_id,
        Route::Home => {
            miss("No assistant messages found");
            return;
        }
    };
    match copy_last_assistant_text(store, &session_id) {
        Err(message) => miss(message),
        Ok(text) => match store.clipboard_write(&text) {
            Ok(()) => hit("Message copied to clipboard!"),
            Err(_) => miss("Failed to copy to clipboard"),
        },
    }
}

/// TUI-TD13 — `formatSessionTranscript(info, messages, thinking =
/// true)` (index.tsx:3503-3533, the `/copy` arm): the store's live
/// transcript as markdown — `## User` / `## Assistant` sections
/// (reasoning included, the thinking=true arm), joined by `---`
/// under the session header.
#[must_use]
pub fn session_transcript(store: &Store, session_id: &str) -> String {
    use crate::components::message::model::{Message, Part};
    let title = store
        .tabs()
        .tabs
        .iter()
        .find(|tab| tab.session_id == session_id)
        .and_then(|tab| store.display_title(tab))
        .or_else(|| store.session_title(session_id))
        .unwrap_or_else(|| format!("Session {session_id}"));
    let body: Vec<String> = store
        .messages(session_id)
        .iter()
        .filter_map(|message| match message {
            Message::User { text, .. } => Some(format!("## User\n\n{text}")),
            Message::Assistant { parts, .. } => {
                let content: Vec<String> = parts
                    .iter()
                    .map(|part| match part {
                        Part::Text { text, .. } => text.clone(),
                        Part::Reasoning { text, .. } => format!("_Thinking:_\n\n{text}"),
                        // The tool parts carry no display content in
                        // the port's model (the row renderer owns the
                        // projection) — the name rides as the section.
                        Part::Tool { name, .. } => format!("**Tool: {name}**"),
                    })
                    .collect();
                (!content.is_empty()).then(|| format!("## Assistant\n\n{}", content.join("\n\n")))
            }
            _ => None,
        })
        .collect();
    let (created, updated) = store
        .session_history()
        .iter()
        .find(|session| session.id == session_id)
        .map_or((0, 0), |session| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let seconds = |value: Option<f64>| value.map_or(0.0, f64::round) as u64;
            (seconds(session.time_created), seconds(session.time_updated))
        });
    format!(
        "# {title}\n\n**Session ID:** {session_id}\n**Created:** {created}\n**Updated:** {updated}\n\n---\n\n{}\n",
        body.join("\n\n---\n\n")
    )
}

/// TUI-TD13 — `session.copy`'s clipboard write (index.tsx:1183-1199):
/// the transcript to the clipboard; `Err` carries the v2 catch-arm
/// toast text. `None` resolves the routed session (home = the miss).
pub fn copy_session_transcript(
    store: &Store,
    session_id: Option<&str>,
) -> Result<(), &'static str> {
    let routed = match store.route() {
        Route::Session { session_id } => Some(session_id),
        Route::Home => None,
    };
    let Some(session_id) = session_id.or(routed.as_deref()) else {
        return Err("Failed to copy session transcript");
    };
    let text = session_transcript(store, session_id);
    store.clipboard_write(&text).map_err(|_| "Failed to copy session transcript")
}

/// Run `session.copy` end to end (the exact v2 toasts,
/// index.tsx:1192-1199).
pub fn copy_transcript_to_clipboard(store: &Store) {
    let miss = |message: &str| {
        store.toast_show(crate::components::toast::ToastOptions {
            message: message.to_owned(),
            variant: "error",
            ..crate::components::toast::ToastOptions::info("")
        });
    };
    let hit = |message: &str| {
        store.toast_show(crate::components::toast::ToastOptions {
            message: message.to_owned(),
            variant: "success",
            ..crate::components::toast::ToastOptions::info("")
        });
    };
    match copy_session_transcript(store, None) {
        Ok(()) => hit("Session transcript copied to clipboard!"),
        Err(message) => miss(message),
    }
}

/// The theme registry listing (`dialog-theme-list.tsx`):
/// `Object.keys(themes.all()).sort(localeCompare base)` — the store's
/// registry (builtins + the `system` slot + discovered customs),
/// value = the name, `current` = the active theme. The options carry
/// NO category — the ref's dialog renders a FLAT list (no group
/// header), unlike the palette's categorized sections.
#[must_use]
pub fn theme_list_props(store: &Store) -> SelectProps {
    theme_list_props_with_current(store, None)
}

/// The themes dialog's `current` is the MOUNT-TIME capture
/// (`current={initial}`), not the live preview: while the dialog is
/// open, the `●` marker and the filter-clear re-focus key on the
/// captured initial, so a previewed theme never re-marks itself
/// (under `filter: "nord"` the initial `opencode` is filtered OUT and
/// the marker disappears — the ref's exact shape).
#[must_use]
pub fn theme_list_props_with_current(store: &Store, current: Option<&str>) -> SelectProps {
    let registry = store.registry();
    let current = current.map_or_else(|| store.theme().name.clone(), str::to_owned);
    let options: Vec<SelectOption> =
        registry.names().into_iter().map(|name| SelectOption::titled(name.clone(), name)).collect();
    SelectProps::new("Themes", options).with_current(Some(current))
}

// -- command execution --------------------------------------------------------

/// The result of dispatching one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    /// The command ran (store mutation and/or dialog transition).
    Handled,
    /// The command needs a server call — stubbed until wave 3.
    ServerPending(&'static str),
    /// No handler knows the command.
    Unknown,
}

/// Command execution seam: maps keymap/palette commands onto store
/// actions and dialog transitions. The store owns the default
/// ([`StoreDispatcher`]); tests and future server-backed
/// implementations override.
pub trait CommandDispatcher {
    fn dispatch(&mut self, store: &Store, command: &str, args: Option<&str>) -> Dispatch;
}

/// The default dispatcher: local store actions run; server-backed
/// commands report [`Dispatch::ServerPending`] stubs.
#[derive(Debug, Default, Clone, Copy)]
pub struct StoreDispatcher;

impl CommandDispatcher for StoreDispatcher {
    #[allow(clippy::too_many_lines)] // one arm per command, the palette dispatch table
    fn dispatch(&mut self, store: &Store, command: &str, args: Option<&str>) -> Dispatch {
        match command {
            "command.palette.show" => {
                store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
                Dispatch::Handled
            }
            "session.new" => {
                // v2 app.tsx:726-731: the new session inherits the
                // CURRENT selection (`local.model.set` / the agent) —
                // staging it onto the home draft (F10).
                inherit_selection_onto_home(store);
                store.navigate(Route::Home);
                store.clear_dialogs();
                Dispatch::Handled
            }
            "session.clear" => {
                if let Route::Session { session_id } = store.route() {
                    store.close_tab(Some(&session_id));
                }
                // Same inheritance as session.new (app.tsx:742-749).
                inherit_selection_onto_home(store);
                store.navigate(Route::Home);
                store.clear_dialogs();
                Dispatch::Handled
            }
            "session.tab.close" => {
                if let Route::Session { session_id } = store.route() {
                    store.close_tab(Some(&session_id));
                }
                Dispatch::Handled
            }
            "session.tab.reopen" => {
                let _ = store.reopen_tab(None);
                Dispatch::Handled
            }
            // `<leader>b` (config/keybind.ts:95): v2's toggleSidebar
            // (session-frame.tsx:118-130) — the store no-ops off the
            // session route (the command lives in the session frame's
            // keymap layer).
            "session.sidebar.toggle" => {
                store.toggle_sidebar();
                Dispatch::Handled
            }
            // `session.child.first` (config/keybind.ts:136,
            // index.tsx:1255-1263): toggle the composer's subagent
            // picker (#44).
            "session.child.first" => {
                store.toggle_picker();
                Dispatch::Handled
            }
            // The composer panel's keybind family (composer mode —
            // pushed while the picker is open; #44).
            "composer.close" => {
                store.picker_close();
                Dispatch::Handled
            }
            "composer.tab.prev" => {
                store.picker_switch_tab(false);
                Dispatch::Handled
            }
            "composer.tab.next" => {
                store.picker_switch_tab(true);
                Dispatch::Handled
            }
            "composer.subagent.up" => {
                store.picker_move(false);
                Dispatch::Handled
            }
            "composer.subagent.down" => {
                store.picker_move(true);
                Dispatch::Handled
            }
            "composer.subagent.select" => {
                store.picker_navigate();
                Dispatch::Handled
            }
            "composer.subagent.toggle-activity" => {
                store.picker_toggle_filter();
                Dispatch::Handled
            }
            "composer.subagent.interrupt" => {
                store.picker_interrupt();
                Dispatch::Handled
            }
            "session.tab.next" | "session.tab.previous" => {
                let direction = if command == "session.tab.next" { 1 } else { -1 };
                let _ = store.cycle_tab(direction, false);
                Dispatch::Handled
            }
            "session.tab.next_unread" | "session.tab.previous_unread" => {
                let direction = if command == "session.tab.next_unread" { 1 } else { -1 };
                let _ = store.cycle_tab(direction, true);
                Dispatch::Handled
            }
            command
                if command.starts_with("session.tab.select.")
                    || command.starts_with("session.quick_switch.") =>
            {
                let slot = command
                    .rsplit('.')
                    .next()
                    .and_then(|digit| digit.parse::<u8>().ok())
                    .unwrap_or(1);
                let _ = store.select_quick_slot(slot);
                Dispatch::Handled
            }
            "session.rename" => {
                let session_id = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => return Dispatch::Handled,
                };
                // TUI-TD12 — `/rename <title>` (index.tsx:899-911): a
                // NON-empty argument renames DIRECTLY (the optimistic
                // tab patch + the `session.update` request; the empty
                // string is the title-GENERATE arm); NO argument opens
                // the rename dialog.
                if let Some(title) = args.map(str::trim) {
                    store.clear_dialogs();
                    store.request_session_rename(&session_id, title);
                } else {
                    store.replace_dialog(
                        DialogKind::SessionRename { session_id: session_id.clone() },
                        DialogSize::Medium,
                    );
                    // The prompt seeds with the session's current title
                    // (`value={props.currentTitle}`,
                    // dialog-session-rename.tsx:16) — the session row's
                    // live title (registry-first, index.tsx:905's
                    // `session()?.title`); an untitled session opens
                    // empty.
                    let tabs = store.tabs();
                    let current = tabs
                        .tabs
                        .iter()
                        .find(|tab| tab.session_id == session_id)
                        .and_then(|tab| store.display_title(tab))
                        .unwrap_or_default();
                    store.update_dialog(|top| top.value = current);
                }
                Dispatch::Handled
            }
            "theme.switch" => {
                store.replace_dialog(DialogKind::ThemeList, DialogSize::Medium);
                Dispatch::Handled
            }
            // `theme.switch_mode` / `theme.mode.lock` (app.tsx:
            // 1054-1073): toggle-and-pin / lock-unlock through the
            // config sink (SURF-2, the F-1 consumption halves).
            "theme.switch_mode" => {
                store.theme_switch_mode();
                store.clear_dialogs();
                Dispatch::Handled
            }
            "theme.mode.lock" => {
                store.theme_mode_lock();
                store.clear_dialogs();
                Dispatch::Handled
            }
            "help.show" => {
                // v2's DialogHelp takes the default (medium) size —
                // no explicit setSize in dialog-help.tsx.
                store.replace_dialog(DialogKind::Help, DialogSize::Medium);
                Dispatch::Handled
            }
            // `/worktrees` + the palette's "Manage workspaces"
            // (`session.move`): the Worktrees dialog (R-05).
            "session.move" => {
                store.open_worktrees_dialog();
                Dispatch::Handled
            }
            "prompt.editor" => {
                // The run loop drains the flag: suspend → $EDITOR →
                // resume → replace the focused draft (`editor.rs`).
                store.request_editor();
                Dispatch::Handled
            }
            // -- TUI-8: `/skills` + the palette's "Skills"
            // (`prompt.skills`, prompt/index.tsx:578-605): the large
            // DialogSkill over the skill.list state.
            "prompt.skills" => {
                store.clear_dialogs();
                store.open_skills_dialog();
                Dispatch::Handled
            }
            // -- TUI-9: the stash family (prompt/index.tsx:874-920):
            // push stashes + resets, pop restores, list opens the
            // DialogStash.
            "prompt.stash" => {
                store.stash_push();
                Dispatch::Handled
            }
            "prompt.stash.pop" => {
                store.stash_pop();
                Dispatch::Handled
            }
            "prompt.stash.list" => {
                if store.stash_entries().is_empty() {
                    return Dispatch::Handled;
                }
                store.clear_dialogs();
                store.replace_dialog(DialogKind::Stash, DialogSize::Medium);
                Dispatch::Handled
            }
            // -- TUI-10: `<leader>i` ("View image attachments",
            // prompt.images.view — prompt/index.tsx:493-498): the
            // xlarge DialogImagePreview over the focused composer's
            // data:image attachments (deduplicated,
            // deduplicatePromptImages).
            // TUI-15 — `prompt.editor_context.clear`
            // (index.tsx:459-466): pin the current selection's key as
            // dismissed and drop it (`dismissEditorContext`).
            "prompt.editor_context.clear" => {
                store.editor_context_clear();
                Dispatch::Handled
            }
            "prompt.images.view" => {
                let key = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
                };
                let composer = store.composer(&key);
                let mut images: Vec<crate::store::ImagePreviewItem> = Vec::new();
                for file in &composer.draft.files {
                    let Some(payload) = &file.payload else { continue };
                    if !payload.mime.starts_with("image/") {
                        continue;
                    }
                    let uri = payload.to_data_uri();
                    if images.iter().any(|item| item.uri == uri) {
                        continue;
                    }
                    images.push(crate::store::ImagePreviewItem {
                        uri,
                        mention: Some(file.mention_text()),
                    });
                }
                if images.is_empty() {
                    return Dispatch::Handled;
                }
                store.clear_dialogs();
                store.open_image_preview(images, 0);
                Dispatch::Handled
            }
            // -- TUI-TD22 (PORT ADDITION, the toast.activate
            // precedent): the palette row opening DialogExperiments —
            // v2's entry is the DevToolsBar item (06 #20 KNOWN, a
            // separate ledger row); the dialog itself is the faithful
            // port (v2.0.24's empty table → "No experiments
            // available").
            "opencode.experiments" => {
                store.clear_dialogs();
                // G-07: ref's DevToolsBar item opens the experiments
                // dialog — with the bar enabled the row also reveals
                // it (the bar's own entry point, devtools-bar.tsx:429).
                if store.devtools_enabled() {
                    store.set_devtools_bar_visible(true);
                }
                store.replace_dialog(DialogKind::Experiments, DialogSize::Medium);
                Dispatch::Handled
            }
            // G-07 — the devtools bar's palette rows.
            "devtools.toggle" => {
                if store.devtools_enabled() {
                    store.toggle_devtools_bar();
                } else {
                    store.set_devtools_enabled(true);
                    store.set_devtools_bar_visible(true);
                }
                store.request_redraw();
                Dispatch::Handled
            }
            "devtools.panel" => {
                store.devtools_cycle_panel(1);
                store.request_redraw();
                Dispatch::Handled
            }
            "devtools.dump" => {
                if let Some(path) = store.devtools_dump_state((0, 0)) {
                    store.toast_show(crate::components::toast::ToastOptions {
                        title: Some("Devtools state dumped".to_owned()),
                        message: path.display().to_string(),
                        ..crate::components::toast::ToastOptions::info("")
                    });
                }
                Dispatch::Handled
            }
            "messages.copy" => {
                copy_message_to_clipboard(store);
                Dispatch::Handled
            }
            // TUI-TD13 — `session.copy` / `/copy` (index.tsx:1180-
            // 1200): the transcript markdown to the clipboard + the
            // exact v2 toasts.
            "session.copy" => {
                copy_transcript_to_clipboard(store);
                Dispatch::Handled
            }
            // TUI-34 — `prompt.queue` (prompt/index.tsx:421-436): the
            // queue submit (`<leader>return`'s command through the
            // palette; the same guards as the keybind path).
            "prompt.queue" => {
                let key = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
                };
                let draft = store.prompt_draft(&key);
                if !draft.is_empty() {
                    let trimmed = draft.text.trim().to_owned();
                    let slash = trimmed.starts_with('/');
                    let exit_word = matches!(trimmed.as_str(), "exit" | "quit" | ":q");
                    let running = match store.route() {
                        Route::Session { session_id } => store.session_running(&session_id),
                        Route::Home => false,
                    };
                    if exit_word || slash {
                        store.toast_show(crate::components::toast::ToastOptions {
                            message: "This prompt cannot be queued".to_owned(),
                            variant: "warning",
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    } else if running {
                        let files = draft
                            .files
                            .iter()
                            .filter_map(|file| {
                                file.payload.as_ref().map(
                                    crate::components::composer::AttachmentPayload::to_data_uri,
                                )
                            })
                            .collect::<Vec<String>>();
                        let Route::Session { session_id } = store.route() else {
                            return Dispatch::Handled;
                        };
                        store.queue_prompt(&session_id, &draft.text, &files);
                        let mut composer = store.composer(&key);
                        if let Some(prompt) = composer.take_submit() {
                            composer.history = composer.history.clone().push(&prompt.text);
                            store.set_composer(&key, &composer);
                        }
                    } else {
                        // Idle (or home): an ordinary submit.
                        let _ = store.submit_prompt(&key);
                    }
                }
                Dispatch::Handled
            }
            // -- Lane FINAL: the stub family's real surfaces -----------
            // `provider.connect` (app.tsx:946-958): DialogIntegration
            // over the BE-3 host — the list → method → key/code entry
            // states; on connect the models dialog follows in v2's
            // onConnected, the port refetches the lists + toasts.
            "provider.connect" => {
                store.integration_dialog_open();
                Dispatch::Handled
            }
            // `open.menu` (app.tsx:763-774): DialogOpen (dialog-open.
            // tsx) — the Sessions recents + the Projects sections in
            // one large select; right-arrow on a project row opens
            // the project's worktrees view. The mount refetches both
            // halves (`session.list` + `project.list`).
            "open.menu" => {
                store.open_menu_pressed();
                Dispatch::Handled
            }
            // `opencode.settings` (app.tsx:959-968): DialogConfig — the
            // settings rows this build reads + persists.
            "opencode.settings" => {
                store.replace_dialog(DialogKind::Settings, DialogSize::Medium);
                Dispatch::Handled
            }
            // `session.share` (index.tsx:892-897): v2's own behavior is
            // the unavailable toast — mirrored exactly (the row stays
            // listed; the command reports the v2 state).
            "session.share" => {
                store.clear_dialogs();
                store.toast_show(crate::components::toast::ToastOptions {
                    message: "Sharing is not implemented for V2 sessions yet".to_owned(),
                    variant: "error",
                    duration_ms: 5000,
                    ..crate::components::toast::ToastOptions::info("")
                });
                Dispatch::Handled
            }
            // `session.fork` (index.tsx:928-944): DialogFork over the
            // FIXED fork API (the picker, not the message-context
            // direct path).
            "session.fork" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                store.replace_dialog(
                    DialogKind::ForkPicker { session_id: session_id.clone() },
                    DialogSize::Large,
                );
                Dispatch::Handled
            }
            // `location.reload` (app.tsx:1019-1034): the toast pair
            // around `location.reload` POST.
            "location.reload" => {
                store.location_reload_pressed();
                Dispatch::Handled
            }
            // `docs.open` (app.tsx:1084-1092): openUrl + clear.
            "docs.open" => {
                store.clear_dialogs();
                store.open_url("https://opencode.ai/docs");
                Dispatch::Handled
            }
            // The six dead session commands (index.tsx:945-1254):
            // compact POSTs with the current model; undo reverts the
            // last user message + seeds the composer; redo clears the
            // staged revert; export opens the destination choice;
            // background POSTs. (delete lives in the Sessions dialog's
            // two-press action — dialog-session-list.tsx:255-279.)
            "session.compact" => {
                store.session_compact_pressed();
                Dispatch::Handled
            }
            "session.undo" => {
                store.session_undo_pressed();
                Dispatch::Handled
            }
            "session.redo" => {
                store.session_redo_pressed();
                Dispatch::Handled
            }
            "session.export" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                store.replace_dialog(
                    DialogKind::SessionExport { session_id: session_id.clone() },
                    DialogSize::Medium,
                );
                Dispatch::Handled
            }
            "session.background" => {
                store.session_background_pressed();
                Dispatch::Handled
            }
            "model.list" | "agent.list" | "session.list" => {
                // The mounted dialog's title is the COMPONENT mount's,
                // not the palette row's (the models-dialog lane's #7):
                // DialogModel "Select model" (dialog-model.tsx:122),
                // DialogAgent "Select agent" (dialog-agent.tsx:22),
                // DialogVariant "Select variant" (dialog-variant.tsx:35),
                // DialogMcp "MCP servers" (dialog-mcp.tsx:140),
                // DialogSessions "Sessions" (dialog-session-list.tsx:198).
                // The PALETTE entries keep "Switch model"/"Switch agent"/
                // "Switch model variant" (app.tsx:840/870/921).
                let title: &'static str = match command {
                    "model.list" => "Select model",
                    "agent.list" => "Select agent",
                    "variant.list" => "Select variant",
                    "session.list" => "Sessions",
                    _ => palette_command(command).map_or("", |entry| entry.title),
                };
                let size =
                    if command == "open.menu" { DialogSize::Large } else { DialogSize::Medium };
                // F13: the session list refetches on open (the boot
                // resync snapshot goes stale — v2's DialogSessionList
                // syncs `session.list` on mount).
                if command == "session.list" {
                    store.request_session_list_refresh();
                }
                store.replace_dialog(
                    DialogKind::ServerList { command: static_command(command), title },
                    size,
                );
                Dispatch::Handled
            }
            // -- SURF-2: the diff viewer (`diff.open`'s plugin route +
            // the `diff` key layer, diff-viewer.tsx:559-733) ------
            "diff.open" => {
                store.open_diff_viewer(None);
                Dispatch::Handled
            }
            "diff.close" => {
                store.close_diff_viewer();
                Dispatch::Handled
            }
            "diff.down"
            | "diff.up"
            | "diff.page.down"
            | "diff.page.up"
            | "diff.half_page.down"
            | "diff.half_page.up"
            | "diff.first"
            | "diff.last" => {
                let viewport = usize::from(store.terminal_size().1).max(1);
                store.update_diff_viewer(|live| {
                    crate::components::diff_viewer_mount::diff_scroll_command(
                        live, command, viewport,
                    )
                });
                Dispatch::Handled
            }
            "diff.next_hunk" | "diff.previous_hunk" => {
                let viewport = usize::from(store.terminal_size().1).max(1);
                store.update_diff_viewer(|live| {
                    crate::components::diff_viewer_mount::diff_jump_hunk(
                        live,
                        command == "diff.next_hunk",
                        viewport,
                    )
                });
                Dispatch::Handled
            }
            "diff.next_file" | "diff.previous_file" => {
                let viewport = usize::from(store.terminal_size().1).max(1);
                store.update_diff_viewer(|live| {
                    crate::components::diff_viewer_mount::diff_jump_file(
                        live,
                        command == "diff.next_file",
                        viewport,
                    )
                });
                Dispatch::Handled
            }
            "diff.mark_reviewed"
            | "diff.toggle_file_tree"
            | "diff.single_patch"
            | "diff.toggle_view" => {
                // The viewer's own preference machine
                // (`DiffViewerState::run_command`): tree/view/single/
                // reviewed over the loaded files.
                let live = store.diff_viewer();
                let files = match &live.phase {
                    crate::store::DiffPhase::Loaded(files) => files.clone(),
                    _ => Vec::new(),
                };
                let layout = crate::components::diff_viewer::diff_layout(
                    usize::from(store.terminal_size().0),
                    live.viewer.tree_enabled,
                    files.len(),
                );
                store.update_diff_viewer(|live| live.viewer.run_command(command, &layout, &files));
                Dispatch::Handled
            }
            "diff.switch_source" => {
                store.replace_dialog(DialogKind::DiffSource, DialogSize::Medium);
                Dispatch::Handled
            }
            "diff.help" => {
                store.replace_dialog(DialogKind::DiffHelp, DialogSize::Medium);
                Dispatch::Handled
            }
            // -- SURF-2: the six pure-TUI command wirings (auditE's
            // traceability row: working core, TUI wiring only) ------
            // `/mcps` (mcp.list — dialog-mcp.tsx): the live MCP rows.
            "mcp.list" => {
                store.replace_dialog(DialogKind::Mcp, DialogSize::Medium);
                Dispatch::Handled
            }
            // `/status` (opencode.status — dialog-status.tsx).
            "opencode.status" => {
                store.replace_dialog(DialogKind::Status, DialogSize::Medium);
                Dispatch::Handled
            }
            // `/update` (update.check — dialog-update.tsx): the check
            // starts on open (the dialog's `createResource`).
            "update.check" => {
                store.set_update_dialog(crate::components::update::UpdateDialogState::Checking);
                store.replace_dialog(DialogKind::Update, DialogSize::Medium);
                Dispatch::Handled
            }
            // `/variants` (variant.list — dialog-variant.tsx:919-934):
            // the no-variants guard toasts instead of opening; the
            // palette row is removed per v2's `palette: undefined`.
            "variant.list" => {
                if store.current_model_variants().is_empty() {
                    store.toast_show(crate::components::toast::ToastOptions {
                        title: Some("No variants available".to_owned()),
                        message: "The current model does not support any variants.".to_owned(),
                        variant: "info",
                        ..crate::components::toast::ToastOptions::info("")
                    });
                    return Dispatch::Handled;
                }
                store.replace_dialog(DialogKind::Variant, DialogSize::Medium);
                Dispatch::Handled
            }
            // `/pair` (server.pair — dialog-pair.tsx:18-19): large +
            // centered, the fetch rides the pair sink.
            "server.pair" => {
                store.set_pair_state(crate::store::PairState::Loading);
                if let Some(sink) = store.pair_refresh_sink() {
                    sink.refresh();
                }
                store.replace_dialog(DialogKind::Pair, DialogSize::Large);
                Dispatch::Handled
            }
            // `/debug` (opencode.debug — dialog-debug.tsx:23): large.
            "opencode.debug" => {
                store.replace_dialog(DialogKind::Debug, DialogSize::Large);
                Dispatch::Handled
            }
            // `session.timeline` (dialog-timeline.tsx:17-19): large,
            // session-route only (the message list source).
            "session.timeline" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                store.replace_dialog(
                    DialogKind::Timeline { session_id: session_id.clone() },
                    DialogSize::Large,
                );
                Dispatch::Handled
            }
            // -- lane KM (auditA #8a): dispatchers for the newly-bound
            // v2 commands --------------------------------------------
            // The tab-history walks (keybind.ts:114-115): back/forward
            // through the in-memory navigation stack.
            "session.tab.history.back" => {
                let _ = store.tab_history_back();
                Dispatch::Handled
            }
            "session.tab.history.forward" => {
                let _ = store.tab_history_forward();
                Dispatch::Handled
            }
            // `queued_prompt.undo` (TUI-TD07, index.tsx:653-673): the
            // LAST queued prompt cancels server-side
            // (`session.inbox.cancel`) and its payload refills the
            // composer (`appendPrompt` — text + files appended; v2
            // toasts on a miss).
            "queued_prompt.undo" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                match store.undo_queued_prompt(&session_id) {
                    Some(payload) => {
                        append_prompt_payload(store, &session_id, &payload);
                    }
                    None => {
                        store.toast_show(crate::components::toast::ToastOptions {
                            message: "No queued prompt to undo".to_owned(),
                            variant: "error",
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    }
                }
                Dispatch::Handled
            }
            // `session.copy.id` (index.tsx:1170): the id to the
            // clipboard + the info toast.
            "session.copy.id" => {
                let session_id = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => return Dispatch::Handled,
                };
                match store.clipboard_write(&session_id) {
                    Ok(()) => store.toast_show(crate::components::toast::ToastOptions {
                        message: "Session ID copied to clipboard".to_owned(),
                        variant: "info",
                        ..crate::components::toast::ToastOptions::info("")
                    }),
                    Err(_) => store.toast_show(crate::components::toast::ToastOptions {
                        message: "Failed to copy to clipboard".to_owned(),
                        variant: "error",
                        ..crate::components::toast::ToastOptions::info("")
                    }),
                }
                Dispatch::Handled
            }
            // `session.cd` (prompt/index.tsx:267-289): the full command
            // takes a directory ARGUMENT over the slash route; the
            // palette/dispatch path opens the working-directory select
            // (the footer label's dialog) — the port's pick surface.
            "session.cd" => {
                let directory = store.footer_location();
                store
                    .replace_dialog(DialogKind::WorkingDirectory { directory }, DialogSize::Medium);
                Dispatch::Handled
            }
            // RE-13 — `session.aside` (`/btw`, btw.tsx:58-78): the
            // one-shot side answer. Session route required (the
            // route's context is the generate input); an argument
            // runs directly, the bare command opens the question
            // prompt (`ctx.ui.dialog.prompt`, btw.tsx:64-65).
            "session.aside" => {
                let Route::Session { session_id } = store.route() else {
                    store.clear_dialogs();
                    store.toast_show(crate::components::toast::ToastOptions {
                        message: "Open a session first".to_owned(),
                        variant: "warning",
                        ..crate::components::toast::ToastOptions::info("")
                    });
                    return Dispatch::Handled;
                };
                match args.map(str::trim).filter(|text| !text.is_empty()) {
                    Some(question) => store.btw_ask(&session_id, question),
                    None => store.replace_dialog(DialogKind::BtwPrompt, DialogSize::Medium),
                }
                Dispatch::Handled
            }
            // RE-14 — `stats.open` (stats.tsx:196-209): clear the
            // dialogs, mount the usage poster page (its resource —
            // `session.stats` over the current year, `tools: "none"`
            // — fetches through the stats sink; escape returns).
            "stats.open" => {
                store.open_stats_page(crate::components::stats_page::now_ms());
                Dispatch::Handled
            }
            "stats.close" => {
                store.close_stats_page();
                Dispatch::Handled
            }
            // `session.toggle.exploration_grouping` (index.tsx:
            // 1049-1061): flip the transcript's grouping flag.
            "session.toggle.exploration_grouping" => {
                store.toggle_exploration_grouping();
                Dispatch::Handled
            }
            // `plugins.list` (feature-plugins' DialogPlugins): the
            // plugin manager select — one row per registered plugin
            // id; RE-02 models per-plugin active state (enter =
            // `plugins.toggle` flips it, faulted rows open the
            // error details).
            "plugins.list" => {
                store.replace_dialog(DialogKind::Plugins, DialogSize::Medium);
                Dispatch::Handled
            }
            // `app.clear` (mini/footer.view.tsx:553): clears the MINI
            // screen — the main TUI has no mini surface, so the stroke
            // consumes and redraws (v2's keymap eats the event too).
            "app.clear" => {
                store.request_redraw();
                Dispatch::Handled
            }
            // `toast.activate` (PORT ADDITION, lane KM — auditA #10):
            // v2's toast action is pointer-only (ui/toast.tsx:57-59);
            // the port additionally reaches it from the input layer —
            // dismiss + dispatch the action's command (the MCP
            // failure/needs-auth toasts carry `mcp.list`).
            "toast.activate" => {
                let action = store.toast_activate();
                match action {
                    Some(command) => self.dispatch(store, &command, None),
                    None => Dispatch::Handled,
                }
            }
            // TUI-4 — the pane/terminal family
            // (session-frame.tsx:173-249 + terminals-tab.tsx:66-79).
            "terminal.toggle" => {
                if let Route::Session { session_id } = store.route() {
                    store.terminal_toggle(&session_id);
                }
                Dispatch::Handled
            }
            "terminal.select" => {
                store.terminal_open_picker();
                Dispatch::Handled
            }
            "terminal.close" => {
                if let Route::Session { session_id } = store.route() {
                    store.terminal_close(&session_id);
                }
                Dispatch::Handled
            }
            "pane.focus.left" => {
                // focusSession (session-frame.tsx:150-157).
                store.set_terminal_focus_right(false);
                Dispatch::Handled
            }
            "pane.focus.right" => {
                // focusRightPane — only while a right pane exists.
                if let Route::Session { session_id } = store.route() {
                    if store.terminal_pane_mounted(&session_id).is_some() {
                        store.set_terminal_focus_right(true);
                    }
                }
                Dispatch::Handled
            }
            // The picker's terminals tab rows (terminals-tab.tsx:
            // 66-79): the wrap move + the select (a terminal or the
            // `+ New terminal` slot).
            "composer.terminal.up" => {
                store.picker_move(false);
                Dispatch::Handled
            }
            "composer.terminal.down" => {
                store.picker_move(true);
                Dispatch::Handled
            }
            "composer.terminal.select" => {
                store.picker_terminal_select();
                Dispatch::Handled
            }
            // TUI-33 — `session.verbosity.cycle` (index.tsx:1064-
            // 1076): cycle `low | medium | high` through the config
            // state (the dialog-config row's cycle — the live consumer
            // + the config-sink write-through), then `dialog.clear()`.
            "session.verbosity.cycle" => {
                store.setting_cycle("session.verbosity", 1);
                store.clear_dialogs();
                Dispatch::Handled
            }
            // TUI-29 — `app.console` (app.tsx:1110-1118):
            // `renderer.console.toggle()` + `dialog.clear()`.
            "app.console" => {
                store.console_toggle();
                store.clear_dialogs();
                Dispatch::Handled
            }
            // TUI-18 — `terminal.title.toggle` (app.tsx:1141-1159,
            // palette: undefined — the settings row carries the
            // surface): flip `terminal.title`; the DISABLED arm clears
            // the window name (the title computation returns "" and
            // the draw seam emits the empty OSC 0).
            "terminal.title.toggle" => {
                store.setting_cycle("terminal.title", 1);
                store.clear_dialogs();
                Dispatch::Handled
            }
            // RD-06 — `app.toggle.paste_summary` (app.tsx:1193-1205):
            // flip `prompt.paste` between "compact" (fold big pastes
            // into `[Pasted ~N lines]` chips) and "full" (paste
            // verbatim); persisted through the config sink.
            "app.toggle.paste_summary" => {
                store.toggle_paste_summary();
                store.clear_dialogs();
                Dispatch::Handled
            }
            // The registered-but-surfaceless families (v2 parity: the
            // keymap consumes the stroke; no component answers):
            // which-key panel (the keymap addon's surface), scrap/
            // debug panel, service restart, verbosity
            // model, plugin install flow, the integrations + plugin
            // dialog actions.
            "app.debug"
            | "app.scrap"
            | "service.restart"
            | "plugins.install"
            | "dialog.integration.rename"
            | "dialog.integration.delete"
            | "dialog.plugins.error"
            | "dialog.plugins.install"
            | "dialog.plugins.update"
            | "dialog.plugins.check"
            | "which-key.toggle"
            | "which-key.layout.toggle"
            | "which-key.pending.toggle"
            | "which-key.group.previous"
            | "which-key.group.next"
            | "which-key.scroll.up"
            | "which-key.scroll.down"
            | "which-key.page.up"
            | "which-key.page.down"
            | "which-key.home"
            | "which-key.end" => Dispatch::Handled,
            "app.exit" => {
                store.clear_dialogs();
                // The app shell exits the run loop (wave 3); the replay
                // corpus's exit path is binding-gated there.
                Dispatch::ServerPending("app.exit run-loop teardown")
            }
            // -- Wave A: the scroll driver (routes/session/index.tsx:763-887)
            "session.page.up"
            | "session.page.down"
            | "session.line.up"
            | "session.line.down"
            | "session.half.page.up"
            | "session.half.page.down"
            | "session.first"
            | "session.last" => {
                let Some(command) = ScrollCommand::parse(command) else {
                    return Dispatch::Unknown;
                };
                match store.route() {
                    Route::Session { session_id } => {
                        store.scroll_command(&session_id, command);
                        Dispatch::Handled
                    }
                    Route::Home => Dispatch::Handled,
                }
            }
            // -- TUI-TD02: message navigation (index.tsx:1101-1128 +
            // message-navigation.ts). The five registered-but-dead
            // commands now dispatch: next/previous (+ the user-only
            // pair) stage the boundary walk; alt+end jumps to the
            // last user message.
            "session.message.next" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_message(&session_id, crate::store::NavDirection::Next, false);
                }
                Dispatch::Handled
            }
            "session.message.previous" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_message(
                        &session_id,
                        crate::store::NavDirection::Previous,
                        false,
                    );
                }
                Dispatch::Handled
            }
            "session.message.user.next" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_message(&session_id, crate::store::NavDirection::Next, true);
                }
                Dispatch::Handled
            }
            "session.message.user.previous" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_message(&session_id, crate::store::NavDirection::Previous, true);
                }
                Dispatch::Handled
            }
            "session.messages_last_user" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_last_user(&session_id);
                }
                Dispatch::Handled
            }
            // -- TUI-TD03: the up-route (index.tsx:1272-1288): `up` in a
            // child session navigates to the parent (the registry's
            // `parentID` link; a root session no-ops).
            "session.parent" => {
                if let Route::Session { session_id } = store.route() {
                    store.navigate_parent(&session_id);
                }
                Dispatch::Handled
            }
            // Wave A: the 2-press escape interrupt
            // (prompt/index.tsx:500-529) — the sink POSTs
            // `session.interrupt { resume: true }`.
            "session.interrupt" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                match store.interrupt_pressed(&session_id, store.clock()) {
                    crate::store::InterruptStep::Ignored => Dispatch::Unknown,
                    crate::store::InterruptStep::Armed | crate::store::InterruptStep::Fired => {
                        Dispatch::Handled
                    }
                }
            }
            // Wave A: the cycles (app.tsx:848-944 + local.tsx's
            // move/cycle implementations).
            "agent.cycle" | "agent.cycle.reverse" => {
                let direction = if command == "agent.cycle" { 1 } else { -1 };
                if let Some(agent) = store.agent_cycle(direction) {
                    match store.route() {
                        Route::Session { session_id } => store.select_agent(&session_id, &agent),
                        Route::Home => store.stage_agent(crate::app::HOME_PROMPT_KEY, &agent),
                    }
                }
                Dispatch::Handled
            }
            "model.cycle_recent" | "model.cycle_recent_reverse" => {
                let direction = if command == "model.cycle_recent" { 1 } else { -1 };
                if let Some(model) = store.model_cycle_recent(direction) {
                    match store.route() {
                        Route::Session { session_id } => store.select_model(&session_id, &model),
                        Route::Home => store.stage_model(crate::app::HOME_PROMPT_KEY, model),
                    }
                }
                Dispatch::Handled
            }
            "model.cycle_favorite" | "model.cycle_favorite_reverse" => {
                let direction = if command == "model.cycle_favorite" { 1 } else { -1 };
                match store.model_cycle_favorite(direction) {
                    Some(model) => match store.route() {
                        Route::Session { session_id } => store.select_model(&session_id, &model),
                        Route::Home => store.stage_model(crate::app::HOME_PROMPT_KEY, model),
                    },
                    // local.tsx:427-431: no favorites configured.
                    None => {
                        store.toast_show(crate::components::toast::ToastOptions {
                            variant: "info",
                            message: "Add a favorite model to use this shortcut".to_owned(),
                            duration_ms: 3000,
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    }
                }
                Dispatch::Handled
            }
            "variant.cycle" => {
                // local.tsx:494-498: cycle through the named variants
                // (none → default).
                match store.variant_cycle() {
                    Some(next) => {
                        if let Route::Session { session_id } = store.route() {
                            let mut selection =
                                store.selection_for(&session_id).model.unwrap_or_default();
                            selection.variant = next;
                            store.select_model(&session_id, &selection);
                        } else {
                            let mut selection = store
                                .selection_for(crate::app::HOME_PROMPT_KEY)
                                .model
                                .unwrap_or_default();
                            selection.variant = next;
                            store.stage_model(crate::app::HOME_PROMPT_KEY, selection);
                        }
                    }
                    // app.tsx:926-931: "No variants available".
                    None => {
                        store.toast_show(crate::components::toast::ToastOptions {
                            title: Some("No variants available".to_owned()),
                            message: "The current model does not support any variants.".to_owned(),
                            variant: "info",
                            ..crate::components::toast::ToastOptions::info("")
                        });
                    }
                }
                Dispatch::Handled
            }
            // Wave A: the queued-prompts manager (`<leader>q`, v2
            // session route index.tsx:627-675 — `enabled: queuedPrompts()
            // .length > 0`, so an empty queue is a no-op).
            "session.queued_prompts" => {
                let Route::Session { session_id } = store.route() else {
                    return Dispatch::Handled;
                };
                if store.queued_prompts(&session_id).is_empty() {
                    return Dispatch::Handled;
                }
                store.replace_dialog(DialogKind::QueuedPrompts, DialogSize::Medium);
                Dispatch::Handled
            }
            // The plugin layer: anything the plugin host registered
            // dispatches through the store's sink (timeout-bounded on
            // the host side); unmatched falls through.
            other if store.execute_plugin_command(other, args) => Dispatch::Handled,
            _ => Dispatch::Unknown,
        }
    }
}

/// Registry keys are static: dispatch inputs resolve back to the
/// registry entry's own `&'static str` id.
fn static_command(command: &str) -> &'static str {
    palette_command(command).map_or("", |entry| entry.command)
}

/// Stage the CURRENT route's selection onto the home draft (v2
/// `session.new`'s `local.model.set(model)` + `local.agent.set(agent)`,
/// app.tsx:728-731 — the new tab inherits what was active).
fn inherit_selection_onto_home(store: &Store) {
    let key = match store.route() {
        Route::Session { session_id } => session_id,
        Route::Home => return, // home already carries the staged draft
    };
    let selection = store.selection_for(&key);
    if let Some(model) = selection.model {
        store.stage_model(crate::app::HOME_PROMPT_KEY, model);
    }
    if let Some(agent) = selection.agent {
        store.stage_agent(crate::app::HOME_PROMPT_KEY, &agent);
    }
}

/// TUI-TD07 — `appendPrompt` (prompt/history.tsx:33-58) over a queued
/// payload: the undo's composer refill — the text appends after a
/// blank-line separator, the file payloads stage as attachment chips
/// (`{...projectedPromptInput(payload), pasted: []}`,
/// index.tsx:662-668).
pub fn append_prompt_payload(store: &Store, session_id: &str, payload: &crate::store::InboxItem) {
    use crate::components::composer::attachment::prompt_attachment_label;
    use crate::components::composer::{AttachmentPayload, FileMention, PromptDraft};
    let mut composer = store.composer(session_id);
    let agents = std::mem::take(&mut composer.draft.agents);
    let mut files = std::mem::take(&mut composer.draft.files);
    let mut text = if composer.draft.text.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", composer.draft.text)
    };
    for uri in &payload.files {
        let Some(attachment) = AttachmentPayload::from_data_uri(uri) else { continue };
        let label = prompt_attachment_label(&files, &Some(attachment.clone()), None);
        if !text.is_empty() && !text.ends_with(' ') {
            text.push(' ');
        }
        let start = text.chars().count();
        text.push_str(&label);
        let end = start + label.chars().count();
        text.push(' ');
        files.push(FileMention {
            name: "file".to_owned(),
            start,
            end,
            payload: Some(attachment),
            mention: Some(label),
        });
    }
    text.push_str(&payload.text);
    // `{...projectedPromptInput(payload), pasted: []}` (index.tsx:
    // 662-668): the refill carries no folded pastes.
    composer.draft = PromptDraft { text, files, agents, skills: Vec::new(), pasted: Vec::new() };
    store.set_composer(session_id, &composer);
}

/// Execute a submitted palette/theme selection: the suggested-section
/// values carry a `suggested:` prefix to strip.
#[must_use]
pub fn command_of_value(value: &str) -> &str {
    value.strip_prefix("suggested:").unwrap_or(value)
}

/// Initialize a freshly-opened select dialog's state (focus `current`).
pub fn init_select_state(state: &mut SelectState, props: &SelectProps) {
    state.focus_current(props);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::store::{ListState, ServerProvider};

    #[test]
    fn registry_covers_the_replay_anchors() {
        assert_eq!(palette_command("session.new").unwrap().title, "New session");
        assert_eq!(palette_command("session.rename").unwrap().title, "Rename session");
        assert_eq!(palette_command("session.rename").unwrap().group, "Session");
        assert_eq!(palette_command("model.list").unwrap().bindings, &["ctrl+x m"][..]);
        // The v2 table leaves theme.switch unbound.
        assert_eq!(palette_command("theme.switch").unwrap().bindings.len(), 0);
    }

    /// L1 — the palette's suggested set carries "Connect an
    /// integration" while NO integration is connected
    /// (`suggested: !connected()`, app.tsx:946-948), between
    /// "Switch model" and "Open settings" (registration order); a
    /// connected integration drops it again.
    #[test]
    fn palette_suggests_connect_an_integration_only_while_unconnected() {
        let store = Store::new(Runtime::new());
        let props = palette_props(&store);
        let suggested: Vec<&str> = props
            .options
            .iter()
            .filter(|option| option.category.as_deref() == Some("Suggested"))
            .map(|option| option.title.as_str())
            .collect();
        assert_eq!(
            suggested,
            vec!["Switch model", "Connect an integration", "Open settings"],
            "the unconnected suggested trio"
        );

        store.set_integration_connected(true);
        let props = palette_props(&store);
        let suggested: Vec<&str> = props
            .options
            .iter()
            .filter(|option| option.category.as_deref() == Some("Suggested"))
            .map(|option| option.title.as_str())
            .collect();
        assert_eq!(suggested, vec!["Switch model", "Open settings"], "connected drops it");
    }

    /// L1 — the agent/variant listings mount their component titles
    /// ("Select agent" / "Select variant"), not the palette rows'.
    #[test]
    fn agent_and_variant_dialogs_mount_component_titles() {
        let store = Store::new(Runtime::new());
        let mut dispatcher = StoreDispatcher;
        assert_eq!(dispatcher.dispatch(&store, "agent.list", None), Dispatch::Handled);
        match &store.top_dialog().unwrap().kind {
            DialogKind::ServerList { command, title } => {
                assert_eq!(*command, "agent.list");
                assert_eq!(*title, "Select agent");
            }
            other => panic!("unexpected dialog {other:?}"),
        }
        // SURF-2: variant.list now mounts the REAL DialogVariant —
        // seeded with a variant-carrying model (the no-variants guard
        // toasts instead, dialog-variant.tsx:926-931).
        store.set_models(
            ListState::Loaded(vec![
                {
                    let mut entry = model("opencode", "grok", "Grok", false, false);
                    entry.variants = vec!["high".to_owned(), "low".to_owned()];
                    entry
                },
                model("opencode", "kimi", "Kimi", false, false),
            ]),
            Vec::new(),
        );
        store.stage_model(
            crate::app::HOME_PROMPT_KEY,
            ModelSelection {
                provider_id: "opencode".to_owned(),
                model_id: "grok".to_owned(),
                variant: None,
            },
        );
        assert_eq!(dispatcher.dispatch(&store, "variant.list", None), Dispatch::Handled);
        assert!(
            matches!(&store.top_dialog().unwrap().kind, DialogKind::Variant),
            "the variant dialog mounts: {:?}",
            store.top_dialog().unwrap().kind
        );
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let titles: Vec<&str> = props.options.iter().map(|option| option.title.as_str()).collect();
        assert_eq!(titles, vec!["Default", "high", "low"], "the variant rows");
        assert_eq!(props.current.as_deref(), Some("default"), "the current marker");

        // The no-variants guard: without variants the command toasts.
        store.clear_dialogs();
        store.stage_model(
            crate::app::HOME_PROMPT_KEY,
            ModelSelection {
                provider_id: "opencode".to_owned(),
                model_id: "kimi".to_owned(),
                variant: None,
            },
        );
        assert_eq!(dispatcher.dispatch(&store, "variant.list", None), Dispatch::Handled);
        assert!(store.dialogs().is_empty(), "no dialog on the guard path");
        let toast = store.toasts().current().cloned().expect("the guard toast");
        assert_eq!(toast.title.as_deref(), Some("No variants available"));
        assert_eq!(toast.message, "The current model does not support any variants.");
    }

    /// SURF-2: the six pure-TUI command wirings mount their REAL
    /// surfaces (auditE's traceability row — TUI wiring over working
    /// core): /mcps /status /variants /pair /debug /timeline.
    #[test]
    #[allow(clippy::too_many_lines)] // one command per block, the SURF-2 wiring proof
    fn six_tui_commands_mount_real_surfaces() {
        use crate::store::{McpServerRow, PairState, RecordingMcpToggleSink};
        let store = Store::new(Runtime::new());
        let toggle_sink = std::rc::Rc::new(RecordingMcpToggleSink::default());
        store.set_mcp_toggle_sink(toggle_sink.clone());
        let mut dispatcher = StoreDispatcher;

        // /mcps — the live rows with status footers (dialog-mcp.tsx).
        store.set_mcp_servers(vec![
            McpServerRow { name: "beta".into(), status: "connected", error: None },
            McpServerRow {
                name: "alpha".into(),
                status: "failed",
                error: Some("spawn failed".into()),
            },
        ]);
        assert_eq!(dispatcher.dispatch(&store, "mcp.list", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Mcp));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.options[0].value, "alpha", "sorted by name");
        assert_eq!(props.options[0].footer.as_deref(), Some("Failed !"));
        assert_eq!(props.options[1].footer.as_deref(), Some("Connected ✓"));
        // Submit on the failed row → the error details dialog.
        let details =
            crate::components::dialog::execute_submission(&store, &mut dispatcher, "alpha");
        assert_eq!(details, Dispatch::Handled);
        assert!(matches!(
            &store.top_dialog().unwrap().kind,
            DialogKind::McpError { name } if name == "alpha"
        ));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let text = props.options.iter().map(|o| o.title.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("spawn failed") && text.contains("Status: failed"));

        // /status — the count + bullet rows (dialog-status.tsx).
        assert_eq!(dispatcher.dispatch(&store, "opencode.status", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Status));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let rows: Vec<&str> = props.options.iter().map(|o| o.title.as_str()).collect();
        assert_eq!(rows[0], "2 MCP servers");
        assert!(rows[1].contains("• beta Connected"), "list order: {rows:?}");
        assert!(rows[2].contains("• alpha spawn failed"), "the failed error text: {rows:?}");
        store.clear_dialogs();

        // /pair — the CUSTOM dialog's rows (F15: dialog-pair.tsx is
        // not a DialogSelect — the word-wrapped content walk).
        assert_eq!(dispatcher.dispatch(&store, "server.pair", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Pair));
        let pair_rows = |store: &crate::store::Store| {
            crate::components::dialog_pair::rows(&store.pair_state(), 84)
        };
        assert!(pair_rows(&store).iter().any(|row| matches!(
            row,
            crate::components::dialog_pair::PairRow::Text { text, style: crate::components::dialog_pair::PairStyle::Muted }
                if text == "Loading server information…"
        )));
        store.set_pair_state(PairState::Loaded {
            links: vec!["http://127.0.0.1:4096/auth/connect/abc".to_owned()],
            localhost: Some("http://localhost:4096/auth/connect/abc".to_owned()),
            minutes: 5,
            loopback: true,
        });
        let joined = pair_rows(&store)
            .iter()
            .filter_map(|row| match row {
                crate::components::dialog_pair::PairRow::Gap
                | crate::components::dialog_pair::PairRow::Qr { .. } => None,
                crate::components::dialog_pair::PairRow::Text { text, .. } => Some(text.as_str()),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("expire in 5 minutes"), "{joined}");
        assert!(joined.contains("This device"));
        assert!(joined.contains("opencode service set hostname 0.0.0.0"));
        store.set_pair_state(PairState::Failed("connection refused".to_owned()));
        assert!(pair_rows(&store).iter().any(|row| matches!(
            row,
            crate::components::dialog_pair::PairRow::Text { text, style: crate::components::dialog_pair::PairStyle::ErrorBold }
                if text == "Could not load server information"
        )));
        store.clear_dialogs();

        // /debug — the env entries (dialog-debug.tsx:25-35).
        assert_eq!(dispatcher.dispatch(&store, "opencode.debug", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Debug));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let text = props.options.iter().map(|o| o.title.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Version"));
        assert!(text.contains("Session ID"));
        assert!(text.starts_with("Debug") || props.title == "Debug");
        store.clear_dialogs();

        // session.timeline — the user-message rows newest-first
        // (dialog-timeline.tsx:21-39).
        store.open_tab("s1", None);
        store.ingest_all([crate::data::Ingest::MessagesSnapshot {
            session_id: "s1".to_owned(),
            messages: vec![
                crate::components::message::model::Message::User {
                    id: "u1".to_owned(),
                    text: "first turn".to_owned(),
                    created: 1_000,
                    files: Vec::new(),
                },
                crate::components::message::model::Message::User {
                    id: "u2".to_owned(),
                    text: "second turn".to_owned(),
                    created: 3_600_000,
                    files: Vec::new(),
                },
            ],
        }]);
        assert_eq!(dispatcher.dispatch(&store, "session.timeline", None), Dispatch::Handled);
        assert!(matches!(
            &store.top_dialog().unwrap().kind,
            DialogKind::Timeline { session_id } if session_id == "s1"
        ));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.options[0].value, "u2", "newest first");
        assert_eq!(props.options[1].value, "u1");
        // Submit → the message actions over that message.
        let dispatch = crate::components::dialog::execute_submission(&store, &mut dispatcher, "u1");
        assert_eq!(dispatch, Dispatch::Handled);
        assert!(matches!(
            &store.top_dialog().unwrap().kind,
            DialogKind::MessageActions { message_id, .. } if message_id == "u1"
        ));
    }

    /// The MCP toggle action (`dialog.mcp.toggle` = space) rides the
    /// sink with the right direction (dialog-mcp.tsx:113-132).
    #[test]
    fn mcp_toggle_action_directions() {
        use crate::store::{McpServerRow, RecordingMcpToggleSink};
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingMcpToggleSink::default());
        store.set_mcp_toggle_sink(sink.clone());
        store.set_mcp_servers(vec![
            McpServerRow { name: "on".into(), status: "connected", error: None },
            McpServerRow { name: "off".into(), status: "failed", error: Some("x".into()) },
            McpServerRow { name: "wait".into(), status: "pending", error: None },
        ]);
        store.mcp_toggle("on");
        store.mcp_toggle("off");
        store.mcp_toggle("wait");
        let calls = sink.calls.borrow().clone();
        assert_eq!(calls, vec![("on".to_owned(), false), ("off".to_owned(), true)]);
    }
    /// (`focusCurrent={false}`, dialog-model.tsx:171): the current
    /// model only carries its `●` marker, never the selection.
    #[test]
    fn models_dialog_cursor_rests_on_row_zero() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.set_models(
            ListState::Loaded(vec![
                model("opencode", "grok", "Grok", false, false),
                model("anthropic", "claude", "Claude", false, false),
            ]),
            vec![ServerProvider { id: "anthropic".into(), name: "Anthropic".into() }],
        );
        store.set_integration_connected(true);
        let _ = store.effective_model("s1");
        store.stage_model(
            "s1",
            crate::store::ModelSelection {
                provider_id: "anthropic".into(),
                model_id: "claude".into(),
                variant: None,
            },
        );
        store.open_dialog(
            DialogKind::ServerList { command: "model.list", title: "Select model" },
            DialogSize::Medium,
        );
        let entry = store.top_dialog().unwrap();
        assert_eq!(entry.select.selected, 0, "cursor on the first row, not the current model");
    }

    #[test]
    fn palette_options_carry_both_footers_and_id_search() {
        let options = palette_options(PALETTE_COMMANDS);
        let new_session = options.iter().find(|o| o.value == "session.new").unwrap();
        assert_eq!(new_session.footer.as_deref(), Some("ctrl+x n"));
        assert_eq!(new_session.search_footer.as_deref(), Some("Session · ctrl+x n"));
        assert_eq!(new_session.search_text.as_deref(), Some("session.new"));
        assert_eq!(new_session.category.as_deref(), Some("Session"));
    }

    #[test]
    fn palette_props_prepend_the_suggested_section() {
        let store = Store::new(Runtime::new());
        let props = palette_props(&store);
        let first = props.options.first().unwrap();
        assert_eq!(first.category.as_deref(), Some("Suggested"));
        assert!(first.value.starts_with("suggested:"));
        assert_eq!(command_of_value(&first.value), "model.list");
        assert!(props.flat);
        assert_eq!(props.filter_threshold, Some(0.7));
    }

    /// RD-17 — searching the palette yields the per-setting rows
    /// (`settingOptions`, command-palette.tsx:36-45): "theme" surfaces
    /// the theme row with its keywords search text, the setting's
    /// category, and the `Settings · {category}` search footer.
    #[test]
    fn palette_search_yields_setting_rows() {
        let store = Store::new(Runtime::new());
        let props = palette_props_filtered(&store, "theme");
        let theme = props
            .options
            .iter()
            .find(|option| option.value == "setting:theme.name")
            .expect("the theme setting row");
        assert_eq!(theme.title, "Theme");
        assert_eq!(theme.category.as_deref(), Some("Appearance"));
        assert_eq!(theme.search_footer.as_deref(), Some("Settings · Appearance"));
        let search = theme.search_text.as_deref().unwrap_or_default();
        assert!(search.contains("color scheme"), "keywords ride the search text");
        // The fuzzy rank admits the row into the visible set.
        let indices = crate::components::dialog_select::filter_indices(&props, "theme");
        assert!(indices.iter().any(|&index| props.options[index].value == "setting:theme.name"));
    }

    /// RD-17 — ref `list()` (command-palette.tsx:49-60): the
    /// unfiltered palette shows the suggested section and NO setting
    /// rows; a filter swaps them — no suggested mirrors, settings in.
    #[test]
    fn palette_filter_swaps_suggested_for_setting_rows() {
        let store = Store::new(Runtime::new());
        let unfiltered = palette_props(&store);
        assert!(unfiltered.options.iter().any(|option| option.value.starts_with("suggested:")));
        assert!(!unfiltered.options.iter().any(|option| option.value.starts_with("setting:")));
        let filtered = palette_props_filtered(&store, "the");
        assert!(!filtered.options.iter().any(|option| option.value.starts_with("suggested:")));
        assert!(filtered.options.iter().any(|option| option.value.starts_with("setting:")));
    }

    /// RD-17 — activating a setting row replaces the palette with
    /// `DialogConfig` AT that setting (`onSelect` → `dialog.replace(
    /// () => <DialogConfig current={settingID(setting)} />`;
    /// `findIndex` clamped to 0 seeds the row selection).
    #[test]
    fn activating_a_setting_row_opens_config_at_that_setting() {
        use crate::components::dialog::execute_submission;
        let store = Store::new(Runtime::new());
        store.replace_dialog(DialogKind::CommandPalette, crate::store::DialogSize::Medium);
        let mut dispatcher = StoreDispatcher;
        execute_submission(&store, &mut dispatcher, "setting:session.sidebar");
        let entry = store.top_dialog().unwrap();
        assert!(matches!(entry.kind, DialogKind::Settings), "palette replaced by config");
        let expected = crate::components::dialog_config::settings_table()
            .into_iter()
            .position(|setting| setting.id() == "session.sidebar")
            .expect("the sidebar row exists");
        assert_eq!(entry.select.selected, expected, "selection rests on the setting row");
    }

    #[test]
    fn dispatcher_routes_local_actions_and_stubbed_server_calls() {
        let store = Store::new(Runtime::new());
        let mut dispatcher = StoreDispatcher;
        assert_eq!(dispatcher.dispatch(&store, "session.new", None), Dispatch::Handled);
        assert_eq!(store.route(), Route::Home);
        assert_eq!(dispatcher.dispatch(&store, "command.palette.show", None), Dispatch::Handled);
        assert_eq!(store.top_dialog().unwrap().kind, DialogKind::CommandPalette);
        assert_eq!(dispatcher.dispatch(&store, "theme.switch", None), Dispatch::Handled);
        assert_eq!(store.top_dialog().unwrap().kind, DialogKind::ThemeList);
        // Server-backed listings open the live-data surface — the
        // mounted DIALOG title is the component mount's
        // (`DialogModel` → "Select model", dialog-model.tsx:122),
        // NOT the palette row's "Switch model" (app.tsx:840).
        assert_eq!(dispatcher.dispatch(&store, "model.list", None), Dispatch::Handled);
        match &store.top_dialog().unwrap().kind {
            DialogKind::ServerList { command, title } => {
                assert_eq!(*command, "model.list");
                assert_eq!(*title, "Select model");
            }
            other => panic!("unexpected dialog {other:?}"),
        }
        assert_eq!(dispatcher.dispatch(&store, "nope.nope", None), Dispatch::Unknown);
    }

    /// SURF-2: the diff viewer's open → fetch → scroll → close cycle
    /// through the dispatcher (the `diff` plugin route).
    #[test]
    fn diff_viewer_opens_scrolls_and_closes() {
        use crate::store::{DiffPhase, RecordingDiffSink};
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingDiffSink::default());
        store.set_diff_sink(sink.clone());
        let mut dispatcher = StoreDispatcher;

        // `diff.open` from home: loading phase, no session (the turn
        // source unavailable), dialog cleared.
        assert_eq!(dispatcher.dispatch(&store, "diff.open", None), Dispatch::Handled);
        let live = store.diff_viewer();
        assert!(live.open, "mounted");
        assert_eq!(live.return_route, Route::Home);
        assert_eq!(live.mode, crate::store::DiffSource::Branch, "default source");
        assert!(matches!(live.phase, DiffPhase::Loading));
        assert_eq!(sink.calls.borrow().len(), 1, "fetch kicked");
        assert!(store.dialogs().is_empty(), "dialog cleared (diff-viewer.tsx:1225)");

        // The fetch lands: files + a resolved base.
        store.diff_fetch_result(
            live.generation,
            Ok((Some("main".to_owned()), crate::components::diff_viewer::diff_viewer_fixture())),
        );
        let live = store.diff_viewer();
        assert!(matches!(live.phase, DiffPhase::Loaded(_)), "loaded");
        assert_eq!(live.base.as_deref(), Some("main"));
        assert_eq!(live.source_detail(), "vs main");

        // The viewer preferences + scroll family.
        assert_eq!(dispatcher.dispatch(&store, "diff.toggle_file_tree", None), Dispatch::Handled);
        assert!(!store.diff_viewer().viewer.tree_enabled, "tree toggled off");
        assert_eq!(dispatcher.dispatch(&store, "diff.down", None), Dispatch::Handled);
        assert_eq!(store.diff_viewer().scroll, 1, "scrolled one line");
        assert_eq!(dispatcher.dispatch(&store, "diff.first", None), Dispatch::Handled);
        assert_eq!(store.diff_viewer().scroll, 0, "back to top");

        // The source dialog + switch.
        assert_eq!(dispatcher.dispatch(&store, "diff.switch_source", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::DiffSource));
        store.clear_dialogs();
        store.switch_diff_source(crate::store::DiffSource::Working);
        assert_eq!(store.diff_viewer().mode, crate::store::DiffSource::Working);
        assert_eq!(sink.calls.borrow().len(), 2, "switch refetches");
        assert!(matches!(store.diff_viewer().phase, DiffPhase::Loading));

        // Empty diff → the "No changes to show" body.
        let generation = store.diff_viewer().generation;
        store.diff_fetch_result(generation, Ok((None, Vec::new())));
        assert!(
            matches!(store.diff_viewer().phase, DiffPhase::Loaded(ref files) if files.is_empty())
        );

        // Close returns to the captured route.
        assert_eq!(dispatcher.dispatch(&store, "diff.close", None), Dispatch::Handled);
        assert!(!store.diff_viewer().open, "unmounted");
        assert_eq!(store.route(), Route::Home);
    }

    // -- F3.i: the server-backed pickers ---------------------------------

    fn model(
        provider: &str,
        id: &str,
        name: &str,
        free: bool,
        deprecated: bool,
    ) -> crate::store::ServerModel {
        crate::store::ServerModel {
            provider_id: provider.into(),
            id: id.into(),
            name: name.into(),
            cost_input: if free { 0.0 } else { 1.25 },
            free,
            deprecated,
            variants: Vec::new(),
            released: 0.0,
            context_limit: None,
        }
    }

    fn agent(id: &str, mode: &str, hidden: bool) -> crate::store::ServerAgent {
        crate::store::ServerAgent {
            id: id.into(),
            name: id.into(),
            description: Some("Does things".into()),
            mode: mode.into(),
            hidden,
        }
    }

    /// The session-list dialog lists the HISTORY (the resync's
    /// session.list), not the open tabs — and selecting a row
    /// navigates (the route effect opens the tab).
    #[test]
    fn session_list_dialog_rows_come_from_history_not_tabs() {
        use crate::data::{Ingest, SessionSnapshot};
        let store = Store::new(Runtime::new());
        store.open_tab("s_open", Some("Open tab".to_owned()));
        store.ingest_all([Ingest::Resync {
            sessions: vec![
                SessionSnapshot {
                    model: None,
                    id: "s_hist".into(),
                    title: Some("History row".into()),
                    parent_id: None,
                    time_idle: None,
                    viewed: None,
                    ..Default::default()
                },
                SessionSnapshot {
                    model: None,
                    id: "s_open".into(),
                    title: Some("Open tab".into()),
                    parent_id: None,
                    time_idle: None,
                    viewed: None,
                    ..Default::default()
                },
            ],
            unread: Vec::new(),
            messages: Vec::new(),
            cursors: Vec::new(),
            permissions: Vec::new(),
            forms: Vec::new(),
        }]);
        let props = select_props_for(
            &DialogKind::ServerList { command: "session.list", title: "Sessions" },
            &store,
        );
        let titles: Vec<&str> = props.options.iter().map(|option| option.title.as_str()).collect();
        assert_eq!(titles, vec!["History row", "Open tab"], "history rows, newest first");
        // Accepting a history row opens its tab through navigation.
        execute_server_list_selection(&store, "session.list", "s_hist");
        assert_eq!(store.route(), crate::store::Route::Session { session_id: "s_hist".to_owned() });
        assert!(store.tabs().tabs.iter().any(|tab| tab.session_id == "s_hist"));
    }

    /// The model picker renders loaded rows (Free footers,
    /// provider/model values), a muted loading row while the fetch is
    /// in flight, and the error text when it failed. While
    /// UNCONNECTED (the rig + fresh boots) the rows carry NO
    /// categories and the footer action reads "View all integrations"
    /// (dialog-model.tsx:77-97/168-176); connected flips to provider
    /// categories + "Connect an integration".
    #[test]
    fn model_list_props_cover_loaded_loading_and_error_states() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);

        // Loading: one muted status row.
        let props = model_list_props(&store, "Select model");
        assert_eq!(props.options.len(), 1);
        assert!(props.options[0].title.contains("Loading models"));

        // Loaded (UNCONNECTED default): deprecated filtered, Free
        // footer, no categories, the integrations footer action.
        store.set_models(
            ListState::Loaded(vec![
                model("anthropic", "claude", "Claude", true, false),
                model("anthropic", "old", "Old", false, true),
                model("custom", "local-1", "Local One", false, false),
            ]),
            vec![ServerProvider { id: "anthropic".into(), name: "Anthropic".into() }],
        );
        let props = model_list_props(&store, "Select model");
        assert_eq!(props.options.len(), 2, "deprecated row filtered");
        assert_eq!(props.options[0].title, "Claude");
        assert_eq!(props.options[0].value, "anthropic/claude");
        assert_eq!(props.options[0].category, None, "unconnected renders flat");
        assert_eq!(props.options[0].footer.as_deref(), Some("Free"));
        assert_eq!(props.options[1].footer, None, "paid model has no footer");
        assert_eq!(
            props.actions,
            vec![SelectAction::new("model.dialog.provider", "View all integrations", "ctrl+a")],
            "the unconnected footer action (dialog-model.tsx:168-176)"
        );

        // Connected: provider-name categories (group headers return)
        // and the "Connect an integration" wording.
        store.set_integration_connected(true);
        let props = model_list_props(&store, "Select model");
        assert_eq!(props.options[0].category.as_deref(), Some("Anthropic"));
        assert_eq!(props.options[1].category.as_deref(), Some("custom"), "no provider row");
        assert_eq!(
            props.actions,
            vec![SelectAction::new("model.dialog.provider", "Connect an integration", "ctrl+a")]
        );

        // Failed: the error surfaces in the row.
        store.set_models(ListState::Failed("503".into()), Vec::new());
        let props = model_list_props(&store, "Select model");
        assert!(props.options[0].title.contains("Models unavailable"));
        assert!(props.options[0].title.contains("503"));
    }

    /// D-05 — `sortModelOptions` ported verbatim from
    /// `dialog-model.tsx:185-211`. Vectors from the reference's
    /// model-options test (the SHIPPED comparator's behavior —
    /// verified against the ref source in both bun and node).
    #[test]
    fn sort_model_options_orders_opencode_go_before_zen_and_others() {
        let rows = [
            row("openai", "OpenAI", "GPT 5", false, 3.0),
            row("opencode", "OpenCode Zen", "Claude Sonnet 4", false, 1.0),
            row("anthropic", "Anthropic", "Claude Opus 4", false, 2.0),
            row("opencode-go", "OpenCode Go", "Kimi K3", false, 0.0),
        ];
        let titles = sorted_titles(&rows, true);
        assert_eq!(titles, vec!["Kimi K3", "Claude Sonnet 4", "Claude Opus 4", "GPT 5"]);
    }

    #[test]
    fn sort_model_options_grouped_puts_the_opencode_group_first() {
        // The ref's test-1 input under `grouped = true` (browse): the
        // opencode provider group wins before anthropic's rows — the
        // shipped comparator's output (the ref test file's expectation
        // here predates the provider-grouping keys).
        let rows = [
            row("opencode", "OpenCode Zen", "Claude Haiku 3", false, 1.0),
            row("anthropic", "Anthropic", "Claude Haiku 4.5", false, 2.0),
            row("anthropic", "Anthropic", "Claude Haiku Free", true, 0.0),
        ];
        let titles = sorted_titles(&rows, true);
        assert_eq!(titles, vec!["Claude Haiku 3", "Claude Haiku Free", "Claude Haiku 4.5"]);
        // Ungrouped (the search path): free-first, then release desc.
        let titles = sorted_titles(&rows, false);
        assert_eq!(titles, vec!["Claude Haiku Free", "Claude Haiku 4.5", "Claude Haiku 3"]);
    }

    #[test]
    fn sort_model_options_ungrouped_free_first_regardless_of_provider() {
        let rows = [
            row("opencode-go", "OpenCode Go", "Go model", false, 3.0),
            row("opencode", "OpenCode Zen", "Free Zen model", true, 1.0),
            row("anthropic", "Anthropic", "Anthropic model", false, 4.0),
        ];
        let titles = sorted_titles(&rows, false);
        assert_eq!(titles, vec!["Free Zen model", "Anthropic model", "Go model"]);
    }

    #[test]
    fn sort_model_options_groups_by_provider_name_then_newest_release() {
        let rows = [
            row("google", "Google", "Gemini 2.5 Pro", false, 5.0),
            row("anthropic", "Anthropic", "Claude Sonnet 4", false, 4.0),
            row("anthropic", "Anthropic", "Claude Opus 4", false, 6.0),
            row("openai", "OpenAI", "GPT 5", false, 7.0),
        ];
        let titles = sorted_titles(&rows, true);
        assert_eq!(
            titles,
            vec!["Claude Opus 4", "Claude Sonnet 4", "Gemini 2.5 Pro", "GPT 5"],
            "provider-name groups stay contiguous, newest first inside"
        );
    }

    #[test]
    fn sort_model_options_title_collation_breaks_release_ties() {
        let rows = [
            row("anthropic", "Anthropic", "Claude Sonnet 4", false, 5.0),
            row("anthropic", "Anthropic", "Claude Opus 4", false, 5.0),
        ];
        let titles = sorted_titles(&rows, true);
        assert_eq!(titles, vec!["Claude Opus 4", "Claude Sonnet 4"]);
    }

    /// Sort-input row helper: one `ServerModel` + its provider name.
    fn row(
        provider_id: &str,
        provider_name: &str,
        title: &str,
        free: bool,
        released: f64,
    ) -> (crate::store::ServerModel, String) {
        (
            crate::store::ServerModel {
                provider_id: provider_id.into(),
                id: title.to_lowercase().replace(' ', "-"),
                name: title.into(),
                cost_input: if free { 0.0 } else { 1.0 },
                free,
                deprecated: false,
                variants: Vec::new(),
                released,
                context_limit: None,
            },
            provider_name.into(),
        )
    }

    fn sorted_titles(rows: &[(crate::store::ServerModel, String)], grouped: bool) -> Vec<String> {
        let mut borrowed: Vec<&crate::store::ServerModel> =
            rows.iter().map(|(model, _)| model).collect();
        let names: Vec<(String, String)> =
            rows.iter().map(|(model, name)| (model.provider_id.clone(), name.clone())).collect();
        let resolve = move |id: &str| -> String {
            names
                .iter()
                .find(|(provider, _)| provider == id)
                .map_or_else(|| id.to_owned(), |(_, name)| name.clone())
        };
        sort_model_options(&mut borrowed, grouped, &resolve);
        borrowed.iter().map(|model| model.name.clone()).collect()
    }

    /// The agent picker filters subagent/hidden rows, carries the
    /// description, and marks the active session's selection current.
    #[test]
    fn agent_list_props_filter_and_mark_current() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.set_agents(ListState::Loaded(vec![
            agent("build", "all", false),
            agent("explore", "subagent", false),
            agent("secret", "all", true),
        ]));
        store.select_agent("s1", "build");
        // Re-point the sink log assertion: the default recording sink
        // captured the call.
        let props = agent_list_props(&store, "Switch agent");
        assert_eq!(props.options.len(), 1, "subagent + hidden filtered");
        assert_eq!(props.options[0].title, "build");
        assert_eq!(props.options[0].description.as_deref(), Some("Does things"));
        assert_eq!(props.current.as_deref(), Some("build"), "current from the selection");
    }

    /// Model/agent submissions apply through the selection sink on a
    /// session route and stage without a POST on home; unknown agents
    /// warn (v2's `local.agent.set` toast).
    #[test]
    fn server_list_submissions_apply_via_the_sink() {
        use crate::store::{RecordingSelectionSink, SelectionCall};
        let mut store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingSelectionSink::default());
        let sink_dyn: std::rc::Rc<dyn crate::store::SelectionSink> = sink.clone();
        store.set_selection_sink(sink_dyn);
        store.open_tab("s1", None);

        // Model on the session route: state + sink.
        assert_eq!(
            execute_server_list_selection(&store, "model.list", "anthropic/claude"),
            Dispatch::Handled
        );
        assert_eq!(
            sink.calls.borrow().as_slice(),
            &[(
                "s1".into(),
                SelectionCall::Model {
                    provider_id: "anthropic".into(),
                    model_id: "claude".into(),
                    variant: None
                }
            )]
        );
        let selection = store.selection_for("s1");
        assert_eq!(selection.model.as_ref().unwrap().model_id, "claude");

        // Agent on the session route: state + sink.
        store.set_agents(ListState::Loaded(vec![agent("review", "primary", false)]));
        assert_eq!(
            execute_server_list_selection(&store, "agent.list", "review"),
            Dispatch::Handled
        );
        assert_eq!(
            sink.calls.borrow().last(),
            Some(&("s1".into(), SelectionCall::Agent("review".into())))
        );

        // Home: staged only — no sink call.
        store.navigate(Route::Home);
        execute_server_list_selection(&store, "model.list", "opencode/grok");
        assert_eq!(sink.calls.borrow().len(), 2, "no third call from the home stage");
        assert_eq!(
            store.selection_for(crate::app::HOME_PROMPT_KEY).model.as_ref().unwrap().model_id,
            "grok"
        );

        // Unknown agent (list loaded, id absent): warning toast, no call.
        execute_server_list_selection(&store, "agent.list", "ghost");
        assert_eq!(sink.calls.borrow().len(), 2);
        let toasts = store.toasts();
        assert_eq!(
            toasts.current().map(|toast| toast.message.as_str()),
            Some("Agent not found: ghost")
        );
        assert_eq!(toasts.current().unwrap().variant, "warning");
    }

    /// `messages.copy`: the exact v2 toasts for every miss, the OSC-52
    /// write through the clipboard sink on hit.
    #[test]
    fn messages_copy_resolves_text_and_toasts() {
        use crate::components::message::model::{Message, PartTime};
        let mut store = Store::new(Runtime::new());
        let clipboard = std::rc::Rc::new(crate::clipboard::RecordingClipboardSink::default());
        let clipboard_dyn: std::rc::Rc<dyn crate::clipboard::ClipboardSink> = clipboard.clone();
        store.set_clipboard_sink(clipboard_dyn);
        store.open_tab("s1", None);

        // No assistant messages yet.
        copy_message_to_clipboard(&store);
        assert!(
            clipboard.writes.lock().unwrap().is_empty(),
            "nothing written without an assistant message"
        );
        assert_eq!(
            store.toasts().current().map(|toast| toast.message.as_str()),
            Some("No assistant messages found")
        );
        assert_eq!(store.toasts().current().unwrap().variant, "error");

        // An assistant message whose LAST turn has the text parts.
        store.set_messages(
            "s1",
            vec![
                Message::User {
                    id: "u1".into(),
                    text: "question".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".into(),
                    agent: "build".into(),
                    model: String::new(),
                    parts: vec![
                        crate::components::message::model::Part::Tool {
                            id: "t1".into(),
                            name: "read".into(),
                            time: PartTime::default(),
                            state: crate::components::message::model::ToolState::default(),
                        },
                        crate::components::message::model::Part::Text {
                            text: "first ".into(),
                            time: PartTime::default(),
                        },
                        crate::components::message::model::Part::Text {
                            text: "second".into(),
                            time: PartTime::default(),
                        },
                    ],
                    finish: None,
                    error: None,
                    time: PartTime::default(),
                    tokens: None,
                    usage_total: None,
                },
            ],
        );
        copy_message_to_clipboard(&store);
        assert_eq!(
            clipboard.writes.lock().unwrap().as_slice(),
            &["first \nsecond".to_owned()],
            "text parts joined \\n and trimmed"
        );
        assert_eq!(
            store.toasts().current().map(|toast| toast.message.as_str()),
            Some("Message copied to clipboard!")
        );
        assert_eq!(store.toasts().current().unwrap().variant, "success");
    }

    /// `<leader>e` requests the editor flow; `help.show` opens the Help
    /// dialog.
    #[test]
    fn editor_request_and_help_dialog_commands() {
        let store = Store::new(Runtime::new());
        let mut dispatcher = StoreDispatcher;
        assert!(!store.take_editor_request());
        assert_eq!(dispatcher.dispatch(&store, "prompt.editor", None), Dispatch::Handled);
        assert!(store.take_editor_request(), "the flag drained once");
        assert!(!store.take_editor_request(), "and exactly once");

        assert_eq!(dispatcher.dispatch(&store, "help.show", None), Dispatch::Handled);
        assert_eq!(store.top_dialog().unwrap().kind, DialogKind::Help);
    }

    // -- dogfood wave 1 -------------------------------------------------------

    /// F10 — `session.new` inherits the active session's selection (v2
    /// app.tsx:728-731 `local.model.set(model)` + the agent): the
    /// staged home draft carries the pick into the next submission.
    #[test]
    fn session_new_inherits_the_current_selection() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("S".to_owned()));
        store.select_model(
            "s1",
            &ModelSelection {
                provider_id: "parityfake".into(),
                model_id: "picked-model".into(),
                variant: None,
            },
        );
        store.select_agent("s1", "review");

        let mut dispatcher = StoreDispatcher;
        assert_eq!(dispatcher.dispatch(&store, "session.new", None), Dispatch::Handled);
        assert_eq!(store.route(), Route::Home, "session.new routed home");
        let staged = store.selection_for(crate::app::HOME_PROMPT_KEY);
        assert_eq!(
            staged.model.map(|model| model.model_id),
            Some("picked-model".to_owned()),
            "the model staged onto the home draft"
        );
        assert_eq!(staged.agent.as_deref(), Some("review"), "the agent staged too");

        // Home's own stage (no session route) is untouched by the walk.
        assert_eq!(dispatcher.dispatch(&store, "session.new", None), Dispatch::Handled);
        assert_eq!(
            store.selection_for(crate::app::HOME_PROMPT_KEY).model.map(|m| m.model_id),
            Some("picked-model".to_owned()),
            "a home-route session.new keeps the existing stage"
        );
    }

    /// F13 — opening the session-list dialog fires a `session.list`
    /// refetch through the sink (the boot resync snapshot goes stale).
    #[test]
    fn session_list_dialog_open_fires_the_refresh_sink() {
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(crate::store::RecordingSessionListSink::default());
        store.set_session_list_sink(
            std::rc::Rc::clone(&sink) as std::rc::Rc<dyn crate::store::SessionListSink>
        );
        let mut dispatcher = StoreDispatcher;
        assert_eq!(dispatcher.dispatch(&store, "session.list", None), Dispatch::Handled);
        assert_eq!(sink.calls.get(), 1, "the dialog open refreshed the list");
        // Other listings do not fire it.
        assert_eq!(dispatcher.dispatch(&store, "model.list", None), Dispatch::Handled);
        assert_eq!(sink.calls.get(), 1);
    }

    /// F14 — a plugin command claiming a builtin id renders ONE row
    /// (the registry's own); the suggested-section mirrors stay.
    #[test]
    fn palette_dedups_plugin_entries_claiming_builtin_ids() {
        let store = Store::new(Runtime::new());
        store.set_plugin_commands(vec![crate::store::PluginCommandEntry {
            command: "help.show".to_owned(),
            title: "Help again".to_owned(),
            plugin_id: "omos".to_owned(),
        }]);
        let props = palette_props(&store);
        let values: Vec<&str> = props.options.iter().map(|option| option.value.as_str()).collect();
        let duplicates = values.iter().filter(|value| **value == "help.show").count();
        assert_eq!(duplicates, 1, "one dispatchable help.show row: {values:?}");
        // The suggested mirror (prefixed) survives by design.
        assert!(
            values.iter().any(|value| value.starts_with("suggested:")),
            "v2's suggested section keeps its prefixed copies"
        );
    }

    // -- Lane FINAL: the stub-dispatch family + the six session
    // commands ----------------------------------------------------------

    fn final_session_store() -> Store {
        let store = Store::new(crate::reactive::Runtime::new());
        store.open_tab("ses_1", Some("Session one".to_owned()));
        store.navigate(Route::Session { session_id: "ses_1".to_owned() });
        store.set_messages(
            "ses_1",
            vec![
                crate::components::message::model::Message::User {
                    id: "u1".into(),
                    text: "first question".into(),
                    created: 1_000,
                    files: Vec::new(),
                },
                crate::components::message::model::Message::User {
                    id: "u2".into(),
                    text: "second question".into(),
                    created: 2_000,
                    files: Vec::new(),
                },
            ],
        );
        store
    }

    /// `provider.connect` mounts the real integration list over the
    /// BE-3 host (the list fetch fires through the sink) — never the
    /// old "Not wired in this build" stub.
    #[test]
    fn final_provider_connect_mounts_integration_list() {
        use crate::store::RecordingIntegrationSink;
        let store = Store::new(crate::reactive::Runtime::new());
        let sink = std::rc::Rc::new(RecordingIntegrationSink::default());
        store.set_integration_sink(sink.clone());
        let mut dispatcher = StoreDispatcher;
        assert_eq!(dispatcher.dispatch(&store, "provider.connect", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Integration));
        assert_eq!(
            sink.calls.borrow().as_slice(),
            &[crate::store::IntegrationCall::List],
            "the open fired integration.list"
        );
        // The list dialog renders the rows (v2's sort + categories +
        // the connected footer).
        store.integration_list_loaded(vec![crate::store::IntegrationRow {
            id: "anthropic".into(),
            name: "Anthropic".into(),
            category: "Popular",
            methods: vec![crate::store::IntegrationMethod {
                id: "key".into(),
                kind: "key",
                label: "API key".into(),
            }],
            accounts: Vec::new(),
            summary: String::new(),
            sign_in_required: false,
        }]);
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.title, "Connect an integration");
        assert_eq!(props.options[0].title, "Anthropic");
        assert_eq!(props.options[0].category.as_deref(), Some("Popular"));

        // The connect chain: row → (single method) → the key prompt →
        // the sink POST.
        let submit =
            crate::components::dialog::execute_submission(&store, &mut dispatcher, "anthropic");
        assert_eq!(submit, Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::IntegrationKey { .. }));
        store.integration_key_submit("anthropic", "sk-test");
        assert_eq!(
            sink.calls.borrow()[1],
            crate::store::IntegrationCall::ConnectKey {
                integration_id: "anthropic".into(),
                key: "sk-test".into()
            }
        );
        // The connected event: toast + clear + refetch.
        store.integration_connected("Anthropic");
        assert!(store.dialogs().is_empty(), "the flow closed on connect");
        assert_eq!(sink.calls.borrow().last(), Some(&crate::store::IntegrationCall::List));
        assert!(store.toasts().current().is_some_and(|toast| toast.message.contains("Connected")));
    }

    /// The OAuth code path: `oauth.connect` (code mode) mounts the
    /// code-entry prompt with the attempt's URL; the submit fires
    /// `oauth.complete`.
    #[test]
    fn final_oauth_code_entry_flow() {
        use crate::store::{IntegrationAttempt, RecordingIntegrationSink};
        let store = Store::new(crate::reactive::Runtime::new());
        let sink = std::rc::Rc::new(RecordingIntegrationSink::default());
        store.set_integration_sink(sink.clone());
        store.integration_list_loaded(vec![crate::store::IntegrationRow {
            id: "github".into(),
            name: "GitHub".into(),
            category: "Popular",
            methods: vec![crate::store::IntegrationMethod {
                id: "oauth-browser".into(),
                kind: "oauth",
                label: "Browser".into(),
            }],
            accounts: Vec::new(),
            summary: String::new(),
            sign_in_required: false,
        }]);
        let mut dispatcher = StoreDispatcher;
        dispatcher.dispatch(&store, "provider.connect", None);
        store.integration_list_loaded(store_rows(&store));
        crate::components::dialog::execute_submission(&store, &mut dispatcher, "github");
        store.integration_oauth_started(IntegrationAttempt {
            integration_id: "github".into(),
            attempt_id: "att_1".into(),
            kind: "oauth",
            code_mode: true,
            url: "https://github.com/login/device".into(),
            instructions: "Enter code ABCD-1234".into(),
            title: "Browser".into(),
        });
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::IntegrationCode { .. }));
        // The prompt shows the URL (v2's description block).
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert!(props.hints.iter().any(|hint| hint.contains("github.com/login/device")));
        store.integration_code_submit("github", "att_1", "ABCD-1234");
        assert_eq!(
            sink.calls.borrow()[1],
            crate::store::IntegrationCall::OauthComplete {
                integration_id: "github".into(),
                attempt_id: "att_1".into(),
                code: "ABCD-1234".into()
            }
        );
    }

    fn store_rows(store: &Store) -> Vec<crate::store::IntegrationRow> {
        match store.integration_list() {
            crate::store::IntegrationListState::Loaded(rows) => rows,
            _ => Vec::new(),
        }
    }

    /// The other six stub dispatches resolve to their real v2
    /// surfaces (or v2's own mirrored behavior).
    #[test]
    fn final_six_stub_dispatches_resolve() {
        use crate::store::{RecordingRevertSink, RecordingSessionActionSink};
        let store = final_session_store();
        let actions = std::rc::Rc::new(RecordingSessionActionSink::default());
        store.set_session_action_sink(actions.clone());
        store.set_revert_sink(std::rc::Rc::new(RecordingRevertSink::default()));
        store.set_open_url_sink(Some(std::rc::Rc::new(|_url: String| {})));
        let mut dispatcher = StoreDispatcher;

        // open.menu → DialogOpen (TUI-5, lane-tui2): the Sessions
        // recents + Projects sections select at the large size
        // (dialog-open.tsx:338's `size: "large"`).
        dispatcher.dispatch(&store, "open.menu", None);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Open));
        assert_eq!(store.top_dialog().unwrap().size, crate::store::DialogSize::Large);
        store.clear_dialogs();

        // opencode.settings → the FULL Settings table (TUI-23 — every
        // v2.0.24 row, order verbatim).
        dispatcher.dispatch(&store, "opencode.settings", None);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Settings));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let ids: Vec<&str> = props.options.iter().map(|option| option.value.as_str()).collect();
        assert_eq!(ids.len(), crate::components::dialog_config::settings_table().len());
        for expected in
            ["theme.name", "theme.mode", "session.sidebar", "session.grouping", "debug.devtools"]
        {
            assert!(ids.contains(&expected), "the {expected} row is present");
        }
        store.clear_dialogs();

        // session.share mirrors v2's unavailable toast.
        dispatcher.dispatch(&store, "session.share", None);
        let toast = store.toasts().current().cloned().expect("the share toast");
        assert_eq!(toast.message, "Sharing is not implemented for V2 sessions yet");
        store.clear_dialogs();

        // location.reload → the info toast + the POST.
        dispatcher.dispatch(&store, "location.reload", None);
        assert!(store
            .toasts()
            .current()
            .is_some_and(|toast| toast.message == "Reloading configuration…"));
        assert_eq!(
            actions.calls.borrow().as_slice(),
            &[crate::store::SessionActionCall::LocationReload]
        );
        store.clear_dialogs();

        // session.fork → the fork picker ("Full session" + the user
        // messages newest-first).
        dispatcher.dispatch(&store, "session.fork", None);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::ForkPicker { .. }));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.options[0].title, "Full session");
        assert_eq!(props.options[1].title, "second question");
        assert_eq!(props.options[2].title, "first question");
        // Submitting a row fires the fork POST with `before`.
        let submit = crate::components::dialog::execute_submission(
            &store,
            &mut dispatcher,
            props.options[1].value.as_str(),
        );
        assert_eq!(submit, Dispatch::Handled);
        assert_eq!(
            actions.calls.borrow()[1],
            crate::store::SessionActionCall::Fork {
                session_id: "ses_1".into(),
                before: Some("u2".into())
            }
        );

        // docs.open → the URL open path.
        let opened = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let opened_url = opened.clone();
        store.set_open_url_sink(Some(std::rc::Rc::new(move |url: String| {
            *opened_url.borrow_mut() = url;
        })));
        dispatcher.dispatch(&store, "docs.open", None);
        assert_eq!(*opened.borrow(), "https://opencode.ai/docs");
    }

    /// The six dead session commands act through the sink.
    #[test]
    fn final_six_session_commands_act() {
        use crate::store::{ModelSelection, RecordingRevertSink, RecordingSessionActionSink};
        let store = final_session_store();
        let actions = std::rc::Rc::new(RecordingSessionActionSink::default());
        store.set_session_action_sink(actions.clone());
        let reverts = std::rc::Rc::new(RecordingRevertSink::default());
        store.set_revert_sink(reverts.clone());
        let mut dispatcher = StoreDispatcher;

        // compact → the POST with the current model.
        store.select_model(
            "ses_1",
            &ModelSelection {
                provider_id: "anthropic".into(),
                model_id: "claude".into(),
                variant: None,
            },
        );
        dispatcher.dispatch(&store, "session.compact", None);
        assert_eq!(
            actions.calls.borrow()[0],
            crate::store::SessionActionCall::Compact {
                session_id: "ses_1".into(),
                model: Some(ModelSelection {
                    provider_id: "anthropic".into(),
                    model_id: "claude".into(),
                    variant: None
                })
            }
        );

        // undo → revert.stage on the LAST user message + the composer
        // seed.
        dispatcher.dispatch(&store, "session.undo", None);
        assert_eq!(
            actions.calls.borrow()[1],
            crate::store::SessionActionCall::RevertStage {
                session_id: "ses_1".into(),
                message_id: "u2".into()
            }
        );
        assert_eq!(store.composer("ses_1").draft.text, "second question");

        // redo → the staged revert clears through the revert sink
        // (the SSE staged event seeds the state first).
        store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "session.revert.staged",
            serde_json::json!({
                "sessionID": "ses_1",
                "revert": { "messageID": "u1", "files": [] }
            }),
        ))]);
        dispatcher.dispatch(&store, "session.redo", None);
        assert_eq!(reverts.calls.borrow().as_slice(), &["ses_1".to_owned()]);

        // background → the POST.
        dispatcher.dispatch(&store, "session.background", None);
        assert_eq!(
            actions.calls.borrow().last(),
            Some(&crate::store::SessionActionCall::Background("ses_1".into()))
        );

        // export → the destination dialog → the POST.
        dispatcher.dispatch(&store, "session.export", None);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::SessionExport { .. }));
        let submit =
            crate::components::dialog::execute_submission(&store, &mut dispatcher, "markdown:file");
        assert_eq!(submit, Dispatch::Handled);
        assert_eq!(
            actions.calls.borrow().last(),
            Some(&crate::store::SessionActionCall::Export {
                session_id: "ses_1".into(),
                sanitize: false,
                format: "markdown".into(),
                destination: "file".into()
            })
        );

        // delete: NOT a palette row (v2 lists it only as the sessions
        // dialog's ctrl+d action); the two-press fires session.remove.
        assert!(palette_props(&store)
            .options
            .iter()
            .all(|option| option.value != "session.delete"));
        store.session_delete_pressed("ses_1");
        assert!(actions
            .calls
            .borrow()
            .iter()
            .all(|call| !matches!(call, crate::store::SessionActionCall::Remove(_))));
        store.session_delete_pressed("ses_1");
        assert_eq!(
            actions.calls.borrow().last(),
            Some(&crate::store::SessionActionCall::Remove("ses_1".into()))
        );
    }

    /// `session.redo`'s palette row delists exactly when v2's
    /// `enabled` gate is off (no staged revert on the route).
    #[test]
    fn final_session_redo_delists_without_revert() {
        let store = final_session_store();
        let props = palette_props(&store);
        let advertised: Vec<&str> =
            props.options.iter().map(|option| option.value.as_str()).collect();
        assert!(!advertised.contains(&"session.redo"), "no revert staged — delisted");
        store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "session.revert.staged",
            serde_json::json!({
                "sessionID": "ses_1",
                "revert": { "messageID": "u1", "files": [] }
            }),
        ))]);
        let props = palette_props(&store);
        let advertised: Vec<&str> =
            props.options.iter().map(|option| option.value.as_str()).collect();
        assert!(advertised.contains(&"session.redo"), "staged revert — listed");
        // The export row is palette-visible (index.tsx:1201-1207).
        assert!(advertised.contains(&"session.export"));
    }

    /// The Settings dialog's cycles write through the config sink
    /// (should-fix #5's grouping half).
    #[test]
    fn final_settings_cycles_persist() {
        use crate::store::RecordingConfigSink;
        let store = final_session_store();
        let config = std::rc::Rc::new(RecordingConfigSink::default());
        store.set_config_sink(config.clone());
        // The store default groups (v2 `session.grouping: "auto"`);
        // one cycle flips to none, the next back to auto.
        store.setting_cycle("session.grouping", 1);
        assert_eq!(config.calls.borrow().as_slice(), &[("session.grouping", "none".to_owned())]);
        store.setting_cycle("session.grouping", 1);
        assert_eq!(
            config.calls.borrow().as_slice(),
            &[("session.grouping", "none".to_owned()), ("session.grouping", "auto".to_owned())]
        );
    }

    /// RD-06 — `app.toggle.paste_summary` (app.tsx:1193-1205): the
    /// command flips `prompt.paste` compact↔full through the store
    /// (the fold gate) and persists the value via the config sink.
    #[test]
    fn toggle_paste_summary_flips_and_persists() {
        use crate::store::ConfigSink;
        #[derive(Default)]
        struct PasteSink(std::cell::RefCell<Vec<String>>);
        impl ConfigSink for PasteSink {
            fn update_theme_name(&self, _: &str) {}
            fn update_theme_mode(&self, _: &str) {}
            fn update_session_sidebar(&self, _: bool) {}
            fn update_session_grouping(&self, _: bool) {}
            fn update_setting(&self, path: &str, value: &str) {
                assert_eq!(path, "prompt.paste");
                self.0.borrow_mut().push(value.to_owned());
            }
        }
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(PasteSink::default());
        store.set_config_sink(sink.clone());
        let mut dispatcher = StoreDispatcher;
        // Default armed (absent config = `!== "full"`).
        assert!(store.prompt_paste_compact());
        assert_eq!(
            dispatcher.dispatch(&store, "app.toggle.paste_summary", None),
            Dispatch::Handled
        );
        assert!(!store.prompt_paste_compact(), "disabled → paste \"full\"");
        assert_eq!(store.settings_snapshot_value("prompt.paste"), "full");
        assert_eq!(
            dispatcher.dispatch(&store, "app.toggle.paste_summary", None),
            Dispatch::Handled
        );
        assert!(store.prompt_paste_compact(), "re-enabled → paste \"compact\"");
        assert_eq!(store.settings_snapshot_value("prompt.paste"), "compact");
        assert_eq!(sink.0.borrow().as_slice(), ["full".to_owned(), "compact".to_owned()]);
    }

    /// SA-F6: the Plugins dialog renders the ref's live shape — the
    /// shared "No items available" empty state (ui/dialog-select.tsx
    /// :723) and the `ctrl+a show internal` footerHints pair riding
    /// the footer bar (plugins.tsx:199 + dialog-select.tsx:170-173).
    #[test]
    fn plugins_dialog_renders_the_reference_shape() {
        let theme = crate::theme::ThemeState::v2_dark();
        let store = final_session_store();
        store.replace_dialog(crate::store::DialogKind::Plugins, crate::store::DialogSize::Medium);
        let props = select_props_for(&crate::store::DialogKind::Plugins, &store);
        assert_eq!(props.title, "Plugins");
        assert!(
            props.options.is_empty(),
            "no registered plugin ids → the shared empty state renders"
        );
        assert_eq!(props.empty_text, None, "no override — the shared placeholder shows");
        assert!(
            props
                .actions
                .iter()
                .any(|action| action.title == "ctrl+a" && action.label == "show internal"),
            "the footer hint pair: {:?}",
            props.actions
        );
        // The rendered overlay carries the rows (Search placeholder,
        // the empty state, the footer hint).
        let cell = crate::render::Cell {
            ch: ' ',
            attrs: 0,
            fg: crate::render::CellColor::Rgb(255, 255, 255),
            bg: crate::render::CellColor::Rgb(10, 10, 10),
        };
        let mut frame =
            crate::render::Frame { cells: vec![vec![cell; 80]; 24], width: 80, height: 24 };
        crate::components::dialog::render_overlay_with(
            &mut frame,
            &store.top_dialog().expect("open"),
            &props,
            None,
            &theme,
            (80, 24),
        );
        let rows = frame.text_grid();
        let text = rows.concat();
        assert!(text.contains("Plugins"), "the title");
        assert!(text.contains("Search"), "the filter placeholder");
        assert!(text.contains("No items available"), "the shared empty state");
        assert!(text.contains("ctrl+a show internal"), "the footer hint");
    }

    /// RE-02: the plugins dialog models per-plugin active state —
    /// rows default ACTIVE (no footer), `plugin_toggle` disables
    /// (footer "inactive", the palette drops the plugin's command
    /// rows) and the enable→disable→enable round trip rides the
    /// toggle sink (v2's TUI-local activate/deactivate,
    /// plugins.tsx:116-138 — the active map is the persistence
    /// layer).
    #[test]
    fn plugins_dialog_active_state_toggle_round_trip() {
        use crate::store::{PluginCommandEntry, RecordingPluginToggleSink};
        let store = final_session_store();
        store.set_plugin_commands(vec![
            PluginCommandEntry {
                command: "hw.hello".into(),
                title: "Say hello".into(),
                plugin_id: "helloworld".into(),
            },
            PluginCommandEntry {
                command: "om.about".into(),
                title: "About".into(),
                plugin_id: "opencode.about".into(),
            },
        ]);
        let sink = std::rc::Rc::new(RecordingPluginToggleSink::default());
        store.set_plugin_toggle_sink(sink.clone());
        store.replace_dialog(crate::store::DialogKind::Plugins, crate::store::DialogSize::Medium);
        let props = select_props_for(&crate::store::DialogKind::Plugins, &store);
        assert_eq!(props.title, "Plugins");
        assert_eq!(props.options.len(), 2, "one row per registered plugin id");
        assert!(props.options.iter().all(|option| option.footer.is_none()), "active → no footer");
        assert!(
            props.actions.iter().any(|action| action.command == "plugins.toggle"),
            "the toggle action rides the footer: {:?}",
            props.actions
        );

        // Default active for every registered id.
        assert!(store.plugin_active("helloworld"));
        assert!(store.plugin_active("opencode.about"));
        // Both plugins' commands ride the palette while active.
        assert_eq!(
            palette_props(&store).options.iter().filter(|o| o.value == "hw.hello").count(),
            1
        );

        // disable → the row carries the "inactive" footer (ref
        // footer(), plugins.tsx:308-318) and the palette drops the
        // plugin's command row (clearContributions).
        assert!(!store.plugin_toggle("helloworld"), "toggle returns the NEW state");
        assert!(!store.plugin_active("helloworld"));
        assert!(store.plugin_active("opencode.about"));
        let props = select_props_for(&crate::store::DialogKind::Plugins, &store);
        let row =
            props.options.iter().find(|option| option.value == "helloworld").expect("row stays");
        assert_eq!(row.footer.as_deref(), Some("inactive"));
        assert_eq!(
            palette_props(&store).options.iter().filter(|o| o.value == "hw.hello").count(),
            0,
            "inactive plugin's palette rows drop"
        );
        assert!(
            store.plugin_command("hw.hello").is_none(),
            "inactive plugin's commands no longer dispatch"
        );

        // re-enable → active again, round trip recorded on the sink.
        assert!(store.plugin_toggle("helloworld"));
        assert!(store.plugin_active("helloworld"));
        let props = select_props_for(&crate::store::DialogKind::Plugins, &store);
        assert!(props
            .options
            .iter()
            .find(|option| option.value == "helloworld")
            .expect("row")
            .footer
            .is_none());
        assert_eq!(
            sink.calls.borrow().as_slice(),
            [("helloworld".to_owned(), false), ("helloworld".to_owned(), true)],
            "enable → disable → enable round trip on the toggle sink"
        );
    }

    // -- RE-13: /btw (session.aside) --------------------------------
    /// The dispatch is no longer swallowed (the surfaceless bucket
    /// is gone): the session route + argument runs the one-shot
    /// generate through the session-action sink, the bare command
    /// opens the question prompt, and the home route warns.
    #[test]
    fn session_aside_dispatch_runs_generate_prompts_and_warns_off_session() {
        use crate::store::{RecordingSessionActionSink, SessionActionCall};
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingSessionActionSink::default());
        store.set_session_action_sink(sink.clone());
        let mut dispatcher = StoreDispatcher;

        // Home route: v2's "Open a session first" warning toast
        // (btw.tsx:60-62) — nothing runs.
        assert_eq!(dispatcher.dispatch(&store, "session.aside", None), Dispatch::Handled);
        assert!(sink.calls.borrow().is_empty(), "no generate off a session route");
        let toast = store.toasts().current().cloned().expect("the warning toast");
        assert_eq!(toast.message, "Open a session first");
        assert_eq!(toast.variant, "warning");
        store.toast_dismiss();

        // Session route, NO argument: the fallback question prompt
        // (btw.tsx:64-65 — ctx.ui.dialog.prompt's title/placeholder).
        store.open_tab("s1", Some("S1".to_owned()));
        assert!(matches!(store.route(), Route::Session { .. }));
        assert_eq!(dispatcher.dispatch(&store, "session.aside", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::BtwPrompt));
        assert!(sink.calls.borrow().is_empty(), "the prompt defers the generate");
        store.clear_dialogs();

        // Session route WITH the argument: the sink's `aside` call
        // carries the instructions-joined prompt (btw.tsx:17-21, 69).
        assert_eq!(
            dispatcher.dispatch(&store, "session.aside", Some("what did we do?")),
            Dispatch::Handled
        );
        assert_eq!(store.btw_pending(), 1, "the footer spinner arms");
        let calls = sink.calls.borrow().clone();
        match calls.as_slice() {
            [SessionActionCall::Aside { session_id, question, prompt }] => {
                assert_eq!(session_id, "s1");
                assert_eq!(question, "what did we do?");
                assert!(
                    prompt.starts_with("The user is asking a quick side question"),
                    "the fixed instructions prelude rides the prompt"
                );
                assert!(prompt.ends_with("what did we do?"));
            }
            other => panic!("unexpected sink calls {other:?}"),
        }

        // The result lands as the Answer dialog (btw.tsx:70-75).
        store.btw_answer("what did we do?", "you ported /btw");
        assert_eq!(store.btw_pending(), 0, "the spinner drops");
        match &store.top_dialog().unwrap().kind {
            DialogKind::BtwAnswer { question, answer } => {
                assert_eq!(question, "what did we do?");
                assert_eq!(answer, "you ported /btw");
            }
            other => panic!("unexpected dialog {other:?}"),
        }
    }
}

    // -- RE-14: /stats (stats.open) ---------------------------------
    /// The command is registered on every surface (stats.tsx:194-
    /// 211): the palette's System group, the slash table's `/stats`,
    /// and the keymap's `stats.open` row (base, unbound) + the
    /// page's own escape layer.
    #[test]
    fn stats_command_is_registered_on_every_surface() {
        let row = PALETTE_COMMANDS
            .iter()
            .find(|command| command.command == "stats.open")
            .expect("the palette row");
        assert_eq!(row.title, "Usage statistics");
        assert_eq!(row.group, "System");
        assert!(crate::components::composer::autocomplete::SLASH_COMMANDS
            .contains(&("stats", "stats.open")));
        let table = crate::keymap::default_table();
        let open = table.iter().find(|def| def.command == "stats.open").expect("stats.open def");
        assert_eq!(open.layer, "base");
        assert_eq!(open.defaults, "none");
        let close = table.iter().find(|def| def.command == "stats.close").expect("stats.close def");
        assert_eq!(close.layer, "stats");
        assert!(close.defaults.contains("escape"));
    }

    /// `stats.open`'s dispatch (stats.tsx:196-209): mount the page
    /// (loading), kick the stats fetch with the local year range,
    /// ignore a re-run while open; `stats.close` returns to the
    /// previous route; the resource landing paints the metrics;
    /// stale generations drop.
    #[test]
    fn stats_open_dispatch_mounts_page_and_fetches() {
        use crate::reactive::Runtime;
        use crate::store::{RecordingStatsSink, StatsPhase};
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingStatsSink::default());
        store.set_stats_sink(sink.clone());
        let mut dispatcher = StoreDispatcher;

        // Off a session route: the page mounts over it and the fetch
        // carries the year range (from = local Jan 1, tools none).
        store.open_tab("s1", Some("S1".to_owned()));
        assert!(matches!(store.route(), Route::Session { .. }));
        assert_eq!(dispatcher.dispatch(&store, "stats.open", None), Dispatch::Handled);
        assert!(store.stats_page_open());
        assert!(matches!(store.stats_page_state().phase, StatsPhase::Loading));
        let calls = sink.calls.borrow().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, 1, "the first generation");
        assert_eq!(
            calls[0].1,
            crate::components::stats_page::year_start_ms(crate::components::stats_page::now_ms()),
            "from = the current year's Jan 1"
        );

        // A re-run while the page is open does nothing (stats.tsx:
        // 203-204).
        assert_eq!(dispatcher.dispatch(&store, "stats.open", None), Dispatch::Handled);
        assert_eq!(sink.calls.borrow().len(), 1, "no refetch while open");

        // The result lands: the payload rides the page.
        let info = oc_client::types::SessionStatsInfo {
            active_days: 3,
            activity: Vec::new(),
            cost: 0.0,
            models: Vec::new(),
            prompts: 4,
            range: oc_client::types::SessionStatsInfoRange { from: 0.0, to: 1.0 },
            sessions: 2,
            steps: 4,
            streak: 1,
            subagents: 0,
            tokens: oc_client::types::TokenUsageInfo::default(),
            tools: oc_client::types::SessionStatsTools::None(
                oc_client::types::SessionStatsToolsVariant1 {
                    mode: oc_client::types::SessionStatsToolsVariant1Mode::None,
                },
            ),
        };
        store.stats_fetch_result(1, Ok(info));
        assert!(matches!(store.stats_page_state().phase, StatsPhase::Loaded(_)));

        // A stale generation drops.
        store.stats_fetch_result(0, Err("stale".to_owned()));
        assert!(!matches!(store.stats_page_state().phase, StatsPhase::Failed(_)));

        // stats.close (the page's escape layer) returns to the route.
        assert_eq!(dispatcher.dispatch(&store, "stats.close", None), Dispatch::Handled);
        assert!(!store.stats_page_open());
        assert!(matches!(store.route(), Route::Session { session_id } if session_id == "s1"));
    }
