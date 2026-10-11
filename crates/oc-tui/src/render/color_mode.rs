//! Terminal color-capability degradation — S0 parity with v2's raster.
//!
//! v2.0.24 degrades colors at EMISSION time, never in the frame model:
//! the JS side keeps every theme color as truecolor RGBA
//! (`packages/tui/src/app.tsx` passes a `CliRendererConfig` with no
//! color options; the RGBA intents RGB/INDEXED/DEFAULT live in
//! `@opentui/core` `src/lib/RGBA.ts`, shipped compiled in
//! `node_modules/@opentui/core/dist/chunk-bun-sjw2d9bq.js:1030-1105`),
//! and the native raster (`@opentui/core-linux-x64@0.5.14`,
//! `libopentui.so` — the zig frame renderer behind
//! `resolveRenderLib`) decides per terminal:
//!
//! * capability decision (raster capability probe; rodata strings
//!   `COLORTERM`/`truecolor`/`24bit`/`256color`/`WT_SESSION`):
//!   - `COLORTERM` ∈ {`truecolor`, `24bit`} → truecolor
//!   - `WT_SESSION` set → truecolor (Windows Terminal)
//!   - `SSH_CONNECTION`/`SSH_CLIENT`/`SSH_TTY` set → truecolor
//!     (empirical: any one of the three forces RGB SGR even under
//!     `TERM=xterm-256color` — probed against the ref raster)
//!   - `TERM` containing `256color` → 256-index
//!   - anything else (`xterm`, `screen`, `dumb`, …) → truecolor; the
//!     raster has NO 16-color degradation (verified empirically)
//! * RGB → index mapping (raster emit path; the palette is the one
//!   exposed JS-side by `ansi256IndexToRgb`): the standard xterm-256
//!   palette — ANSI-16 table, 6×6×6 cube with levels
//!   `[0,95,135,175,215,255]`, gray ramp `8 + 10·n` — nearest member
//!   by SUM OF SQUARED channel differences, LOWEST index winning
//!   ties (`#ffffff` → 15, not the equal-distance cube corner 231).
//!
//! The forward mapping is not expressible from ref source (it lives
//! in the compiled raster), so it was derived empirically and pinned
//! by test: a 144-color sweep (gray ramp, every default-theme
//! constant, cube corners, seeded randoms) rendered through the ref's
//! raster under `TERM=xterm-256color` (clean env, PTY) matches
//! [`quantize`] 144/144 — see `tools/parity/tui-diff/REPORT.md` (S0)
//! for the probe transcript. The ref's *mini* scrollback uses a
//! DIFFERENT quantizer (oklab-weighted `nearestIndexed`,
//! `packages/tui/src/mini/theme.ts:95-110`) — that is the `opencode
//! run` path, not the interactive TUI this module serves.
//!
//! Our replication degrades the frame right before the ratatui draw
//! (`crate::app` run loop): `App::frame()` and every golden stay
//! truecolor — exactly v2's split (framebuffer RGB, emission
//! indexed). The golden corpus is therefore TERM-independent by
//! construction: it serializes frames, never the degraded emission.

use std::collections::HashMap;

use super::{CellColor, Frame};

/// The raster's terminal color capability, reduced to the two modes
/// its emission paths actually distinguish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// Emit `38;2;r;g;b` / `48;2;r;g;b` truecolor SGR.
    TrueColor,
    /// Degrade every RGB to the nearest xterm-256 palette index and
    /// emit `38;5;n` / `48;5;n`.
    Indexed256,
}

/// Resolve the color mode the way the v2 raster's capability probe
/// does (see the module header for the decision table and its
/// provenance).
///
/// `ssh` stands for "any of `SSH_CONNECTION`/`SSH_CLIENT`/`SSH_TTY`
/// is set" — all three have the same effect, so the caller folds them.
#[must_use]
pub fn detect(
    term: Option<&str>,
    colorterm: Option<&str>,
    ssh: bool,
    wt_session: bool,
) -> ColorMode {
    if matches!(colorterm, Some("truecolor" | "24bit")) {
        return ColorMode::TrueColor;
    }
    if wt_session || ssh {
        return ColorMode::TrueColor;
    }
    if term.is_some_and(|term| term.contains("256color")) {
        return ColorMode::Indexed256;
    }
    ColorMode::TrueColor
}

