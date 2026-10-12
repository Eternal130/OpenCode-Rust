//! The composer's subagent picker (#44) — the port of v2's
//! `Composer` panel (`routes/session/composer/index.tsx`) with the
//! Subagents tab (`composer/subagents-tab.tsx`) and the Shell/
//! Terminals tab labels. Replaces the prompt at the session route's
//! bottom while open (or on a child session — #45).
//!
//! Geometry (read off the corpus3 grids, 80×24/160×36):
//!
//! ```text
//! ┃  ← border col 2, box cols [2, W−3]
//! ┃  Subagents  Shell  Terminals … esc   tabs row (gap 2, esc right)
//! ┃                                       gap 1
//! ┃  General: Check the fixture           body (5 rows, maxHeight)
//! ┃  show inactive ctrl+a  tabs ←/→       hints row
//! ┃                                       paddingBottom
//! ```
//!
//! The box: `SplitBorder` left in `border.base` over
//! `background.raised`, `paddingLeft 1` / `paddingRight 2` /
//! `paddingTop 1` / `paddingBottom 1`; content columns
//! `[4, W−5]`; every row adds `gap 1` (the column's `gap={1}`).
//! Entry rows fill `background.action.primary.*` with
//! `text.action.primary.*` glyphs (focused = selected+bold).

use crate::layout::{Placed, PlacedLine};
use crate::render::{paint_placed_with_fills, Fill, Frame};
use crate::store::{PickerEntry, PickerState, PickerTab};
use crate::theme::ThemeState;
use ratatui::style::{Color, Modifier, Style};

/// The body's fixed height (`maxHeight={5}` on the scrollbox — the
/// corpus grids pin 5 body rows whether the list is empty or short).
pub const BODY_ROWS: usize = 5;
/// The panel's own height: paddingTop + tabs + gap + body + gap +
/// hints + paddingBottom.
pub const PANEL_ROWS: usize = BODY_ROWS + 6;
/// The composer-area mount: the panel plus the session column's
/// bottom padding row (the prompt frame's 7-row mount analog).
pub const PICKER_MOUNT_ROWS: usize = PANEL_ROWS + 1;

/// The panel's resolved action colors (the transparent base/selected
/// composite over the raised panel; focused is the peach fill).
struct PickerColors {
    raised: Color,
    row_bg: Color,
    row_fg: Color,
    row_fg_current: Color,
    focused_fill: Color,
    focused_text: Color,
}

impl PickerColors {
    fn resolve(theme: &ThemeState) -> Self {
        let tokens = &theme.resolved.tokens;
        let raised = theme.styles().background_raised;
        Self {
            raised,
            // `background.action.primary.base`/`.selected` are
            // transparent in v2-dark — they composite over the panel.
            row_bg: raised,
            row_fg: rgb(tokens.text.action.primary.base),
            row_fg_current: rgb(tokens.text.action.primary.selected),
            focused_fill: rgb(tokens.background.action.primary.focused),
            focused_text: rgb(tokens.text.action.primary.focused),
        }
    }
}

