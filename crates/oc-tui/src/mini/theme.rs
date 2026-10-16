//! Mini's scrollback color quantizer — the 1:1 port of v2's oklab
//! `nearestIndexed` (`packages/tui/src/mini/theme.ts:77-109`).
//!
//! Mini does NOT share the main TUI's RGB→xterm-256 downgrade
//! ([`crate::render::color_mode`], a sum-of-squared channel-distance
//! mapping derived from the compiled raster): v2 quantizes the mini
//! scrollback theme in JS with a perceptual oklab nearest-neighbor
//! walk, and the two algorithms intentionally disagree (18 of the 60
//! pinned corpus colors below map to a different index). This module
//! replicates the mini one exactly; the raster one stays in
//! `render/color_mode.rs`, untouched.
//!
//! Provenance — every constant and step cites the ref source:
//!
//! * `srgb_to_linear` — `theme.ts:77-79`
//! * `oklab` — `theme.ts:81-93` (Björn Ottosson's oklab matrix,
//!   line-cited per coefficient below)
//! * `nearest_indexed` — `theme.ts:95-109` (reduce over the 256-entry
//!   palette in index order, distance `2·dl² + da² + db²`, FIRST
//!   minimum wins ties)
//! * `palette_rgb` — the palette build `theme.ts:74` folds over
//!   `ansiToRgba` (`theme/color.ts:3-41`): ANSI-16 hex table (:5-22),
//!   6×6×6 cube `val(x) = x === 0 ? 0 : x*40 + 55` (:27-32), gray
//!   ramp `(code - 232) * 10 + 8` (:35-37)
//! * `scrollback_color` — the `scrollback()` guard `theme.ts:123-126`:
//!   transparent or non-RGB intents pass through, everything else
//!   quantizes (our [`CellColor`] has no alpha channel — the guard
//!   reduces to "only `Rgb` quantizes")
//!
//! The ref computes over `@opentui/core` `RGBA` whose channels are
//! 0..1 floats built by `RGBA.fromInts(r,g,b)` as `r/255` etc.
//! (`color.ts:43-47`'s `tint` rounds `r * 255` back), so this port
//! takes `[u8; 3]` and divides by 255.0 — bit-identical inputs.

// The oklab matrix coefficients stay VERBATIM from `theme.ts:85-91`
// (no digit separators) so the port is textually diffable against
// the ref source; the parity corpus pins their behavior.
#![allow(clippy::unreadable_literal)]

use crate::render::CellColor;

/// `srgbToLinear` (`theme.ts:77-79`):
/// `value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4`.
fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// The oklab triple `oklab()` returns (`theme.ts:88-92`).
#[derive(Debug, Clone, Copy)]
struct OkLab {
    l: f64,
    a: f64,
    b: f64,
}

/// `oklab(color)` (`theme.ts:81-93`) over an sRGB `[u8; 3]` (the ref
/// feeds `RGBA` 0..1 floats — see the module header). The binding
/// names mirror the ref (`r/g/b`, LMS `l/m/s`, `theme.ts:82-87`).
#[allow(clippy::many_single_char_names)] // verbatim ref naming
fn oklab(rgb: [u8; 3]) -> OkLab {
    let r = srgb_to_linear(f64::from(rgb[0]) / 255.0);
    let g = srgb_to_linear(f64::from(rgb[1]) / 255.0);
    let b = srgb_to_linear(f64::from(rgb[2]) / 255.0);
    // The LMS roots (`theme.ts:85-87`).
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    OkLab {
        // `theme.ts:89-91`.
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

/// The ANSI-16 hex table (`theme/color.ts:5-22` — `ansiToRgba`'s
/// `code < 16` branch).
const ANSI16_RGB: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], // #000000
    [0x80, 0x00, 0x00], // #800000
    [0x00, 0x80, 0x00], // #008000
    [0x80, 0x80, 0x00], // #808000
    [0x00, 0x00, 0x80], // #000080
    [0x80, 0x00, 0x80], // #800080
    [0x00, 0x80, 0x80], // #008080
    [0xc0, 0xc0, 0xc0], // #c0c0c0
    [0x80, 0x80, 0x80], // #808080
    [0xff, 0x00, 0x00], // #ff0000
    [0x00, 0xff, 0x00], // #00ff00
    [0xff, 0xff, 0x00], // #ffff00
    [0x00, 0x00, 0xff], // #0000ff
    [0xff, 0x00, 0xff], // #ff00ff
    [0x00, 0xff, 0xff], // #00ffff
    [0xff, 0xff, 0xff], // #ffffff
];

