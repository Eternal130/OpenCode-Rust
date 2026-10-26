//! G-07 — the devtools registry + bottom devtools bar (ref
//! `devtools/index.ts` + `devtools-bar.tsx`, v2.0.24).
//!
//! Strictly opt-in: the bar renders only while `config.debug.devtools`
//! is set (the settings row `debug.devtools` — `dialog_config.rs:413`;
//! ref gates on `config.data.debug?.devtools`, app.tsx:457). Disabled,
//! nothing registers on the frame and every key falls through — the
//! golden frames are byte-identical (the guard test below pins that).
//!
//! The registry mirrors ref's shape — groups `{id, title, entries:
//! [{key, value}]}` — parked on the Store as one signal
//! ([`crate::store::Store::devtools_state`]) with an upsert `set` API.
//! The bar paints the LAST screen row (over the status line — ref
//! mounts its own bottom bar; overlays still render ABOVE it, matching
//! ref's z-order where dialogs cover the bar).
//!
//! Panels (ref's switchable set): `server | theme | tools | ui`.
//! Bar keys (leader chords, consumed ONLY while enabled):
//! `<leader>d` toggle visibility · `<leader>[` / `<leader>]` switch
//! panel · `<leader>j` dump state to `/tmp/opencode/` (ref's
//! state-dump action).

use crate::store::Store;
use crate::theme::ThemeStyles;
use ratatui::style::Color;

/// The panel order on the bar (ref devtools-bar.tsx's tabs).
pub const PANEL_IDS: &[&str] = &["server", "theme", "tools", "ui"];

/// One registry row (ref `entries[{key, value}]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevtoolsEntry {
    pub key: String,
    pub value: String,
}

/// One registry group (ref `{id, title, entries}`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevtoolsGroup {
    pub id: String,
    pub title: String,
    pub entries: Vec<DevtoolsEntry>,
}

/// The whole devtools model: enable/visibility flags, the active panel
/// and the registry — one signal on the Store.
#[derive(Debug, Clone, PartialEq)]
pub struct DevtoolsState {
    /// `config.debug.devtools` (live-updated by the settings row).
    pub enabled: bool,
    /// `<leader>d` — the bar can be hidden while the feature is on.
    pub visible: bool,
    /// Index into [`PANEL_IDS`].
    pub panel: usize,
    /// The registry groups (upsert through `set_entry`).
    pub registry: Vec<DevtoolsGroup>,
}

impl Default for DevtoolsState {
    fn default() -> Self {
        Self {
            enabled: false,
            visible: true,
            panel: 0,
            registry: PANEL_IDS
                .iter()
                .map(|id| DevtoolsGroup {
                    id: (*id).to_owned(),
                    title: id_title(id),
                    entries: Vec::new(),
                })
                .collect(),
        }
    }
}

fn id_title(id: &str) -> String {
    match id {
        "server" => "Server".to_owned(),
        "theme" => "Theme".to_owned(),
        "tools" => "Tools".to_owned(),
        "ui" => "UI".to_owned(),
        other => other.to_owned(),
    }
}

impl DevtoolsState {
    /// Ref's `set(group, key, value)` — upsert one entry inside a
    /// group (the group is created on first write).
    pub fn set_entry(&mut self, group_id: &str, key: &str, value: &str) {
        let title = id_title(group_id);
        if !self.registry.iter().any(|group| group.id == group_id) {
            self.registry.push(DevtoolsGroup {
                id: group_id.to_owned(),
                title,
                entries: Vec::new(),
            });
        }
        let Some(group) = self.registry.iter_mut().find(|group| group.id == group_id) else {
            return;
        };
        match group.entries.iter_mut().find(|entry| entry.key == key) {
            Some(entry) => {
                entry.value.clear();
                entry.value.push_str(value);
            }
            None => {
                group.entries.push(DevtoolsEntry { key: key.to_owned(), value: value.to_owned() });
            }
        }
    }

