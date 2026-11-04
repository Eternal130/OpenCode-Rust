//! PNG decoding for the image lanes (the raster/emission family lives
//! in opentui since lane D3-4b).
//!
//! v2 decodes through opentui's native image pipeline (libwebp /
//! wuffs / stb per the bundled licenses). The Rust port decodes just
//! enough PNG for pasted screenshots: 8/16-bit depths, gray /
//! gray+alpha / RGB / RGBA / palette+tRNS, all five row filters.
//! Adam7-interlaced images (vanishingly rare for screenshots) and
//! sub-byte depths decode to `None` — the caller keeps the "No
//! preview" placeholder, exactly like a v2 load error.
//!
//! The decoded pixels feed two opentui lanes:
//! * `NativeImage::from_rgba` for sixel encoding (`raw()` is the
//!   encoder's input; encoded-only images would stay undecoded);
//! * `NativeImage::from_rgba` + `draw_image(Blocks)` +
//!   `materialize_image_fallbacks` for the blocks cell grid.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use flate2::read::ZlibDecoder;
use std::io::Read;

/// A decoded RGBA image, row-major, 4 bytes per pixel (straight
/// alpha, sRGB — the opentui paths premultiply/quantize themselves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The raw pieces of a parsed PNG stream (the chunk scan's haul).
struct PngChunks {
    width: usize,
    height: usize,
    bit_depth: u8,
    color_type: u8,
    interlace: bool,
    palette: Vec<[u8; 3]>,
    transparency: Vec<u8>,
    idat: Vec<u8>,
}

/// Walk the chunk stream (IHDR/PLTE/tRNS/IDAT/IEND + skips).
fn scan_png_chunks(bytes: &[u8]) -> Option<PngChunks> {
    let mut pos = 8usize;
    let mut chunks = PngChunks {
        width: 0,
        height: 0,
        bit_depth: 0,
        color_type: 0,
        interlace: false,
        palette: Vec::new(),
        transparency: Vec::new(),
        idat: Vec::new(),
    };
    let mut saw_iend = false;
    while pos + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[pos..pos + 4].try_into().ok()?) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.checked_add(length)?;
        if data_end + 4 > bytes.len() {
            return None;
        }
        let data = &bytes[data_start..data_end];
        match kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return None;
                }
                chunks.width = u32::from_be_bytes(data[0..4].try_into().ok()?) as usize;
                chunks.height = u32::from_be_bytes(data[4..8].try_into().ok()?) as usize;
                chunks.bit_depth = data[8];
                chunks.color_type = data[9];
                chunks.interlace = data[12] != 0;
            }
            b"PLTE" => {
                let (triples, _) = data.as_chunks::<3>();
                chunks.palette = triples.iter().map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => {
                chunks.transparency = data.to_vec();
            }
            b"IDAT" => {
                chunks.idat.extend_from_slice(data);
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
        pos = data_end + 4; // skip the CRC
    }
    if !saw_iend
        || chunks.interlace
        || chunks.width == 0
        || chunks.height == 0
        || chunks.idat.is_empty()
    {
        return None;
    }
    Some(chunks)
}

