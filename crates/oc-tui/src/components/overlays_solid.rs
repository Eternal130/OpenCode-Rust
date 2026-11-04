//! D2.2 overlays lane — the app-shell state overlays AND the family's
//! shared mount seam on the shared tree
//! ([`crate::components::chrome_solid`]):
//!
//! * **Reconnecting** — the scrim dims through a seam STAMP
//!   (`ChromeStamp::DimBg`, ref `reconnecting.tsx:9-17`
//!   `RGBA(0,0,0,150)` zIndex 10000): the port's buffer blends alpha
//!   fills over a BLACK backdrop (`buffer.rs` `blend_backdrop`), so
//!   an in-buffer alpha box would paint hand-painted hole cells
//!   opaque black — the stamp reproduces the old frame painter's
//!   bg-dim bytes over tree pixels AND holes alike. The centered
//!   raised BOX rides the tree as the family's LAST child (above the
//!   pill/migration, exactly the old frame-tail z).
//! * **`StartupLoading`** — the bottom-centered pill as ONE opaque
//!   chunk row, a family wrapper at skeleton index 8 (moved up from
//!   the chrome family: today's frame-tail z, ref zIndex 5000).
//! * **The migration widget** — index 9, above the pill exactly like
//!   the old frame tail.
//!
//! The `mount_overlay_surface` helper is the family's shared seat:
//! one wrapper per surface under its own `surface-{family}`
//! container, the flat keyed `For` of units as the content subtree,
//! rebuilt only when the geometry key changes.

use std::cell::RefCell;
use std::collections::HashMap;

use opentui_solid::Signal;

use super::chrome_solid::{
    chrome_run_units, family_wrapper_live, mount_family_wrapper, surface_element, ChromeInputs,
    ChromeStamp, ChromeUnit,
};
use super::migration_overlay::{progress_count, MigrationProgress};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// The overlays family's skeleton indices (D2.2, above the dialogs
/// family's 4 — today's paste order frozen: console < which-key <
/// (permission, in-tree at 3) < toast < pill < migration < scrim).
pub(crate) const OVERLAYS_CONSOLE: usize = 5;
pub(crate) const OVERLAYS_WHICH_KEY: usize = 6;
pub(crate) const OVERLAYS_TOAST: usize = 7;
pub(crate) const OVERLAYS_STARTUP_PILL: usize = 8;
pub(crate) const OVERLAYS_MIGRATION: usize = 9;
pub(crate) const OVERLAYS_RECONNECTING: usize = 10;

struct OverlaySurface {
    signal: Signal<ChromeInputs>,
    key: (usize, usize),
}

thread_local! {
    /// The overlays family's content states (one mount per surface
    /// per thread-process — the wrapper itself lives in the
    /// shared-tree hub, `chrome_solid::mount_family_wrapper`).
    static OVERLAY_SURFACES: RefCell<HashMap<&'static str, OverlaySurface>> =
        RefCell::new(HashMap::new());
}

/// The overlays family's shared mount seam: sync one surface's units
/// into the shared tree as a family wrapper — the content subtree is
/// the flat keyed `For` of units (surface-local coordinates), rebuilt
/// only when the geometry key `(width × height)` changes or the
/// wrapper left the tree (a hub reset, a seam prune). NOT calling
/// this for a frame unmounts the wrapper at the seam (the old
/// painters' visibility branches).
#[allow(clippy::too_many_arguments)] // the family's shared seat (rect/units/blank/stamps)
pub(crate) fn mount_overlay_surface(
    family: &'static str,
    skeleton_index: usize,
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    items: &[ChromeUnit],
    blank: bool,
    stamps: &[ChromeStamp],
) {
    let key = (width, height);
    let rebuild = !family_wrapper_live(family)
        || OVERLAY_SURFACES.with_borrow(|slot| {
            !matches!(slot.get(family), Some(entry) if entry.key == key)
        });
    let (content, content_key) = if rebuild {
        let signal = Signal::new(ChromeInputs { items: items.to_vec() });
        let element = surface_element(&signal, width, height);
        OVERLAY_SURFACES.with_borrow_mut(|slot| {
            slot.insert(family, OverlaySurface { signal, key });
        });
        (Some(element), Some(format!("{family}-content@{width}x{height}")))
    } else {
        OVERLAY_SURFACES.with_borrow_mut(|slot| {
            slot.get_mut(family).expect("checked above").signal.set(ChromeInputs { items: items.to_vec() });
        });
        (None, None)
    };
    mount_family_wrapper(
        family,
        skeleton_index,
        left,
        top,
        width,
        height,
        content_key,
        content,
        stamps,
        items,
        blank,
    );
}

