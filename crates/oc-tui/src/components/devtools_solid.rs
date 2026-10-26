//! Lane R1 — the devtools bar on the shared opentui-solid chrome
//! bridge ([`crate::components::chrome_solid`]): the bottom row's
//! muted-on-raised line as ONE explicit-bg chunk (the old painter
//! stamps every cell of the row — glyphs AND trailing spaces — with
//! the same fg/bg pair, which an opaque chunk writes natively). The
//! panel text stays the shared [`super::devtools::bar_text`] solver.

use super::chrome_solid::{chrome_frame, ChromeChunk, ChromeUnit};
use crate::render::{CellColor, Frame};
use crate::store::Store;
use crate::theme::ThemeStyles;

/// The bar's 1-row surface — the new-path twin of the old
/// `paint_devtools_bar` row write.
#[must_use]
pub fn devtools_bar_row_solid(store: &Store, styles: &ThemeStyles, size: (u16, u16)) -> Frame {
    let width = usize::from(size.0).max(1);
    let full = super::devtools::bar_text(store, styles, size);
    let text: String = full.chars().take(width.saturating_sub(1)).collect();
    // The old row write covers EVERY column (width cols, the text
    // truncated to width − 1 chars).
    let padded = format!("{text:<width$}", width = width);
    let fg = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let bg = CellColor::from(styles.background_raised);
    let units = vec![ChromeUnit {
        key: "devtools.bar@0".to_owned(),
        left: 0,
        top: 0,
        body: super::chrome_solid::ChromeBody::Text(vec![ChromeChunk::new(padded, fg, bg, 0)]),
    }];
    chrome_frame("devtools_bar", width, 1, CellColor::Default, units)
}

/// Paint the bar over the frame's LAST row (the golden-frame guard
/// lives at the caller — a no-op unless enabled AND visible).
pub fn paint_devtools_bar_solid(
    frame: &mut Frame,
    store: &Store,
    styles: &ThemeStyles,
    size: (u16, u16),
) {
    let state = store.devtools_state();
    if !state.enabled || !state.visible || frame.height == 0 || frame.width == 0 {
        return;
    }
    let row_frame = devtools_bar_row_solid(store, styles, size);
    let row = frame.height - 1;
    if let Some(slot) = frame.cells.get_mut(row) {
        let width = frame.width.min(row_frame.width);
        for col in 0..width {
            slot[col] = row_frame.cell(0, col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Runtime;
    use crate::render::{Cell, CellColor};
    use crate::store::Store;
    use crate::theme::ThemeState;

    fn base_frame(width: usize, height: usize) -> Frame {
        Frame {
            cells: vec![
                vec![Cell { bg: CellColor::Rgb(60, 60, 60), ..Cell::default() }; width];
                height
            ],
            width,
            height,
        }
    }
}
