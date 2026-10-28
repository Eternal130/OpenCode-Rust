//! PNG → sixel transcoding (RB-07 residual, registry gap).
//!
//! Sixel terminals cannot display PNG bytes; until this module the
//! sixel path only passed *raw* sixel data through (DCS `q`…ST) and
//! PNG attachments fell back to the blocks renderer. This module
//! closes that residual: it reuses the pure-Rust PNG decoder
//! ([`crate::image_blocks::decode_png`], already in the crate — the
//! `image` crate is *not* in the workspace dependency graph, and
//! adding it would be a new lock entry for nothing), quantizes the
//! decoded RGBA to ≤ 16 color registers (median cut, deterministic —
//! no dithering, matching the crate's golden-test discipline), and
//! emits a standard sixel DCS sequence:
//!
//! ```text
//! ESC P q "1;1;W;H #0;2;r;g;b … #n;2;r;g;b <color bands> ESC \
//! ```
//!
//! * **color registers** `#i;2;r;g;b` — RGB in sixel's 0–100
//!   percent scale.
//! * **raster bands** — rows group in bands of 6 (sixel = six
//!   pixels); each band emits one data run per color, `?`-based
//!   charset (0x3F + row mask), `$` column skips, `!` run-length
//!   repeats, `-` between bands.
//! * **transparency** — alpha < 128 pixels are never painted (no
//!   register claims them), so the terminal shows the cells behind.
//! * **size cap** — huge payloads downscale (box-average) to
//!   [`MAX_DIM`] on the longest side before quantizing, keeping the
//!   emitted stream bounded and deterministic.
//!
//! Cursor placement stays the caller's business (the existing
//! passthrough semantics: [`crate::sixel_display_escape`] CUPs to
//! the rect first); no DECSTBM/origin tricks here.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]

use crate::image_blocks::{decode_png, Rgba8Image};

/// Longest-side pixel cap before quantizing (a "400×400 cells-ish"
/// bound on the emitted raster — thumbnails are far below it).
pub const MAX_DIM: usize = 400;
/// Sixel color-register budget (attribute-mode-0 friendly).
pub const MAX_COLORS: usize = 16;
/// Rows per sixel band (sixel = six vertical pixels).
const BAND: usize = 6;

/// Transcode PNG bytes to a complete sixel escape sequence
/// (`ESC P q … ESC \`), or `None` if the PNG fails to decode (the
/// caller keeps the placeholder/blocks fallback).
#[must_use]
pub fn encode_sixel_from_png(png: &[u8]) -> Option<Vec<u8>> {
    let image = decode_png(png)?;
    let image = downscale(image);
    let (indexed, palette) = quantize(&image);
    Some(emit(&image, &indexed, &palette))
}

/// Box-average downscale when the longest side exceeds
/// [`MAX_DIM`]; otherwise the image passes through unchanged.
fn downscale(image: Rgba8Image) -> Rgba8Image {
    let longest = image.width.max(image.height);
    if longest <= MAX_DIM {
        return image;
    }
    let scale = longest as f64 / MAX_DIM as f64;
    let w = ((image.width as f64 / scale).round() as usize).max(1);
    let h = ((image.height as f64 / scale).round() as usize).max(1);
    let mut data = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            // The exact source box this target pixel covers.
            let x0 = (x * image.width) / w;
            let x1 = (((x + 1) * image.width) / w).max(x0 + 1);
            let y0 = (y * image.height) / h;
            let y1 = (((y + 1) * image.height) / h).max(y0 + 1);
            let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for sy in y0..y1.min(image.height) {
                for sx in x0..x1.min(image.width) {
                    let i = (sy * image.width + sx) * 4;
                    r += u32::from(image.data[i]);
                    g += u32::from(image.data[i + 1]);
                    b += u32::from(image.data[i + 2]);
                    a += u32::from(image.data[i + 3]);
                    n += 1;
                }
            }
            data.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8]);
        }
    }
    Rgba8Image { width: w, height: h, data }
}

/// One median-cut box: a run of pixels inside a channel range.
struct Box {
    pixels: Vec<[u8; 3]>, // premultiplied-over-black RGB, opaque only
}

