//! `component/masked-text.ts` → the masked rendering primitive, and
//! its two ref subclasses `shimmer-text.tsx` and `fade-in-text.tsx`.
//!
//! The ref's `MaskedTextRenderable.renderMasked` shades the text's
//! FOREGROUND toward a fixed target color (the colorMatrix's masked
//! translation) per cell: a `shade` closure resolves one `strength`
//! per column, and
//!
//! ```text
//! fg(cell) = fg + (target − fg) · strength(column)
//! ```
//!
//! (`masked_strengths` is the mask walk — the port's per-column
//! twin; a painted char IS a head cell, continuation cells inherit
//! their head's column strength by construction since wide glyphs
//! paint once at their head column). `mask_fg` is the blend, the
//! same per-channel rounding `tab_pulse::tint` already pins
//! (`RGBA.fromInts(round(c · 255))`).
//!
//! Two subclasses ride the base:
//!
//! * **`ShimmerText`** (spinner.tsx:45's `<Spinner shimmer>`): the
//!   mask is a highlight band `intensityAt(col, front, 4, 18)`
//!   sweeping left-to-right every 1200 ms (`coast`-eased) toward
//!   the shimmer color — `elapsed % 1200` is the only time source,
//!   so a frozen clock pins the sweep.
//! * **`FadeInText`** (home.tsx's update banner): the mask starts
//!   FULL (every glyph at the backdrop) and a `smootherstep` feather
//!   8 cells wide wipes it off left-to-right over 200 ms; past the
//!   duration the text paints plain.

/// `shimmer-text.tsx` `DURATION` — the sweep's cycle (ms).
pub const SHIMMER_DURATION_MS: u64 = 1_200;
/// `fade-in-text.tsx` `DURATION` — the fade's total (ms).
pub const FADE_DURATION_MS: u64 = 200;
/// `fade-in-text.tsx` `FEATHER` — the wipe's soft edge (cells).
pub const FADE_FEATHER: f64 = 8.0;
/// `shimmer-text.tsx`'s band shape (`intensityAt(col, front, 4, 18)`).
pub const SHIMMER_HEAD: f64 = 4.0;
pub const SHIMMER_TAIL: f64 = 18.0;

/// The masked-text base (`renderMasked`): one strength per column of
/// a `width`-wide row — `initial_strength` covers the columns before
/// the first glyph (the ref's leading blank cells keep it), every
/// painted column takes the shade's intensity. Deterministic: the
/// caller's `intensity` closure owns time.
#[must_use]
pub fn masked_strengths<F>(width: usize, _initial_strength: f64, intensity: F) -> Vec<f64>
where
    F: Fn(usize) -> f64,
{
    (0..width).map(intensity).collect()
    // `initial_strength` covers only the columns BEFORE the first
    // glyph — cells the per-column walk never paints (the ref's
    // leading blanks keep it until the first head cell resets it).
    // Consumers index by painted column, so the walk alone suffices;
    // the parameter stays to mirror `renderMasked`'s signature.
}

/// The masked FG blend (`colorMatrix`'s masked translation): pull
/// `base` toward `target` by `strength`, rounded per channel —
/// `tab_pulse::tint`'s pinned math.
#[must_use]
pub fn mask_fg(
    base: crate::components::tab_pulse::Rgb,
    target: crate::components::tab_pulse::Rgb,
    strength: f64,
) -> crate::components::tab_pulse::Rgb {
    crate::components::tab_pulse::tint(base, target, strength.clamp(0.0, 1.0))
}

