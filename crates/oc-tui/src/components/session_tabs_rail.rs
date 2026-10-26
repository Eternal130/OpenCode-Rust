//! The vertical session-tab rail (TUI-3 + TD-25) — the port of v2's
//! `VerticalSessionTabs` (`component/session-tabs.tsx:513-1287`) and
//! the compact rail controls (`component/session-tabs-rail.tsx`).
//!
//! Layout contract (v2 `app.tsx:1341-1384`): when
//! [`crate::store::Store::vertical_rail_width`] is `Some`, the rail
//! owns the LEFT `width` columns at full height, the horizontal strip
//! does NOT render (`tabsVisible() && !tabsVertical()`), and the
//! content column reflows to `total − rail` (`panels.setWidth`,
//! app.tsx:585). The `PaneResizeHandle` (the rail's last column,
//! `left = size − 1`) drags the width — [`RailHit::Handle`] is its
//! hit-test mirror (`ui/pane-resize.ts` + `pane-resize-handle.tsx`).
//!
//! Rendered subset (animations off — the deterministic shapes the
//! goldens/PTY suites pin):
//!
//! * **non-compact** (`width ≥ SESSION_TABS_COMPACT_BREAKPOINT`):
//!   one pad row, then per tab a 2-row item — the number indicator
//!   (`numberWidth` right-aligned + 1 pad) with the title (bold
//!   `text.base` on the selected `action.selected` pill, muted
//!   otherwise), and the detail row (`{project} ⎇ {branch}` shape,
//!   indented by the prefix) — a 1-row gap between items, then the
//!   `+ New session` add slot (`session-tabs.tsx:1042-1233`).
//! * **compact** (`width < 12`, TD-25): the rail controls — the
//!   centered `⌕` search button (release → `session.list`) and the
//!   half-row seam (`▄` while the row below is highlighted, `▀` on
//!   hover — `session-tabs-rail.tsx:20-66`) — then 1-row centered
//!   indicator items (`▄`/`▀` highlight half-rows around the
//!   highlighted rows) and the centered `+` add slot; the hover
//!   tooltip rides at `left = width`
//!   (`session-tabs.tsx:1240-1280`).

// Terminal dimensions only (far below 2^16) — the truncation lints
// carry no real risk on this surface.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::components::session_tabs::{TabStatus, TabView, NEW_SESSION_TAB_TITLE};
use crate::components::tab_pulse::tint;
use crate::components::tool_rows::DEFAULT_FOREGROUND;
use crate::layout::{Placed, PlacedLine};
use crate::render::{CellColor, Fill, Frame};
use crate::theme::ThemeStyles;
use crate::wrap::take_width;

/// `SESSION_TABS_COMPACT_WIDTH` (ui/layout.ts:2) — the clamp floor.
pub const SESSION_TABS_COMPACT_WIDTH: usize = 5;
/// `SESSION_SIDEBAR_MAX_WIDTH` (ui/layout.ts:4) — the clamp ceiling.
pub const SESSION_SIDEBAR_MAX_WIDTH: usize = 72;
/// `SESSION_CONTENT_MIN_WIDTH` (ui/layout.ts:5) — the content keeps
/// at least this much (`clampSessionTabsWidth`).
pub const SESSION_CONTENT_MIN_WIDTH: usize = 44;
/// `SESSION_CONTENT_PREFERRED_WIDTH` (ui/layout.ts:6) — the split
/// must leave room for this much content (`sessionTabsFitVertically`).
pub const SESSION_CONTENT_PREFERRED_WIDTH: usize = 64;
/// `SESSION_TABS_COMPACT_BREAKPOINT` (ui/layout.ts:3).
pub const SESSION_TABS_COMPACT_BREAKPOINT: usize = 12;
/// `SESSION_SIDEBAR_WIDTH` — the rail's default width (the storage
/// doc's initial `verticalTabsWidth`, app.tsx:504).
pub const SESSION_SIDEBAR_WIDTH: usize = 42;
/// The hover tooltip's width ceiling (`Math.min(54, width − rail)`,
/// session-tabs.tsx:561).
pub const TOOLTIP_MAX_WIDTH: usize = 54;
/// `SESSION_TABS_INDICATOR_WIDTH`-shaped detail indent: the compact
/// search glyph (`⌕`, session-tabs-rail.tsx:47).
pub const SEARCH_GLYPH: char = '⌕';