    /// One group's entries (cloned) — the panel data source.
    #[must_use]
    pub fn group_entries(&self, group_id: &str) -> Vec<DevtoolsEntry> {
        self.registry
            .iter()
            .find(|group| group.id == group_id)
            .map(|group| group.entries.clone())
            .unwrap_or_default()
    }

    /// One entry's value (`""` when absent).
    #[must_use]
    pub fn entry(&self, group_id: &str, key: &str) -> String {
        self.group_entries(group_id)
            .into_iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value)
            .unwrap_or_default()
    }

    /// Cycle the active panel (wraps; `<leader>[` / `<leader>]`).
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
    pub fn cycle_panel(&mut self, direction: i32) {
        let count = PANEL_IDS.len() as i32;
        let next = (self.panel as i32 + direction).rem_euclid(count);
        self.panel = next.clamp(0, count - 1) as usize;
    }
}

fn connection_label(connection: &crate::data::ConnectionState) -> String {
    match connection {
        crate::data::ConnectionState::Connecting => "connecting".to_owned(),
        crate::data::ConnectionState::Connected => "connected".to_owned(),
        crate::data::ConnectionState::Reconnecting { attempt } => format!("reconnecting#{attempt}"),
        crate::data::ConnectionState::Failed => "failed".to_owned(),
    }
}

fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(i) => format!("#idx{i}"),
        _ => "-".to_owned(),
    }
}

/// The bar's single-line text for the active panel — the panel data
/// sources are the LIVE store state (server lists, theme, the frame
/// tick) plus the registry's `server`/`tools` rows.
#[must_use]
pub fn bar_text(store: &Store, styles: &ThemeStyles, size: (u16, u16)) -> String {
    let state = store.devtools_state();
    let panel = PANEL_IDS.get(state.panel).copied().unwrap_or("server");
    let mut parts = vec![format!("devtools[{panel}]")];
    match panel {
        "server" => {
            // `/api/info` essentials: version rides the registry (the
            // port has no /api/info model — the gap noted below),
            // counts + SSE state come from the live store.
            let lists = store.server_lists();
            let version = state.entry("server", "version");
            if !version.is_empty() {
                parts.push(format!("v{version}"));
            }
            parts.push(format!("sse={}", connection_label(&store.connection())));
            parts.push(format!("models={}", lists.models.rows().map_or(0, <[ServerModel]>::len)));
            parts.push(format!("providers={}", lists.providers.len()));
        }
        "theme" => {
            parts.push(format!("theme={}", store.theme_name()));
            parts.push(format!("accent={}", hex(styles.accent)));
            parts.push(format!("bg={}", hex(styles.background)));
        }
        "tools" => {
            // KNOWN GAP (trimmed): the port has no registered-tool
            // inventory — only the LAST tool call (hooked off the
            // `session.tool.called` SSE reducer in store.rs).
            let entries = state.group_entries("tools");
            if entries.is_empty() {
                parts.push("no tool calls yet".to_owned());
            } else {
                for entry in entries {
                    parts.push(format!("{}={}", entry.key, entry.value));
                }
            }
        }
        _ => {
            // "ui": screen size, the redraw tick as the fps-ish
            // counter, and the dump hint.
            parts.push(format!("size={}x{}", size.0, size.1));
            parts.push(format!("redraws={}", store.redraw_generation()));
            parts.push("dump=ctrl+x j".to_owned());
        }
    }
    parts.join(" ")
}

