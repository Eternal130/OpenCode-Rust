//! The plugin route page (PLUG-TUI-SLOT's router surface): the
//! native full-screen page a plugin's registered route renders into —
//! the guest's `render({data})` ran through the host's bounded
//! `route.render` call and the flattened `{title, rows}` crossed
//! once; this module paints it (no per-frame IPC). The ref's shape:
//! a full-box page over the app (feature-plugins/system/stats.tsx:
//! 140-186 — `StatsPage` paints its box edge-to-edge with the plugin
//! page's own keymap layer providing the escape/back bind).

/// TUI-TD28 — the `go home` button's rect (`go home` + 2 padding
/// columns, one row, centered two rows under the warning line). Both
/// the renderer and the app's click handler derive it from the size —
/// the same geometry, no recorded state.
#[must_use]
pub fn plugin_route_home_rect(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let button = "go home";
    let w = button.chars().count() + 2;
    let x = width.saturating_sub(w) / 2;
    (x, height / 2 + 2, w, 1)
}

#[cfg(test)]
mod tests {
    use crate::theme::ThemeState;

    #[test]
    fn page_paints_title_and_rows() {
        let theme = ThemeState::v2_dark();
        let frame = crate::components::plugin_page_solid::render_plugin_page_solid(
            "stats",
            &["Usage statistics".to_owned(), "sessions: 12".to_owned()],
            &theme.styles(),
            (40, 10),
        );
        let grid = frame.text_grid();
        assert!(grid[1].contains("stats"), "the route title: {:?}", grid[1]);
        assert!(grid[3].contains("Usage statistics"), "row 1: {:?}", grid[3]);
        assert!(grid[4].contains("sessions: 12"), "row 2: {:?}", grid[4]);
    }
}