/// [`detect`] over the process environment (one-shot at TUI boot).
#[must_use]
pub fn detect_from_env() -> ColorMode {
    let ssh = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|key| std::env::var_os(key).is_some());
    detect(
        std::env::var("TERM").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
        ssh,
        std::env::var_os("WT_SESSION").is_some(),
    )
}

/// The raster's ANSI-16 base table (`src/lib/RGBA.ts`, `ANSI16_RGB`).
const ANSI16_RGB: [[u8; 3]; 16] = [
    [0, 0, 0],
    [128, 0, 0],
    [0, 128, 0],
    [128, 128, 0],
    [0, 0, 128],
    [128, 0, 128],
    [0, 128, 128],
    [192, 192, 192],
    [128, 128, 128],
    [255, 0, 0],
    [0, 255, 0],
    [255, 255, 0],
    [0, 0, 255],
    [255, 0, 255],
    [0, 255, 255],
    [255, 255, 255],
];

/// The raster's 6×6×6 cube channel levels (`ANSI_256_CUBE_LEVELS`).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Palette member `index` → RGB (the inverse of [`quantize`]; mirrors
/// the JS `ansi256IndexToRgb` exactly, gray ramp included).
#[must_use]
pub fn palette_rgb(index: u8) -> [u8; 3] {
    if usize::from(index) < ANSI16_RGB.len() {
        return ANSI16_RGB[usize::from(index)];
    }
    if index < 232 {
        let cube = usize::from(index) - 16;
        let r = CUBE_LEVELS[cube / 36];
        let g = CUBE_LEVELS[(cube / 6) % 6];
        let b = CUBE_LEVELS[cube % 6];
        return [r, g, b];
    }
    let gray = 8 + (u16::from(index) - 232) * 10;
    let gray = u8::try_from(gray).unwrap_or(u8::MAX);
    [gray, gray, gray]
}