/// `config.tabs.layout` ("horizontal" | "vertical", config/index.tsx:
/// 330 — anything but `"vertical"` is horizontal).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TabsLayout {
    #[default]
    Horizontal,
    Vertical,
}

impl TabsLayout {
    /// Resolve the raw config string (`"vertical"` is the only
    /// vertical spelling; absent/garbage stays horizontal).
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("vertical") => Self::Vertical,
            _ => Self::Horizontal,
        }
    }
}

/// `sessionTabsFitVertically` (ui/layout.ts:8-10): the split needs
/// `total ≥ rail + SESSION_CONTENT_PREFERRED_WIDTH`.
#[must_use]
pub fn session_tabs_fit_vertically(total: usize, width: usize) -> bool {
    total >= width + SESSION_CONTENT_PREFERRED_WIDTH
}

/// `clampSessionTabsWidth` (ui/layout.ts:12-17): `max(5, min(width,
/// 72, total − 44))` — verbatim, floor included.
#[must_use]
pub fn clamp_session_tabs_width(width: usize, total: usize) -> usize {
    width
        .min(SESSION_SIDEBAR_MAX_WIDTH)
        .min(total.saturating_sub(SESSION_CONTENT_MIN_WIDTH))
        .max(SESSION_TABS_COMPACT_WIDTH)
}

/// The rail's row geometry — where each item, the add slot and the
/// compact controls sit (the scrollbox is unscrolled: `scrollTop` 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RailGeometry {
    /// The rail's column width.
    pub width: usize,
    /// `width < SESSION_TABS_COMPACT_BREAKPOINT`.
    pub compact: bool,
    /// `stride()` — rows per item (compact 2: item + gap; else 3:
    /// title + detail + gap).
    pub stride: usize,
    /// `numberWidth` — `max(2, digits(tabs.len()))`.
    pub number_width: usize,
    /// The first item's row (compact: pad + controls + seam + scroll
    /// padding = 3; else the root's pad row = 1).
    pub first_tab_row: usize,
    /// The add slot's row (after the items + gap).
    pub add_row: usize,
}

impl RailGeometry {
    /// Solve the geometry for `count` tabs at `width`
    /// (`compact`/`stride`/`numberWidth`, session-tabs.tsx:560-566).
    #[must_use]
    pub fn new(count: usize, width: usize) -> Self {
        let compact = width < SESSION_TABS_COMPACT_BREAKPOINT;
        let stride = if compact { 2 } else { 3 };
        let number_width = count.to_string().len().max(2);
        let first_tab_row = if compact { 3 } else { 1 };
        Self {
            width,
            compact,
            stride,
            number_width,
            first_tab_row,
            add_row: first_tab_row + stride * count,
        }
    }

    /// Item `index`'s top row.
    #[must_use]
    pub fn item_row(&self, index: usize) -> usize {
        self.first_tab_row + self.stride * index
    }

    /// The compact rail controls' row (the `⌕` button — row 1 after
    /// the root's pad; the seam paints row 2).
    #[must_use]
    pub fn search_row(&self) -> usize {
        1
    }

    /// The seam row under the controls (TD-25's half-row border).
    #[must_use]
    pub fn seam_row(&self) -> usize {
        2
    }
}