use crate::store::ServerModel;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::dialog::render_app;
    use crate::reactive::Runtime;
    use crate::store::Store;
    use crate::theme::ThemeState;

    fn store() -> Store {
        Store::new(Runtime::new())
    }

    fn theme() -> ThemeState {
        ThemeState::v2_dark()
    }

    #[test]
    fn registry_set_upserts_and_gets() {
        let mut state = DevtoolsState::default();
        state.set_entry("tools", "last", "read");
        state.set_entry("tools", "last", "grep");
        state.set_entry("tools", "count", "2");
        let tools = state.group_entries("tools");
        assert_eq!(tools.len(), 2, "upsert keeps one row per key");
        assert_eq!(state.entry("tools", "last"), "grep");
        assert_eq!(state.entry("tools", "count"), "2");
        assert_eq!(state.entry("server", "version"), "", "absent reads empty");
        // Unknown groups are created on first write (ref's register).
        state.set_entry("custom", "k", "v");
        assert_eq!(state.entry("custom", "k"), "v");
        assert!(state.registry.iter().any(|group| group.id == "custom"));
    }

    #[test]
    fn panel_cycling_wraps() {
        let mut state = DevtoolsState::default();
        assert_eq!(PANEL_IDS[state.panel], "server");
        state.cycle_panel(1);
        assert_eq!(PANEL_IDS[state.panel], "theme");
        state.cycle_panel(1);
        state.cycle_panel(1);
        assert_eq!(PANEL_IDS[state.panel], "ui");
        state.cycle_panel(1);
        assert_eq!(PANEL_IDS[state.panel], "server", "wraps forward");
        state.cycle_panel(-1);
        assert_eq!(PANEL_IDS[state.panel], "ui", "wraps backward");
    }

    /// The golden-frame guard: DISABLED, `render_app` is byte-identical
    /// to the bare route render (the bar paints nothing); enabled it
    /// owns the bottom row.
    #[test]
    fn bar_renders_only_when_enabled() {
        let store = store();
        let theme = theme();
        let size = (80_u16, 24_u16);
        let bare = render_app(&store, &theme, size);
        let route = crate::components::home::render_home(&store, &theme.styles(), size);
        assert_eq!(bare, route, "disabled: zero delta on the frame");

        store.set_devtools_enabled(true);
        let with_bar = render_app(&store, &theme, size);
        assert_ne!(with_bar, bare, "enabled: the bottom row changes");
        let bottom = with_bar.text_grid()[23].trim().to_owned();
        assert!(bottom.starts_with("devtools[server]"), "server panel first: {bottom}");

        // Hidden again (visible=false) restores the byte-identical
        // frame — the strictness holds for the toggle too.
        store.toggle_devtools_bar();
        assert!(!store.devtools_state().visible);
        assert_eq!(render_app(&store, &theme, size), bare);

        // Panel switching repaints the header + rows.
        store.toggle_devtools_bar();
        store.devtools_cycle_panel(2); // → "tools"
        let tools = render_app(&store, &theme, size).text_grid()[23].trim().to_owned();
        assert!(tools.starts_with("devtools[tools]"), "{tools}");
        assert!(tools.contains("no tool calls yet"), "{tools}");
    }

    #[test]
    fn bar_text_panels_read_live_state() {
        let store = store();
        store.set_devtools_enabled(true);
        let styles = theme().styles();
        assert!(bar_text(&store, &styles, (80, 24)).contains("sse=connecting"));
        store.devtools_set_entry("tools", "last", "bash");
        store.devtools_cycle_panel(2);
        assert!(bar_text(&store, &styles, (80, 24)).contains("last=bash"));
        store.devtools_cycle_panel(1); // → "ui"
        assert!(bar_text(&store, &styles, (100, 30)).contains("size=100x30"));
        assert!(bar_text(&store, &styles, (100, 30)).contains("redraws="));
    }

    #[test]
    fn dump_state_writes_parseable_json() {
        let store = store();
        store.set_devtools_enabled(true);
        let path = store.devtools_dump_state((80, 24)).expect("dump writes under /tmp/opencode");
        let raw = std::fs::read_to_string(&path).expect("dump file readable");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parseable JSON");
        assert_eq!(value["size"]["width"], 80);
        assert_eq!(value["size"]["height"], 24);
        assert_eq!(value["theme"], "opencode");
        assert!(value["redraws"].is_u64());
        assert!(path.starts_with("/tmp/opencode/"), "{}", path.display());
        assert!(path.to_string_lossy().contains("devtools-state"));
        let _ = std::fs::remove_file(&path);
    }
}
