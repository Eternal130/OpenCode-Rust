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
    DialogKind, DialogSize, DiffSource, ListState, ModelSelection, PairState, Route, ServerAgent,
    Store,
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
    PaletteCommand {
        command: "opencode.status",
        title: "View status",
        group: "System",
        bindings: &["ctrl+x s"],
    },
    PaletteCommand { command: "server.pair", title: "Pair device", group: "System", bindings: &[] },
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
    PaletteCommand {
        command: "diff.open",
        title: "Open diff viewer",
        group: "VCS",
        bindings: &[],
    },
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
fn palette_options_slice(commands: &[&PaletteCommand]) -> Vec<SelectOption> {
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

/// The full palette props for the current store state: the suggested
/// section first (a duplicate set under "Suggested") when no filter is
/// active, then every registry command — `list()` in
/// `command-palette.tsx` — then the plugin-registered commands (the
/// oc-plugin-host registration snapshot's palette entries, group
/// "Plugin"). The palette mounts flat with a `0.7` threshold.
#[must_use]
pub fn palette_props(store: &Store) -> SelectProps {
    let mut options = Vec::new();
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
    // v2's conditional palette entries (`palette: undefined` — e.g.
    // variant.list without variants, app.tsx:923): a command whose
    // surface cannot open is REMOVED from the list, never advertised
    // as a dead row.
    let advertised: Vec<&PaletteCommand> = PALETTE_COMMANDS
        .iter()
        .filter(|entry| {
            !(entry.command == "variant.list" && store.current_model_variants().is_empty())
        })
        .collect();
    options.extend(palette_options_slice(&advertised));
    // The plugin layer (registration-time snapshot): id search text,
    // the plugin id beside the group, no bindings display.
    for entry in store.plugin_commands() {
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
    SelectProps::new("Commands", options).flat(true).threshold(0.7)
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
        DialogKind::Help | DialogKind::SessionRename { .. } | DialogKind::WorktreeName => {
            SelectProps::new("", Vec::new())
        }
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
        ),
        DialogKind::ServerList { command: "agent.list", title } => agent_list_props(store, title),
        DialogKind::ServerList { command: "session.list", title } => {
            // The session list dialog renders the HISTORY (v2's
            // `data.session.list()` — every session the resync
            // reported, newest first), NOT the open tabs; selecting a
            // row navigates and the route effect opens its tab.
            // Rows resolve the live registry title first (v2's list
            // memo re-derives from `session.info`, which the renamed
            // handler writes — data.ts:687-699).
            let history = store.session_history();
            let options: Vec<SelectOption> = history
                .iter()
                .map(|session| {
                    SelectOption::titled(
                        store
                            .session_title(&session.id)
                            .or_else(|| session.title.clone())
                            .unwrap_or_else(|| session.id.clone()),
                        session.id.clone(),
                    )
                    .categorized("Sessions")
                })
                .collect();
            SelectProps::new(*title, options)
        }
        DialogKind::ServerList { title, .. } => SelectProps::new(
            *title,
            vec![SelectOption::titled("Not wired in this build", "stub").muted_placeholder()],
        ),
        // SURF-2 `DialogMcp` (`/mcps`, dialog-mcp.tsx:70-81): the
        // name-sorted server rows with the status footer text
        // (`Status`, :20-34).
        DialogKind::Mcp => {
            let mut servers = store.mcp_servers();
            servers.sort_by(|left, right| left.name.cmp(&right.name));
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
            let row = store
                .mcp_servers()
                .into_iter()
                .find(|server| server.name == *name);
            let mut options = Vec::new();
            if let Some(row) = row {
                options.push(SelectOption::titled(
                    row.error.clone().unwrap_or_else(|| "Unknown MCP connection error".to_owned()),
                    "error",
                ));
                options.push(SelectOption::titled(
                    format!("Status: {}", row.status),
                    "context",
                ).muted_placeholder());
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
                    format!("{} MCP server{}", servers.len(), if servers.len() == 1 { "" } else { "s" }),
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
                Route::Session { session_id } => {
                    store.selection_for(&session_id).model
                }
                Route::Home => store.selection_for(crate::app::HOME_PROMPT_KEY).model,
            }
            .and_then(|model| model.variant)
            .unwrap_or_else(|| "default".to_owned());
            SelectProps::new("Select variant", options).flat(true).with_current(Some(current))
        }
        // SURF-2 `DialogPair` (`/pair`, dialog-pair.tsx:99-127): the
        // loading / error / loaded link list (the QR block deferred —
        // see docs/surf2.log).
        DialogKind::Pair => {
            let options = match store.pair_state() {
                PairState::Loading => vec![status_option("Loading server information…")],
                PairState::Failed(error) => vec![
                    SelectOption::titled("Could not load server information", "error"),
                    SelectOption::titled(error, "detail").muted_placeholder(),
                    SelectOption::titled("Close and reopen Pair to try again.", "retry")
                        .muted_placeholder(),
                ],
                PairState::Loaded { links, localhost, minutes, loopback } => {
                    let mut rows = vec![SelectOption::titled(
                        format!(
                            "Open a link to connect. Links work once and expire in {minutes} minutes."
                        ),
                        "intro",
                    )
                    .muted_placeholder()];
                    if let Some(url) = &localhost {
                        rows.push(SelectOption::titled("This device", "label").muted_placeholder());
                        rows.push(SelectOption::titled(url.clone(), "localhost"));
                    }
                    rows.push(SelectOption::titled("Links", "label").muted_placeholder());
                    for (index, url) in links.iter().enumerate() {
                        rows.push(SelectOption::titled(url.clone(), format!("link:{index}")));
                    }
                    if loopback {
                        rows.push(
                            SelectOption::titled(
                                "Run `opencode service set hostname 0.0.0.0` to access the service remotely.",
                                "loopback",
                            )
                            .muted_placeholder(),
                        );
                    }
                    rows
                }
            };
            SelectProps::new("Pair", options).flat(true)
        }
        // SURF-2 `DialogDebug` (`/debug`, dialog-debug.tsx:25-35): the
        // env/config entries + the share hint (enter copies).
        DialogKind::Debug => {
            let model = match store.route() {
                Route::Session { session_id } => {
                    store.effective_model(&session_id)
                }
                Route::Home => store.effective_model(crate::app::HOME_PROMPT_KEY),
            };
            let session = match store.route() {
                Route::Session { session_id } => session_id,
                Route::Home => "n/a".to_owned(),
            };
            let terminal = std::env::var("TERM").unwrap_or_default();
            let entries = [
                ("Version", format!("{} (local)", env!("CARGO_PKG_VERSION"))),
                (
                    "Date",
                    chrono_like_now(),
                ),
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
            SelectProps::new("Debug", options)
                .flat(true)
                .with_actions(vec![SelectAction::new("dialog.debug.copy", "copy", "return")])
        }
        // SURF-2 `DialogTimeline` (`session.timeline`,
        // dialog-timeline.tsx:21-39): the user messages newest-first
        // with the short-time footer.
        DialogKind::Timeline { session_id } => {
            let mut options = Vec::new();
            for message in store.messages(session_id) {
                if let crate::components::message::model::Message::User { id, text, created, .. } =
                    &message
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
                if shortcuts.is_empty() { "unbound".to_owned() } else { shortcuts.join(" / ") }
            };
            let mut options = Vec::new();
            let mut group = |title: &str, rows: &[(&str, &str)]| {
                for (commands, label) in rows {
                    let shortcut_text = shortcut(commands);
                    options.push(
                        SelectOption::titled(
                            format!("{shortcut_text:<17}{}", label),
                            format!("help:{}", label.replace('/', "-")),
                        )
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
/// is case-insensitive; tertiary puts lowercase first on ties):
/// case-folded compare, then lowercase-first tie-break. Identical to
/// ICU for the ASCII corpus of provider/model names.
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
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
                let current = store
                    .tabs()
                    .tabs
                    .iter()
                    .find(|tab| tab.session_id == session_id)
                    .and_then(|tab| store.display_title(tab))
                    .unwrap_or_default();
                store.update_dialog(|top| top.value = current);
                Dispatch::Handled
            }
            "theme.switch" => {
                store.replace_dialog(DialogKind::ThemeList, DialogSize::Medium);
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
            "messages.copy" => {
                copy_message_to_clipboard(store);
                Dispatch::Handled
            }
            "model.list" | "agent.list" | "session.list"
            | "open.menu" | "provider.connect" | "opencode.settings"
            | "session.share" | "session.fork" | "location.reload"
            | "docs.open" => {
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
            "diff.down" | "diff.up" | "diff.page.down" | "diff.page.up" | "diff.half_page.down"
            | "diff.half_page.up" | "diff.first" | "diff.last" => {
                let viewport = usize::from(store.terminal_size().1).max(1);
                store.update_diff_viewer(|live| {
                    crate::components::diff_viewer_mount::diff_scroll_command(live, command, viewport)
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
            "diff.mark_reviewed" | "diff.toggle_file_tree" | "diff.single_patch"
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
                store.update_diff_viewer(|live| {
                    live.viewer.run_command(command, &layout, &files)
                });
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
        store.stage_model(crate::app::HOME_PROMPT_KEY, ModelSelection {
            provider_id: "opencode".to_owned(),
            model_id: "grok".to_owned(),
            variant: None,
        });
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
        store.stage_model(crate::app::HOME_PROMPT_KEY, ModelSelection {
            provider_id: "opencode".to_owned(),
            model_id: "kimi".to_owned(),
            variant: None,
        });
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
    fn six_tui_commands_mount_real_surfaces() {
        use crate::store::{McpServerRow, PairState, RecordingMcpToggleSink};
        let store = Store::new(Runtime::new());
        let toggle_sink = std::rc::Rc::new(RecordingMcpToggleSink::default());
        store.set_mcp_toggle_sink(toggle_sink.clone());
        let mut dispatcher = StoreDispatcher;

        // /mcps — the live rows with status footers (dialog-mcp.tsx).
        store.set_mcp_servers(vec![
            McpServerRow {
                name: "beta".into(),
                status: "connected",
                error: None,
            },
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
        let details = crate::components::dialog::execute_submission(
            &store,
            &mut dispatcher,
            "alpha",
        );
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

        // /pair — the loading row, then the loaded links.
        assert_eq!(dispatcher.dispatch(&store, "server.pair", None), Dispatch::Handled);
        assert!(matches!(store.top_dialog().unwrap().kind, DialogKind::Pair));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.options[0].title, "Loading server information…");
        store.set_pair_state(PairState::Loaded {
            links: vec!["http://127.0.0.1:4096/auth/connect/abc".to_owned()],
            localhost: Some("http://localhost:4096/auth/connect/abc".to_owned()),
            minutes: 5,
            loopback: true,
        });
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        let text = props.options.iter().map(|o| o.title.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("expire in 5 minutes"), "{text}");
        assert!(text.contains("This device"));
        assert!(text.contains("opencode service set hostname 0.0.0.0"));
        store.set_pair_state(PairState::Failed("connection refused".to_owned()));
        let props = select_props_for(&store.top_dialog().unwrap().kind, &store);
        assert_eq!(props.options[0].title, "Could not load server information");
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
        use crate::store::{DiffPhase, DiffSink, RecordingDiffSink};
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
        assert_eq!(matches!(live.phase, DiffPhase::Loading), true);
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
        assert!(matches!(store.diff_viewer().phase, DiffPhase::Loaded(ref files) if files.is_empty()));

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
                    time_idle: None,
                    viewed: None,
                },
                SessionSnapshot {
                    model: None,
                    id: "s_open".into(),
                    title: Some("Open tab".into()),
                    time_idle: None,
                    viewed: None,
                },
            ],
            unread: Vec::new(),
            messages: Vec::new(),
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
}
