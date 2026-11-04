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
use super::toast::{toast_geometry, ToastOptions};
use crate::render::{Cell, CellColor, Frame};
use crate::wrap::display_width;
use oc_theme::types::ResolvedThemeTokens;

/// The panel's tree units (panel-local): the interior fill + the
/// content runs, CLIPPED at the right rail (atoms at panel-local
/// `x >= right` never composited in the old path — the col loop
/// stopped at `right`; the right-rail column itself is the post-seam
/// rail paint's).
fn toast_tree_units(
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) -> (usize, usize, usize, Vec<ChromeUnit>) {
    let border = feedback_color(feedback, toast.variant);
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let (_left, panel_width, _top) = toast_geometry(message_width, title_width, usize::MAX);
    let rows = super::toast::panel_rows_public(toast);
    let bg = theme.pane;
    let right = panel_width.saturating_sub(1);

    let mut units = vec![ChromeUnit {
        key: "toast.fill".to_owned(),
        left: 1,
        top: 0,
        body: ChromeBody::Fill { width: panel_width.saturating_sub(2), height: rows, color: bg },
    }];
    let content_x = 3; // rail + paddingLeft 2 (panel-local)
    let content_right = panel_width.saturating_sub(4); // last content column
    // Clip a chrome_cluster_units run at the right rail: atoms whose
    // first cell is at (or past) the rail column are dropped (the
    // old composite's col loop never took them; the rail paint owns
    // the column). A WIDE atom straddling the rail stays in the tree
    // (dropping it would lose the interior ink at its origin).
    let clip_rail = |units: Vec<ChromeUnit>| -> Vec<ChromeUnit> {
        units.into_iter().filter(|unit| unit.left < right).collect()
    };
    if let Some(title) = &toast.title {
        units.extend(clip_rail(chrome_cluster_units(
            "toast.title",
            content_x,
            1,
            title,
            theme.text,
            bg,
            1, // bold
            FILL_FG,
        )));
        // The dismiss affordance, right-aligned so the last cell
        // lands at content_right: two fill-fg pads + the muted `x`.
        let start = content_right.saturating_sub(2);
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
            body: ChromeBody::Text(vec![ChromeChunk::new("x", theme.muted, bg, 0)]),
        });
        units.extend(clip_rail(chrome_cluster_units(
            "toast.msg",
            content_x,
            3,
            &toast.message,
            theme.text,
            bg,
            0,
            FILL_FG,
        )));
    } else {
        units.extend(clip_rail(chrome_cluster_units(
            "toast.msg",
            content_x,
            1,
            &toast.message,
            theme.text,
            bg,
            0,
            FILL_FG,
        )));
    }
    if pending > 0 && toast.title.is_some() {
        units.extend(clip_rail(chrome_cluster_units(
            "toast.more",
            content_x,
            2,
            &format!("+{pending} more"),
            theme.muted,
            bg,
            0,
            FILL_FG,
        )));
    }
    (panel_width, right, rows, units)
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
    if let Some(title) = &toast.title {
        consider(title, content_x, 1, theme.text, 1);
        consider(&toast.message, content_x, 3, theme.text, 0);
    } else {
        consider(&toast.message, content_x, 1, theme.text, 0);
    }
    if pending > 0 && toast.title.is_some() {
        consider(&format!("+{pending} more"), content_x, 2, theme.muted, 0);
    }
    overhang
}

/// Paint the two `┃` rail columns over the composited frame (the
/// POST-SEAM half of the family, dialog-scrim paradigm: the rails
/// keep the UNDERLYING cell background — `rail_bg = None` — exactly
/// the old `overlay_toast_over_frame`; the headless blank twin passes
/// the old canvas surface's rail bg instead).
#[allow(clippy::too_many_arguments)] // the rail seam (toast/pending/theme/geometry/bg)
pub fn paint_toast_rails(
    frame: &mut Frame,
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
    rail_bg: Option<CellColor>,
) {
    let border = feedback_color(feedback, toast.variant);
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let term_width = frame.width;
    let (left, panel_width, top) = toast_geometry(message_width, title_width, term_width);
    let right = left + panel_width - 1;
    let rows = super::toast::panel_rows_public(toast);
    for row in top..top + rows {
        // The left rail: nothing overwrites column 0 (content starts
        // at 3, the fill at 1) — always the plain rail.
        if let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(left)) {
            let bg = rail_bg.unwrap_or(slot.bg);
            *slot = Cell { ch: '┃', attrs: 0, fg: border, bg };
        }
        if let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(right)) {
            let bg = rail_bg.unwrap_or(slot.bg);
            let (ch, fg, attrs) = right_rail_cell(toast, pending, theme, panel_width - 1, row - top)
                .unwrap_or(('┃', border, 0));
            *slot = Cell { ch, attrs, fg, bg };
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
) {
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let (left, panel_width, top) = toast_geometry(message_width, title_width, term_width);
    let (_panel_width, _right, rows, units) = toast_tree_units(toast, pending, theme, feedback);
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
    mount_toast_tree(toast, pending, frame.width, theme, feedback);
    composite_chrome_tree(frame);
    paint_toast_rails(frame, toast, pending, theme, feedback, None);
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
    paint_toast_rails(&mut frame, toast, pending, theme, feedback, Some(theme.surface));
    frame
}