/// The cube channel map `val(x) = x === 0 ? 0 : x * 40 + 55`
/// (`theme/color.ts:31`) — `[0, 95, 135, 175, 215, 255]`.
fn cube_val(x: usize) -> u8 {
    if x == 0 {
        0
    } else {
        // x <= 5 ⇒ 255 max (`color.ts:31`'s domain is the 6-level cube)
        u8::try_from(x * 40 + 55).expect("cube level below 6")
    }
}

/// `ansiToRgba(index)` as plain RGB (`theme/color.ts:3-41`): the
/// ANSI-16 table (:5-22), the 6×6×6 cube with `b = index % 6`,
/// `g = floor(index / 6) % 6`, `r = floor(index / 36)` (:27-32), and
/// the gray ramp `(code - 232) * 10 + 8` (:35-37 — `code < 256`
/// covers 232..=255; the black fallthrough at :40 only exists for
/// code ≥ 256, which the 256-entry palette build never produces).
#[must_use]
pub fn palette_rgb(index: u8) -> [u8; 3] {
    if usize::from(index) < ANSI16_RGB.len() {
        return ANSI16_RGB[usize::from(index)];
    }
    if index < 232 {
        let cube = usize::from(index) - 16;
        let b = cube % 6;
        let g = (cube / 6) % 6;
        let r = cube / 36;
        return [cube_val(r), cube_val(g), cube_val(b)];
    }
    // The gray ramp (:35-37) — `code < 256` covers 232..=255, so the
    // tail is gray 238; the black fallthrough (:40) only exists for
    // code >= 256, which the `Array.from({length: 256})` build
    // (`theme.ts:74`) never produces.
    let gray =
        u8::try_from((usize::from(index) - 232) * 10 + 8).expect("index <= 255 ⇒ gray <= 238");
    [gray, gray, gray]
}

/// `nearestIndexed(indexed, color)` (`theme.ts:95-109`): the oklab
/// nearest neighbor over the 256-entry palette. Distance is
/// `dl*dl*2 + da*da + db*db` (:100-103) — lightness weighted ×2 —
/// and the reduce keeps the FIRST minimum (`dist >= best.dist ? best
/// : {dist, item}`, :104), so the LOWEST index wins ties (:106 seeds
/// with `indexed[0]` at `+Infinity`).
#[must_use]
pub fn nearest_indexed(rgb: [u8; 3]) -> u8 {
    let target = oklab(rgb);
    let mut best_index = 0u8;
    let mut best_distance = f64::INFINITY;
    for index in 0..=u8::MAX {
        let sample = oklab(palette_rgb(index));
        let dl = sample.l - target.l;
        let da = sample.a - target.a;
        let db = sample.b - target.b;
        let distance = dl * dl * 2.0 + da * da + db * db;
        // `dist >= best.dist ? best : …` — strictly smaller replaces,
        // so the first (lowest) index wins ties.
        if distance < best_distance {
            best_distance = distance;
            best_index = index;
        }
    }
    best_index
}

