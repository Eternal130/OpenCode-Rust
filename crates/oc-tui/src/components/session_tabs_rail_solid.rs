//! Lane R1 — the vertical session-tab rail (+ its compact hover
//! tooltip) on the shared opentui-solid chrome bridge
//! ([`crate::components::chrome_solid`]): the M4/M5 flat-units shape
//! over the SHARED [`super::session_tabs_rail::rail_paint_list`] walk
//! (extracted by this lane — both renderers consume the one solver,
//! so the geometry can never drift). The base fill (white fg over
//! raised) is the old `paint_placed_with_fills` base style; every
//! walk fill becomes a chrome fill (the same white-fg signature);
//! every placed line becomes an absolutely-positioned text unit keyed
//! by the M5 position-stable rule (tag + row). The resize handle's
//! highlight stays the shared bridge recolor
//! ([`super::session_tabs_rail::paint_rail_handle_hot`]).
//!
//! Cluster semantics match natively: the walk's `paint_run` plants
//! `'\0'` continuation cells carrying the run's style — the raster
//! does exactly that (lane M4's finding), so a wide title's cells are
//! byte-identical through both paths.

use super::chrome_solid::{chrome_frame, ChromeBody, ChromeChunk, ChromeUnit};
use super::session_tabs_rail::{
    paint_rail_handle_hot, rail_paint_list, RailGeometry, RailInputs, TOOLTIP_MAX_WIDTH,
};
use crate::layout::Placed;
use crate::render::Fill;
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// ratatui modifier bits → the oc-tui ECMA-ordered word (1:1 — the
/// rail carries BOLD only).
fn attrs_of(style: ratatui::style::Style) -> u16 {
    use ratatui::style::Modifier;
    let mut bits = 0u16;
    let modifier = style.add_modifier;
    if modifier.contains(Modifier::BOLD) {
        bits |= 1 << 0;
    }
    if modifier.contains(Modifier::DIM) {
        bits |= 1 << 1;
    }
    if modifier.contains(Modifier::ITALIC) {
        bits |= 1 << 2;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        bits |= 1 << 3;
    }
    if modifier.intersects(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        bits |= 1 << 4;
    }
    if modifier.contains(Modifier::REVERSED) {
        bits |= 1 << 5;
    }
    if modifier.contains(Modifier::HIDDEN) {
        bits |= 1 << 6;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        bits |= 1 << 7;
    }
    bits
}

fn color_of(color: Option<ratatui::style::Color>) -> CellColor {
    color.map_or(CellColor::Default, CellColor::from)
}

/// The M5 position-stable key: the walk's leaf tag (`session-tab:{id}`,
/// `session-tabs.add`, `rail.seam`, …) or `u`, plus the placed ROW —
/// a same-key collision falls back to an index suffix.
fn placed_key(
    placed: &Placed,
    y: usize,
    index: usize,
    seen: &mut std::collections::HashSet<String>,
) -> String {
    let base = match placed.tag.as_deref() {
        Some(tag) => format!("t:{tag}@{y}"),
        None => format!("u@{y}"),
    };
    if seen.insert(base.clone()) {
        base
    } else {
        format!("{base}#{index}")
    }
}

/// Shape the shared walk's output as chrome units — the base fill,
/// then the walk's fills, then the placed lines (the paint order:
/// fills before any text, later rows overwrite earlier half-rows).
fn shape(
    placed: &[Placed],
    fills: &[Fill],
    width: usize,
    height: usize,
    styles: &ThemeStyles,
) -> Vec<ChromeUnit> {
    let raised = CellColor::from(styles.background_raised);
    let mut units = vec![ChromeUnit {
        key: "rail.base".to_owned(),
        left: 0,
        top: 0,
        body: ChromeBody::Fill { width, height, color: raised },
    }];
    for (index, fill) in fills.iter().enumerate() {
        units.push(ChromeUnit {
            key: format!("rail.fill@{}#{index}", fill.y),
            left: fill.x as usize,
            top: fill.y as usize,
            body: ChromeBody::Fill {
                width: fill.width as usize,
                height: fill.height as usize,
                color: color_of(fill.style.bg),
            },
        });
    }
    let mut seen = std::collections::HashSet::new();
    for (index, item) in placed.iter().enumerate() {
        for (line_index, line) in item.lines.iter().enumerate() {
            let y = item.y + line_index;
            let chunks: Vec<ChromeChunk> = line
                .runs
                .iter()
                .map(|run| {
                    let style = run.style;
                    ChromeChunk {
                        text: run.text.clone(),
                        fg: color_of(style.fg),
                        bg: color_of(style.bg),
                        attrs: attrs_of(style),
                    }
                })
                .collect();
            if chunks.is_empty() {
                continue;
            }
            units.push(ChromeUnit {
                key: placed_key(item, y, index, &mut seen),
                left: item.x,
                top: y,
                body: ChromeBody::Text(chunks),
            });
        }
    }
    units
}

