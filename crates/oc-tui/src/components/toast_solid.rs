//! The toast surface on the shared tree (D2.2 overlays lane): the
//! bordered panel (variant-colored `┃` rails over the raised-high
//! interior, the title row + dismiss `x`, the message, `+N more`)
//! mounted as the `surface-toast` family wrapper at skeleton index 8
//! — the panel INTERIOR rides the tree; the two `┃` rails stay a
//! POST-SEAM frame paint (they keep the UNDERLYING cell background —
//! a tree cell cannot: the walk writes opaque backgrounds, and a
//! hole skips the glyph entirely). The rails' glyphs are derived
//! from the same atom walk the shaping uses, so a clamped run that
//! overhangs the right rail reproduces the old composite's
//! rail-column byte (`Cell { bg: slot.bg, ..overlay }`).
//!
//! Parity gates pin the composited frames cell-for-cell against the
//! old painters ([`super::toast::overlay_toast_over_frame`] /
//! `render_toast_over_blank_solid`, both retired with this lane).

use super::chrome_solid::{
    blank_frame, chrome_cluster_units, composite_chrome_tree, reset_hub, ChromeBody, ChromeChunk,
    ChromeUnit,
};
use super::footer::canvas::FILL_FG;
use super::footer::theme::{feedback_color, FooterTheme};
use super::overlays_solid::{mount_overlay_surface, OVERLAYS_TOAST};
use super::toast::{toast_layout, ToastLayout, ToastOptions};
use crate::render::{Cell, CellColor, Frame};
use oc_theme::types::ResolvedThemeTokens;

/// The panel's tree units (panel-local): the interior fill + the
/// content runs, CLIPPED at the right rail (atoms at panel-local
/// `x >= right` never composited in the old path — the col loop
/// stopped at `right`; the right-rail column itself is the post-seam
/// rail paint's).
#[allow(clippy::too_many_lines)] // the G-424b wrap engine + dialog cover clip (1:1 ref port)
fn toast_tree_units(
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
    layout: &ToastLayout,
    cover: Option<ToastRailCover>,
) -> (usize, usize, usize, Vec<ChromeUnit>) {
    let _border = feedback_color(feedback, toast.variant);
    let rows = layout.rows;
    let bg = theme.pane;
    let right = layout.panel_width.saturating_sub(1);

    // The dialog cover (the ref's z-3000 dialog box over the toast):
    // a cell inside the box's rect never paints — interior units drop
    // and the fill splits into bands around the covered rows.
    let covered = |x: usize, y: usize| -> bool {
        matches!(cover, Some(ToastRailCover { rect, .. })
            if layout.top + y >= rect.1
                && layout.top + y < rect.1 + rect.3
                && layout.left + x >= rect.0
                && layout.left + x < rect.0 + rect.2)
    };
    let row_covered = |y: usize| -> bool {
        // Any covered cell in the row (the interior spans [1,
        // panel_width-1); the fill spans [1, panel_width-2)).
        (1..layout.panel_width).any(|x| covered(x, y))
    };
    let mut units: Vec<ChromeUnit> = Vec::new();
    let mut band_start = 0usize;
    for y in 0..=rows {
        let band_break = y == rows || row_covered(y);
        if band_break && y > band_start {
            units.push(ChromeUnit {
                key: "toast.fill".to_owned(),
                left: 1,
                top: band_start,
                body: ChromeBody::Fill {
                    width: layout.panel_width.saturating_sub(2),
                    height: y - band_start,
                    color: bg,
                },
            });
        }
        if band_break {
            band_start = y + 1;
        }
    }
    let content_x = 3; // rail + paddingLeft 2 (panel-local)
    let content_right = layout.panel_width.saturating_sub(4); // last content column
                                                              // Clip a chrome_cluster_units run at the right rail: atoms whose
                                                              // first cell is at (or past) the rail column are dropped (the
                                                              // old composite's col loop never took them; the rail paint owns
                                                              // the column). A WIDE atom straddling the rail stays in the tree
                                                              // (dropping it would lose the interior ink at its origin).
    let clip_rail = |units: Vec<ChromeUnit>| -> Vec<ChromeUnit> {
        units.into_iter().filter(|unit| unit.left < right).collect()
    };
    // A content unit survives only when its whole leading cell row is
    // uncovered (the ref's dialog box is opaque).
    let keep = |unit: &ChromeUnit| -> bool {
        let y = unit.top;
        match &unit.body {
            ChromeBody::Text(_) => !row_covered(y),
            _ => true,
        }
    };
    if let Some(_title) = &toast.title {
        // G-424b — the title shrinks and word-wraps inside the
        // clamped panel (ui/toast.tsx:83-89): one run per wrapped
        // line, the affordance right-aligned on the FIRST line.
        for (index, line) in layout.title_lines.iter().enumerate() {
            units.extend(
                clip_rail(chrome_cluster_units(
                    "toast.title",
                    content_x,
                    1 + index,
                    line,
                    theme.text,
                    bg,
                    1, // bold
                    FILL_FG,
                ))
                .into_iter()
                .filter(keep),
            );
        }
        // The affordance, right-aligned so the last cell lands at
        // content_right: two fill-fg pads + the muted run — `x` for a
        // plain toast, `› {label}` for an action toast (G-424,
        // ui/toast.tsx:75-81, fg muted save hover).
        let aff_text = match &toast.action_label {
            Some(label) => format!("› {label}"),
            None => "x".to_owned(),
        };
        let aff_width = 2 + crate::wrap::display_width(&aff_text);
        let start = content_right + 1 - aff_width;
        if !row_covered(1) {
            units.push(ChromeUnit {
                key: "toast.aff.pad".to_owned(),
                left: start,
                top: 1,
                body: ChromeBody::Text(vec![ChromeChunk::new("  ", FILL_FG, bg, 0)]),
            });
            units.push(ChromeUnit {
                key: "toast.aff.x".to_owned(),
                left: start + 2,
                top: 1,
                body: ChromeBody::Text(vec![ChromeChunk::new(&aff_text, theme.muted, bg, 0)]),
            });
        }
        let message_top = 1 + layout.title_lines.len() + 1; // marginBottom 1
        for (index, line) in layout.message_lines.iter().enumerate() {
            units.extend(
                clip_rail(chrome_cluster_units(
                    "toast.msg",
                    content_x,
                    message_top + index,
                    line,
                    theme.text,
                    bg,
                    0,
                    FILL_FG,
                ))
                .into_iter()
                .filter(keep),
            );
        }
    } else {
        for (index, line) in layout.message_lines.iter().enumerate() {
            units.extend(
                clip_rail(chrome_cluster_units(
                    "toast.msg",
                    content_x,
                    1 + index,
                    line,
                    theme.text,
                    bg,
                    0,
                    FILL_FG,
                ))
                .into_iter()
                .filter(keep),
            );
        }
    }
    if let Some(more_row) = layout.pending_row {
        units.extend(
            clip_rail(chrome_cluster_units(
                "toast.more",
                content_x,
                more_row,
                &format!("+{pending} more"),
                theme.muted,
                bg,
                0,
                FILL_FG,
            ))
            .into_iter()
            .filter(keep),
        );
    }
    (layout.panel_width, right, rows, units)
}