/// Nearest xterm-256 palette index for `rgb` — the raster's mapping:
/// minimum sum of squared channel differences over all 256 members in
/// index order, so the LOWEST index wins ties (`#ffffff` → 15, not
/// the equal-distance cube corner 231).
///
/// Iterating 256 candidates per call is what the compiled raster
/// does; hot paths should go through [`ColorCache`].
#[must_use]
pub fn quantize(rgb: [u8; 3]) -> u8 {
    let mut best_index = 0u8;
    let mut best_distance = u32::MAX;
    for index in 0..=u8::MAX {
        let [pr, pg, pb] = palette_rgb(index);
        let dr = u32::from(pr.abs_diff(rgb[0]));
        let dg = u32::from(pg.abs_diff(rgb[1]));
        let db = u32::from(pb.abs_diff(rgb[2]));
        let distance = dr * dr + dg * dg + db * db;
        // strict `<` keeps the first (lowest) index on ties
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

/// Memoized [`quantize`] — frames reuse the theme's few dozen colors
/// across thousands of cells.
#[derive(Debug, Default)]
pub struct ColorCache {
    map: HashMap<[u8; 3], u8>,
}

impl ColorCache {
    #[must_use]
    pub fn quantize(&mut self, rgb: [u8; 3]) -> u8 {
        *self.map.entry(rgb).or_insert_with(|| quantize(rgb))
    }
}

/// Degrade every cell color of `frame` to the raster's nearest
/// palette index (a no-op for indexed/default cells). Called at the
/// terminal-draw seam only; `App::frame()` and the goldens keep
/// truecolor.
pub fn degrade_frame(frame: &mut Frame, cache: &mut ColorCache) {
    for row in &mut frame.cells {
        for cell in row.iter_mut() {
            cell.fg = degrade_color(cell.fg, cache);
            cell.bg = degrade_color(cell.bg, cache);
        }
    }
}

fn degrade_color(color: CellColor, cache: &mut ColorCache) -> CellColor {
    match color {
        CellColor::Rgb(r, g, b) => CellColor::Indexed(cache.quantize([r, g, b])),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Capability matrix (empirical table from the ref raster — probe
    // runs under a real PTY with a clean env; see module header).
    // ------------------------------------------------------------------
    #[test]
    fn capability_matrix_matches_the_ref_raster() {
        let tc = ColorMode::TrueColor;
        let i256 = ColorMode::Indexed256;
        // no COLORTERM, no ssh, no WT_SESSION
        for ssh in [false, true] {
            for wt in [false, true] {
                // COLORTERM wins everything
                assert_eq!(
                    detect(Some("xterm-256color"), Some("truecolor"), ssh, wt),
                    tc,
                    "COLORTERM=truecolor must force truecolor"
                );
                assert_eq!(
                    detect(Some("dumb"), Some("24bit"), ssh, wt),
                    tc,
                    "COLORTERM=24bit must force truecolor"
                );
                // unknown COLORTERM values do NOT force truecolor
                assert_eq!(detect(Some("xterm-256color"), Some("yes"), false, false), i256);
            }
        }
        // ssh / WT_SESSION force truecolor over TERM
        assert_eq!(detect(Some("xterm-256color"), None, true, false), tc, "SSH_* forces truecolor");
        assert_eq!(
            detect(Some("xterm-256color"), None, false, true),
            tc,
            "WT_SESSION forces truecolor"
        );
        // TERM decides otherwise
        assert_eq!(detect(Some("xterm-256color"), None, false, false), i256);
        assert_eq!(detect(Some("screen-256color"), None, false, false), i256);
        assert_eq!(detect(Some("xterm"), None, false, false), tc, "plain xterm stays truecolor");
        assert_eq!(detect(Some("screen"), None, false, false), tc);
        assert_eq!(detect(Some("dumb"), None, false, false), tc);
        assert_eq!(detect(None, None, false, false), tc, "TERM unset stays truecolor");
    }

    // ------------------------------------------------------------------
    // Quantization: every pair below was OBSERVED from the ref raster
    // under TERM=xterm-256color (144-color sweep, 0 mismatches).
    // ------------------------------------------------------------------
    #[test]
    fn quantize_matches_the_ref_raster_sweep() {
        // (input hex, observed index) — every color constant of the
        // default v2 theme (dark), plus fg/bg defaults.
        let observed: &[(&str, u8)] = &[
            ("eeeeee", 255), // text.base / gray.200
            ("b5b5b5", 249), // gray.300
            ("808080", 8),   // gray.400 — exact ANSI16[8]
            ("4c4c4c", 239), // gray.500
            ("1e1e1e", 234), // gray.600
            ("141414", 233), // background.raised.base / gray.700
            ("0a0a0a", 232), // background.base / gray.800
            ("030303", 0),   // gray.900 — nearer black than idx232
            ("484848", 238), // border.base
            ("606060", 59),  // scrollbar — the (95,95,95) CUBE corner beats the gray ramp
            ("f5a742", 215), // feedback.warning / syntax.number
            ("e5c07b", 180), // syntax.type / markdown.blockQuote
            ("82b4fb", 111), // blue.100
            ("5c9cf5", 75),  // blue.200 (interactive.200 in light mode)
            ("4c86d6", 68),  // blue.300
            ("b38ff4", 141), // purple.100 (accent.100 in dark)
            ("9d7cd8", 140), // purple.200 (accent.200 in dark)
            ("e06c75", 168), // red.200
            ("c35a63", 131), // red.300
            ("7fd88f", 114), // green.200
            ("56b6c2", 73),  // cyan.200
            ("68d0dd", 80),  // cyan.100
            ("4fd6be", 79),  // diff.text.added
            ("c53b53", 167), // diff.text.removed
            ("828bb8", 103), // diff.text.context
            ("8f8f8f", 245), // diff.lineNumber.text
            ("20303b", 236), // diff.background.added
            ("37222c", 236), // diff.background.removed
            ("1b2b34", 235), // diff.lineNumber.background.added
            ("2d1f26", 235), // diff.lineNumber.background.removed
            ("b8db87", 150), // diff.highlight.added
            ("e26a75", 168), // diff.highlight.removed
            ("fd7e87", 210), // red.100
            ("ffffff", 15),  // default fg — tie 15/231, lowest wins
            ("000000", 0),   // default bg
        ];
        for (hex, expected) in observed {
            let rgb = hex_to_rgb(hex);
            let actual = quantize(rgb);
            assert_eq!(actual, *expected, "#{hex} ({rgb:?}) → ref emitted 38;5;{expected}");
        }
    }

    #[test]
    fn quantize_tie_breaks_pick_the_lowest_index() {
        // #ffffff is an exact member TWICE: ANSI16[15] and cube 231.
        assert_eq!(quantize([255, 255, 255]), 15);
        // exact cube corners map to themselves
        assert_eq!(quantize([95, 135, 215]), 68);
        assert_eq!(quantize([0, 95, 0]), 22);
        // exact gray-ramp members map to themselves
        assert_eq!(quantize([8, 8, 8]), 232);
        assert_eq!(quantize([238, 238, 238]), 255);
        // (13,13,13) is EXACTLY between idx232 (#080808) and idx233
        // (#121212) — both d=75; the lowest index wins (sweep-observed).
        assert_eq!(quantize([13, 13, 13]), 232);
        // (10,10,10): 232 (#080808) d=12 beats 0 (#000000) d=300
        assert_eq!(quantize([10, 10, 10]), 232);
    }

    #[test]
    fn palette_round_trips_every_index() {
        for index in 0..=u8::MAX {
            let rgb = palette_rgb(index);
            // An exact palette member must quantize back to a member
            // with IDENTICAL rgb (possibly a lower tie-equal index).
            let back = quantize(rgb);
            assert_eq!(palette_rgb(back), rgb, "idx {index} round-trips rgb-exact");
        }
    }

    #[test]
    fn degrade_frame_maps_rgb_only_and_memoizes() {
        use crate::render::Cell;
        let mut frame = Frame {
            cells: vec![vec![
                Cell { ch: 'a', fg: CellColor::Rgb(10, 10, 10), bg: CellColor::Default, attrs: 0 },
                Cell {
                    ch: 'b',
                    fg: CellColor::Indexed(7),
                    bg: CellColor::Rgb(238, 238, 238),
                    attrs: 0,
                },
            ]],
            width: 2,
            height: 1,
        };
        let mut cache = ColorCache::default();
        degrade_frame(&mut frame, &mut cache);
        assert_eq!(frame.cells[0][0].fg, CellColor::Indexed(232));
        assert_eq!(frame.cells[0][0].bg, CellColor::Default, "default stays default");
        assert_eq!(frame.cells[0][1].fg, CellColor::Indexed(7), "indexed passes through");
        assert_eq!(frame.cells[0][1].bg, CellColor::Indexed(255));
        assert_eq!(cache.map.len(), 2, "two distinct rgb colors memoized");
        assert_eq!(cache.quantize([10, 10, 10]), 232);
        assert_eq!(cache.map.len(), 2, "repeat hit does not grow the cache");
    }

    fn hex_to_rgb(hex: &str) -> [u8; 3] {
        let value = u32::from_str_radix(hex, 16).expect("valid hex");
        let r = u8::try_from(value >> 16).expect("hex channel fits u8");
        let g = u8::try_from((value >> 8) & 0xFF).expect("hex channel fits u8");
        let b = u8::try_from(value & 0xFF).expect("hex channel fits u8");
        [r, g, b]
    }
}
