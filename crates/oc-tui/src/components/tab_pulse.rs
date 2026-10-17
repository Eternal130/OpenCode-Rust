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
// and indexes cells by `usize` against small f64 tails; clock ms are
// bounded monotonic instants. The pedantic cast lints carry no real
// risk on terminal dimensions/instants.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
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

// ---------------------------------------------------------------------------
// The animated phases (tab-pulse.tsx:31-115 + 125-427) — lane TAIL, the
// user's busy-session findings (ROADMAP 2026-10-16): the clockwise dot
// spinner, the left-to-right cycling sweep, and the envelopes that
// shape them. The envelope levels derive PURELY from a transition-stamp
// track + the injected clock, so every render is stateless and
// reproducible: a frozen clock pins the sustain state (golden frames
// stay byte-stable), a fresh store starts each envelope at its origin.
// ---------------------------------------------------------------------------

/// `RUN_DURATION` — one left-to-right sweep pass (ms).
pub const RUN_DURATION: u64 = 2_800;
/// `RUN_ATTACK` — the sweep's fade-in (ms).
pub const RUN_ATTACK: u64 = 450;
/// `RUN_HEAD`/`RUN_TAIL` — the sweep band's cells ahead/behind the front.
pub const RUN_HEAD: f64 = 4.0;
pub const RUN_TAIL: f64 = 18.0;
/// `RUN_FADE_OUT` — the sweep's note-off fade (ms).
pub const RUN_FADE_OUT: u64 = 500;
/// The sweep band's peak amplitude over the tab background (`0.14`,
/// tab-pulse.tsx renderSelf).
pub const RUN_SWEEP_OPACITY: f64 = 0.14;
/// `COMPLETION_*` — the resolve flash (`attackDecay` peak 1 × 0.18).
pub const COMPLETION_DURATION: u64 = 1_200;
pub const COMPLETION_ATTACK: f64 = 0.12;
pub const COMPLETION_OPACITY: f64 = 0.18;
/// `GLOW_IGNITION_*`/`GLOW_RELEASE_*` — the glow's attack/release shapes.
pub const GLOW_IGNITION_DURATION: u64 = 600;
pub const GLOW_IGNITION_PEAK: f64 = 1.5;
pub const GLOW_IGNITION_ATTACK: f64 = 0.3;
pub const GLOW_RELEASE_DURATION: u64 = 900;
pub const GLOW_RELEASE_ATTACK: f64 = 0.12;
pub const GLOW_RELEASE_PEAK: f64 = 1.25;
/// `GLOW_FADE_OUT` — the drain window inside the release (ms).
pub const GLOW_FADE_OUT_MS: u64 = 200;

/// `intensityAt(index, front, head, tail)` (tab-pulse.tsx:66-69): the
/// sweep band's spatial profile — a `head`-wide smootherstep ramp
/// ahead of the front, a `tail`-long falloff behind it.
#[must_use]
pub fn intensity_at(index: usize, front: f64, head: f64, tail: f64) -> f64 {
    let distance = front - f64::from(index as i32);
    if distance < 0.0 {
        smootherstep(clamp01(1.0 + distance / head))
    } else {
        smootherstep(clamp01(1.0 - distance / tail))
    }
}

/// `coast(value)` (tab-pulse.tsx:70-75): the sweep's eased traversal —
/// a quadratic ramp-in/out around a linear middle.
#[must_use]
pub fn coast(value: f64) -> f64 {
    let ramp = 0.2;
    let v = clamp01(value);
    if v < ramp {
        (v * v) / (2.0 * ramp * (1.0 - ramp))
    } else if v > 1.0 - ramp {
        1.0 - ((1.0 - v) * (1.0 - v)) / (2.0 * ramp * (1.0 - ramp))
    } else {
        (v - ramp / 2.0) / (1.0 - ramp)
    }
}

/// `fadeOut(progress)` = `1 − smootherstep(progress)`.
#[must_use]
pub fn fade_out(progress: f64) -> f64 {
    1.0 - smootherstep(clamp01(progress))
}

/// `attackDecay(progress, attack, peak, rest)` (tab-pulse.tsx:78-81):
/// rise to `peak` over the attack fraction, settle to `rest` after.
#[must_use]
pub fn attack_decay(progress: f64, attack: f64, peak: f64, rest: f64) -> f64 {
    let p = clamp01(progress);
    if p < attack {
        peak * smootherstep(clamp01(p / attack))
    } else {
        peak - (peak - rest) * smootherstep(clamp01((p - attack) / (1.0 - attack)))
    }
}

/// `glowIgnitionLevel` — the glow attack rises past 1 then settles.
#[must_use]
pub fn glow_ignition_level(progress: f64) -> f64 {
    attack_decay(progress, GLOW_IGNITION_ATTACK, GLOW_IGNITION_PEAK, 1.0)
}

