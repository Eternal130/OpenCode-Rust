//! The home route skeleton: the opencode logo block plus the session
//! tab strip placeholder row (`routes/home.tsx` foundation — content
//! sections land in 7.2).
//!
//! Visual fidelity of the placeholder body is NOT gated this wave; the
//! route frame exists so the app shell (tabs bar + route area) is
//! exercisable end-to-end from the store.

use crate::layout::{BoxSpec, LayoutTree};
use crate::render::{paint_placed, Frame};
use crate::store::Store;
use crate::theme::ThemeStyles;
use ratatui::style::Style;
use taffy::AvailableSpace;

/// The `opencode` wordmark (`logo.ts` shape).
pub const LOGO: &str = "opencode";

/// Render the home route at `size`: logo line, a hint line, then the
/// rest of the route area empty (content sections: 7.2).
#[must_use]
pub fn render_home(store: &Store, theme: &ThemeStyles, size: (u16, u16)) -> Frame {
    let style = Style::new().fg(theme.text.fg.unwrap_or_default());
    let muted = Style::new().fg(theme.text_muted.fg.unwrap_or_default());
    let tabs = store.tabs();
    let _route = store.route();
    let mut tree = LayoutTree::new();
    let logo = tree.text(LOGO, style);
    let logo_row = tree.box_node(&BoxSpec::row().padding_left(2.0), &[logo]);
    let hint =
        tree.text(if tabs.tabs.is_empty() { "ctrl+p commands · ctrl+x leader" } else { "" }, muted);
    let hint_row = tree.box_node(&BoxSpec::row().padding_left(2.0).padding_top(1.0), &[hint]);
    let body = tree.box_node(&BoxSpec::column().grow(1.0), &[]);
    let column = tree.box_node(&BoxSpec::column().padding_top(1.0), &[logo_row, hint_row, body]);
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
    use crate::store::Route;

    #[test]
    fn home_renders_the_logo_and_hint() {
        let store = Store::new(Runtime::new());
        assert_eq!(store.route(), Route::Home);
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (40, 6));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("opencode"), "{grid}");
        assert!(grid.contains("ctrl+p"), "{grid}");
    }
}
