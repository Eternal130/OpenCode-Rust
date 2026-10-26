//! The searchable panel substrate (`mini/footer.command.tsx`
//! `PanelShell` + `mini/footer.menu.tsx` `RunFooterMenu` + the
//! select-controller window math): the title / search / list / bottom
//! rail layout every picker panel renders through.
//!
//! The select CONTROLLER (the window/reveal/move math and
//! [`FooterMenuState`]) and the menu item data live in
//! [`super::state`] — extracted painter-agnostic in lane R2 step 1;
//! this module re-exports them and keeps the shell painter.

#![allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]

use super::canvas::{Canvas, Span, FILL_BG};
use super::theme::FooterTheme;
use crate::render::CellColor;

use crate::wrap::{display_width, take_width};

pub use super::state::{
    move_selection_clamp, reconcile_selection, reveal_selection_offset, FooterMenuItem,
    FooterMenuState, FooterTone, PANEL_FRAME_ROWS, PANEL_LIST_ROWS, RUN_COMMAND_PANEL_ROWS,
};

/// `FOOTER_COMPACT_WIDTH`.
pub const FOOTER_COMPACT_WIDTH: usize = 40;
/// `PANEL_PAD` (`panelPad`, non-mono).
pub const PANEL_PAD: usize = 2;

/// `footerPanelLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelLayout {
    pub compact: bool,
    pub frame: usize,
    pub limit: usize,
}

#[must_use]
pub fn footer_panel_layout(height: usize, limit: usize) -> PanelLayout {
    let available = height.saturating_sub(1).max(3);
    let compact = available < limit + PANEL_FRAME_ROWS;
    let frame = if compact { 2 } else { PANEL_FRAME_ROWS };
    PanelLayout { compact, frame, limit: limit.min(available.saturating_sub(frame)).max(1) }
}


// -- menu items ---------------------------------------------------------------


/// `footerMenuText` — truncate to a display width with `…`.
#[must_use]
pub fn footer_menu_text(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_owned();
    }
    format!("{}…", take_width(text, width - 1))
}

/// One rendered list row (`RunFooterMenuRow`).
#[derive(Debug, Clone, PartialEq)]
pub enum MenuRow<'a> {
    Header { label: &'a str },
    Spacer,
    Item { item: &'a FooterMenuItem, index: usize },
}

/// `groupedRows` — category headers (with spacer rows between groups).
#[must_use]
pub fn grouped_rows(items: &[FooterMenuItem], compact: bool) -> Vec<MenuRow<'_>> {
    let mut all: Vec<MenuRow> = Vec::new();
    let mut category = String::new();
    for (index, item) in items.iter().enumerate() {
        if !item.category.is_empty() && item.category != category {
            if !all.is_empty() && !compact {
                all.push(MenuRow::Spacer);
            }
            category.clone_from(&item.category);
            if !compact {
                all.push(MenuRow::Header { label: &items[index].category });
            }
        }
        all.push(MenuRow::Item { item, index });
    }
    all
}

/// `countLabel`.
#[must_use]
pub fn count_label(count: usize, total: usize, query: &str) -> String {
    if query.trim().is_empty() {
        total.to_string()
    } else {
        format!("{count}/{total}")
    }
}

/// Render the whole panel shell: optional blank/title/search frame
/// rows, the menu list (grouped or flat) and the half-block rail.
pub fn render_panel_shell(
    canvas: &mut Canvas,
    start_y: usize,
    title: &str,
    count_visible: bool,
    count: usize,
    total: usize,
    query: &str,
    placeholder: &str,
    hint: Option<&str>,
    items: &[FooterMenuItem],
    menu: &FooterMenuState,
    layout: PanelLayout,
    grouped: bool,
    background: bool,
    header_color: Option<CellColor>,
    empty: &str,
    theme: &FooterTheme,
    width: usize,
    height: usize,
) {
    let pad = if width < FOOTER_COMPACT_WIDTH { 1 } else { PANEL_PAD };
    let shade = if background { theme.shade } else { FILL_BG };
    let mut y = start_y;
    let frame_blank = |canvas: &mut Canvas, y: usize| {
        if y < height {
            canvas.fill_bg(y, 0, width, shade);
        }
    };
    if !layout.compact {
        frame_blank(canvas, y);
        y += 1;
    }
    // Title row.
    if y < height {
        canvas.fill_bg(y, 0, width, shade);
        let header_width = width.saturating_sub(pad * 2 + 4);
        let title_span = Span::new(footer_menu_text(title, header_width), theme.text, shade).bold();
        let count_text = count_label(count, total, query);
        let show_count =
            count_visible && display_width(title) + display_width(&count_text) < header_width;
        let hint_text = hint.filter(|hint| {
            display_width(title)
                + usize::from(show_count) * (display_width(&count_text) + 1)
                + display_width(hint)
                + 3
                <= header_width
        });
        let mut spans = vec![title_span];
        if show_count {
            spans.push(Span::new(format!(" {count_text}"), theme.muted, shade));
        }
        canvas.spans(y, pad, &spans);
        let mut right: Vec<Span> = Vec::new();
        if let Some(hint) = hint_text {
            right.push(Span::new(format!("{hint} · "), theme.muted, shade));
        }
        right.push(Span::new("esc", theme.muted, shade));
        canvas.spans_right(y, width - pad - 1, &right);
    }
    y += 1;
    if !layout.compact {
        frame_blank(canvas, y);
        y += 1;
    }
    // Search row: the focused input region across the content width
    // (transparent panels leave the terminal fill showing through).
    if y < height {
        canvas.fill_bg(y, 0, width, shade);
        let input_width = width.saturating_sub(pad * 2);
        let input_bg = if background { theme.formfield_focused_bg } else { FILL_BG };
        canvas.fill_bg(y, pad, input_width, input_bg);
        if query.is_empty() && !placeholder.is_empty() {
            canvas.spans(y, pad, &[Span::new(placeholder, theme.muted, input_bg)]);
        } else if !query.is_empty() {
            canvas.spans(y, pad, &[Span::new(query, theme.formfield_focused_text, input_bg)]);
        }
    }
    y += 1;
    if !layout.compact {
        frame_blank(canvas, y);
        y += 1;
    }
    // List region.
    render_footer_menu(
        canvas,
        y,
        items,
        menu,
        layout,
        grouped,
        background,
        header_color,
        empty,
        theme,
        width,
        height,
    );
}

