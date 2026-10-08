//! RGBA color values and the OKLCH math used by theme generation.
//!
//! The OKLCH conversion is a verbatim port of `packages/theme/src/tui/color.ts`
//! (Björn Ottosson's Oklab matrices plus a chroma-decay gamut fit). It is
//! implemented locally instead of pulling in a color crate so the arithmetic
//! matches the TypeScript pipeline bit-for-bit: v1 theme migration quantizes
//! generated scales down to hex bytes and golden tests compare every byte.

use serde::{ser::SerializeTuple, Serialize};

/// An 8-bit-per-channel RGBA color. Alpha `255` is opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn from_ints(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn to_ints(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Fully transparent black, the resolved value of `"transparent"`.
    pub const TRANSPARENT: Self = Self::from_ints(0, 0, 0, 0);

    /// Parses `#rgb`, `#rgba`, `#rrggbb` and `#rrggbbaa` (case-insensitive),
    /// mirroring `RGBA.fromHex` in `@opentui/core`: 3/4-digit forms repeat
    /// every digit. Returns `None` for anything else.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let body = hex.strip_prefix('#')?;
        let expanded: String = match body.len() {
            3 => body.chars().flat_map(|c| [c, c]).collect(),
            4 => body.chars().flat_map(|c| [c, c]).collect(),
            6 | 8 => body.to_string(),
            _ => return None,
        };
        if !expanded.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&expanded[range], 16).ok();
        Some(Self::from_ints(
            channel(0..2)?,
            channel(2..4)?,
            channel(4..6)?,
            if expanded.len() == 8 { channel(6..8)? } else { 255 },
        ))
    }

    /// `#rrggbb`, or `#rrggbbaa` when the alpha is not fully opaque
    /// (lowercase, matching the TS `hex()` helper).
    pub fn hex(self) -> String {
        let [r, g, b, a] = self.to_ints();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    /// Integer channels in `0..=255` scaled to `0.0..=1.0`, as used by the
    /// TS luminance helpers (`color.r` getters).
    pub fn floats(self) -> [f64; 4] {
        let [r, g, b, a] = self.to_ints();
        [f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0, f64::from(a) / 255.0]
    }
}

impl Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = serializer.serialize_tuple(4)?;
        for channel in self.to_ints() {
            tuple.serialize_element(&channel)?;
        }
        tuple.end()
    }
}

/// An OKLCH color (lightness `0..=1`, chroma, hue degrees `0..360`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn hue_degrees(value: f64) -> f64 {
    (value % 360.0 + 360.0) % 360.0
}

fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// `rgbToOklch` from `color.ts`. Channels are `0.0..=1.0` sRGB floats.
pub fn rgb_to_oklch(red: f64, green: f64, blue: f64) -> Oklch {
    let linear_red = srgb_to_linear(red);
    let linear_green = srgb_to_linear(green);
    let linear_blue = srgb_to_linear(blue);
    let l_root = (0.412_221_470_8 * linear_red
        + 0.536_332_536_3 * linear_green
        + 0.051_445_992_9 * linear_blue)
        .cbrt();
    let m_root = (0.211_903_498_2 * linear_red
        + 0.680_699_545_1 * linear_green
        + 0.107_396_956_6 * linear_blue)
        .cbrt();
    let s_root = (0.088_302_461_9 * linear_red
        + 0.281_718_837_6 * linear_green
        + 0.629_978_700_5 * linear_blue)
        .cbrt();
    let lightness = 0.210_454_255_3 * l_root + 0.793_617_785 * m_root - 0.004_072_046_8 * s_root;
    let a = 1.977_998_495_1 * l_root - 2.428_592_205 * m_root + 0.450_593_709_9 * s_root;
    let b = 0.025_904_037_1 * l_root + 0.782_771_766_2 * m_root - 0.808_675_766 * s_root;
    let chroma = (a * a + b * b).sqrt();
    let angle = f64::atan2(b, a) * (180.0 / std::f64::consts::PI);
    Oklch { l: lightness, c: chroma, h: if angle < 0.0 { angle + 360.0 } else { angle } }
}

/// `toOklch` from `v1-migrate.ts`: integer channels in.
pub fn ints_to_oklch(r: u8, g: u8, b: u8) -> Oklch {
    rgb_to_oklch(f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0)
}

