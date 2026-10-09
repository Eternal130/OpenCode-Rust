//! The deterministic tab-glow tint math, ported from
//! `component/tab-pulse.tsx:33-80` + `session-tabs.tsx:124-125` +
//! `theme/color.ts` (gate-9 finding 1 — the Tier-A obligation).
//!
//! The unread glow is **pure position math, no clock**: a resting
//! (sustain-level) glow paints each cell of a pulse strip as
//!
//! ```text
//! bg(cell) = background + (glowColor − background) · GLOW_OPACITY · intensity(cell)
//! ```
//!
//! and the title characters riding it tint with
//!
//! ```text
//! fg(char) = tint(base, glowColor, 0.12 · intensity(1 + numberWidth + charIndex, tabWidth))
//! ```
//!
//! where `intensity` is a smootherstep falloff clamped to a tail of
//! `min(GLOW_TAIL, max(1, width − 2))` cells. Only the time-varying
//! envelopes (ignition, release swell, sweep, flash, marquee) stay C2 —
//! a golden frame is the sustain state, which is why the corpus bytes
//! are stable.
//!
//! `tint` operates in the reference's float space and rounds per
//! channel at the end (`RGBA.fromInts(round(c · 255))`) — computing in
//! 0..=255 space is linearly identical, which the golden-pinned tests
//! below verify cell-for-cell.

// The tint math rounds per channel by design (`fromInts(round(c))`)
// and indexes cells by `usize` against small f64 tails; the pedantic
// cast lints carry no real risk on terminal dimensions.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]
/// A truecolor channel triple (the painter's color vocabulary).
pub type Rgb = (u8, u8, u8);
/// the strip width (`tab-pulse.tsx`).
pub const GLOW_TAIL: f64 = 12.0;
/// `GLOW_OPACITY` — the resting glow's amplitude over the background.
pub const GLOW_OPACITY: f64 = 0.16;
/// The title-tint amplitude (`glowTextColor`: `0.12 · intensity`).
pub const GLOW_TEXT_ALPHA: f64 = 0.12;

/// `clamp` — saturation to `[0, 1]`.
#[must_use]
pub fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// `smootherstep` — the easing every pulse shape rides.
#[must_use]
pub fn smootherstep(value: f64) -> f64 {
    let v = clamp01(value);
    v * v * v * (v * (v * 6.0 - 15.0) + 10.0)
}

/// `glowIntensityAt(index, tail)` — the resting glow's spatial falloff:
/// full at cells 0-1, then a smootherstep decay over `tail` cells
/// behind cell 1.
#[must_use]
pub fn glow_intensity_at(index: usize, tail: f64) -> f64 {
    let distance = f64::from(u32::try_from(index.saturating_sub(1)).unwrap_or(u32::MAX));
    smootherstep(clamp01(1.0 - distance / tail))
}

/// `unreadGlowIntensity(index, width, maximumTail = GLOW_TAIL)` —
/// [`glow_intensity_at`] with the tail clamped to the strip
/// (`min(maximumTail, max(1, width − 2))`).
#[must_use]
pub fn unread_glow_intensity(index: usize, width: usize, maximum_tail: f64) -> f64 {
    glow_intensity_at(index, pulse_tail(maximum_tail, width))
}

/// The effective pulse tail for a strip of `width` cells
/// (`renderSelf`'s clamp).
#[must_use]
pub fn pulse_tail(glow_tail: f64, width: usize) -> f64 {
    let clamp = f64::from(u32::try_from(width.max(3) - 2).unwrap_or(u32::MAX)).max(1.0);
    glow_tail.min(clamp)
}

/// `tint` (`theme/color.ts`): per-channel linear blend toward
/// `overlay` by `alpha`, rounded once at the end.
#[must_use]
pub fn tint(base: Rgb, overlay: Rgb, alpha: f64) -> Rgb {
    let mix = |b: u8, o: u8| (f64::from(b) + (f64::from(o) - f64::from(b)) * alpha).round();
    (mix(base.0, overlay.0) as u8, mix(base.1, overlay.1) as u8, mix(base.2, overlay.2) as u8)
}