/// What a pointer cell over the rail addresses — the hit-test mirror
/// of [`paint_vertical_rail`]'s geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailHit {
    /// A tab's rows (release selects; middle-press closes — the
    /// pointer layer routes both like the strip's).
    Tab(String),
    /// The `+ New session` slot (release → home route,
    /// session-tabs.tsx:1160-1207).
    Add,
    /// The compact rail's `⌕` search button (release → `tabs.search()
    /// ?? keymap.dispatch("session.list")`, session-tabs-rail.tsx:26).
    Search,
    /// The resize handle's columns (`contains: x ∈ [size−1, size]`,
    /// app.tsx:511).
    Handle,
    /// Outside every control (pad/gap/seam rows).
    Outside,
}

/// Resolve a rail-local (`col`, `row`) against the geometry. The
/// handle's two columns win over content (v2's handle box overlays
/// the boundary — `left = size − 1`, width 2).
#[must_use]
pub fn rail_hit(
    tabs: &[TabView],
    geometry: &RailGeometry,
    col: usize,
    row: usize,
    home_new_tab: bool,
) -> RailHit {
    if col + 1 >= geometry.width {
        return RailHit::Handle;
    }
    if col >= geometry.width {
        return RailHit::Outside;
    }
    if geometry.compact && row == geometry.search_row() {
        return RailHit::Search;
    }
    if row >= geometry.first_tab_row {
        let offset = row - geometry.first_tab_row;
        let index = offset / geometry.stride;
        let within = offset % geometry.stride < if geometry.compact { 1 } else { 2 };
        if within {
            if let Some(view) = tabs.get(index) {
                return RailHit::Tab(view.tab.session_id.clone());
            }
        }
        if row == geometry.add_row {
            // `if (!newTab()) tabs.add?.()` — on the home route the
            // slot IS the new tab; a release re-adds nothing.
            return if home_new_tab { RailHit::Outside } else { RailHit::Add };
        }
    }
    RailHit::Outside
}

/// One frame's pointer-derived paint state (hover lifts + the
/// highlighted seam), mirroring the store's strip hover signals.
#[allow(clippy::struct_excessive_bools)] // the pointer flag set mirrors the reference's signals
#[derive(Debug, Clone, Default)]
pub struct RailInputs<'a> {
    pub tabs: &'a [TabView],
    /// The active tab's id (`None` on the home route).
    pub active: Option<&'a str>,
    pub hover: Option<&'a str>,
    pub add_hovered: bool,
    /// The compact search button's hover lift.
    pub search_hovered: bool,
    /// The home route's promoted new-tab slot (`newTab()`).
    pub home_new_tab: bool,
    /// The resize handle's hover/drag highlight (the 1-wide raised
    /// strip on the rail's last column).
    pub handle_hot: bool,
    /// The compact hover tooltip's session (`hovered()` — renders at
    /// `left = width`).
    pub tooltip: Option<&'a TabView>,
}

fn rgb(color: ratatui::style::Color) -> (u8, u8, u8) {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128),
    }
}

/// `idleNumber()` — the add slot's `+` and the unselected numbers'
/// tint (`tint(text.formfield.base, background, 0.55)`,
/// session-tabs.tsx:565).
fn idle_number(styles: &ThemeStyles) -> ratatui::style::Color {
    let bg = rgb(styles.background_raised);
    let fg = rgb(styles.formfield_base);
    let (r, g, b) = tint(fg, bg, 0.55);
    ratatui::style::Color::Rgb(r, g, b)
}

/// `hoverColor()` — the rail controls' seam/lift tint
/// (`tint(raised.base, raised.high, raised.high.a)`,
/// session-tabs-rail.tsx:13-15; both opaque in v2-dark).
fn hover_color(styles: &ThemeStyles) -> ratatui::style::Color {
    let bg = rgb(styles.background_raised);
    let high = rgb(styles.action_hovered);
    let (r, g, b) = tint(bg, high, 1.0);
    ratatui::style::Color::Rgb(r, g, b)
}

/// [`hover_color`] exposed for the solid tooltip's seam chunks.
#[must_use]
pub fn hover_color_public(styles: &ThemeStyles) -> ratatui::style::Color {
    hover_color(styles)
}