/// The right-rail column's expected cell for one row: `None` = the
/// plain `┃` rail; `Some((ch, fg, attrs))` = a content run's atom
/// overhanging the rail (the old surface painted it there and the
/// composite took it with the underlying bg). Paint order:
/// title → affordance → message → `+N more` (last wins).
fn right_rail_cell(
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    right: usize,
    row: usize,
) -> Option<(char, CellColor, u16)> {
    let layout = toast_layout(toast, pending, usize::MAX);
    let mut overhang: Option<(char, CellColor, u16)> = None;
    let mut consider = |text: &str, x0: usize, run_row: usize, fg: CellColor, attrs: u16| {
        if run_row != row {
            return;
        }
        let mut x = x0;
        for atom in crate::wrap::flatten(text, crate::style::Style::new()) {
            if x == right {
                overhang = Some((atom.text.chars().next().unwrap_or(' '), fg, attrs));
            }
            x += atom.width.max(1);
        }
    };
    let content_x = 3;
    if let Some(_title) = &toast.title {
        for (index, line) in layout.title_lines.iter().enumerate() {
            consider(line, content_x, 1 + index, theme.text, 1);
        }
        let message_top = 1 + layout.title_lines.len() + 1;
        for (index, line) in layout.message_lines.iter().enumerate() {
            consider(line, content_x, message_top + index, theme.text, 0);
        }
    } else {
        for (index, line) in layout.message_lines.iter().enumerate() {
            consider(line, content_x, 1 + index, theme.text, 0);
        }
    }
    if let Some(more_row) = layout.pending_row {
        consider(&format!("+{pending} more"), content_x, more_row, theme.muted, 0);
    }
    overhang
}