/// `SPINNER_FRAMES` (component/spinner, 80 ms) — shared with the old
/// painters; read through the same table.
fn spinner_frame(elapsed_ms: u64) -> char {
    const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    SPINNER_FRAMES[((elapsed_ms / 80) % SPINNER_FRAMES.len() as u64) as usize]
}

/// The reconnecting box's absolute geometry (reconnecting.tsx:8-39):
/// the 48-wide centered box clamped to `width·9/10` (min 3), the
/// copy row's tail past the clamped right edge keeps the DIMMED
/// background (the old paste loop never reached it).
#[must_use]
pub(crate) fn reconnecting_geometry(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let box_width = 48.min(width.saturating_mul(9) / 10).max(3);
    let left = width.saturating_sub(box_width) / 2;
    let top = height.saturating_sub(4) / 2;
    let w = box_width.min(width.saturating_sub(left));
    let h = 4.min(height.saturating_sub(top));
    (left, top, w, h)
}

/// Mount the reconnecting scrim + box into the shared tree (the
/// family's LAST child — ref zIndex 10000): the wrapper covers the
/// whole screen, its content is the raised box (surface-local at the
/// box origin), and its single stamp dims every RGB background
/// outside the box rect at `150/255` — the old frame painter's
/// `blend_to_black` walk, byte for byte (glyphs and non-RGB
/// backgrounds survive; the box's keep-rect is exactly the rect the
/// old box paste overwrote after the dim).
pub(crate) fn mount_reconnecting_solid(styles: &ThemeStyles, elapsed_ms: u64, width: usize, height: usize) {
    if width == 0 || height == 0 {
        return;
    }
    let (left, top, box_w, box_h) = reconnecting_geometry(width, height);
    let raised = CellColor::from(styles.background_raised);
    let fg_base = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let mut units = Vec::new();
    // The raised interior rows (clipped to the box width — the old
    // paste loop never wrote past the clamped right edge), at the
    // box's ABSOLUTE origin (the wrapper covers the whole screen:
    // surface-local == screen-local).
    for row in 0..box_h {
        units.push(ChromeUnit {
            key: format!("recon.bg@{row}"),
            left,
            top: top + row,
            body: super::chrome_solid::ChromeBody::Text(vec![super::chrome_solid::ChromeChunk::new(
                " ".repeat(box_w),
                fg_base,
                raised,
                0,
            )]),
        });
    }
    // The headline + copy runs, clipped to the box interior (the
    // narrow-frame tail chars stay a post-seam paint: the old paste
    // loop clipped them out and the follow-up walk re-inked them over
    // the dimmed base).
    let headline: String =
        format!("{} Connection lost…", spinner_frame(elapsed_ms)).chars().take(box_w.saturating_sub(2)).collect();
    units.extend(chrome_run_units("recon.head", left + 2, top + 1, &headline, fg_base, raised, 0));
    let copy = "Reconnecting to the server automatically.";
    let copy_clipped: String =
        copy.chars().take(box_w.saturating_sub(2)).collect();
    units.extend(chrome_run_units("recon.copy", left + 2, top + 2, &copy_clipped, fg_muted, raised, 0));
    // The scrim: a DimBg stamp over the whole screen minus the box
    // (the stamp dims the composited frame — tree pixels AND the
    // hand-painted holes — the box's opaque pixels excluded exactly
    // like the old paste-over-dim).
    let stamps = vec![ChromeStamp::DimBg {
        left: 0,
        top: 0,
        width,
        height,
        alpha: 150,
        keep_left: left,
        keep_top: top,
        keep_width: box_w,
        keep_height: box_h,
    }];
    mount_overlay_surface(
        "reconnecting",
        OVERLAYS_RECONNECTING,
        0,
        0,
        width,
        height,
        &units,
        false,
        &stamps,
    );
}