/// `sessionTabDetail`'s detail color (`tint(text.muted, bg, 0.35)`,
/// session-tabs.tsx:820).
fn detail_color(styles: &ThemeStyles, over: ratatui::style::Color) -> ratatui::style::Color {
    let bg = rgb(over);
    let muted = rgb(styles.text_muted.fg.unwrap_or(ratatui::style::Color::Rgb(128, 128, 128)));
    let (r, g, b) = tint(muted, bg, 0.35);
    ratatui::style::Color::Rgb(r, g, b)
}

/// The rail's paint walk — the ONE solver both renderers consume:
/// the placed lines + fills (base style excluded). Extracted by lane
/// R1 so the hand-rolled painter and the solid bridge share it.
#[must_use]
pub fn rail_paint_list(
    inputs: &RailInputs,
    geometry: &RailGeometry,
    styles: &ThemeStyles,
) -> (Vec<Placed>, Vec<Fill>) {
    let mut placed: Vec<Placed> = Vec::new();
    let mut fills = Vec::new();
    rail_paint_walk(inputs, geometry, styles, &mut placed, &mut fills);
    (placed, fills)
}

/// The resize handle's highlight strip (the last column, painted
/// while hovered/resizing) — a bridge recolor (bg only, the cell's
/// glyph/fg kept), shared by both renderers.
pub fn paint_rail_handle_hot(
    frame: &mut Frame,
    inputs: &RailInputs,
    geometry: &RailGeometry,
    styles: &ThemeStyles,
) {
    let width = geometry.width;
    if inputs.handle_hot && width >= 1 {
        for row_cells in &mut frame.cells {
            if let Some(cell) = row_cells.get_mut(width - 1) {
                cell.bg = CellColor::from(styles.action_hovered);
            }
        }
    }
}

/// The compact hover tooltip (session-tabs.tsx:1240-1280): at
/// `left = width`, `min(54, total − width)` wide, 4 rows — seam,
/// title, detail, seam. It rides OUTSIDE the rail frame (over the
/// content column) — handed back to the compositor.
#[must_use]
pub fn paint_rail_tooltip(
    inputs: &RailInputs,
    geometry: &RailGeometry,
    styles: &ThemeStyles,
) -> Option<(usize, Frame)> {
    let (true, Some(view)) = (geometry.compact, inputs.tooltip) else {
        return None;
    };
    let index =
        inputs.tabs.iter().position(|tab| tab.tab.session_id == view.tab.session_id).unwrap_or(0);
    let row = geometry.item_row(index).saturating_sub(1);
    Some((row, crate::components::session_tabs_rail_solid::paint_tooltip_solid(view, styles)))
}