/// The live dialog's cover over the rails (the G-424b lane): rail
/// cells inside the dialog box's rect never paint (the ref's
/// z-3000 box covers the toast), and a dimming backdrop dims the
/// rail glyph's fg with the same blend the scrim applies (the rail
/// keeps its already-scrimmed underlying bg).
#[derive(Clone, Copy)]
pub struct ToastRailCover {
    pub rect: (usize, usize, usize, usize),
    pub dim: bool,
}

/// Paint the two `┃` rail columns over the composited frame (the
/// POST-SEAM half of the family, dialog-scrim paradigm: the rails
/// keep the UNDERLYING cell background — `rail_bg = None` — exactly
/// the old `overlay_toast_over_frame`; the headless blank twin passes
/// the old canvas surface's rail bg instead).
#[allow(clippy::too_many_arguments)] // the rail seam (toast/pending/theme/geometry/bg/cover)
pub fn paint_toast_rails(
    frame: &mut Frame,
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
    rail_bg: Option<CellColor>,
    cover: Option<ToastRailCover>,
) {
    let border = feedback_color(feedback, toast.variant);
    let term_width = frame.width;
    let layout = toast_layout(toast, pending, term_width);
    let (left, panel_width, top) = (layout.left, layout.panel_width, layout.top);
    let right = left + panel_width - 1;
    let rows = layout.rows;
    let covered = |x: usize, y: usize| -> bool {
        matches!(cover, Some(ToastRailCover { rect, .. })
                if y >= rect.1 && y < rect.1 + rect.3 && x >= rect.0 && x < rect.0 + rect.2)
    };
    for row in top..top + rows {
        // The left rail: nothing overwrites column 0 (content starts
        // at 3, the fill at 1) — always the plain rail.
        if !covered(left, row) {
            if let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(left)) {
                let bg = rail_bg.unwrap_or(slot.bg);
                let fg = match cover {
                    Some(ToastRailCover { dim: true, .. }) => {
                        crate::components::dialog::blend_backdrop_fg(border)
                    }
                    _ => border,
                };
                *slot = Cell { ch: '┃', attrs: 0, fg, bg };
            }
        }
        if !covered(right, row) {
            if let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(right)) {
                let bg = rail_bg.unwrap_or(slot.bg);
                let (ch, fg, attrs) =
                    right_rail_cell(toast, pending, theme, panel_width - 1, row - top)
                        .unwrap_or(('┃', border, 0));
                let fg = match cover {
                    Some(ToastRailCover { dim: true, .. }) => {
                        crate::components::dialog::blend_backdrop_fg(fg)
                    }
                    _ => fg,
                };
                *slot = Cell { ch, attrs, fg, bg };
            }
        }
    }
}

/// Mount the toast panel interior into the shared tree (the live
/// path — called from `render_app` before the compositor seam; the
/// rails are [`paint_toast_rails`], the post-seam half).
pub fn mount_toast_tree(
    toast: &ToastOptions,
    pending: usize,
    term_width: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
    cover: Option<ToastRailCover>,
) {
    let layout = toast_layout(toast, pending, term_width);
    let (left, panel_width, top) = (layout.left, layout.panel_width, layout.top);
    let (_panel_width, _right, rows, units) =
        toast_tree_units(toast, pending, theme, feedback, &layout, cover);
    mount_overlay_surface(
        "toast",
        OVERLAYS_TOAST,
        left,
        top,
        panel_width,
        rows,
        &units,
        false,
        &[],
    );
}

/// The full-frame headless twin (the golden fixtures' shape): mount
/// the panel over a fresh hub, composite the seam over `frame`, then
/// the rails — the bytes the old `overlay_toast_over_frame`
/// composited, by construction.
pub fn composite_toast_over_frame(
    frame: &mut Frame,
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) {
    reset_hub();
    mount_toast_tree(toast, pending, frame.width, theme, feedback, None);
    composite_chrome_tree(frame);
    paint_toast_rails(frame, toast, pending, theme, feedback, None, None);
}

/// The blank-canvas headless twin (the frame-budget instrument and
/// the unit tests' directly-constructible surface): the panel over a
/// blank frame, the rails at the old blank surface's own rail bg.
#[must_use]
pub fn toast_over_blank_frame(
    toast: &ToastOptions,
    pending: usize,
    term_width: usize,
    term_height: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) -> Frame {
    let mut frame = blank_frame(term_width, term_height);
    composite_toast_over_frame(&mut frame, toast, pending, theme, feedback);
    paint_toast_rails(&mut frame, toast, pending, theme, feedback, Some(theme.surface), None);
    frame
}