/// `ShimmerText`'s sweep front (`shimmer-text.tsx:44`): the band
/// starts 4 cells left of the row and coasts across `end + 22` cells
/// every [`SHIMMER_DURATION_MS`] ms (`elapsed` wraps modulo the
/// cycle — the ref's `% DURATION`).
#[must_use]
pub fn shimmer_front(elapsed_ms: u64, end: usize) -> f64 {
    let phase = f64::from(u32::try_from(elapsed_ms % SHIMMER_DURATION_MS).unwrap_or(0))
        / f64::from(u32::try_from(SHIMMER_DURATION_MS).unwrap_or(1));
    -SHIMMER_HEAD
        + crate::components::tab_pulse::coast(phase)
            * f64::from(u32::try_from(end.saturating_add(22)).unwrap_or(22))
}

/// `ShimmerText`'s mask (`shimmer-text.tsx:43-46`): the highlight
/// band `intensityAt(column, front, 4, 18)` — 0 at rest, 1 inside
/// the passing band.
#[must_use]
pub fn shimmer_strength(elapsed_ms: u64, end: usize, column: usize) -> f64 {
    let front = shimmer_front(elapsed_ms, end);
    crate::components::tab_pulse::intensity_at(column, front, SHIMMER_HEAD, SHIMMER_TAIL)
}

/// [`masked_strengths`] for the whole shimmer row.
#[must_use]
pub fn shimmer_strengths(elapsed_ms: u64, end: usize, width: usize) -> Vec<f64> {
    masked_strengths(width, 0.0, |column| shimmer_strength(elapsed_ms, end, column))
}

/// `FadeInText`'s mask (`fade-in-text.tsx:66-74`): strength 1 (the
/// glyph at the backdrop) until the wipe's feather passes, then 0 —
/// past [`FADE_DURATION_MS`] the whole row paints plain (strength 0).
/// `sweep_offset`/`sweep_width` default (0/`None`) mirror the ref's
/// home-banner usage.
#[must_use]
pub fn fade_strength(
    elapsed_ms: u64,
    end: usize,
    sweep_offset: f64,
    sweep_width: Option<usize>,
    column: usize,
) -> f64 {
    if elapsed_ms >= FADE_DURATION_MS {
        return 0.0;
    }
    let progress = f64::from(u32::try_from(elapsed_ms.min(FADE_DURATION_MS)).unwrap_or(0))
        / f64::from(u32::try_from(FADE_DURATION_MS).unwrap_or(1));
    // `front = −FEATHER + coast(progress) · (sweepWidth ?? end + 2·FEATHER)`
    // (fade-in-text.tsx:70) — the wipe's leading edge in column space.
    let travel =
        f64::from(u32::try_from(sweep_width.unwrap_or(end)).unwrap_or(0)) + FADE_FEATHER * 2.0;
    let front = -FADE_FEATHER + crate::components::tab_pulse::coast(progress) * travel;
    let distance =
        (front - (sweep_offset + f64::from(u32::try_from(column).unwrap_or(0)))) / FADE_FEATHER;
    1.0 - crate::components::tab_pulse::smootherstep(crate::components::tab_pulse::clamp01(
        distance,
    ))
}