/// The walk body (the painter's own geometry decisions, verbatim) —
/// fills + placed lines, NO frame assembly (the renderers own that).
#[allow(clippy::too_many_lines)] // one arm per rail mode, mirroring the tsx body
fn rail_paint_walk(
    inputs: &RailInputs,
    geometry: &RailGeometry,
    styles: &ThemeStyles,
    placed: &mut Vec<Placed>,
    fills: &mut Vec<Fill>,
) {
    let width = geometry.width;
    let number_width = geometry.number_width;
    let highlighted = |id: &str, active: Option<&str>, hover: Option<&str>| -> bool {
        Some(id) == active || Some(id) == hover
    };

    // The compact rail controls (TD-25): the ⌕ row then the seam.
    if geometry.compact {
        let button_bg =
            if inputs.search_hovered { styles.action_hovered } else { styles.background_raised };
        fills.push(Fill {
            x: 0,
            y: geometry.search_row() as u16,
            width: width as u16,
            height: 1,
            style: ratatui::style::Style::new().fg(DEFAULT_FOREGROUND).bg(button_bg),
        });
        let center = width.saturating_sub(1) / 2;
        placed.push(Placed {
            x: center,
            y: geometry.search_row(),
            width: 1,
            lines: vec![PlacedLine::plain(
                SEARCH_GLYPH.to_string().as_str(),
                ratatui::style::Style::new().fg(styles.action_secondary).bg(
                    if inputs.search_hovered {
                        styles.action_hovered
                    } else {
                        styles.background_raised
                    },
                ),
            )],
            tag: Some("rail.search".to_owned()),
        });
        // The seam: `▄` while the row below highlights, `▀` on hover,
        // spaces otherwise — fg ALWAYS hoverColor over raised.base
        // (session-tabs-rail.tsx:54-66).
        let below = inputs
            .tabs
            .first()
            .is_some_and(|view| highlighted(&view.tab.session_id, inputs.active, inputs.hover))
            || (inputs.tabs.is_empty() && inputs.home_new_tab);
        let glyph = if inputs.search_hovered {
            "▀"
        } else if below {
            "▄"
        } else {
            " "
        };
        placed.push(Placed {
            x: 0,
            y: geometry.seam_row(),
            width,
            lines: vec![PlacedLine::plain(
                glyph.repeat(width).as_str(),
                ratatui::style::Style::new().fg(hover_color(styles)).bg(styles.background_raised),
            )],
            tag: Some("rail.seam".to_owned()),
        });
    }

    // The items.
    for (index, view) in inputs.tabs.iter().enumerate() {
        let id = view.tab.session_id.as_str();
        let selected = inputs.active == Some(id);
        let hovered = inputs.hover == Some(id);
        let row = geometry.item_row(index);
        if geometry.compact {
            // 1-row centered indicator + the highlight half-rows.
            if highlighted(id, inputs.active, inputs.hover) {
                let border = half_row_border_bg(styles, index == 0, inputs);
                push_half_row(placed, row.saturating_sub(1), width, "▄", border, styles);
                push_half_row(placed, row + 1, width, "▀", border, styles);
            }
            // Compact selected uses the HOVERED bg (never the pill —
            // `tabBackground`, session-tabs.tsx:787-791).
            let bg =
                if selected || hovered { styles.action_hovered } else { styles.background_raised };
            fills.push(Fill {
                x: 0,
                y: row as u16,
                width: width as u16,
                height: 1,
                style: ratatui::style::Style::new().fg(DEFAULT_FOREGROUND).bg(bg),
            });
            let label = compact_indicator_label(view);
            let fg = if selected {
                styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)
            } else {
                styles.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND)
            };
            let mut style = ratatui::style::Style::new().fg(fg).bg(bg);
            if selected {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            placed.push(Placed {
                x: width.saturating_sub(1) / 2,
                y: row,
                width: label.chars().count(),
                lines: vec![PlacedLine::plain(label.as_str(), style)],
                tag: Some(format!("session-tab:{id}")),
            });
        } else {
            // The 2-row item: the selected pill, the number + title
            // row, the detail row.
            let bg = if selected {
                styles.action_selected
            } else if hovered {
                styles.action_hovered
            } else {
                styles.background_raised
            };
            fills.push(Fill {
                x: 0,
                y: row as u16,
                width: width as u16,
                height: 2,
                style: ratatui::style::Style::new().fg(DEFAULT_FOREGROUND).bg(bg),
            });
            // The indicator: right-aligned in numberWidth, one pad
            // cell, the title from x = numberWidth + 1.
            let number_fg = number_color(styles, view.status, selected, hovered, bg);
            placed.push(Placed {
                x: number_width.saturating_sub(1),
                y: row,
                width: 1,
                lines: vec![PlacedLine::plain(
                    (index + 1).to_string().as_str(),
                    ratatui::style::Style::new().fg(number_fg).bg(bg),
                )],
                tag: Some(format!("session-tab:{id}")),
            });
            let title_width = width.saturating_sub(number_width + 2).max(1);
            let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".to_owned());
            let title = take_width(&title, title_width);
            let mut style = if selected || hovered {
                ratatui::style::Style::new().fg(styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)).bg(bg)
            } else {
                ratatui::style::Style::new()
                    .fg(styles.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND))
                    .bg(bg)
            };
            if selected {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            placed.push(Placed {
                x: number_width + 1,
                y: row,
                width: crate::wrap::display_width(&title),
                lines: vec![PlacedLine::plain(title.as_str(), style)],
                tag: Some(format!("session-tab:{id}")),
            });
            // The detail row (`{project} ⎇ {branch}`, paddingLeft
            // prefixWidth — session-tabs.tsx:1080-1102).
            if let Some(detail) = &view.detail {
                let text = take_width(detail, width.saturating_sub(number_width + 3).max(1));
                if !text.is_empty() {
                    placed.push(Placed {
                        x: number_width + 1,
                        y: row + 1,
                        width: crate::wrap::display_width(&text),
                        lines: vec![PlacedLine::plain(
                            text.as_str(),
                            ratatui::style::Style::new().fg(detail_color(styles, bg)).bg(bg),
                        )],
                        tag: None,
                    });
                }
            }
        }
    }

    // The add slot: `+ New session` (non-compact) / centered `+`
    // (compact); the home route promotes it to the selected new-tab
    // row (`newTab()`, session-tabs.tsx:1153-1233).
    let add_selected = inputs.home_new_tab;
    let add_hover = inputs.add_hovered;
    if geometry.compact {
        let bg = if add_selected || add_hover {
            styles.action_hovered
        } else {
            styles.background_raised
        };
        fills.push(Fill {
            x: 0,
            y: geometry.add_row as u16,
            width: width as u16,
            height: 1,
            style: ratatui::style::Style::new().fg(DEFAULT_FOREGROUND).bg(bg),
        });
        let fg = if add_selected || add_hover {
            styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)
        } else {
            idle_number(styles)
        };
        let mut style = ratatui::style::Style::new().fg(fg).bg(bg);
        if add_selected {
            style = style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        placed.push(Placed {
            x: width.saturating_sub(1) / 2,
            y: geometry.add_row,
            width: 1,
            lines: vec![PlacedLine::plain("+", style)],
            tag: Some("session-tabs.add".to_owned()),
        });
    } else {
        let bg = if add_selected {
            styles.action_selected
        } else if add_hover {
            styles.action_hovered
        } else {
            styles.background_raised
        };
        fills.push(Fill {
            x: 0,
            y: geometry.add_row as u16,
            width: width as u16,
            height: 1,
            style: ratatui::style::Style::new().fg(DEFAULT_FOREGROUND).bg(bg),
        });
        let fg = if add_selected || add_hover {
            styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)
        } else {
            styles.text_muted.fg.unwrap_or(DEFAULT_FOREGROUND)
        };
        let mut style = ratatui::style::Style::new().fg(fg).bg(bg);
        if add_selected {
            style = style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        placed.push(Placed {
            x: 1,
            y: geometry.add_row,
            width: 1,
            lines: vec![PlacedLine::plain(
                "+",
                ratatui::style::Style::new()
                    .fg(if add_selected || add_hover {
                        styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)
                    } else {
                        idle_number(styles)
                    })
                    .bg(bg),
            )],
            tag: Some("session-tabs.add".to_owned()),
        });
        placed.push(Placed {
            x: 3,
            y: geometry.add_row,
            width: NEW_SESSION_TAB_TITLE.len(),
            lines: vec![PlacedLine::plain(NEW_SESSION_TAB_TITLE, style)],
            tag: None,
        });
    }
}