/// `glowReleaseSwell` — the diffusing glow swells above its resting
/// level, then decays to nothing.
#[must_use]
pub fn glow_release_swell_shape(progress: f64) -> f64 {
    attack_decay(progress, GLOW_RELEASE_ATTACK, GLOW_RELEASE_PEAK, 0.0)
}

/// `glowReleaseSpread(progress, tail, width)` — how far a resolving
/// glow has diffused: the resting tail spreads across the full width.
// Terminal widths are far below 2^52 — the pedantic precision lint
// carries no real risk (the module's standing allowance).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn glow_release_spread(progress: f64, tail: f64, width: usize) -> f64 {
    tail + smootherstep(clamp01(progress)) * width.max(1) as f64
}

/// `blendTabPulseColor` (tab-pulse.tsx:116-130): the staged blend —
/// glow tint, then sweep, flash and completion each pull the running
/// color the rest of the way. (One argument per stage, the reference
/// signature.)
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn blend_tab_pulse_color(
    background: Rgb,
    glow_color: Rgb,
    running_color: Rgb,
    flash_color: Rgb,
    completion_color: Rgb,
    glow: f64,
    running: f64,
    flash: f64,
    completion: f64,
) -> Rgb {
    let mut out = if glow == 0.0 { background } else { tint(background, glow_color, glow) };
    if running != 0.0 {
        out = tint(out, running_color, running);
    }
    if flash != 0.0 {
        out = tint(out, flash_color, flash);
    }
    if completion != 0.0 {
        out = tint(out, completion_color, completion);
    }
    out
}

/// `elapsed(now, since)` in whole ms (monotonic, saturating).
fn elapsed(now: u64, since: u64) -> u64 {
    now.saturating_sub(since)
}

/// One tab's pulse history — the transition stamps the live path
/// records when a tab's status flips ([`TabPulseTrack::sync`], the
/// tsx prop setters' effect); every envelope level below derives
/// purely from a track + the clock.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TabPulseTrack {
    /// When the busy sweep began (`setActive(true)`).
    pub active_since: Option<u64>,
    /// When busy ended (the release fade's origin).
    pub idle_since: Option<u64>,
    /// When the unread glow ignited.
    pub glow_since: Option<u64>,
    /// When the glow released.
    pub glow_off_since: Option<u64>,
    /// When the completion flash fired.
    pub completion_since: Option<u64>,
}

impl TabPulseTrack {
    /// Stamp any status transition at `now`.
    pub fn sync(&mut self, runs: bool, glow: bool, complete: bool, now: u64) {
        if runs != self.active_since.is_some() {
            if runs {
                self.active_since = Some(now);
                self.idle_since = None;
                self.completion_since = None;
            } else {
                self.idle_since = Some(now);
            }
        }
        if glow != self.glow_since.is_some() {
            if glow {
                self.glow_since = Some(now);
                self.glow_off_since = None;
            } else {
                self.glow_off_since = Some(now);
            }
        }
        if complete && self.completion_since.is_none() {
            self.completion_since = Some(now);
        }
    }

    /// The run envelope's level: attack over [`RUN_ATTACK`] to sustain
    /// 1 while busy, then the [`RUN_FADE_OUT`] note-off from the
    /// level held at note-off (`GatedEnvelope.release`).
    #[must_use]
    pub fn running(&self, now: u64) -> f64 {
        let Some(active) = self.active_since else { return 0.0 };
        let Some(idle) = self.idle_since else {
            return smootherstep(clamp01(elapsed(now, active) as f64 / RUN_ATTACK as f64));
        };
        let note_off = smootherstep(clamp01(elapsed(idle, active) as f64 / RUN_ATTACK as f64));
        note_off * fade_out(clamp01(elapsed(now, idle) as f64 / RUN_FADE_OUT as f64))
    }

    /// The sweep clock: coasts from note-on THROUGH the release fade,
    /// stopped once the release completes (`advance`'s `!idle` gate).
    #[must_use]
    pub fn sweep_clock(&self, now: u64) -> u64 {
        let Some(active) = self.active_since else { return 0 };
        let stop = self.idle_since.map_or(u64::MAX, |idle| idle.saturating_add(RUN_FADE_OUT));
        now.min(stop).saturating_sub(active)
    }

    /// The sweep fronts at `now` (see [`fronts_at`]).
    #[must_use]
    pub fn fronts(&self, width: usize, now: u64) -> (f64, f64) {
        fronts_at(self.sweep_clock(now), width)
    }

