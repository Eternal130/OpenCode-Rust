//! Lane R1 — the app-shell state overlays on the shared
//! opentui-solid chrome bridge ([`crate::components::chrome_solid`]):
//!
//! * **Reconnecting** — the scrim blend stays at the bridge (the M2
//!   backdrop rule: the alpha math runs over the base frame); the
//!   centered 48-wide raised BOX is a chrome mount (fg-base-on-raised
//!   interior + the spinner headline + the muted copy) pasted into
//!   the dimmed frame.
//! * **`StartupLoading`** — the bottom-centered pill as ONE chunk
//!   (` {spinner} {label} ` — every cell the same muted-on-raised
//!   pair, exactly the old row write).
//! * **The migration widget** — the top-right raised box with the
//!   `▎` info rail + the spinner/label/counters row as a chrome
//!   mount pasted above every other surface.
//!
//! Parity gates pin the composited frames cell-for-cell against the
//! old painters ([`super::overlays`] + [`super::migration_overlay`]).

use super::chrome_solid::{chrome_frame, chrome_run_units, ChromeBody, ChromeChunk, ChromeUnit};
use super::migration_overlay::{progress_count, MigrationProgress};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// `SPINNER_FRAMES` (component/spinner, 80 ms) — shared with the old
/// painters; read through the same table.
fn spinner_frame(elapsed_ms: u64) -> char {
    const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    SPINNER_FRAMES[((elapsed_ms / 80) % SPINNER_FRAMES.len() as u64) as usize]
}

/// The reconnecting box mount — the new-path twin of the old
/// `overlay_reconnecting`'s box pass + copy rows.
#[must_use]
fn reconnecting_box(box_width: usize, styles: &ThemeStyles, elapsed_ms: u64) -> Frame {
    let raised = CellColor::from(styles.background_raised);
    let fg_base = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let mut units = Vec::new();
    for row in 0..4 {
        units.push(ChromeUnit {
            key: format!("recon.bg@{row}"),
            left: 0,
            top: row,
            body: ChromeBody::Text(vec![ChromeChunk::new(
                " ".repeat(box_width),
                fg_base,
                raised,
                0,
            )]),
        });
    }
    units.extend(chrome_run_units(
        "recon.head",
        2,
        1,
        &format!("{} Connection lost…", spinner_frame(elapsed_ms)),
        fg_base,
        raised,
        0,
    ));
    units.extend(chrome_run_units(
        "recon.copy",
        2,
        2,
        "Reconnecting to the server automatically.",
        fg_muted,
        raised,
        0,
    ));
    chrome_frame("reconnecting_box", box_width.max(1), 4, CellColor::Default, units)
}