/// `scrollback(color)` (`theme.ts:123-126`): the guard
/// `value.a === 0 || value.intent !== "rgb" ? value :
/// nearestIndexed(indexed, value)` — transparent and non-RGB intents
/// pass through untouched. Our [`CellColor`] carries no alpha; only
/// `Rgb` cells are quantization candidates, `Indexed`/`Default`
/// pass through (the ref's `INDEXED`/`DEFAULT_*` intents).
#[must_use]
pub fn scrollback_color(color: CellColor) -> CellColor {
    match color {
        CellColor::Rgb(r, g, b) => CellColor::Indexed(nearest_indexed([r, g, b])),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The v2 parity corpus — every expected index was produced by
    /// EXECUTING the verbatim ref source (`theme.ts:77-109` +
    /// `theme/color.ts:3-41`, extracted by line range and run under
    /// node with an `RGBA` float shim; the tracked-index walk was
    /// cross-checked against the untouched `nearestIndexed` reduce).
    ///
    /// Groups: (a) the v2 default-theme constants mini's `scrollback()`
    /// and splash triple feed the quantizer (`theme.ts:165-190`,
    /// `:173-175`), (b) a deterministic cube sweep
    /// `(i·89, i·151+37, i·37+113) mod 256`, (c) structural probes
    /// (exact cube corner, gray-ramp members, the 15/231 tie).
    const V2_PARITY: &[([u8; 3], u8)] = &[
        // (a) default-theme scrollback inputs
        ([238, 238, 238], 255), // text.base — gray ramp 238
        ([128, 128, 128], 8),   // text.muted — exact ANSI16[8]
        ([20, 20, 20], 233),    // background.raised.base (shade)
        ([10, 10, 10], 232),    // background.base
        ([224, 108, 117], 167), // text.feedback.error.base — oklab 167 (main: 168)
        ([245, 167, 66], 215),  // feedback.warning
        ([92, 156, 245], 39),   // blue.200 — oklab picks cube 39 (main: 75)
        ([76, 134, 214], 68),   // blue.300
        ([130, 180, 251], 111), // blue.100
        ([157, 124, 216], 104), // purple.100 — oklab 104 (main: 140)
        ([127, 216, 143], 114), // green.200
        ([86, 182, 194], 73),   // cyan.200
        ([104, 208, 221], 80),  // cyan.100
        ([79, 214, 190], 79),   // diff.text.added
        ([197, 59, 83], 161),   // diff.text.removed — oklab 161 (main: 167)
        ([130, 139, 184], 103), // diff.text.context
        ([143, 143, 143], 246), // diff.lineNumber.text — oklab 246 (main: 245)
        ([32, 48, 59], 236),    // diff.background.added
        ([55, 34, 44], 235),    // diff.background.removed — oklab 235 (main: 236)
        ([27, 43, 52], 235),    // diff.lineNumber.background.added
        ([45, 31, 38], 235),    // diff.lineNumber.background.removed
        ([184, 219, 135], 150), // diff.highlight.added
        ([226, 106, 117], 167), // diff.highlight.removed — oklab 167 (main: 168)
        ([253, 126, 135], 210), // red.100
        ([229, 192, 123], 215), // syntax type — oklab 215 (main: 180)
        ([72, 72, 72], 238),    // border.base
        ([255, 255, 255], 15),  // tie 15/231 — FIRST minimum wins
        ([0, 0, 0], 0),         // exact black
        // (b) deterministic sweep (28 values)
        ([0, 37, 113], 18),
        ([89, 188, 150], 72),
        ([178, 83, 187], 133),
        ([11, 234, 224], 80),
        ([100, 129, 5], 64),
        ([189, 24, 42], 124),
        ([22, 175, 79], 35),
        ([111, 70, 116], 60),
        ([200, 221, 153], 186),
        ([33, 116, 190], 31),
        ([122, 11, 227], 92),
        ([211, 162, 8], 178),
        ([44, 57, 45], 236),
        ([133, 208, 82], 113),
        ([222, 103, 119], 168),
        ([55, 254, 156], 49),
        ([144, 149, 193], 139),
        ([233, 44, 230], 200),
        ([66, 195, 11], 70),
        ([155, 90, 48], 94),
        ([244, 241, 85], 191),
        ([77, 136, 122], 66),
        ([166, 31, 159], 127),
        ([255, 182, 196], 217),
        ([88, 77, 233], 62),
        ([177, 228, 14], 148),
        ([10, 123, 51], 2),
        ([99, 18, 88], 53),
        // (c) structural probes
        ([95, 135, 215], 68), // exact cube member → itself
        ([8, 8, 8], 232),     // exact gray-ramp member → itself
        ([13, 13, 13], 233),  // between 232/233 — oklab 233 (main: 232)
        ([60, 60, 60], 237),  // oklab takes ramp 237 (main: cube 59)
    ];

    /// The pinned parity sweep: our port must reproduce the v2
    /// algorithm's output on the whole corpus.
    #[test]
    fn nearest_indexed_matches_the_v2_source_corpus() {
        assert_eq!(V2_PARITY.len(), 60, "the synthetic corpus stays ~64 entries");
        for (rgb, expected) in V2_PARITY {
            assert_eq!(nearest_indexed(*rgb), *expected, "rgb {rgb:?} → v2 emitted idx {expected}");
        }
    }

    /// Ties resolve to the LOWEST index exactly like the ref reduce
    /// (`theme.ts:104` — `dist >= best.dist` keeps the earlier item;
    /// `#ffffff` exists as ANSI16[15] AND cube 231 with distance 0).
    #[test]
    fn ties_pick_the_lowest_index() {
        assert_eq!(nearest_indexed([255, 255, 255]), 15);
        assert_eq!(nearest_indexed([0, 0, 0]), 0);
        assert_eq!(nearest_indexed([128, 128, 128]), 8, "exact ANSI16[8]");
        assert_eq!(nearest_indexed([238, 238, 238]), 255, "exact ramp tail");
    }

    /// The mini palette (`theme/color.ts:3-41`) and the raster
    /// palette (`render/color_mode.rs:101-141`) are the SAME 256
    /// table — v2 builds both from the standard xterm-256 layout —
    /// while the DISTANCE METRICS intentionally differ. Pin the
    /// shared table so neither side can drift silently.
    #[test]
    fn mini_palette_matches_the_raster_palette() {
        for index in 0..=u8::MAX {
            assert_eq!(
                palette_rgb(index),
                crate::render::color_mode::palette_rgb(index),
                "palette index {index} diverged from the raster table"
            );
        }
    }

    /// The two quantizers are intentionally DIFFERENT algorithms in
    /// v2 (oklab `nearestIndexed` for mini vs the raster's
    /// channel-space nearest for the main TUI). These corpus entries
    /// MUST disagree — if they ever agree, one side was mis-merged
    /// into the other.
    #[test]
    fn mini_quantizer_intentionally_diverges_from_the_main_tui_quantizer() {
        let divergent: &[([u8; 3], u8, u8)] = &[
            // (rgb, mini oklab idx, main raster idx)
            ([92, 156, 245], 39, 75),    // blue.200
            ([157, 124, 216], 104, 140), // purple.100
            ([13, 13, 13], 233, 232),    // gray midpoint
            ([229, 192, 123], 215, 180), // syntax type
            ([224, 108, 117], 167, 168), // error red
        ];
        for (rgb, mini, main) in divergent {
            assert_eq!(nearest_indexed(*rgb), *mini, "mini oklab idx for {rgb:?}");
            assert_eq!(
                crate::render::color_mode::quantize(*rgb),
                *main,
                "main raster idx for {rgb:?} (must stay untouched)"
            );
        }
    }

    /// `scrollback()` passes non-RGB intents through and quantizes
    /// RGB (`theme.ts:123-126`).
    #[test]
    fn scrollback_color_guards_non_rgb_intents() {
        assert_eq!(scrollback_color(CellColor::Default), CellColor::Default);
        assert_eq!(scrollback_color(CellColor::Indexed(42)), CellColor::Indexed(42));
        assert_eq!(scrollback_color(CellColor::Rgb(238, 238, 238)), CellColor::Indexed(255));
        assert_eq!(scrollback_color(CellColor::Rgb(128, 128, 128)), CellColor::Indexed(8));
        assert_eq!(scrollback_color(CellColor::Rgb(20, 20, 20)), CellColor::Indexed(233));
    }
}
