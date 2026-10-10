//! The home route (`routes/home.tsx`): the opencode wordmark, the
//! prompt composer's placeholder hint, recent sessions (data-driven
//! from the store's tabs), quick actions, and footer hints.
//!
//! No golden exists for the full home route (golden-export registered
//! as a Phase 8 chore in the wave report); the layout renders
//! reasonably from the v2 source: centered logo + prompt mount with
//! rotating placeholders, the home.footer slot below (the quick-action
//! strip + hints).

use crate::layout::{BoxSpec, LayoutTree};
use crate::render::{paint_placed, Frame};
use crate::store::Store;
use crate::theme::ThemeStyles;
use ratatui::style::Style;
use taffy::AvailableSpace;

/// The `opencode` wordmark (`logo.ts` shape).
pub const LOGO: &str = "opencode";

/// The home placeholder rotation (`routes/home.tsx` placeholder.normal).
pub const PLACEHOLDERS: &[&str] =
    &["Fix a TODO in the codebase", "What is the tech stack of this project?", "Fix broken tests"];

/// The quick-action strip (`home.footer` slot content): label + binding
/// pairs from the operative v2 table.
pub const QUICK_ACTIONS: &[(&str, &str)] = &[
    ("New session", "ctrl+x n"),
    ("Switch model", "ctrl+x m"),
    ("Switch session", "ctrl+x l"),
    ("Commands", "ctrl+p"),
];

/// The maximum recent-session rows rendered.
const RECENT_LIMIT: usize = 6;

/// Render the home route at `size`: logo, placeholder hint, the recent
/// session list (store tabs), quick actions, and footer hints.
#[must_use]
pub fn render_home(store: &Store, theme: &ThemeStyles, size: (u16, u16)) -> Frame {
    let style = Style::new().fg(theme.text.fg.unwrap_or_default());
    let muted = Style::new().fg(theme.text_muted.fg.unwrap_or_default());
    let accent = Style::new().fg(theme.accent);
    let tabs = store.tabs();
    let _route = store.route();

    let mut tree = LayoutTree::new();
    let logo = tree.text(LOGO, style);
    let logo_row = tree.box_node(&BoxSpec::row().padding_left(2.0), &[logo]);
    let tick = usize::try_from(store.clock()).unwrap_or(0);
    let placeholder = PLACEHOLDERS[tick / 4 % PLACEHOLDERS.len()];
    let hint_text = format!("Ask anything… \"{placeholder}\"");
    let hint = tree.text(&hint_text, muted);
    let hint_row = tree.box_node(&BoxSpec::row().padding_left(2.0).padding_top(1.0), &[hint]);

    // Recent sessions (the store's tabs — the sessions the app knows).
    let mut recent_nodes = Vec::new();
    if !tabs.tabs.is_empty() {
        let header = tree.text("Recent sessions", accent);
        recent_nodes
            .push(tree.box_node(&BoxSpec::row().padding_left(2.0).padding_top(1.0), &[header]));
        for tab in tabs.tabs.iter().take(RECENT_LIMIT) {
            let label = match (&tab.title, tabs.unread.get(&tab.session_id)) {
                (Some(title), Some(_)) => format!("{title} •"),
                (Some(title), None) => title.clone(),
                (None, Some(_)) => format!("{} •", tab.session_id),
                (None, None) => tab.session_id.clone(),
            };
            let row = tree.text(&label, style);
            recent_nodes.push(tree.box_node(&BoxSpec::row().padding_left(4.0), &[row]));
        }
    }

    // Quick actions (label + binding).
    let mut actions: Vec<taffy::tree::NodeId> = Vec::new();
    for (index, (label, binding)) in QUICK_ACTIONS.iter().enumerate() {
        let action_text = format!("{label} {binding}");
        let text = tree.text(&action_text, muted);
        let spec = if index == 0 {
            BoxSpec::row().padding_left(2.0).padding_top(1.0)
        } else {
            BoxSpec::row().padding_left(2.0)
        };
        actions.push(tree.box_node(&spec, &[text]));
    }

    // Footer hints (the composer's status row subset).
    let footer = tree.text("ctrl+p commands · ctrl+x leader · ctrl+c exit", muted);
    let footer_row = tree.box_node(&BoxSpec::row().padding_left(2.0), &[footer]);

    let body = tree.box_node(&BoxSpec::column().grow(1.0), &[]);
    let mut column_children = vec![logo_row, hint_row];
    column_children.extend(recent_nodes);
    column_children.extend(actions);
    column_children.push(body);
    column_children.push(footer_row);
    let column = tree.box_node(&BoxSpec::column().padding_top(1.0), &column_children);
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
        let frame = render_home(&store, &theme.styles(), (40, 12));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("opencode"), "{grid}");
        assert!(grid.contains("ctrl+p"), "{grid}");
    }

    /// The data-driven content: recent sessions from the store's tabs
    /// (with the unread dot) and the quick-action strip.
    #[test]
    fn home_lists_recent_sessions_and_quick_actions() {
        let store = Store::new(Runtime::new());
        store.open_tab("ses_a", Some("Alpha".to_owned()));
        store.open_tab("ses_b", Some("Beta".to_owned()));
        store.navigate(Route::Home);
        store.set_unread("ses_b", Some(crate::store::Unread::Activity));
        let theme = crate::theme::ThemeState::v2_dark();
        let frame = render_home(&store, &theme.styles(), (60, 20));
        let grid = frame.text_grid().join("\n");
        assert!(grid.contains("Recent sessions"), "{grid}");
        assert!(grid.contains("Alpha"), "{grid}");
        assert!(grid.contains("Beta •"), "unread mark on the tab row: {grid}");
        assert!(grid.contains("New session ctrl+x n"), "{grid}");
        assert!(grid.contains("Switch session ctrl+x l"), "{grid}");
        assert!(grid.contains("Ask anything…"), "{grid}");
    }
}