/// `numberColor()` at rest (animations off — the ignition/sweep
/// envelopes are 0): selected blends toward `text.base` over the
/// pulse background; idle is the formfield tint
/// (session-tabs.tsx:795-803).
fn number_color(
    styles: &ThemeStyles,
    status: TabStatus,
    selected: bool,
    hovered: bool,
    bg: ratatui::style::Color,
) -> ratatui::style::Color {
    let _ = status;
    let base = if hovered && !selected {
        styles.text.fg.unwrap_or(DEFAULT_FOREGROUND)
    } else {
        idle_number(styles)
    };
    if !selected {
        return base;
    }
    let pulse = rgb(bg);
    let text = rgb(styles.text.fg.unwrap_or(DEFAULT_FOREGROUND));
    let inner = tint(text, pulse, 0.25);
    let (r, g, b) = tint(rgb(base), inner, 1.0);
    ratatui::style::Color::Rgb(r, g, b)
}

/// The compact indicator's label: the status glyph when busy/
/// unread/attention, else the title's first grapheme (`idleLabel`,
/// session-tabs.tsx:179-184).
fn compact_indicator_label(view: &TabView) -> String {
    let label = view.status.indicator_label();
    if label.is_empty() {
        view.tab
            .title
            .as_deref()
            .unwrap_or("Untitled session")
            .trim_start()
            .chars()
            .next()
            .map_or_else(|| "U".to_owned(), String::from)
    } else {
        label.to_owned()
    }
}

