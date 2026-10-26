//! Lane R1 — the toast surface on the shared opentui-solid chrome
//! bridge ([`crate::components::chrome_solid`]): the bordered panel
//! (variant-colored `┃` rails over the raised-high interior, the
//! title row + dismiss `x`, the message, `+N more`) rendered as a
//! `panel_width × rows` chrome mount PASTED into the footer Canvas
//! frame at the shared [`super::toast::toast_geometry`] origin — so
//! the composite ([`super::toast::overlay_toast_over_frame`], whose
//! rails keep the UNDERLYING cell background) consumes the same
//! absolute-cell surface shape. The parity gate pins the panel frame
//! cell-for-cell against [`super::toast::render_toast_over_blank`].

use super::chrome_solid::{
    chrome_cluster_units, chrome_frame, ChromeBody, ChromeChunk, ChromeUnit,
};
use super::footer::canvas::{Canvas, FILL_FG};
use super::footer::theme::{feedback_color, FooterTheme};
use super::toast::{toast_geometry, ToastOptions};
use crate::render::{CellColor, Frame};
use crate::wrap::display_width;
use oc_theme::types::ResolvedThemeTokens;

/// The new-path twin of [`super::toast::render_toast_over_blank`].
#[must_use]
pub fn render_toast_over_blank_solid(
    toast: &ToastOptions,
    pending: usize,
    term_width: usize,
    term_height: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) -> Frame {
    let border = feedback_color(feedback, toast.variant);
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let (left, panel_width, top) = toast_geometry(message_width, title_width, term_width);
    let rows = super::toast::panel_rows_public(toast);
    let bg = theme.pane;

    // The panel mount (panel-local coordinates): the interior fill
    // (the Canvas prefill's white fg + the pane bg), the two rails,
    // then the content rows.
    let mut units = vec![ChromeUnit {
        key: "toast.fill".to_owned(),
        left: 1,
        top: 0,
        body: ChromeBody::Fill { width: panel_width.saturating_sub(2), height: rows, color: bg },
    }];
    for row in 0..rows {
        for (key, x) in [("l", 0usize), ("r", panel_width.saturating_sub(1))] {
            units.push(ChromeUnit {
                key: format!("toast.rail.{key}@{row}"),
                left: x,
                top: row,
                body: ChromeBody::Text(vec![ChromeChunk::new("┃", border, theme.surface, 0)]),
            });
        }
    }
    let content_x = 3; // rail + paddingLeft 2 (panel-local)
    let content_right = panel_width.saturating_sub(4); // last content column
    if let Some(title) = &toast.title {
        units.extend(chrome_cluster_units(
            "toast.title",
            content_x,
            1,
            title,
            theme.text,
            bg,
            1, // bold
            FILL_FG,
        ));
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
        units.extend(chrome_cluster_units(
            "toast.msg",
            content_x,
            3,
            &toast.message,
            theme.text,
            bg,
            0,
            FILL_FG,
        ));
    } else {
        units.extend(chrome_cluster_units(
            "toast.msg",
            content_x,
            1,
            &toast.message,
            theme.text,
            bg,
            0,
            FILL_FG,
        ));
    }
    if pending > 0 && toast.title.is_some() {
        units.extend(chrome_cluster_units(
            "toast.more",
            content_x,
            2,
            &format!("+{pending} more"),
            theme.muted,
            bg,
            0,
            FILL_FG,
        ));
    }
    // The mount is as wide as the longest unclamped run: the old
    // canvas `spans` never truncate, so a clamped-geometry panel can
    // still carry glyphs past its right rail (paint order — the
    // message overwrites the rail cell).
    let mount_width = panel_width.max(content_x + message_width).max(content_x + title_width);
    let panel =
        chrome_frame("toast_panel", mount_width.max(1), rows.max(1), CellColor::Default, units);

    // Paste into the Canvas frame (the black prefill outside the
    // panel is the golden's own backdrop).
    let mut canvas = Canvas::new(term_width, term_height);
    {
        let frame = canvas.frame_mut();
        for row in 0..rows.min(term_height.saturating_sub(top)) {
            for col in 0..mount_width.min(term_width.saturating_sub(left)) {
                // Skip the mount's untouched cells (the wide-mount
                // tail past the panel): the canvas prefill shows.
                let cell = panel.cell(row, col);
                if cell.ch == ' '
                    && cell.attrs == 0
                    && cell.fg == CellColor::Default
                    && cell.bg == CellColor::Default
                {
                    continue;
                }
                if let Some(slot) =
                    frame.cells.get_mut(top + row).and_then(|line| line.get_mut(left + col))
                {
                    *slot = cell;
                }
            }
        }
    }
    if pending > 0 && toast.title.is_none() {
        // On the no-title panel `+N more` lands at top+2, one row
        // BELOW the panel (over the canvas, bg pane) — painted as
        // span cells directly, exactly the old `canvas.spans` call.
        let more = format!("+{pending} more");
        let mut x = left + content_x;
        for atom in crate::wrap::flatten(&more, ratatui::style::Style::new()) {
            if let Some(slot) = canvas.frame_mut().cells.get_mut(top + 2).and_then(|l| l.get_mut(x))
            {
                *slot = crate::render::Cell {
                    ch: atom.text.chars().next().unwrap_or(' '),
                    attrs: 0,
                    fg: theme.muted,
                    bg,
                };
            }
            x += atom.width.max(1);
        }
    }
    canvas.into_frame()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn footer() -> (FooterTheme, ResolvedThemeTokens) {
        let state = ThemeState::v2_dark();
        (FooterTheme::from_resolved(&state.resolved.tokens), state.resolved.tokens)
    }
}