/// The reconnecting overlay: the scrim blend (bridge math) + the
/// solid box composite. The new-path twin of
/// [`super::overlays::overlay_reconnecting`].
pub fn overlay_reconnecting_solid(frame: &mut Frame, styles: &ThemeStyles, elapsed_ms: u64) {
    let width = frame.width;
    let height = frame.height;
    let alpha = 150.0 / 255.0;
    for row in 0..height {
        for col in 0..width {
            if let Some(cell) = frame.cells.get_mut(row).and_then(|line| line.get_mut(col)) {
                cell.bg = blend_to_black(cell.bg, alpha);
            }
        }
    }
    let box_width = 48.min(width.saturating_mul(9) / 10).max(3);
    let left = width.saturating_sub(box_width) / 2;
    let top = height.saturating_sub(4) / 2;
    let box_frame = reconnecting_box(box_width, styles, elapsed_ms);
    for row in 0..4.min(height.saturating_sub(top)) {
        for col in 0..box_width.min(width.saturating_sub(left)) {
            if let Some(slot) = frame.cells.get_mut(top + row).and_then(|l| l.get_mut(left + col)) {
                *slot = box_frame.cell(row, col);
            }
        }
    }
    // Narrow frames: the copy row's tail may run past the box's right
    // edge — the old `write` loop keeps the underlying (dimmed)
    // background there; ch/fg only.
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let copy = "Reconnecting to the server automatically.";
    let copy_start = left + 2;
    for (offset, ch) in copy.chars().enumerate() {
        let col = copy_start + offset;
        if col >= left + box_width || col >= left + box_width.min(width.saturating_sub(left)) {
            if col < width {
                if let Some(cell) = frame.cells.get_mut(top + 2).and_then(|line| line.get_mut(col))
                {
                    cell.ch = ch;
                    cell.fg = fg_muted;
                }
            }
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // the dialog backdrop's dim math
fn blend_to_black(color: CellColor, alpha: f64) -> CellColor {
    let dim = |value: u8| -> u8 { (f64::from(value) * (1.0 - alpha)).round() as u8 };
    match color {
        CellColor::Rgb(r, g, b) => CellColor::Rgb(dim(r), dim(g), dim(b)),
        other => other,
    }
}

/// The `StartupLoading` pill at `bottom=1` — the new-path twin of
/// [`super::overlays::overlay_startup_loading`].
pub fn overlay_startup_loading_solid(
    frame: &mut Frame,
    styles: &ThemeStyles,
    text: &str,
    elapsed_ms: u64,
) {
    let width = frame.width;
    if width < 4 || frame.height < 2 {
        return;
    }
    let spinner = spinner_frame(elapsed_ms);
    let label = format!("{spinner} {text}");
    let pill_width = label.chars().count() + 2;
    let left = width.saturating_sub(pill_width) / 2;
    let row = frame.height - 2;
    let raised = CellColor::from(styles.background_raised);
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    // One chunk: pads + label, the same muted-on-raised pair on
    // every cell (the old row write stamps exactly that).
    let pill = format!(" {label} ");
    let units = vec![ChromeUnit {
        key: "startup.pill@0".to_owned(),
        left: 0,
        top: 0,
        body: ChromeBody::Text(vec![ChromeChunk::new(pill, fg_muted, raised, 0)]),
    }];
    let pill_frame = chrome_frame("startup_pill", pill_width.max(1), 1, CellColor::Default, units);
    for col in 0..pill_width.min(width.saturating_sub(left)) {
        if let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(left + col)) {
            *slot = pill_frame.cell(0, col);
        }
    }
}

/// The migration widget — the new-path twin of
/// [`super::migration_overlay::overlay_migration`].
pub fn overlay_migration_solid(
    frame: &mut Frame,
    styles: &ThemeStyles,
    progress: &MigrationProgress,
    elapsed_ms: u64,
) {
    let width = frame.width;
    if width < 6 || frame.height < 4 {
        return;
    }
    let spinner = spinner_frame(elapsed_ms);
    let content = format!("{spinner} {}{}", progress.label, progress_count(progress));
    let box_width = content.chars().count() + 1 + 2 + 2;
    let left = width.saturating_sub(box_width + 2);
    let raised = CellColor::from(styles.background_raised);
    let info = CellColor::from(styles.info);
    let fg_base = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let mut units = Vec::new();
    for row in 0..3 {
        units.push(ChromeUnit {
            key: format!("mig.bg@{row}"),
            left: 0,
            top: row,
            body: ChromeBody::Text(vec![ChromeChunk::new(
                " ".repeat(box_width),
                fg_base,
                raised,
                0,
            )]),
        });
    }
    units.extend(chrome_run_units("mig.rail", 0, 0, "▎", info, raised, 0));
    units.extend(chrome_run_units("mig.rail", 0, 1, "▎", info, raised, 0));
    units.extend(chrome_run_units("mig.rail", 0, 2, "▎", info, raised, 0));
    units.extend(chrome_run_units("mig.row", 3, 1, &content, info, raised, 0));
    let mount = chrome_frame("migration_box", box_width.max(1), 3, CellColor::Default, units);
    for row in 0..3.min(frame.height.saturating_sub(1)) {
        for col in 0..box_width.min(width.saturating_sub(left)) {
            if let Some(slot) =
                frame.cells.get_mut(1 + row).and_then(|line| line.get_mut(left + col))
            {
                *slot = mount.cell(row, col);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::chrome_solid::first_divergence;
    use crate::render::Cell;
    use crate::theme::ThemeState;

    fn styles() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    fn base_frame(width: usize, height: usize, bg: u8) -> Frame {
        Frame {
            cells: vec![
                vec![Cell { bg: CellColor::Rgb(bg, bg, bg), ..Cell::default() }; width];
                height
            ],
            width,
            height,
        }
    }
}