/// The reconnecting narrow-frame tail (the old painter's last walk):
/// copy chars past the clamped box right edge keep the DIMMED
/// background — ch/fg only, the bg stays whatever the scrim left.
pub(crate) fn paint_reconnecting_tail(frame: &mut Frame, styles: &ThemeStyles, width: usize) {
    let (left, top, box_w, _box_h) = reconnecting_geometry(width, frame.height);
    let copy = "Reconnecting to the server automatically.";
    let copy_start = left + 2;
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let edge = left + box_w;
    for (offset, ch) in copy.chars().enumerate() {
        let col = copy_start + offset;
        if col >= edge && col < width {
            if let Some(cell) = frame.cells.get_mut(top + 2).and_then(|line| line.get_mut(col)) {
                cell.ch = ch;
                cell.fg = fg_muted;
            }
        }
    }
}

/// The headless twin (the golden/unit harness shape): mount the
/// reconnecting scrim + box over a LIVE frame, composite, then the
/// tail — the bytes the old `overlay_reconnecting_solid` painter
/// produced, by construction.
pub fn reconnecting_over_frame(
    frame: &mut Frame,
    styles: &ThemeStyles,
    elapsed_ms: u64,
) {
    let width = frame.width;
    let height = frame.height;
    crate::components::chrome_solid::reset_hub();
    mount_reconnecting_solid(styles, elapsed_ms, width, height);
    crate::components::chrome_solid::composite_chrome_tree(frame);
    paint_reconnecting_tail(frame, styles, width);
}

/// The `StartupLoading` pill at `bottom=1` — a family wrapper at
/// skeleton index 8: ONE opaque chunk row, so the tree fully covers
/// the pill's rect (the old painter's row write).
pub fn mount_startup_loading_solid(
    styles: &ThemeStyles,
    text: &str,
    elapsed_ms: u64,
    width: usize,
    height: usize,
) {
    if width < 4 || height < 2 {
        return;
    }
    let spinner = spinner_frame(elapsed_ms);
    let label = format!("{spinner} {text}");
    let pill_width = label.chars().count() + 2;
    let left = width.saturating_sub(pill_width) / 2;
    let row = height - 2;
    let raised = CellColor::from(styles.background_raised);
    let fg_muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    // One chunk: pads + label, the same muted-on-raised pair on
    // every cell (the old row write stamps exactly that).
    let pill = format!(" {label} ");
    let units = vec![ChromeUnit {
        key: "startup.pill@0".to_owned(),
        left: 0,
        top: 0,
        body: super::chrome_solid::ChromeBody::Text(vec![super::chrome_solid::ChromeChunk::new(
            pill,
            fg_muted,
            raised,
            0,
        )]),
    }];
    mount_overlay_surface(
        "startup-pill",
        OVERLAYS_STARTUP_PILL,
        left,
        row,
        pill_width.min(width.saturating_sub(left)),
        1,
        &units,
        false,
        &[],
    );
}

/// The migration widget — a family wrapper at skeleton index 9:
/// three opaque raised rows fully cover the box's rect.
pub fn mount_migration_solid(
    styles: &ThemeStyles,
    progress: &MigrationProgress,
    elapsed_ms: u64,
    width: usize,
    height: usize,
) {
    if width < 6 || height < 4 {
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
            body: super::chrome_solid::ChromeBody::Text(vec![super::chrome_solid::ChromeChunk::new(
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
    mount_overlay_surface(
        "migration",
        OVERLAYS_MIGRATION,
        left,
        1,
        box_width.min(width.saturating_sub(left)),
        3.min(height.saturating_sub(1)),
        &units,
        false,
        &[],
    );
}
