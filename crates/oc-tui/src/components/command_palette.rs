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

use crate::components::dialog_select::{SelectOption, SelectProps, SelectState};
use crate::store::{DialogKind, DialogSize, Route, Store};

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
            "provider.connect" => store.connection() != crate::data::ConnectionState::Connected,
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
    PaletteCommand { command: "help.show", title: "Help", group: "System", bindings: &[] },
    PaletteCommand { command: "docs.open", title: "Open docs", group: "System", bindings: &[] },
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

/// The full palette props for the current store state: the suggested
/// section first (a duplicate set under "Suggested") when no filter is
/// active, then every registry command — `list()` in
/// `command-palette.tsx`. The palette mounts flat with a `0.7`
/// threshold.
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
    options.extend(palette_options(PALETTE_COMMANDS));
    SelectProps::new("Commands", options).flat(true).threshold(0.7)
}

/// The select props for one select-style dialog kind (the dialog
/// renderer's content source).
#[must_use]
pub fn select_props_for(kind: &DialogKind, store: &Store) -> SelectProps {
    match kind {
        DialogKind::CommandPalette => palette_props(store),
        DialogKind::ThemeList => theme_list_props(),
        DialogKind::ServerList { title, .. } => SelectProps::new(
            *title,
            vec![
                // TODO(7.3): server-backed listings — populate from the
                // oc-client `*.list` fetches when the data layer lands.
                SelectOption::titled("Waiting for server… (wave 3 wiring)", "stub")
                    .muted_placeholder(),
            ],
        ),
        DialogKind::SessionRename { .. } => SelectProps::new("", Vec::new()),
    }
}

impl SelectOption {
    /// Marker helper for server-stub rows.
    #[must_use]
    pub fn muted_placeholder(mut self) -> Self {
        self.description = Some("(server-backed list — oc-client wiring lands in wave 3)".into());
        self
    }
}

/// The built-in theme registry (`theme/discovery.ts` full registry is
/// wave 3; the corpus theme resolves today).
#[must_use]
pub fn theme_list_props() -> SelectProps {
    let mut options = vec![SelectOption::titled("opencode", "builtin:v2/opencode")
        .categorized("Built-in")
        .with_footer("v2")];
    for name in oc_theme::builtins::builtin_v1_names() {
        // TODO(7.3 theme discovery): v1 documents need the legacy
        // resolver; listed so the palette shape matches the reference.
        options.push(
            SelectOption::titled(name, format!("builtin:v1/{name}"))
                .categorized("Legacy")
                .with_footer("v1"),
        );
    }
    SelectProps::new("Themes", options).with_current(Some("builtin:v2/opencode".to_owned()))
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
    fn dispatch(&mut self, store: &Store, command: &str, _args: Option<&str>) -> Dispatch {
        match command {
            "command.palette.show" => {
                store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
                Dispatch::Handled
            }
            "session.new" => {
                store.navigate(Route::Home);
                store.clear_dialogs();
                Dispatch::Handled
            }
            "session.clear" => {
                if let Route::Session { session_id } = store.route() {
                    store.close_tab(Some(&session_id));
                }
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
            "session.rename" => {
                let session_id = match store.route() {
                    Route::Session { session_id } => session_id,
                    Route::Home => return Dispatch::Handled,
                };
                store.replace_dialog(DialogKind::SessionRename { session_id }, DialogSize::Medium);
                Dispatch::Handled
            }
            "theme.switch" => {
                store.replace_dialog(DialogKind::ThemeList, DialogSize::Medium);
                Dispatch::Handled
            }
            "model.list" | "agent.list" | "mcp.list" | "variant.list" | "session.list"
            | "open.menu" | "provider.connect" | "opencode.settings" | "opencode.status"
            | "session.share" | "session.fork" | "session.move" | "server.pair"
            | "location.reload" | "opencode.debug" | "help.show" | "docs.open" => {
                let entry = palette_command(command);
                let title = entry.map_or("", |entry| entry.title);
                let size =
                    if command == "open.menu" { DialogSize::Large } else { DialogSize::Medium };
                store.replace_dialog(
                    DialogKind::ServerList { command: static_command(command), title },
                    size,
                );
                Dispatch::Handled
            }
            "app.exit" => {
                store.clear_dialogs();
                // The app shell exits the run loop (wave 3); the replay
                // corpus's exit path is binding-gated there.
                Dispatch::ServerPending("app.exit run-loop teardown")
            }
            _ => Dispatch::Unknown,
        }
    }
}

/// Registry keys are static: dispatch inputs resolve back to the
/// registry entry's own `&'static str` id.
fn static_command(command: &str) -> &'static str {
    palette_command(command).map_or("", |entry| entry.command)
}

/// Execute a submitted palette/theme selection: the suggested-section
/// values carry a `suggested:` prefix to strip.
#[must_use]
pub fn command_of_value(value: &str) -> &str {
    value.strip_prefix("suggested:").unwrap_or(value)
}

/// The theme a `ThemeList` selection resolves to (the corpus theme
/// today; v1 legacy documents resolve in the theme-discovery wave).
#[must_use]
pub fn theme_of_value(value: &str) -> Option<crate::theme::ThemeState> {
    match value {
        "builtin:v2/opencode" => Some(crate::theme::ThemeState::v2_dark()),
        _ => None,
    }
}

/// Initialize a freshly-opened select dialog's state (focus `current`).
pub fn init_select_state(state: &mut SelectState, props: &SelectProps) {
    state.focus_current(props);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn registry_covers_the_replay_anchors() {
        assert_eq!(palette_command("session.new").unwrap().title, "New session");
        assert_eq!(palette_command("session.rename").unwrap().title, "Rename session");
        assert_eq!(palette_command("session.rename").unwrap().group, "Session");
        assert_eq!(palette_command("model.list").unwrap().bindings, &["ctrl+x m"][..]);
        // The v2 table leaves theme.switch unbound.
        assert_eq!(palette_command("theme.switch").unwrap().bindings.len(), 0);
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
        // Server-backed listings open the stub surface.
        assert_eq!(dispatcher.dispatch(&store, "model.list", None), Dispatch::Handled);
        match &store.top_dialog().unwrap().kind {
            DialogKind::ServerList { command, title } => {
                assert_eq!(*command, "model.list");
                assert_eq!(*title, "Switch model");
            }
            other => panic!("unexpected dialog {other:?}"),
        }
        assert_eq!(dispatcher.dispatch(&store, "nope.nope", None), Dispatch::Unknown);
    }
}
