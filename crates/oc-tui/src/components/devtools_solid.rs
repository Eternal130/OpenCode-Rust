//! Lane R1 — the devtools bar on the shared opentui-solid chrome
//! bridge ([`crate::components::chrome_solid`]): the bottom row's
//! muted-on-raised line as ONE explicit-bg chunk (the old painter
//! stamps every cell of the row — glyphs AND trailing spaces — with
//! the same fg/bg pair, which an opaque chunk writes natively). The
//! panel text stays the shared [`super::devtools::bar_text`] solver.

use super::chrome_solid::{chrome_mount, ChromeChunk, ChromeUnit};
use crate::render::CellColor;
#[cfg(test)]
use crate::render::Frame;
use crate::store::Store;
use crate::theme::ThemeStyles;

/// The bar's row units — the shaping seam shared by the mount (the
/// live path) and the golden twin.
#[must_use]
pub(crate) fn devtools_bar_units(store: &Store, styles: &ThemeStyles, size: (u16, u16)) -> Vec<ChromeUnit> {
    let width = usize::from(size.0).max(1);
    let full = super::devtools::bar_text(store, styles, size);
    let text: String = full.chars().take(width.saturating_sub(1)).collect();
    // The old row write covers EVERY column (width cols, the text
    // truncated to width − 1 chars).
    let padded = format!("{text:<width$}");
    let fg = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let bg = CellColor::from(styles.background_raised);
    vec![ChromeUnit {
        key: "devtools.bar@0".to_owned(),
        left: 0,
        top: 0,
        body: super::chrome_solid::ChromeBody::Text(vec![ChromeChunk::new(padded, fg, bg, 0)]),
    }]
}

/// The golden twin: the bar's standalone one-row Frame (the retired
/// per-surface copy's test form).
#[cfg(test)]
#[must_use]
pub(crate) fn devtools_bar_row_solid(store: &Store, styles: &ThemeStyles, size: (u16, u16)) -> Frame {
    let units = devtools_bar_units(store, styles, size);
    super::chrome_solid::mounted_surface_frame(
        "devtools_bar",
        usize::from(size.0).max(1),
        1,
        CellColor::Default,
        &units,
    )
}

/// Mount the bar into the shared chrome tree (D2.2): the wrapper sits
/// at the frame's LAST row, full width — the ONE opaque chunk covers
/// every column (the retired row write's bytes). A no-op unless
/// enabled AND visible (an unsynced surface unmounts at the seam).
pub fn mount_devtools_bar_solid(store: &Store, styles: &ThemeStyles, size: (u16, u16)) {
    let state = store.devtools_state();
    if !state.enabled || !state.visible || size.0 == 0 || size.1 == 0 {
        return;
    }
    let width = usize::from(size.0).max(1);
    let row = usize::from(size.1) - 1;
    let units = devtools_bar_units(store, styles, size);
    chrome_mount("devtools_bar", 0, row, width, 1, CellColor::Default, &units, &[]);
}