/// The picker panel frame at the session column's width (the caller
/// pastes it at the column's bottom, like the prompt mount — with one
/// blank spacing row above and the pad row inside the mount).
#[must_use]
pub fn picker_panel_frame(
    state: &PickerState,
    entries: &[PickerEntry],
    selected: usize,
    theme: &ThemeState,
    width: u16,
) -> Frame {
    let styles = theme.styles();
    let colors = PickerColors::resolve(theme);
    let width = usize::from(width);
    let border = Style::new().fg(styles.border).bg(colors.raised);
    let muted = styles.text_muted.bg(colors.raised);
    let base = styles.text.bg(colors.raised);

    // Content columns [4, W−5] (border 2 + paddingLeft 1 → 4;
    // paddingRight 2 → W−5).
    let left = 4usize;
    let right = width.saturating_sub(5);

    let mut placed: Vec<Placed> = Vec::new();
    let mut fills: Vec<Fill> = Vec::new();

    // The box: raised background across [2, W−3] for all 11 rows.
    fills.push(Fill {
        x: 2,
        y: 0,
        width: u16::try_from(width.saturating_sub(4)).unwrap_or(0),
        height: panel_height(),
        style: Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(colors.raised),
    });
    for row in 0..PANEL_ROWS {
        placed.push(Placed {
            x: 2,
            y: row,
            width: 1,
            lines: vec![PlacedLine::plain("┃", border)],
            tag: Some("picker:border".into()),
        });
    }

    // Tabs row (row 1): labels at col 5 (the row's own paddingLeft 1),
    // gap 2; the active tab `text.base` BOLD, others muted. `esc`
    // right-aligned muted at the content edge.
    let tabs_row = 1usize;
    let mut x = left + 1;
    for tab in [PickerTab::Subagents, PickerTab::Shell, PickerTab::Terminals] {
        let label = tab.label();
        let style = if tab == state.tab { base.add_modifier(Modifier::BOLD) } else { muted };
        placed.push(Placed {
            x,
            y: tabs_row,
            width: label.len(),
            lines: vec![PlacedLine::plain(label, style)],
            tag: Some(format!("picker:tab:{label}")),
        });
        x += label.len() + 2; // gap 2
    }
    if right >= 3 {
        placed.push(Placed {
            x: right.saturating_sub(2),
            y: tabs_row,
            width: 3,
            lines: vec![PlacedLine::plain("esc", muted)],
            tag: Some("picker:close".into()),
        });
    }

    let muted_fg = styles.text_muted.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    paint_body(&mut placed, &mut fills, state, entries, selected, &colors, left, right, muted_fg);
    paint_hints(&mut placed, state, base, muted, left);

    paint_placed_with_fills(
        &placed,
        &fills,
        (u16::try_from(width).unwrap_or(u16::MAX), panel_height()),
        crate::components::tool_rows::fill_style(&styles),
    )
}

/// The body rows (3..8): the ` No {active|inactive} subagents`
/// fallback or the entry rows (subagents-tab.tsx:196-254).
#[allow(clippy::too_many_arguments)] // the body painter's geometry inputs
fn paint_body(
    placed: &mut Vec<Placed>,
    fills: &mut Vec<Fill>,
    state: &PickerState,
    entries: &[PickerEntry],
    selected: usize,
    colors: &PickerColors,
    left: usize,
    right: usize,
    muted_fg: Color,
) {
    let body_top = 3usize;
    let content_width = right.saturating_sub(left).max(1);
    if entries.is_empty() {
        // ` No {active|inactive} subagents` — the text carries its own
        // leading space and starts at the content column (no row
        // padding).
        let which = if state.filter_active { "active" } else { "inactive" };
        let label = format!(" No {which} subagents");
        placed.push(Placed {
            x: left,
            y: body_top,
            width: crate::wrap::display_width(&label),
            lines: vec![PlacedLine::plain(
                crate::wrap::take_width(&label, content_width),
                Style::new().fg(muted_fg).bg(colors.raised),
            )],
            tag: Some("picker:empty".into()),
        });
        return;
    }
    for (index, entry) in entries.iter().enumerate().take(BODY_ROWS) {
        let y = body_top + index;
        let active = index == selected;
        // The entry row's background spans the full content width
        // (`paddingLeft 1`/`paddingRight 1` cells paint too).
        let fill = if active { colors.focused_fill } else { colors.row_bg };
        let fg = if active {
            colors.focused_text
        } else if entry.current {
            colors.row_fg_current
        } else {
            colors.row_fg
        };
        fills.push(Fill {
            x: u16::try_from(left).unwrap_or(0),
            y: u16::try_from(y).unwrap_or(0),
            width: u16::try_from(content_width + 1).unwrap_or(1),
            height: 1,
            style: Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(fill),
        });
        let label = format!("{}{}: {}", entry.prefix, entry.agent, entry.title);
        let mut style = Style::new().fg(fg).bg(fill);
        if active {
            style = style.add_modifier(Modifier::BOLD);
        }
        let budget = content_width.saturating_sub(2).max(1);
        placed.push(Placed {
            x: left + 1,
            y,
            width: budget,
            lines: vec![PlacedLine::plain(crate::wrap::take_width(&label, budget), style)],
            tag: Some(format!("picker:entry:{}", entry.session_id)),
        });
        if entry.running {
            // `Running` at the row's right inner edge.
            let status_style = if active {
                Style::new().fg(colors.focused_text).bg(fill)
            } else {
                Style::new().fg(muted_fg).bg(fill)
            };
            let status_x = right.saturating_sub("Running".len() + 1);
            placed.push(Placed {
                x: status_x,
                y,
                width: "Running".len(),
                lines: vec![PlacedLine::plain("Running", status_style)],
                tag: None,
            });
        }
    }
}