fn oklch_to_rgb(color: &Oklch) -> (f64, f64, f64) {
    let a = color.c * (color.h * std::f64::consts::PI / 180.0).cos();
    let b = color.c * (color.h * std::f64::consts::PI / 180.0).sin();
    let l_root = color.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_root = color.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_root = color.l - 0.089_484_177_5 * a - 1.291_485_548 * b;
    let l = l_root * l_root * l_root;
    let m = m_root * m_root * m_root;
    let s = s_root * s_root * s_root;
    (
        linear_to_srgb(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        linear_to_srgb(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        linear_to_srgb(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    )
}

fn in_gamut(rgb: (f64, f64, f64)) -> bool {
    let (r, g, b) = rgb;
    (0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b)
}

fn fit_oklch(color: &Oklch) -> Oklch {
    let base = Oklch { l: clamp(color.l, 0.0, 1.0), c: color.c.max(0.0), h: hue_degrees(color.h) };
    if in_gamut(oklch_to_rgb(&base)) {
        return base;
    }
    for index in 1..=24 {
        let next = Oklch { c: base.c * 0.9_f64.powf(f64::from(index)), ..base };
        if in_gamut(oklch_to_rgb(&next)) {
            return next;
        }
    }
    Oklch { c: 0.0, ..base }
}

/// `oklchToHex` from `color.ts`: gamut-fits, converts and rounds to `#rrggbb`.
pub fn oklch_to_hex(color: &Oklch) -> String {
    let (r, g, b) = oklch_to_rgb(&fit_oklch(color));
    let to_hex = |value: f64| format!("{:02x}", (clamp(value, 0.0, 1.0) * 255.0).round() as u8);
    format!("#{}{}{}", to_hex(r), to_hex(g), to_hex(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(Rgba::from_hex("#fff"), Some(Rgba::from_ints(255, 255, 255, 255)));
        assert_eq!(Rgba::from_hex("#fffc"), Some(Rgba::from_ints(255, 255, 255, 204)));
        assert_eq!(Rgba::from_hex("#282A36"), Some(Rgba::from_ints(0x28, 0x2a, 0x36, 255)));
        assert_eq!(Rgba::from_hex("#282a3680"), Some(Rgba::from_ints(0x28, 0x2a, 0x36, 0x80)));
        assert_eq!(Rgba::from_hex("282a36"), None);
        assert_eq!(Rgba::from_hex("#12345"), None);
        assert_eq!(Rgba::from_hex("#zzzzzz"), None);
    }

    #[test]
    fn hex_round_trip_matches_ts_format() {
        assert_eq!(Rgba::from_ints(0x28, 0x2a, 0x36, 255).hex(), "#282a36");
        assert_eq!(Rgba::from_ints(0x28, 0x2a, 0x36, 0x0a).hex(), "#282a360a");
    }

    #[test]
    fn oklch_matches_reference_values() {
        // Reference values produced by the TS `rgbToOklch` from color.ts.
        let cases: [(&str, [u8; 3], f64, f64, f64); 5] = [
            (
                "#bd93f9",
                [0xbd, 0x93, 0xf9],
                0.742_022_980_726_796,
                0.148_545_857_379_727_14,
                301.883_094_577_765_35,
            ),
            (
                "#ff5555",
                [0xff, 0x55, 0x55],
                0.682_203_626_615_577_4,
                0.206_328_451_084_358_1,
                24.430_965_357_126_66,
            ),
            (
                "#f8f8f2",
                [0xf8, 0xf8, 0xf2],
                0.977_476_616_069_954_4,
                0.007_913_279_981_609_535,
                106.545_019_273_102_71,
            ),
            (
                "#282a36",
                [0x28, 0x2a, 0x36],
                0.288_228_978_180_492_37,
                0.022_103_291_487_712_493,
                277.508_664_123_661_16,
            ),
            ("#000000", [0x00, 0x00, 0x00], 0.0, 0.0, 0.0),
        ];
        for (_, ints, l, c, h) in cases {
            let value = ints_to_oklch(ints[0], ints[1], ints[2]);
            assert!((value.l - l).abs() < 1e-12, "l {}", value.l);
            assert!((value.c - c).abs() < 1e-12, "c {}", value.c);
            assert!((value.h - h).abs() < 1e-9, "h {}", value.h);
        }
    }

    #[test]
    fn oklch_to_hex_gamut_fits() {
        assert_eq!(oklch_to_hex(&Oklch { l: 0.5, c: 0.4, h: 30.0 }), "#b71a0d");
        assert_eq!(oklch_to_hex(&Oklch { l: 0.99, c: 0.3, h: 150.0 }), "#fcfcfc");
        assert_eq!(oklch_to_hex(&Oklch { l: 0.97, c: 0.0, h: 0.0 }), "#f5f5f5");
    }
}