/// `glowTextColor(base, glow, index, width, level = 1)`
/// (`session-tabs.tsx:124`): the title-character tint riding the glow.
#[must_use]
pub fn glow_text_color(base: Rgb, glow: Rgb, index: usize, width: usize, level: f64) -> Rgb {
    tint(base, glow, GLOW_TEXT_ALPHA * unread_glow_intensity(index, width, GLOW_TAIL) * level)
}

/// The resting (sustain) glow's per-cell background —
/// `blendTabPulseColor` with only the glow stage active:
/// `background + (glowColor − background) · GLOW_OPACITY · level ·
/// intensity(cell)`.
#[must_use]
pub fn resting_glow_bg(
    background: Rgb,
    glow_color: Rgb,
    index: usize,
    pulse_width: usize,
    glow_tail: f64,
    level: f64,
) -> Rgb {
    let tail = pulse_tail(glow_tail, pulse_width);
    tint(background, glow_color, GLOW_OPACITY * glow_intensity_at(index, tail) * level)
}

/// The blend for a cell where every stage is idle — plain background
/// (the separator row's inner color when the tab itself does not glow).
#[must_use]
pub fn idle_pulse_bg(background: Rgb) -> Rgb {
    background
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCENT: Rgb = (157, 124, 216); // hue.accent[200] (v2-dark)
    const BASE: Rgb = (10, 10, 10); // background.base
    const RAISED: Rgb = (20, 20, 20); // background.raised.base
    const MUTED: Rgb = (128, 128, 128); // text.muted

    #[test]
    fn smootherstep_matches_the_reference_curve() {
        assert!((smootherstep(0.0) - 0.0).abs() < 1e-12);
        assert!((smootherstep(0.5) - 0.5).abs() < 1e-12);
        assert!((smootherstep(1.0) - 1.0).abs() < 1e-12);
        // Hand-derived: sm(0.8) = 0.8³·(6·0.64 − 12 + 10) = 0.512 · 1.84.
        assert!((smootherstep(0.8) - 0.942_080).abs() < 1e-9);
        assert!((smootherstep(0.6) - 0.682_560).abs() < 1e-9);
        // Clamped outside [0, 1].
        assert_eq!(smootherstep(-1.0), 0.0);
        assert_eq!(smootherstep(2.0), 1.0);
    }

    #[test]
    fn intensity_falls_to_zero_behind_the_tail() {
        // tail = 12: full at 0-1, first decay cell is 2, zero from 13.
        assert!((glow_intensity_at(0, 12.0) - 1.0).abs() < 1e-12);
        assert!((glow_intensity_at(1, 12.0) - 1.0).abs() < 1e-12);
        assert!((glow_intensity_at(2, 12.0) - 0.994_912).abs() < 1e-6);
        assert!((glow_intensity_at(12, 12.0) - 0.005_088).abs() < 1e-6);
        assert_eq!(glow_intensity_at(13, 12.0), 0.0);
        // Width clamping: a 10-wide strip uses tail 8 (min(12, max(1, 8))).
        assert!((unread_glow_intensity(8, 10, GLOW_TAIL) - 0.016_052).abs() < 1e-6);
        assert_eq!(unread_glow_intensity(9, 10, GLOW_TAIL), 0.0);
        // The separator's outer pulse clamps 5 → 5 over a 10-wide strip.
        assert!((unread_glow_intensity(5, 10, 5.0) - 0.057_920).abs() < 1e-6);
        assert_eq!(unread_glow_intensity(6, 10, 5.0), 0.0);
    }

    /// The horizontal-bar golden (`session-tabs-horizontal.w80.ansi`
    /// row 0): the unread tab's pulse background, cell for cell —
    /// `tint(base, accent, glowLevel=1)` as the glow color (the bar's
    /// `glowColor` memo passes `glowLevel()` itself as the alpha), tail
    /// 12 over the 32-wide tab.
    #[test]
    fn resting_glow_matches_the_bar_golden_cells() {
        let glow = tint(BASE, ACCENT, 1.0);
        assert_eq!(glow, ACCENT, "alpha 1 keeps the accent verbatim");
        let expected = [
            (34, 28, 43),
            (34, 28, 43),
            (33, 28, 43),
            (33, 28, 42),
            (31, 26, 40),
            (29, 24, 36),
            (25, 22, 32),
            (22, 19, 26),
            (18, 16, 21),
            (15, 14, 17),
            (12, 12, 13),
            (11, 11, 11),
            (10, 10, 10),
        ];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(BASE, ACCENT, index, 32, GLOW_TAIL, 1.0),
                want,
                "bar cell {index}"
            );
        }
        // The tail is flat base from cell 13 on.
        assert_eq!(resting_glow_bg(BASE, ACCENT, 20, 32, GLOW_TAIL, 1.0), BASE);
    }

    /// The vertical-rail golden (`session-tabs-vertical.w80.ansi` rows
    /// 1-3): title pulse `tint(raised, accent, 0.45)` tail 8 over the
    /// 10-wide indicator strip, detail pulse `tint(raised, accent,
    /// 0.25)` tail 8, separator outer `tint(raised, accent, 0.1)` tail
    /// 5.
    #[test]
    fn resting_glow_matches_the_rail_golden_cells() {
        let title_glow = tint(RAISED, ACCENT, 0.45);
        assert_eq!(title_glow, (82, 67, 108));
        let expected = [
            (30, 28, 34),
            (30, 28, 34),
            (30, 27, 34),
            (29, 27, 33),
            (27, 25, 30),
            (25, 24, 27),
            (23, 22, 24),
            (21, 21, 21),
            (20, 20, 20),
            (20, 20, 20),
        ];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(RAISED, title_glow, index, 10, GLOW_TAIL, 1.0),
                want,
                "rail title cell {index}"
            );
        }
        let detail_glow = tint(RAISED, ACCENT, 0.25);
        assert_eq!(detail_glow, (54, 46, 69));
        assert_eq!(
            resting_glow_bg(RAISED, detail_glow, 0, 10, 10.0, 1.0),
            (25, 24, 28),
            "detail cell 0 (glowTail prop 10 clamps to 8)"
        );
        let separator_outer = tint(RAISED, ACCENT, 0.1);
        assert_eq!(separator_outer, (34, 30, 40));
        let expected =
            [(22, 22, 23), (22, 22, 23), (22, 22, 23), (22, 21, 22), (21, 21, 21), (20, 20, 20)];
        for (index, want) in expected.iter().enumerate() {
            assert_eq!(
                &resting_glow_bg(RAISED, separator_outer, index, 10, 5.0, 1.0),
                want,
                "separator cell {index}"
            );
        }
    }

    /// Title characters riding the glow: the bar golden's "First" run
    /// (`glowTextColor(muted, accent, 1 + numberWidth + i, 32)`) and
    /// the rail's (`(muted, tint(raised, accent, 0.45), …, 42)`).
    #[test]
    fn glow_text_color_matches_the_golden_characters() {
        let expected =
            [(131, 128, 138), (131, 128, 137), (131, 128, 136), (130, 128, 135), (130, 128, 133)];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                &glow_text_color(MUTED, ACCENT, 1 + 2 + i, 32, 1.0),
                want,
                "bar title char {i}"
            );
        }
        let rail_glow = tint(RAISED, ACCENT, 0.45);
        let expected =
            [(123, 121, 126), (123, 121, 126), (124, 122, 126), (124, 123, 126), (125, 124, 127)];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(
                &glow_text_color(MUTED, rail_glow, 1 + 2 + i, 42, 1.0),
                want,
                "rail title char {i}"
            );
        }
        // Beyond the tail the text stays neutral.
        assert_eq!(glow_text_color(MUTED, ACCENT, 30, 32, 1.0), MUTED);
    }

    #[test]
    fn tint_rounds_each_channel_once() {
        assert_eq!(tint(BASE, ACCENT, 0.0), BASE);
        assert_eq!(tint(BASE, ACCENT, 1.0), ACCENT);
        // 20 + (157 − 20) · 0.45 = 81.65 → 82 (fromInts rounding).
        assert_eq!(tint(RAISED, ACCENT, 0.45).0, 82);
    }
}