/// The hints row (row 9): `show {inactive|active} ctrl+a` + gap 2 +
/// `tabs ←/→` — labels `text.base` BOLD, shortcuts muted (the
/// shortcut cell carries its leading space).
fn paint_hints(
    placed: &mut Vec<Placed>,
    state: &PickerState,
    base: Style,
    muted: Style,
    left: usize,
) {
    let hints_row = 9usize;
    let mut x = left + 1;
    let hint = |placed: &mut Vec<Placed>, x: &mut usize, label: &str, shortcut: &str| {
        placed.push(Placed {
            x: *x,
            y: hints_row,
            width: label.len(),
            lines: vec![PlacedLine::plain(label, base.add_modifier(Modifier::BOLD))],
            tag: None,
        });
        *x += label.len();
        let padded = format!(" {shortcut}");
        placed.push(Placed {
            x: *x,
            y: hints_row,
            width: padded.len(),
            lines: vec![PlacedLine::plain(padded, muted)],
            tag: None,
        });
        *x += shortcut.len() + 1;
    };
    let which = if state.filter_active { "inactive" } else { "active" };
    hint(placed, &mut x, &format!("show {which}"), "ctrl+a");
    x += 2; // gap 2 between hint groups
    hint(placed, &mut x, "tabs", "←/→");
}

/// [`PANEL_ROWS`] as the frame's height type.
fn panel_height() -> u16 {
    u16::try_from(PANEL_ROWS).unwrap_or(u16::MAX)
}

fn rgb(color: oc_theme::Rgba) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::store::Store;

    fn store_with_flow() -> Store {
        let store = Store::new(Runtime::new());
        store.ingest_all([
            crate::data::Ingest::Event(crate::data::BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "parent",
                    "title": "Parity Subagent Flow",
                }),
            )),
            crate::data::Ingest::Event(crate::data::BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "child",
                    "title": "Check the fixture",
                    "parentID": "parent",
                    "agent": "general",
                }),
            )),
        ]);
        store
    }

    /// The corpus3 picker geometry at 80 cols: tabs row col 5, `esc`
    /// right-aligned at the content edge (cols 73-75), the entry at
    /// col 5, hints row, and the 11-row panel with the border at
    /// col 2 on every row.
    #[test]
    fn panel_geometry_matches_the_corpus_grid() {
        let store = store_with_flow();
        store.open_tab("parent", Some("Parity Subagent Flow".to_owned()));
        let mut picker = store.picker();
        picker.open = true;
        picker.filter_active = false; // the completed run shows
        store.navigate(crate::store::Route::Session { session_id: "parent".to_owned() });
        store.set_picker(picker);
        let entries = store.picker_entries("parent");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].agent, "General");
        assert_eq!(entries[0].title, "Check the fixture");
        let selected = store.picker_selection("parent", entries.len());
        assert_eq!(selected, 0);
        let theme = ThemeState::v2_dark();
        let frame = picker_panel_frame(&store.picker(), &entries, selected, &theme, 80);
        let grid = frame.text_grid();
        assert_eq!(grid.len(), PANEL_ROWS);
        assert_eq!(
            grid[1].trim_end(),
            "  ┃  Subagents  Shell  Terminals".to_owned() + &" ".repeat(41) + "esc"
        );
        assert_eq!(grid[3].trim_end(), "  ┃  General: Check the fixture");
        assert_eq!(grid[9].trim_end(), "  ┃  show active ctrl+a  tabs ←/→");
        for (row, line) in grid.iter().enumerate() {
            assert!(line.starts_with("  ┃"), "row {row}: {line}");
            assert!(line.chars().skip(78).all(|ch| ch == ' '), "row {row} inside the box: {line}");
        }
    }

    /// The empty body: ` No active subagents` with the leading space.
    #[test]
    fn empty_active_filter_renders_the_fallback() {
        let store = store_with_flow();
        let mut picker = store.picker();
        picker.open = true;
        picker.filter_active = true; // nothing running → empty
        store.set_picker(picker);
        let entries = store.picker_entries("parent");
        assert!(entries.is_empty(), "nothing running under the active filter");
        let theme = ThemeState::v2_dark();
        let frame = picker_panel_frame(&store.picker(), &entries, 0, &theme, 80);
        let grid = frame.text_grid();
        assert_eq!(grid[3].trim_end(), "  ┃  No active subagents");
        assert_eq!(grid[9].trim_end(), "  ┃  show inactive ctrl+a  tabs ←/→");
    }
}