    /// The glow envelope: ignition past 1 then sustain; on release,
    /// the note-off residue drains over the leading 200 ms (the
    /// release stage's drain shape).
    #[must_use]
    pub fn glow_level(&self, now: u64) -> f64 {
        if let Some(off) = self.glow_off_since {
            let note_off = self.glow_since.map_or(1.0, |on| {
                glow_ignition_level(clamp01(
                    elapsed(off, on) as f64 / GLOW_IGNITION_DURATION as f64,
                ))
            });
            let drain_fraction = GLOW_FADE_OUT_MS as f64 / GLOW_RELEASE_DURATION as f64;
            let progress = elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64;
            return note_off * fade_out(clamp01(progress / drain_fraction));
        }
        let Some(on) = self.glow_since else { return 0.0 };
        glow_ignition_level(clamp01(elapsed(now, on) as f64 / GLOW_IGNITION_DURATION as f64))
    }

    /// The completion flash (0.18 amplitude, 1200 ms attackDecay).
    #[must_use]
    pub fn completion(&self, now: u64) -> f64 {
        self.completion_since.map_or(0.0, |at| {
            COMPLETION_OPACITY
                * attack_decay(
                    clamp01(elapsed(now, at) as f64 / COMPLETION_DURATION as f64),
                    COMPLETION_ATTACK,
                    1.0,
                    0.0,
                )
        })
    }

    /// The diffusing swell riding a released glow (above its resting
    /// level, decaying to nothing over the 900 ms release).
    #[must_use]
    pub fn glow_release_swell(&self, now: u64) -> f64 {
        let Some(off) = self.glow_off_since else { return 0.0 };
        let progress = clamp01(elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64);
        let note_off = self.glow_since.map_or(1.0, |on| {
            glow_ignition_level(clamp01(elapsed(off, on) as f64 / GLOW_IGNITION_DURATION as f64))
        });
        glow_release_swell_shape(progress) * note_off.max(1.0)
    }

    /// How far the released glow has diffused (the tail spreads
    /// across the full width as it thins away).
    #[must_use]
    pub fn glow_release_spread(&self, tail: f64, width: usize, now: u64) -> f64 {
        let Some(off) = self.glow_off_since else { return 0.0 };
        glow_release_spread(
            clamp01(elapsed(now, off) as f64 / GLOW_RELEASE_DURATION as f64),
            tail,
            width,
        )
    }
}

