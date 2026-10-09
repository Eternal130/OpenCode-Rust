//! The session route frame skeleton: a message-list placeholder pane
//! over a composer placeholder box (`routes/session/` foundation).
//! Placeholder fidelity is NOT gated this wave — the frame exists so
//! the app shell routes session views end-to-end.

use crate::layout::{BoxSpec, LayoutTree};
use crate::render::{paint_placed, Frame};
use crate::store::{SessionTab, Store};
use crate::theme::ThemeStyles;
use ratatui::style::Style;
use taffy::AvailableSpace;

/// Render the session route: title line, empty message-list placeholder
/// (7.2: message grouping/virtualization), and a one-row composer
/// placeholder box (7.2: the real composer).
#[must_use]
pub fn render_session(
    store: &Store,
    session_id: &str,
    theme: &ThemeStyles,
    size: (u16, u16),
) -> Frame {
    let tabs = store.tabs();
    let title = tabs
        .tabs
        .iter()
        .find(|tab: &&SessionTab| tab.session_id == session_id)
        .and_then(|tab| tab.title.clone())
        .unwrap_or_else(|| "Untitled session".to_owned());
    let style = Style::new().fg(theme.text.fg.unwrap_or_default());
    let muted = Style::new().fg(theme.text_muted.fg.unwrap_or_default());
    let mut tree = LayoutTree::new();
    let title_leaf = tree.text(&title, style);
    let title_row = tree.box_node(&BoxSpec::row().padding_left(2.0), &[title_leaf]);
    let messages = tree.text("", muted);
    let messages_pane = tree.box_node(&BoxSpec::column().grow(1.0).padding_left(2.0), &[messages]);
    let composer_prompt = tree.text("›", muted);
    let composer = tree.box_node(
        &BoxSpec::row().height(1.0).padding_left(2.0).padding_top(1.0),
        &[composer_prompt],
    );
    let column =
        tree.box_node(&BoxSpec::column().padding_top(1.0), &[title_row, messages_pane, composer]);
    let root = tree
        .box_node(&BoxSpec::column().width(f32::from(size.0)).height(f32::from(size.1)), &[column]);
    tree.set_root(root);
    let placed =
        tree.compute((f32::from(size.0), AvailableSpace::Definite(f32::from(size.1)))).placed;
    paint_placed(&placed, size, Style::new().bg(theme.background))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;

    #[test]
    fn session_frame_shows_title_and_composer_slot() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Bug hunt".to_owned()));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_session(&store, "s1", &theme.styles(), (40, 8));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Bug hunt"), "{grid}");
        assert!(grid.contains('›'), "composer placeholder present");
    }
}