/// Decode a PNG into an RGBA8 image (`None` for anything the
/// scope above does not cover, or malformed input).
#[must_use]
pub fn decode_png(bytes: &[u8]) -> Option<Rgba8Image> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return None;
    }
    let PngChunks { width, height, bit_depth, color_type, palette, transparency, idat, .. } =
        scan_png_chunks(bytes)?;
    let channels = match color_type {
        0 | 3 => 1usize, // gray / palette index
        2 => 3,          // RGB
        4 => 2,          // gray + alpha
        6 => 4,          // RGBA
        _ => return None,
    };
    let depth_ok = if color_type == 3 { bit_depth == 8 } else { bit_depth == 8 || bit_depth == 16 };
    if !depth_ok {
        return None; // sub-byte depths: out of scope
    }
    let sample_bytes = if bit_depth == 16 { 2usize } else { 1 };
    let bpp = channels * sample_bytes; // filter unit
    let stride = width * bpp;
    let mut raw = Vec::new();
    let mut decoder = ZlibDecoder::new(idat.as_slice());
    decoder.read_to_end(&mut raw).ok()?;
    let expected = height.checked_mul(stride + 1)?;
    if raw.len() < expected {
        return None;
    }
    let mut pixels = vec![0u8; width * height * 4];
    let mut prev = vec![0u8; stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        let mut row = line.to_vec();
        unfilter(filter, bpp, stride, &prev, &mut row)?;
        for x in 0..width {
            let source = x * bpp;
            // 16-bit samples downshift to their high byte (the 8-bit
            // display path's precision).
            let sample = |channel: usize| row[source + channel * sample_bytes];
            let [r, g, b, a] = match color_type {
                0 => {
                    let value = sample(0);
                    [value, value, value, 255]
                }
                2 => [sample(0), sample(1), sample(2), 255],
                3 => {
                    let index = usize::from(sample(0));
                    let rgb = *palette.get(index)?;
                    let alpha = transparency.get(index).copied().unwrap_or(255);
                    [rgb[0], rgb[1], rgb[2], alpha]
                }
                4 => {
                    let value = sample(0);
                    [value, value, value, sample(1)]
                }
                6 => [sample(0), sample(1), sample(2), sample(3)],
                _ => return None,
            };
            let target = (y * width + x) * 4;
            pixels[target..target + 4].copy_from_slice(&[r, g, b, a]);
        }
        prev.copy_from_slice(&row);
    }
    Some(Rgba8Image { width, height, data: pixels })
}

/// Reverse one PNG row filter in place (§6.3).
fn unfilter(filter: u8, bpp: usize, stride: usize, prev: &[u8], row: &mut [u8]) -> Option<()> {
    if row.len() != stride {
        return None;
    }
    for i in 0..stride {
        let left = if i >= bpp { row[i - bpp] } else { 0 };
        let up = prev[i];
        let up_left = if i >= bpp { prev[i - bpp] } else { 0 };
        let value = row[i];
        let reconstructed = match filter {
            0 => value,
            1 => value.wrapping_add(left),
            2 => value.wrapping_add(up),
            3 => value
                .wrapping_add(u8::try_from(u16::midpoint(u16::from(left), u16::from(up))).ok()?),
            4 => {
                let paeth = paeth_predictor(i16::from(left), i16::from(up), i16::from(up_left));
                value.wrapping_add(u8::try_from(paeth).unwrap_or(0))
            }
            _ => return None,
        };
        row[i] = reconstructed;
    }
    Some(())
}