/// Render the menu list region (`RunFooterMenu`, border=false).
#[allow(clippy::too_many_arguments)]
pub fn render_footer_menu(
    canvas: &mut Canvas,
    start_y: usize,
    items: &[FooterMenuItem],
    menu: &FooterMenuState,
    layout: PanelLayout,
    grouped: bool,
    background: bool,
    header_color: Option<CellColor>,
    empty: &str,
    theme: &FooterTheme,
    width: usize,
    height: usize,
) {
    let pad = if width < FOOTER_COMPACT_WIDTH { 1 } else { PANEL_PAD };
    let shade = if background { theme.shade } else { FILL_BG };
    let limit = layout.limit;
    let list_width = width.saturating_sub(pad * 2);
    // Description column: widest display among items with a
    // description, plus 2 (`descriptionColumn`).
    let description_column = items
        .iter()
        .filter(|item| item.description.is_some())
        .map(|item| display_width(&item.display))
        .max()
        .map_or(0, |width| width + 2);

    let rows: Vec<MenuRow> = if grouped {
        let all = grouped_rows(items, layout.compact);
        let start = all.len().saturating_sub(limit).min(menu.offset);
        all[start..(start + limit).min(all.len())].to_vec()
    } else {
        items
            .iter()
            .skip(menu.offset)
            .take(limit)
            .enumerate()
            .map(|(position, item)| MenuRow::Item { item, index: position + menu.offset })
            .collect()
    };

    for row in 0..limit {
        let y = start_y + row;
        if y >= height {
            break;
        }
        canvas.fill_bg(y, 0, width, shade);
        match rows.get(row) {
            None | Some(MenuRow::Spacer) => {}
            Some(MenuRow::Header { label }) => {
                canvas.spans(
                    y,
                    pad,
                    &[Span::new(
                        footer_menu_text(label, list_width),
                        header_color.unwrap_or(theme.muted),
                        shade,
                    )
                    .bold()],
                );
            }
            Some(MenuRow::Item { item, index }) => {
                render_menu_item(
                    canvas,
                    y,
                    item,
                    *index,
                    menu.selected,
                    background,
                    description_column,
                    theme,
                    width,
                );
            }
        }
    }
    if rows.is_empty() && !empty.is_empty() && start_y < height {
        canvas.spans(
            start_y,
            pad,
            &[Span::new(footer_menu_text(empty, list_width), theme.muted, shade)],
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_menu_item(
    canvas: &mut Canvas,
    y: usize,
    item: &FooterMenuItem,
    index: usize,
    selected: usize,
    background: bool,
    description_column: usize,
    theme: &FooterTheme,
    width: usize,
) {
    let pad = if width < FOOTER_COMPACT_WIDTH { 1 } else { PANEL_PAD };
    let active = index == selected;
    let shade = if background { theme.shade } else { FILL_BG };
    let bg = if active { theme.action_focused_bg } else { shade };
    let list_width = width.saturating_sub(pad * 2);
    let icon_width = usize::from(item.icon.is_some()) * 2;
    let available = list_width.saturating_sub(icon_width);
    let base_fg = if active { theme.action_focused_text } else { theme.formfield_text };
    canvas.fill_bg(y, 0, width, bg);
    let mut x = pad;
    if let Some(icon) = item.icon {
        canvas.spans(
            y,
            x,
            &[Span::new(
                icon.to_string(),
                if active { theme.action_focused_text } else { theme.formfield_text },
                bg,
            )],
        );
        x += 2;
    }
    // Footer visibility + truncation (RunFooterMenu's footer()).
    let footer = item.footer.as_deref().filter(|footer| {
        let title = display_width(&item.display);
        let primary = item.footer_tone.is_some()
            && !(item.current && item.footer_tone == Some(FooterTone::Selection));
        let fit = if primary { icon_width.max(4).min(title) } else { title };
        fit + 1 + display_width(footer) <= available
    });
    let display_budget =
        available.saturating_sub(footer.map_or(0, |footer| display_width(footer) + 1));
    let display = footer_menu_text(&item.display, display_budget);
    let display_span = Span::new(display, base_fg, bg);
    let display_span = if active { display_span.bold() } else { display_span };
    let mut spans = vec![display_span];
    if let (Some(description), false) = (&item.description, description_column == 0) {
        let remaining = available
            .saturating_sub(description_column)
            .saturating_sub(footer.map_or(0, |footer| display_width(footer) + 1));
        if remaining >= 12.min(display_width(description)) {
            let separator = description_column.saturating_sub(display_width(&item.display)).max(1);
            let sep_fg = if active { theme.action_focused_text } else { theme.muted };
            spans.push(Span::new(" ".repeat(separator), sep_fg, bg));
            let desc_fg = if active { theme.action_focused_text } else { theme.muted };
            spans.push(Span::new(footer_menu_text(description, remaining), desc_fg, bg));
        }
    }
    canvas.spans(y, x, &spans);
    if let Some(footer) = footer {
        let fg = if active {
            theme.action_focused_text
        } else {
            match item.footer_tone {
                Some(FooterTone::Selection) => theme.selection,
                Some(FooterTone::Running) => theme.running,
                Some(FooterTone::Error) => theme.error,
                Some(FooterTone::Success) => theme.success,
                _ => theme.muted,
            }
        };
        let span = Span::new(footer, fg, bg);
        let span = if active { span.bold() } else { span };
        canvas.spans_right(y, width - pad - 1, &[span]);
    }
}

/// The half-block bottom rail (`HALF_BLOCK_BORDER`) — drawn when the
/// panel has a background and is not compact.
#[must_use]
pub fn render_bottom_rail(width: usize, theme: &FooterTheme) -> Vec<Span> {
    vec![Span::new("▀".repeat(width), theme.shade, FILL_BG)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_layout_compact_boundary_matches_the_reference() {
        // Settings/agent/variant dumps: height 16 ⇒ available 15 < 16 ⇒ compact.
        let compact = footer_panel_layout(16, PANEL_LIST_ROWS);
        assert!(compact.compact);
        assert_eq!(compact.frame, 2);
        assert_eq!(compact.limit, 10);
        // Command/model dumps: height 17 ⇒ available 16 ⇒ not compact.
        let full = footer_panel_layout(17, PANEL_LIST_ROWS);
        assert!(!full.compact);
        assert_eq!(full.frame, 6);
        assert_eq!(full.limit, 10);
    }

    #[test]
    fn select_controller_semantics() {
        let mut menu = FooterMenuState::new();
        menu.reveal(5, 20, 10);
        assert_eq!(menu.selected, 5);
        menu.r#move(1, 20, 10);
        assert_eq!(menu.selected, 6);
        // Moving past the window's end reveals forward.
        menu.r#move(1, 20, 10);
        menu.r#move(1, 20, 10);
        menu.r#move(1, 20, 10);
        menu.r#move(1, 20, 10);
        assert_eq!(menu.selected, 10);
        assert!(menu.offset > 0, "window followed the selection");
        menu.reveal(0, 20, 10);
        assert_eq!(menu.offset, 0);
    }

    #[test]
    fn menu_text_truncates_with_ellipsis() {
        assert_eq!(footer_menu_text("Commands", 20), "Commands");
        assert_eq!(footer_menu_text("A very long title", 8), "A very …");
        assert_eq!(footer_menu_text("A very long title", 1), "…");
    }

    #[test]
    fn grouped_rows_insert_headers_and_spacers() {
        let items = vec![
            FooterMenuItem {
                display: "a".into(),
                category: "Session".into(),
                ..FooterMenuItem::default()
            },
            FooterMenuItem {
                display: "b".into(),
                category: "Session".into(),
                ..FooterMenuItem::default()
            },
            FooterMenuItem {
                display: "c".into(),
                category: "Agent".into(),
                ..FooterMenuItem::default()
            },
        ];
        let rows = grouped_rows(&items, false);
        assert_eq!(rows.len(), 6, "header, a, b, spacer, header, c");
        assert!(matches!(rows[0], MenuRow::Header { label: "Session" }));
        assert!(matches!(rows[3], MenuRow::Spacer));
        let compact = grouped_rows(&items, true);
        assert_eq!(compact.len(), 3, "compact drops headers and spacers");
    }
}
