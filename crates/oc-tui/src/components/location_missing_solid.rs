//! Lane R1 — the session-location recovery screen on the shared
//! opentui-solid chrome bridge ([`crate::components::chrome_solid`]):
//! the `▎` rail, the warning header, the truncated directory +
//! guidance body and the reverse-video `Choose directory` pill as
//! absolute text units over a default (transparent) background. The
//! parity gate pins the solid frame cell-for-cell against the old
//! painter ([`super::location_missing::render_location_missing`]).

use super::chrome_solid::{chrome_frame, chrome_run_units, first_divergence, ChromeUnit};
use crate::render::CellColor;
use crate::theme::ThemeStyles;

/// The new-path twin of [`super::location_missing::render_location_missing`].
#[must_use]
pub fn render_location_missing_solid(
    directory: &str,
    styles: &ThemeStyles,
    width: usize,
) -> crate::render::Frame {
    let width = width.max(4);
    let rows = super::location_missing::LOCATION_MISSING_ROWS;
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let warning = CellColor::from(styles.warning);
    let accent = CellColor::from(styles.action_hovered);
    let reverse = 1u16 << 7; // the pill's raised background

    // Clip to the old `put` bound: writes stop at column width − 1.
    let clip = |text: &str, x: usize| -> String {
        let max = width.saturating_sub(1).saturating_sub(x);
        text.chars().take(max).collect()
    };

    let mut units = Vec::new();
    // The left rail (SessionQuestion's `border: ["left"]`).
    for row in 0..rows {
        units.extend(chrome_run_units("location.rail", 0, row, "▎", accent, CellColor::Default, 0));
    }
    let mut line =
        |units: &mut Vec<ChromeUnit>, key: &str, row: usize, x: usize, body: &str, fg| {
            units.extend(chrome_run_units(key, x, row, &clip(body, x), fg, CellColor::Default, 0));
        };
    line(&mut units, "location.header.mark", 0, 2, "△", warning);
    line(&mut units, "location.header", 0, 4, "Session location unavailable", text);
    let home = std::env::var("HOME").unwrap_or_default();
    line(
        &mut units,
        "location.directory",
        1,
        2,
        &super::location_missing::display_directory(directory, &home),
        muted,
    );
    line(
        &mut units,
        "location.guidance",
        2,
        2,
        "Choose another directory to continue this session.",
        text,
    );
    line(&mut units, "location.label", 4, 2, "Recovery actions", muted);
    // The pill: the attrs band spans cols [2, 19) — a Spaces stamp
    // (fg Default, reverse) under the clipped `Choose directory`
    // glyphs (fg text, reverse) — the band covers the cells the
    // width-4 clip leaves glyph-less.
    let button = "Choose directory";
    units.extend(chrome_run_units(
        "location.pill.pad",
        2,
        5,
        " ",
        CellColor::Default,
        CellColor::Default,
        reverse,
    ));
    units.extend(chrome_run_units(
        "location.pill",
        3,
        5,
        &clip(button, 3),
        text,
        CellColor::Default,
        reverse,
    ));
    let shown = clip(button, 3).chars().count();
    let band = 19usize.saturating_sub(3 + shown);
    if band > 0 {
        units.extend(chrome_run_units(
            "location.pill.tail",
            3 + clip(button, 3).chars().count(),
            5,
            &" ".repeat(band),
            CellColor::Default,
            CellColor::Default,
            reverse,
        ));
    }
    chrome_frame("location_missing", width, rows, CellColor::Default, units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::location_missing::render_location_missing;
    use crate::theme::ThemeState;

    fn styles() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// The lane gate: the solid mount equals the old painter CELL
    /// FOR CELL over the width matrix (the header/body/pill rows and
    /// the directory truncation budgets — the truncateMiddle(…, 72)
    /// overflow needs width > 73 to exercise).
    #[test]
    fn solid_location_missing_equals_the_old_renderer() {
        let long = format!("/base/{}", "x".repeat(120));
        for directory in ["/gone/dir", "/home/u/work", &long] {
            for width in [40usize, 60, 80, 120, 4, 6] {
                let old = render_location_missing(directory, &styles(), width);
                let solid = render_location_missing_solid(directory, &styles(), width);
                if let Some((row, col, why)) = first_divergence(&solid, &old) {
                    panic!("dir={directory} width={width} at {row},{col}: {why}");
                }
            }
        }
    }
}