impl Box {
    fn widest_channel(&self) -> usize {
        let mut lo = [255u8; 3];
        let mut hi = [0u8; 3];
        for p in &self.pixels {
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
        let ranges = [u16::from(hi[0] - lo[0]), u16::from(hi[1] - lo[1]), u16::from(hi[2] - lo[2])];
        if ranges[0] >= ranges[1] && ranges[0] >= ranges[2] {
            0
        } else if ranges[1] >= ranges[2] {
            1
        } else {
            2
        }
    }

    fn average(&self) -> [u8; 3] {
        let mut sum = [0u64; 3];
        for p in &self.pixels {
            for c in 0..3 {
                sum[c] += u64::from(p[c]);
            }
        }
        let n = self.pixels.len().max(1) as u64;
        [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
    }
}

/// Median-cut quantization to ≤ [`MAX_COLORS`] registers. Returns
/// per-pixel palette indices (alpha < 128 → `None`, unpainted) and
/// the palette. Deterministic: stable sorts, fixed tie-breaks.
fn quantize(image: &Rgba8Image) -> (Vec<Option<u8>>, Vec<[u8; 3]>) {
    // Flatten premultiplied opaque pixels (the blocks sampling rule).
    let mut opaque: Vec<[u8; 3]> = Vec::new();
    let mut indexed = Vec::with_capacity(image.width * image.height);
    for i in 0..image.width * image.height {
        let a = image.data[i * 4 + 3];
        if a < 128 {
            indexed.push(None);
        } else {
            let a = u16::from(a);
            let px = [
                ((u16::from(image.data[i * 4]) * a) / 255) as u8,
                ((u16::from(image.data[i * 4 + 1]) * a) / 255) as u8,
                ((u16::from(image.data[i * 4 + 2]) * a) / 255) as u8,
            ];
            indexed.push(Some(0)); // fixed up after the palette lands
            opaque.push(px);
        }
    }

    if opaque.is_empty() {
        // Nothing paints; one dummy register keeps the sequence valid.
        return (indexed, vec![[0, 0, 0]]);
    }

    // Median cut: split the widest-channel box at its median until
    // the register budget is spent.
    let mut boxes = vec![Box { pixels: opaque }];
    while boxes.len() < MAX_COLORS {
        // Widest-range, splittable box (≥ 2 distinct pixels).
        let mut pick: Option<usize> = None;
        let mut pick_range = 0u16;
        for (bi, b) in boxes.iter().enumerate() {
            if b.pixels.len() < 2 {
                continue;
            }
            let ch = b.widest_channel();
            let (mut lo, mut hi) = (255u8, 0u8);
            for p in &b.pixels {
                lo = lo.min(p[ch]);
                hi = hi.max(p[ch]);
            }
            let range = u16::from(hi - lo);
            if range > pick_range {
                pick_range = range;
                pick = Some(bi);
            }
        }
        let Some(bi) = pick else { break };
        let ch = boxes[bi].widest_channel();
        let mut pixels = std::mem::take(&mut boxes[bi].pixels);
        pixels.sort_by_key(|p| p[ch]); // stable → deterministic
        let mid = pixels.len() / 2;
        let tail = pixels.split_off(mid);
        boxes[bi] = Box { pixels };
        boxes.push(Box { pixels: tail });
    }

    let palette: Vec<[u8; 3]> = boxes.iter().map(Box::average).collect();
    // Nearest-register pass: every opaque pixel maps to its closest
    // palette entry (Euclidean; ties break on the lower index).
    remap(image, &palette, &mut indexed);
    (indexed, palette)
}

/// Nearest-register mapping off the decoded pixels.
fn remap(image: &Rgba8Image, palette: &[[u8; 3]], indexed: &mut [Option<u8>]) {
    for i in 0..image.width * image.height {
        if indexed[i].is_none() {
            continue;
        }
        let a = u16::from(image.data[i * 4 + 3]);
        let px = [
            ((u16::from(image.data[i * 4]) * a) / 255) as u8,
            ((u16::from(image.data[i * 4 + 1]) * a) / 255) as u8,
            ((u16::from(image.data[i * 4 + 2]) * a) / 255) as u8,
        ];
        let mut best = 0u8;
        let mut best_d = u64::MAX;
        for (ci, c) in palette.iter().enumerate() {
            let d = u64::from(px[0].saturating_sub(c[0])).pow(2)
                + u64::from(px[1].saturating_sub(c[1])).pow(2)
                + u64::from(px[2].saturating_sub(c[2])).pow(2);
            if d < best_d {
                best_d = d;
                best = ci as u8;
            }
        }
        indexed[i] = Some(best);
    }
}

/// Emit the sixel DCS body: raster attributes, color registers,
/// then the banded pixel data.
fn emit(image: &Rgba8Image, indexed: &[Option<u8>], palette: &[[u8; 3]]) -> Vec<u8> {
    let (w, h) = (image.width, image.height);
    let mut out = Vec::with_capacity(64 + w * h / 4);
    out.extend_from_slice(b"\x1bPq");
    out.extend_from_slice(format!("\"1;1;{w};{h}").as_bytes());
    for (i, c) in palette.iter().enumerate() {
        // RGB in sixel's 0–100 percent scale (HIMILTI rounding down
        // is the de-facto encoder convention; deterministic either way).
        let (r, g, b) =
            (u32::from(c[0]) * 100 / 255, u32::from(c[1]) * 100 / 255, u32::from(c[2]) * 100 / 255);
        out.extend_from_slice(format!("#{i};2;{r};{g};{b}").as_bytes());
    }
    let bands = h.div_ceil(BAND);
    for band in 0..bands {
        let y0 = band * BAND;
        let y1 = (y0 + BAND).min(h);
        for (ci, _) in palette.iter().enumerate() {
            // One mask byte per column: bit r set = pixel at band
            // row r belongs to this register.
            let masks: Vec<u8> = (0..w)
                .map(|x| {
                    let mut m = 0u8;
                    for (y, row) in (y0..y1).enumerate() {
                        if indexed[row * w + x] == Some(ci as u8) {
                            m |= 1 << y;
                        }
                    }
                    m
                })
                .collect();
            if masks.iter().all(|m| *m == 0) {
                continue;
            }
            out.extend_from_slice(format!("#{ci}").as_bytes());
            let mut x = 0;
            while x < w {
                if masks[x] == 0 {
                    out.push(b'$');
                    x += 1;
                    continue;
                }
                let mut run = 1;
                while x + run < w && masks[x + run] == masks[x] {
                    run += 1;
                }
                let ch = 0x3F + masks[x];
                if run >= 4 {
                    out.extend_from_slice(format!("!{run}").as_bytes());
                    out.push(ch);
                } else {
                    out.extend(std::iter::repeat_n(ch, run));
                }
                x += run;
            }
        }
        if band + 1 < bands {
            out.push(b'-');
        }
    }
    out.extend_from_slice(b"\x1b\\");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write as IoWrite;

    /// Minimal deterministic PNG writer: 8-bit truecolor RGBA, no
    /// interlace, one IDAT (the same shape the blocks tests use).
    fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
        let mut raw = Vec::with_capacity(height * (1 + width * 4));
        for y in 0..height {
            raw.push(0); // filter: none
            raw.extend_from_slice(&rgba[y * width * 4..(y + 1) * width * 4]);
        }
        let mut idat = ZlibEncoder::new(Vec::new(), Compression::default());
        idat.write_all(&raw).unwrap();
        let idat = idat.finish().unwrap();

        let ihdr = {
            let mut v = (width as u32).to_be_bytes().to_vec();
            v.extend_from_slice(&(height as u32).to_be_bytes());
            v.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA
            v
        };
        let mut png = Vec::new();
        png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let put = |png: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(tag);
            png.extend_from_slice(data);
            let mut c = flate2::Crc::new();
            c.update(tag);
            c.update(data);
            png.extend_from_slice(&c.sum().to_be_bytes());
        };
        put(&mut png, b"IHDR", &ihdr);
        put(&mut png, b"IDAT", &idat);
        put(&mut png, b"IEND", &[]);
        png
    }

