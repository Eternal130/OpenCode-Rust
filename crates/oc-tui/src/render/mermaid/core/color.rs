// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! `core/color/style.ts` — the RGBA type, blends, and color ramps.

/// The canvas-agnostic RGBA (upstream `@opentui/core`'s `RGBA`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    #[must_use]
    pub fn from_ints(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    #[must_use]
    pub fn blend_to(self, to: Self, amount: f64) -> Self {
        let mix = |left: f64, right: f64| left + (right - left) * amount;
        Self {
            r: mix(f64::from(self.r), f64::from(to.r)).round().clamp(0.0, 255.0) as u8,
            g: mix(f64::from(self.g), f64::from(to.g)).round().clamp(0.0, 255.0) as u8,
            b: mix(f64::from(self.b), f64::from(to.b)).round().clamp(0.0, 255.0) as u8,
            a: mix(f64::from(self.a), f64::from(to.a)).round().clamp(0.0, 255.0) as u8,
        }
    }
}

/// `blendColor`.
#[must_use]
pub fn blend_color(from: Option<Rgba>, to: Option<Rgba>, amount: f64) -> Option<Rgba> {
    match (from, to) {
        (None, None) => None,
        (None, to) => to,
        (from, None) => from,
        (Some(from), Some(to)) => Some(from.blend_to(to, amount)),
    }
}

#[must_use]
pub fn colors_equal(left: Option<Rgba>, right: Option<Rgba>) -> bool {
    left == right
}

/// `createColorRampTheme` — one blended color per step
/// (`(index + 1) / (styles + 1)`).
#[must_use]
pub fn color_ramp(steps: usize, from: Option<Rgba>, to: Option<Rgba>) -> Vec<Option<Rgba>> {
    (1..=steps)
        .map(|index| blend_color(from, to, f64::from(index as u32) / f64::from(steps as u32 + 1)))
        .collect()
}

/// `DIAGRAM_FADE_STEPS`.
pub const DIAGRAM_FADE_STEPS: usize = 5;