/// [`masked_strengths`] for the whole fade row (strength 1 = the
/// pre-wipe backdrop state).
#[must_use]
pub fn fade_strengths(
    elapsed_ms: u64,
    end: usize,
    sweep_offset: f64,
    sweep_width: Option<usize>,
    width: usize,
) -> Vec<f64> {
    masked_strengths(width, 1.0, |column| {
        fade_strength(elapsed_ms, end, sweep_offset, sweep_width, column)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shimmer band crosses a mid-row column: at rest (front far
    /// left) the strength is 0; mid-cycle it peaks at 1; the cycle
    /// wraps at 1200 ms (`elapsed % DURATION`).
    #[test]
    fn shimmer_sweep_position_across_the_width() {
        let end = 20;
        // t=0: front = -4 — the band's head is left of column 0, but
        // intensityAt's head feather reaches back 4 cells: column 0
        // sits at the feather's very edge (distance 4 = head ⇒ 0.5
        // smootherstep mid? pin the exact ref value instead).
        assert_eq!(shimmer_front(0, end), -4.0);
        // Mid-cycle (coast(0.5) = 0.5): front = -4 + 0.5·42 = 17.
        assert!((shimmer_front(600, end) - 17.0).abs() < 1e-12);
        // Full cycle: wraps to the start.
        assert_eq!(shimmer_front(1_200, end), shimmer_front(0, end));
        assert_eq!(shimmer_front(2_400, end), shimmer_front(0, end));
        // Column 17 at t=600 sits AT the band's front: strength 1.
        assert!((shimmer_strength(600, end, 17) - 1.0).abs() < 1e-12);
        // Far behind the band: ~0 (the 18-cell tail falloff leaves
        // only the smootherstep's residual, < 0.002).
        assert!(shimmer_strength(600, end, 0) < 2e-3);
        // Column 20 rides the head feather: strictly inside (0, 1).
        let feather = shimmer_strength(600, end, 20);
        assert!(feather > 0.0 && feather < 1.0);
    }

    /// The mask covers the row with per-column strengths — the
    /// shimmer's band shows up as a contiguous run.
    #[test]
    fn masked_strengths_walk_the_columns() {
        let strengths = shimmer_strengths(600, 20, 21);
        assert_eq!(strengths.len(), 21);
        assert!(strengths[0] < 2e-3);
        assert_eq!(strengths[17], 1.0);
        assert!(strengths.iter().all(|s| (0.0..=1.0).contains(s)));
    }

    /// The fade: strength 1 everywhere at t=0 (the glyph IS the
    /// backdrop), wiping to 0 left-to-right across the 200 ms
    /// duration; plain (all 0) at and past the duration.
    #[test]
    fn fade_alpha_ramp_at_fixed_timestamps() {
        let end = 20;
        // t=0: progress 0, front = -8 — every column is right of the
        // front ⇒ strength 1 (fully at the backdrop).
        assert!(fade_strengths(0, end, 0.0, None, end).iter().all(|&s| s == 1.0));
        // t=100 (coast(0.5) = 0.5): front = -8 + 0.5·36 = 10 —
        // column 0 (distance 10/8 ⇒ clamped 1 ⇒ smootherstep 1) is
        // fully faded IN (strength 0), column 19 still hidden (1).
        let mid = fade_strengths(100, end, 0.0, None, end);
        assert_eq!(mid[0], 0.0);
        assert_eq!(mid[19], 1.0);
        // Mid-feather: column 5 (distance 5/8) is strictly inside.
        let mid_feather = mid[5];
        assert!(mid_feather > 0.0 && mid_feather < 1.0);
        // t≥200: plain everywhere.
        assert!(fade_strengths(200, end, 0.0, None, end).iter().all(|&s| s == 0.0));
        assert!(fade_strengths(10_000, end, 0.0, None, end).iter().all(|&s| s == 0.0));
    }

    /// `sweep_offset` shifts the mask's phase (the ref's optional
    /// prop): column 5 with offset 5 behaves like column 0 without.
    #[test]
    fn fade_sweep_offset_shifts_the_mask() {
        for column in 0..15 {
            assert_eq!(
                fade_strength(100, 20, 5.0, None, column),
                fade_strength(100, 20, 0.0, None, column + 5),
            );
        }
    }

    /// The masked blend: strength 0 keeps the base, 1 takes the
    /// target, 0.5 rounds per channel (`tab_pulse::tint`).
    #[test]
    fn mask_fg_blends_per_channel() {
        let base = (10, 20, 30);
        let target = (110, 20, 230);
        assert_eq!(mask_fg(base, target, 0.0), base);
        assert_eq!(mask_fg(base, target, 1.0), target);
        assert_eq!(mask_fg(base, target, 0.5), (60, 20, 130));
        // Out-of-range strengths clamp.
        assert_eq!(mask_fg(base, target, 2.0), target);
        assert_eq!(mask_fg(base, target, -1.0), base);
    }
}