    fn solid(width: usize, height: usize, px: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..width * height {
            v.extend_from_slice(&px);
        }
        v
    }

    #[test]
    fn golden_sixel_structure_is_deterministic() {
        // 4×2: left half red, right half blue.
        let mut rgba = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                rgba.extend_from_slice(if x < 2 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        let png = encode_png(4, 2, &rgba);
        let sixel = encode_sixel_from_png(&png).expect("transcodes");
        // Structural golden: DCS open/close and register definitions.
        assert!(sixel.starts_with(b"\x1bPq"), "opens with DCS Pq");
        assert!(sixel.ends_with(b"\x1b\\"), "closes with ST");
        let text = String::from_utf8_lossy(&sixel).into_owned();
        assert!(text.contains("#0;2;"), "defines color register 0");
        assert!(text.contains("\"1;1;4;2"), "raster attributes carry WxH");
        // Deterministic: same input, byte-identical output.
        assert_eq!(sixel, encode_sixel_from_png(&png).expect("second run"));
    }

    #[test]
    fn transparent_pixels_leave_no_register_runs() {
        // Fully transparent 2×2: no color's data band ever runs.
        let png = encode_png(2, 2, &solid(2, 2, [0, 0, 0, 0]));
        let sixel = encode_sixel_from_png(&png).expect("transcodes");
        let text = String::from_utf8_lossy(&sixel);
        assert!(text.contains("#0;2;"), "one dummy register");
        // No sixel data chars beyond the 0x3F baseline anywhere in the
        // DCS body (strip the ESC P q introducer — 'P'/'q' — and the
        // ST terminator — '\').
        let body = &sixel[3..sixel.len() - 2];
        assert!(!body.iter().any(|b| (0x40..=0x7E).contains(b)), "no painted pixels");
    }

    #[test]
    fn huge_pngs_downscale_to_the_cap() {
        let png = encode_png(900, 900, &solid(900, 900, [10, 200, 30, 255]));
        let sixel = encode_sixel_from_png(&png).expect("transcodes");
        let text = String::from_utf8_lossy(&sixel);
        assert!(text.contains(&format!("\"1;1;{MAX_DIM};{MAX_DIM}")), "capped raster");
    }

    #[test]
    fn undecodable_png_yields_none() {
        assert_eq!(encode_sixel_from_png(b"not a png"), None);
        let png = encode_png(2, 2, &solid(2, 2, [1, 2, 3, 255]));
        assert_eq!(encode_sixel_from_png(&png[..png.len() - 5]), None);
    }
}