/// The solid rail — the new-path twin of
/// [`super::session_tabs_rail::paint_vertical_rail`].
#[must_use]
pub fn paint_vertical_rail_solid(
    inputs: &RailInputs,
    geometry: &RailGeometry,
    height: usize,
    styles: &ThemeStyles,
) -> (Frame, Option<(usize, Frame)>) {
    let (placed, fills) = rail_paint_list(inputs, geometry, styles);
    let units = shape(&placed, &fills, geometry.width, height, styles);
    let mut frame =
        chrome_frame("tab_rail", geometry.width.max(1), height.max(1), CellColor::Default, units);
    paint_rail_handle_hot(&mut frame, inputs, geometry, styles);
    // The compact hover tooltip (the solid twin of the shared
    // `paint_rail_tooltip`'s row math).
    let tooltip = if let (true, Some(view)) = (geometry.compact, inputs.tooltip) {
        let index = inputs
            .tabs
            .iter()
            .position(|tab| tab.tab.session_id == view.tab.session_id)
            .unwrap_or(0);
        let row = geometry.item_row(index).saturating_sub(1);
        Some((row, paint_tooltip_solid(view, styles)))
    } else {
        None
    };
    (frame, tooltip)
}

/// The solid tooltip — the new-path twin of the walk's
/// `paint_tooltip` (4 rows: seam, title, detail, seam).
#[must_use]
pub fn paint_tooltip_solid(
    view: &crate::components::session_tabs::TabView,
    styles: &ThemeStyles,
) -> Frame {
    let width = TOOLTIP_MAX_WIDTH;
    let raised = CellColor::from(styles.background_raised);
    // The tooltip's own walk: the two seam half-rows + the
    // title/detail rows (session-tabs.tsx:1240-1280).
    let title_budget = width.saturating_sub(2).max(1);
    let title = view.tab.title.clone().unwrap_or_else(|| "Untitled session".to_owned());
    let title = crate::wrap::take_width(&title, title_budget);
    let text =
        CellColor::from(styles.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND));
    let muted = CellColor::from(
        styles.text_muted.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND),
    );
    let hover = CellColor::from(super::session_tabs_rail::hover_color_public(styles));
    let mut units = vec![
        ChromeUnit {
            key: "tt.base".to_owned(),
            left: 0,
            top: 0,
            body: ChromeBody::Fill { width, height: 4, color: raised },
        },
        ChromeUnit {
            key: "tt.seam.t".to_owned(),
            left: 0,
            top: 0,
            body: ChromeBody::Text(vec![ChromeChunk::new("▄".repeat(width), hover, raised, 0)]),
        },
    ];
    units.push(ChromeUnit {
        key: "tt.title@1".to_owned(),
        left: 1,
        top: 1,
        body: ChromeBody::Text(vec![ChromeChunk::new(title, text, raised, 0)]),
    });
    if let Some(detail) = &view.detail {
        let detail = crate::wrap::take_width(detail, title_budget);
        if !detail.is_empty() {
            units.push(ChromeUnit {
                key: "tt.detail@2".to_owned(),
                left: 1,
                top: 2,
                body: ChromeBody::Text(vec![ChromeChunk::new(detail, muted, raised, 0)]),
            });
        }
    }
    units.push(ChromeUnit {
        key: "tt.seam.b".to_owned(),
        left: 0,
        top: 3,
        body: ChromeBody::Text(vec![ChromeChunk::new("▀".repeat(width), hover, raised, 0)]),
    });
    chrome_frame("tab_rail_tooltip", width, 4, CellColor::Default, units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::session_tabs::TabView;
    use crate::store::{SessionTab, Unread};
    use crate::theme::ThemeState;

    fn view(id: &str, title: &str) -> TabView {
        TabView {
            tab: SessionTab { session_id: id.to_owned(), title: Some(title.to_owned()) },
            status: crate::components::session_tabs::TabStatus {
                unread: Some(Unread::Activity),
                ..crate::components::session_tabs::TabStatus::EMPTY
            },
            detail: Some("project ⎇ main".to_owned()),
        }
    }

    fn styles() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }
}