/// `PulseState.fronts(width)` (tab-pulse.tsx:404-417): the two sweep
/// fronts — the primary pass coasts left-to-right over [`RUN_DURATION`],
/// a second pass trailing half a cycle behind.
#[must_use]
pub fn fronts_at(sweep_clock: u64, width: usize) -> (f64, f64) {
    let cycles = sweep_clock as f64 / RUN_DURATION as f64;
    let progress = cycles.fract();
    let start = -RUN_HEAD;
    let end = (width.saturating_sub(1)).max(1) as f64 + RUN_TAIL;
    let span = end - start;
    let second_progress = if cycles < 0.5 { 0.0 } else { (cycles + 0.5).fract() };
    (start + coast(progress) * span, start + coast(second_progress) * span)
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

    // -- lane TAIL: the animated phases ----------------------------------

    /// `coast` — the reference's own anchors (the quadratic feet).
    #[test]
    fn coast_matches_the_reference_anchors() {
        assert_eq!(coast(0.0), 0.0);
        // v=0.2: (0.2²)/(2·0.2·0.8) = 0.04/0.32 = 0.125
        assert!((coast(0.2) - 0.125).abs() < 1e-12);
        assert!((coast(0.5) - 0.5).abs() < 1e-12);
        assert_eq!(coast(1.0), 1.0);
        // v=0.8: 1 − (0.2²)/0.32 = 0.875
        assert!((coast(0.8) - 0.875).abs() < 1e-12);
    }

    /// `intensityAt` — the sweep band: full at the front, a 4-cell
    /// head ahead, an 18-cell tail behind.
    #[test]
    fn sweep_band_profile_matches_the_reference() {
        assert!((intensity_at(10, 10.0, RUN_HEAD, RUN_TAIL) - 1.0).abs() < 1e-12);
        // 4 cells AHEAD of the front (index > front): head branch → 0.
        assert_eq!(intensity_at(14, 10.0, RUN_HEAD, RUN_TAIL), 0.0);
        // 2 cells ahead: 1 − 2/4 = 0.5 → smootherstep(0.5) = 0.5.
        assert!((intensity_at(12, 10.0, RUN_HEAD, RUN_TAIL) - 0.5).abs() < 1e-12);
        assert_eq!(intensity_at(28, 10.0, RUN_HEAD, RUN_TAIL), 0.0);
    }

    /// The run envelope: attack over 450 ms to sustain 1, hold while
    /// busy, release over 500 ms after — all from the stamps.
    #[test]
    fn run_envelope_attacks_sustains_and_releases() {
        let mut track = TabPulseTrack::default();
        assert_eq!(track.running(0), 0.0, "silent before the first busy");
        track.sync(true, false, false, 0);
        assert_eq!(track.running(0), 0.0, "attack origin at the stamp");
        assert!((track.running(225) - 0.5).abs() < 1e-9, "attack midpoint");
        assert!((track.running(450) - 1.0).abs() < 1e-9, "sustain after the attack");
        assert!((track.running(5_000) - 1.0).abs() < 1e-9, "sustains while busy");
        track.sync(false, false, false, 5_000);
        assert!((track.running(5_250) - 0.5).abs() < 1e-9, "release midpoint");
        assert_eq!(track.running(5_500), 0.0, "silent after the release");
        // The sweep clock stops when the release completes.
        assert_eq!(track.sweep_clock(9_000), 5_500, "sweep clamped at idle+fade");
    }

    /// The sweep front crosses cells left-to-right (`fronts_at`): the
    /// front starts at −4, coasts across `width − 1 + 18`, and a
    /// second pass joins half a cycle in.
    #[test]
    fn sweep_fronts_cross_left_to_right() {
        let width = 32usize;
        let (f0, f1) = fronts_at(0, width);
        assert_eq!(f0, -4.0);
        assert_eq!(f1, -4.0, "no second pass in the first half-cycle");
        let (f0, f1) = fronts_at(1_400, width);
        // coast(0.5) = 0.5 → f0 = −4 + 0.5·(31 + 18 + 4) = 22.5
        assert!((f0 - 22.5).abs() < 1e-9, "mid-cycle front: {f0}");
        assert!((f1 - -4.0).abs() < 1e-9, "second pass starts at mid-cycle");
        // A mid-strip cell sees the band pass: rises to ~1 then falls.
        let cell = 20usize;
        let mut seen_peak = false;
        let mut values = Vec::new();
        for step in 0..=28 {
            let (f0, f1) = fronts_at(step * 100, width);
            values.push(
                intensity_at(cell, f0, RUN_HEAD, RUN_TAIL)
                    .max(intensity_at(cell, f1, RUN_HEAD, RUN_TAIL)),
            );
            if values.last().is_some_and(|v| *v > 0.99) {
                seen_peak = true;
            }
        }
        assert!(seen_peak, "the band crosses cell {cell}: {values:?}");
    }

    /// The glow envelope: ignition peaks at 1.5 then settles to 1;
    /// release drains over the leading 200 ms with a swelling spread.
    #[test]
    fn glow_envelope_ignites_and_releases() {
        let mut track = TabPulseTrack::default();
        assert_eq!(track.glow_level(0), 0.0, "no glow before ignition");
        track.sync(false, true, false, 0);
        assert!((track.glow_level(180) - 1.5).abs() < 1e-9, "ignition peak at 30% of 600 ms");
        assert!((track.glow_level(601) - 1.0).abs() < 1e-6, "settled to sustain");
        track.sync(false, false, false, 1_000);
        assert_eq!(track.glow_level(1_000), 1.0, "note-off held at the drain's origin");
        assert!((track.glow_level(1_100) - 0.5).abs() < 1e-9, "halfway through the drain");
        assert!(track.glow_release_swell(1_100) > 1.0, "the swell rides above rest");
        assert!(track.glow_release_spread(12.0, 32, 1_450) > 12.0, "the tail spreads");
    }

    /// The completion flash: the activity unread landing after idle
    /// fires the 0.18 wash (1200 ms attackDecay).
    #[test]
    fn completion_flash_fires_when_the_turn_resolves() {
        let mut track = TabPulseTrack::default();
        track.sync(true, false, false, 0);
        track.sync(false, false, false, 4_000);
        assert_eq!(track.completion(4_000), 0.0, "idle without a completion yet");
        track.sync(false, false, true, 4_100);
        assert!(track.completion(4_244) > 0.17, "near the 0.18 peak");
        assert_eq!(track.completion(5_400), 0.0, "decays away");
    }

    /// `blend_tab_pulse_color` — the staged pull.
    #[test]
    fn staged_blend_pulls_toward_each_stage() {
        let bg = (10, 10, 10);
        let glow = (157, 124, 216);
        let run = (100, 100, 100);
        let only_glow = blend_tab_pulse_color(bg, glow, run, run, run, 0.5, 0.0, 0.0, 0.0);
        assert_eq!(only_glow, tint(bg, glow, 0.5));
        let both = blend_tab_pulse_color(bg, glow, run, run, run, 0.5, 0.5, 0.0, 0.0);
        assert_eq!(both, tint(only_glow, run, 0.5));
        assert_eq!(blend_tab_pulse_color(bg, glow, run, run, run, 0.0, 0.0, 0.0, 0.0), bg);
    }
}