/// The Paeth predictor (PNG §6, `PaethPredictor`).
fn paeth_predictor(a: i16, b: i16, c: i16) -> i16 {
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-test PNG encoder (filter 0 rows + zlib) — the
    /// decode fixtures stay hermetic.
    fn encode_png(width: usize, height: usize, raw: &[u8]) -> Vec<u8> {
        fn chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(&kind);
            out.extend_from_slice(data);
            let mut crc = 0xFFFF_FFFFu32;
            for byte in kind.iter().chain(data.iter()) {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
                }
            }
            out.extend_from_slice(&(!crc).to_be_bytes());
            out
        }
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write as IoWrite;
        let mut out = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(height as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
        out.extend_from_slice(&chunk(*b"IHDR", &ihdr));
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        for row in 0..height {
            z.write_all(&[0]).unwrap();
            z.write_all(&raw[row * width * 4..(row + 1) * width * 4]).unwrap();
        }
        out.extend_from_slice(&chunk(*b"IDAT", &z.finish().unwrap()));
        out.extend_from_slice(&chunk(*b"IEND", &[]));
        out
    }

    #[test]
    fn decodes_filter_zero_rgba() {
        let raw: Vec<u8> =
            (0..8u32).flat_map(|i| [i as u8, i as u8 + 1, i as u8 + 2, 255]).collect();
        let png = encode_png(4, 2, &raw);
        let image = decode_png(&png).expect("decodes");
        assert_eq!(image.width, 4);
        assert_eq!(image.height, 2);
        assert_eq!(image.data, raw);
        // Garbage does not decode.
        assert_eq!(decode_png(&png[..png.len() - 5]), None);
        assert_eq!(decode_png(b"not a png"), None);
    }

    #[test]
    fn decodes_the_r07_fixture() {
        // The committed 4×2 fixture (row 0: red green blue yellow;
        // row 1: white black gray teal — all opaque).
        let png = include_bytes!("../tests/fixtures/r07-tiny-4x2.png");
        let image = decode_png(png).expect("fixture decodes");
        assert_eq!((image.width, image.height), (4, 2));
        assert_eq!(image.data[..4], [255, 0, 0, 255]);
        assert_eq!(image.data[4 * 4..4 * 4 + 4], [255, 255, 255, 255]);
    }

    #[test]
    fn unfilter_covers_all_five_filters() {
        // Reconstruct rows encoded with each filter type; the
        // decoder's output must equal the raw pixels.
        for filter in 0u8..=4 {
            let width = 4usize;
            let height = 3usize;
            let pixels: Vec<u8> = (0..width * height * 4).map(|i| (i * 7 % 251) as u8).collect();
            let bpp = 4;
            let mut filtered = Vec::new();
            filtered.push(0u8);
            filtered.extend_from_slice(&pixels[..width * 4]);
            for row in 1..height {
                filtered.push(filter);
                for col in 0..width * 4 {
                    let value = pixels[row * width * 4 + col];
                    let left = if col >= bpp { pixels[row * width * 4 + col - bpp] } else { 0 };
                    let up = pixels[(row - 1) * width * 4 + col];
                    let up_left =
                        if col >= bpp { pixels[(row - 1) * width * 4 + col - bpp] } else { 0 };
                    let encoded = match filter {
                        0 => value,
                        1 => value.wrapping_sub(left),
                        2 => value.wrapping_sub(up),
                        3 => value.wrapping_sub(
                            u8::try_from(u16::midpoint(u16::from(left), u16::from(up)))
                                .expect("midpoint of u8s"),
                        ),
                        4 => {
                            let predicted =
                                paeth_predictor(i16::from(left), i16::from(up), i16::from(up_left));
                            value.wrapping_sub(u8::try_from(predicted).expect("predictor in range"))
                        }
                        _ => unreachable!(),
                    };
                    filtered.push(encoded);
                }
            }
            let png = encode_png(width, height, &pixels);
            let replacement = {
                use flate2::write::ZlibEncoder;
                use flate2::Compression;
                use std::io::Write as IoWrite;
                let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                z.write_all(&filtered).unwrap();
                z.finish().unwrap()
            };
            let png = replace_idat(&png, &replacement);
            let image = decode_png(&png).unwrap_or_else(|| panic!("filter {filter} decodes"));
            assert_eq!(image.data, pixels, "filter {filter} round-trips");
        }
    }

    fn replace_idat(png: &[u8], payload: &[u8]) -> Vec<u8> {
        let signature_end = 8;
        let mut out = png[..signature_end].to_vec();
        out.extend_from_slice(&png[signature_end..signature_end + 8 + 13 + 4]);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(b"IDAT");
        out.extend_from_slice(payload);
        let mut crc = 0xFFFF_FFFFu32;
        for byte in b"IDAT".iter().chain(payload.iter()) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { 0xEDB8_8320 ^ (crc >> 1) } else { crc >> 1 };
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
        out.extend_from_slice(b"\x00\x00\x00\x00IEND\xAE\x42\x60\x82");
        out
    }

    #[test]
    fn rejects_out_of_scope_pngs() {
        let png = encode_png(1, 1, &[1, 2, 3, 255]);
        let mut interlaced = png.clone();
        interlaced[8 + 8 + 12] = 1;
        assert_eq!(decode_png(&interlaced), None);
        assert_eq!(decode_png(&PNG_SIGNATURE), None);
        let png = encode_png(2, 2, &[0; 16]);
        let mut small = png.clone();
        small[8 + 8 + 8] = 4;
        assert_eq!(decode_png(&small), None);
    }
}