/// A highlight half-row's bg (`SessionTabHalfRow`'s `background` —
/// the neighbor's highlight color when it is highlighted too,
/// session-tabs.tsx:923-943).
fn half_row_border_bg(
    styles: &ThemeStyles,
    first: bool,
    inputs: &RailInputs,
) -> ratatui::style::Color {
    let _ = (first, inputs);
    styles.background_raised
}

fn push_half_row(
    placed: &mut Vec<Placed>,
    row: usize,
    width: usize,
    glyph: &str,
    bg: ratatui::style::Color,
    styles: &ThemeStyles,
) {
    placed.push(Placed {
        x: 0,
        y: row,
        width,
        lines: vec![PlacedLine::plain(
            glyph.repeat(width).as_str(),
            ratatui::style::Style::new().fg(hover_color(styles)).bg(bg),
        )],
        tag: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::session_tabs::TabStatus;
    use crate::components::session_tabs_rail_solid::paint_vertical_rail_solid;
    use crate::store::{SessionTab, Unread};

    fn view(id: &str, title: &str) -> TabView {
        TabView {
            tab: SessionTab { session_id: id.to_owned(), title: Some(title.to_owned()) },
            status: TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY },
            detail: Some("project ⎇ main".to_owned()),
        }
    }

    fn styles() -> ThemeStyles {
        crate::theme::ThemeState::v2_dark().styles()
    }

    /// `sessionTabsFitVertically`: 42+64 = 106 is the split floor.
    #[test]
    fn fit_and_clamp_match_the_reference_math() {
        assert!(session_tabs_fit_vertically(106, 42));
        assert!(!session_tabs_fit_vertically(105, 42));
        assert_eq!(clamp_session_tabs_width(42, 120), 42);
        assert_eq!(clamp_session_tabs_width(200, 120), 72, "capped at 72 and total − 44");
        assert_eq!(clamp_session_tabs_width(2, 120), SESSION_TABS_COMPACT_WIDTH);
    }

    /// The non-compact geometry: pad row, 2-row items with 1-row
    /// gaps, the add slot after the last gap.
    #[test]
    fn noncompact_geometry_and_hits() {
        let tabs = [view("a", "Alpha"), view("b", "Beta")];
        let geometry = RailGeometry::new(tabs.len(), 42);
        assert!(!geometry.compact);
        assert_eq!(geometry.stride, 3);
        assert_eq!(geometry.number_width, 2);
        assert_eq!(geometry.item_row(0), 1);
        assert_eq!(geometry.item_row(1), 4);
        assert_eq!(geometry.add_row, 7);
        assert_eq!(rail_hit(&tabs, &geometry, 3, 1, false), RailHit::Tab("a".into()));
        assert_eq!(rail_hit(&tabs, &geometry, 3, 2, false), RailHit::Tab("a".into()));
        assert_eq!(rail_hit(&tabs, &geometry, 3, 4, false), RailHit::Tab("b".into()));
        assert_eq!(rail_hit(&tabs, &geometry, 1, 7, false), RailHit::Add);
        assert_eq!(rail_hit(&tabs, &geometry, 1, 0, false), RailHit::Outside);
        assert_eq!(
            rail_hit(&tabs, &geometry, 3, 7, true),
            RailHit::Outside,
            "home: the slot IS the new tab"
        );
        // The handle's two boundary columns.
        assert_eq!(rail_hit(&tabs, &geometry, 41, 1, false), RailHit::Handle);
        assert_eq!(rail_hit(&tabs, &geometry, 42, 1, false), RailHit::Handle);
    }

    /// The compact geometry (TD-25): controls row 1, seam row 2,
    /// items from row 3 at stride 2.
    #[test]
    fn compact_geometry_hits_and_search() {
        let tabs = [view("a", "Alpha")];
        let geometry = RailGeometry::new(1, 6);
        assert!(geometry.compact);
        assert_eq!(geometry.item_row(0), 3);
        assert_eq!(geometry.add_row, 5);
        assert_eq!(rail_hit(&tabs, &geometry, 2, 1, false), RailHit::Search);
        assert_eq!(rail_hit(&tabs, &geometry, 2, 3, false), RailHit::Tab("a".into()));
        assert_eq!(rail_hit(&tabs, &geometry, 2, 5, false), RailHit::Add);
    }

    /// The painted non-compact rail: the selected item's pill, the
    /// ` 1 Alpha` title row (number right-aligned in numberWidth +
    /// 1 pad), the detail row, the add slot.
    #[test]
    fn paints_noncompact_rows() {
        let tabs = [view("a", "Alpha")];
        let inputs = RailInputs { tabs: &tabs, active: Some("a"), ..RailInputs::default() };
        let geometry = RailGeometry::new(1, 42);
        let (frame, tooltip) = paint_vertical_rail_solid(&inputs, &geometry, 12, &styles());
        let grid = frame.text_grid();
        assert!(grid[0].trim().is_empty(), "the pad row is blank");
        assert!(
            grid[1].starts_with(" 1 Alpha"),
            "the title row: number at numberWidth − 1, title at numberWidth + 1: {:?}",
            grid[1]
        );
        assert!(
            grid[2].starts_with("   project"),
            "the detail row indents past the prefix: {:?}",
            grid[2]
        );
        assert!(grid[4].starts_with(" + New session"), "the add slot: {:?}", grid[4]);
        // The selected pill's background covers both item rows.
        assert_eq!(
            frame.cell(1, 0).bg,
            CellColor::from(crate::theme::ThemeState::v2_dark().styles().action_selected)
        );
        assert!(tooltip.is_none(), "no tooltip while non-compact");
    }

    /// The painted compact rail (TD-25): the ⌕ row, the ▄ seam while
    /// the row below is highlighted, the centered indicator, the
    /// centered add.
    #[test]
    fn paints_compact_controls_and_seam() {
        let tabs = [view("a", "Alpha")];
        let inputs = RailInputs { tabs: &tabs, active: Some("a"), ..RailInputs::default() };
        let geometry = RailGeometry::new(1, 6);
        let (frame, tooltip) = paint_vertical_rail_solid(&inputs, &geometry, 10, &styles());
        let grid = frame.text_grid();
        assert!(grid[1].contains('⌕'), "the search button: {:?}", grid[1]);
        assert_eq!(grid[2], "▄▄▄▄▄▄", "the highlighted seam row");
        assert!(grid[3].contains('•'), "the unread marker centers: {:?}", grid[3]);
        assert!(grid[5].contains('+'), "the centered add: {:?}", grid[5]);
        assert!(tooltip.is_none(), "no tooltip without a hover");
        // Unhighlighted seam: spaces.
        let inputs = RailInputs { tabs: &tabs, ..RailInputs::default() };
        let (frame, _) = paint_vertical_rail_solid(&inputs, &geometry, 10, &styles());
        assert_eq!(frame.text_grid()[2], "      ", "no highlight below the seam");
    }
}
