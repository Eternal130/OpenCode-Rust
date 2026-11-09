//! PNG decoding for the image lanes (the raster/emission family lives
//! in opentui since lane D3-4b).
//!
//! v2 decodes through opentui's native image pipeline (libwebp /
//! wuffs / stb per the bundled licenses). The Rust port decodes the
//! PNG subset previews need: every depth the spec allows per color
//! type (gray 0 and palette 3 also take the sub-byte 1/2/4-bit
//! set), gray / gray+alpha / RGB / RGBA / palette+tRNS, all five
//! row filters, and Adam7 interlacing (seven passes, each
//! unfiltered independently and scattered into the final grid).
//! Malformed or still-out-of-scope streams decode to `None` — the
//! caller keeps the "No preview" placeholder, exactly like a v2
//! load error.
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
                // IHDR byte 12: 0 = no interlace, 1 = Adam7, anything
                // else is malformed (PNG §11.2.2).
                if data[12] > 1 {
                    return None;
                }
                chunks.interlace = data[12] == 1;
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
    if !saw_iend || chunks.width == 0 || chunks.height == 0 || chunks.idat.is_empty() {
        return None;
    }
    Some(chunks)
}

/// The Adam7 pass lattice (PNG §8.2 / §4D.2): `(x_start, y_start,
/// x_step, y_step)` for each of the seven passes, in stream order.
const ADAM7_PASSES: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// The single full-image "pass" a non-interlaced stream consists of.
const FULL_IMAGE_PASS: (usize, usize, usize, usize) = (0, 0, 1, 1);

/// A pass's reduced-image dimensions (PNG §4D.2 `ceil` formulas);
/// a zero side means the pass carries no scanlines at all.
fn pass_dimensions(
    width: usize,
    height: usize,
    (x_start, y_start, x_step, y_step): (usize, usize, usize, usize),
) -> (usize, usize) {
    let pass_width = if width > x_start { (width - x_start).div_ceil(x_step) } else { 0 };
    let pass_height = if height > y_start { (height - y_start).div_ceil(y_step) } else { 0 };
    (pass_width, pass_height)
}

/// Scale a `bit_depth`-wide sample to 8 bits. The spec's recommended
/// scaling equals exact bit replication (PNG §13 / the "fourteen
/// ways" annex: 1→`1111_1111`, 4-bit `1010`→`1010_1010`), which for
/// depths 1/2/4 is the integer identity `v * 255 / (2^d - 1)`
/// (d=1: ×255, d=2: ×85, d=4: ×17) — so 1-bit white becomes 255.
fn scale_sample_to_8(value: u8, bit_depth: u8) -> u8 {
    let bits = u16::from(bit_depth);
    let max = (1u16 << bits) - 1;
    ((u16::from(value) * 255) / max) as u8
}

/// Decode a PNG into an RGBA8 image (`None` for anything the
/// scope above does not cover, or malformed input).
#[must_use]
pub fn decode_png(bytes: &[u8]) -> Option<Rgba8Image> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return None;
    }
    let PngChunks { width, height, bit_depth, color_type, interlace, palette, transparency, idat } =
        scan_png_chunks(bytes)?;
    let channels = match color_type {
        0 | 3 => 1usize, // gray / palette index
        2 => 3,          // RGB
        4 => 2,          // gray + alpha
        6 => 4,          // RGBA
        _ => return None,
    };
    // The depth/color lattice PNG §11.2.2 (Table 11.1) allows: gray
    // takes 1/2/4/8/16, palette 1/2/4/8, the true-colour types only
    // 8/16. Anything else is malformed, not merely unsupported.
    let depth_ok = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        _ => matches!(bit_depth, 8 | 16),
    };
    if !depth_ok {
        return None;
    }
    let bits_per_pixel = channels * usize::from(bit_depth);
    // Filter unit (PNG §9: bytes per complete pixel, rounding up to
    // one — sub-byte depths filter with bpp = 1).
    let filter_bpp = bits_per_pixel.div_ceil(8);
    let mut raw = Vec::new();
    let mut decoder = ZlibDecoder::new(idat.as_slice());
    decoder.read_to_end(&mut raw).ok()?;
    let passes: &[(usize, usize, usize, usize)] =
        if interlace { &ADAM7_PASSES } else { &[FULL_IMAGE_PASS] };
    let mut pixels = vec![0u8; width * height * 4];
    let mut cursor = 0usize;
    for &(x_start, y_start, x_step, y_step) in passes {
        let (pass_width, pass_height) =
            pass_dimensions(width, height, (x_start, y_start, x_step, y_step));
        if pass_width == 0 || pass_height == 0 {
            continue; // empty passes contribute no bytes (PNG §8.2)
        }
        // Sub-byte rows pad to a byte boundary at the pass's own
        // width: ceil(pass_width * bits_per_pixel / 8) (PNG §7.2).
        let row_bytes = (pass_width * bits_per_pixel).div_ceil(8);
        let mut prev = vec![0u8; row_bytes];
        for j in 0..pass_height {
            let end = cursor.checked_add(1 + row_bytes)?;
            if end > raw.len() {
                return None;
            }
            let filter = raw[cursor];
            let mut row = raw[cursor + 1..end].to_vec();
            cursor = end;
            // Filters apply within the pass: "up" sees the same
            // pass's previous reduced row (PNG §8.2 / §9).
            unfilter(filter, filter_bpp, row_bytes, &prev, &mut row)?;
            for i in 0..pass_width {
                let pixel =
                    pixel_rgba(&row, i, bit_depth, channels, color_type, &palette, &transparency)?;
                // Scatter the reduced-image pixel into the final
                // grid at the pass lattice position (PNG §8.2).
                let x = x_start + i * x_step;
                let y = y_start + j * y_step;
                let target = (y * width + x) * 4;
                pixels[target..target + 4].copy_from_slice(&pixel);
            }
            prev.copy_from_slice(&row);
        }
    }
    Some(Rgba8Image { width, height, data: pixels })
}

/// One reconstructed scanline's pixel `i` as straight RGBA8.
fn pixel_rgba(
    row: &[u8],
    i: usize,
    bit_depth: u8,
    channels: usize,
    color_type: u8,
    palette: &[[u8; 3]],
    transparency: &[u8],
) -> Option<[u8; 4]> {
    if bit_depth < 8 {
        // Bit-packed single-channel samples (gray value or palette
        // index), leftmost pixel in the high-order bits, each
        // scanline starting on a byte boundary (PNG §7.2).
        let bits = usize::from(bit_depth);
        let bit_index = i * bits;
        let byte = row[bit_index / 8];
        let shift = 8 - bits - (bit_index % 8);
        let value = (byte >> shift) & ((1 << bits) - 1);
        return match color_type {
            0 => {
                let value = scale_sample_to_8(value, bit_depth);
                Some([value, value, value, 255])
            }
            3 => {
                let index = usize::from(value);
                let rgb = *palette.get(index)?;
                let alpha = transparency.get(index).copied().unwrap_or(255);
                Some([rgb[0], rgb[1], rgb[2], alpha])
            }
            _ => None,
        };
    }
    let sample_bytes = if bit_depth == 16 { 2usize } else { 1 };
    let source = i * channels * sample_bytes;
    // 16-bit samples downshift to their high byte (the 8-bit
    // display path's precision).
    let sample = |channel: usize| row[source + channel * sample_bytes];
    match color_type {
        0 => {
            let value = sample(0);
            Some([value, value, value, 255])
        }
        2 => Some([sample(0), sample(1), sample(2), 255]),
        3 => {
            let index = usize::from(sample(0));
            let rgb = *palette.get(index)?;
            let alpha = transparency.get(index).copied().unwrap_or(255);
            Some([rgb[0], rgb[1], rgb[2], alpha])
        }
        4 => {
            let value = sample(0);
            Some([value, value, value, sample(1)])
        }
        6 => Some([sample(0), sample(1), sample(2), sample(3)]),
        _ => None,
    }
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

    /// One length+kind+data+CRC chunk (the PNG §5.3 container).
    fn png_chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
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

    #[allow(clippy::too_many_arguments)] // the argument set mirrors the ref call shape
    /// Hermetic PNG crafter for arbitrary depth / color type /
    /// interlace / palette fixtures: `stream` is the already-laid-out
    /// filtered byte stream (filter bytes included, Adam7 passes in
    /// order for interlaced fixtures), zlib-compressed here.
    fn craft_png(
        width: usize,
        height: usize,
        bit_depth: u8,
        color_type: u8,
        interlace: u8,
        plte: Option<&[[u8; 3]]>,
        trns: Option<&[u8]>,
        stream: &[u8],
    ) -> Vec<u8> {
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write as IoWrite;
        let mut out = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(height as u32).to_be_bytes());
        ihdr.extend_from_slice(&[bit_depth, color_type, 0, 0, interlace]);
        out.extend_from_slice(&png_chunk(*b"IHDR", &ihdr));
        if let Some(palette) = plte {
            let bytes: Vec<u8> = palette.iter().flat_map(|c| c.iter().copied()).collect();
            out.extend_from_slice(&png_chunk(*b"PLTE", &bytes));
        }
        if let Some(transparency) = trns {
            out.extend_from_slice(&png_chunk(*b"tRNS", transparency));
        }
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(stream).unwrap();
        out.extend_from_slice(&png_chunk(*b"IDAT", &z.finish().unwrap()));
        out.extend_from_slice(&png_chunk(*b"IEND", &[]));
        out
    }

    /// Minimal in-test PNG encoder (filter 0 rows + zlib) — the
    /// decode fixtures stay hermetic.
    fn encode_png(width: usize, height: usize, raw: &[u8]) -> Vec<u8> {
        let mut stream = Vec::new();
        for row in 0..height {
            stream.push(0u8);
            stream.extend_from_slice(&raw[row * width * 4..(row + 1) * width * 4]);
        }
        craft_png(width, height, 8, 6, 0, None, None, &stream)
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
        out.extend_from_slice(&png_chunk(*b"IDAT", payload));
        out.extend_from_slice(&png_chunk(*b"IEND", &[]));
        out
    }

    #[test]
    fn accepts_interlaced_one_by_one_and_rejects_malformed_combinations() {
        // The old rejection pin: flipping the interlace byte on a
        // 1x1 8-bit RGBA PNG is a *valid* Adam7 stream (pass 1 holds
        // the only pixel; passes 2-7 are empty for 1x1), so it now
        // decodes to the same exact pixel.
        let png = encode_png(1, 1, &[1, 2, 3, 255]);
        let mut interlaced = png.clone();
        interlaced[8 + 8 + 12] = 1;
        let image = decode_png(&interlaced).expect("1x1 interlaced decodes");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.data, vec![1, 2, 3, 255]);
        // A bare signature is still not a PNG.
        assert_eq!(decode_png(&PNG_SIGNATURE), None);
        // 4-bit RGBA is a combination PNG §11.2.2 (Table 11.1) does
        // not allow — rejected as malformed, not merely unsupported.
        let png = encode_png(2, 2, &[0; 16]);
        let mut small = png.clone();
        small[8 + 8 + 8] = 4;
        assert_eq!(decode_png(&small), None);
        // An interlace flag byte other than 0/1 is malformed IHDR.
        let mut bad_flag = png.clone();
        bad_flag[8 + 8 + 12] = 2;
        assert_eq!(decode_png(&bad_flag), None);
    }

    #[test]
    fn decodes_sub_byte_grayscale() {
        // 1-bit: row 1010_1100 → white/black alternating tail, with
        // 1-bit white scaling to 255 (bit replication, PNG §13).
        let png = craft_png(8, 1, 1, 0, 0, None, None, &[0, 0b1010_1100]);
        let image = decode_png(&png).expect("1-bit gray decodes");
        assert_eq!((image.width, image.height), (8, 1));
        let want: Vec<u8> = [1u8, 0, 1, 0, 1, 1, 0, 0]
            .iter()
            .flat_map(|&bit| [bit * 255, bit * 255, bit * 255, 255])
            .collect();
        assert_eq!(image.data, want);

        // 2-bit: values 0,1,2,3 → 0,85,170,255 (v*255/3).
        let png = craft_png(4, 1, 2, 0, 0, None, None, &[0, 0b00_01_10_11]);
        let image = decode_png(&png).expect("2-bit gray decodes");
        let want: Vec<u8> =
            [0u8, 1, 2, 3].iter().flat_map(|&v| [v * 85, v * 85, v * 85, 255]).collect();
        assert_eq!(image.data, want);

        // 4-bit: nibbles 0,4,8,F → 0,68,136,255 (v*17).
        let png = craft_png(4, 1, 4, 0, 0, None, None, &[0, 0x04, 0x8F]);
        let image = decode_png(&png).expect("4-bit gray decodes");
        let want: Vec<u8> =
            [0u8, 4, 8, 15].iter().flat_map(|&v| [v * 17, v * 17, v * 17, 255]).collect();
        assert_eq!(image.data, want);
    }

    #[test]
    fn decodes_four_bit_palette_with_trns() {
        // Indices 0,1,2,1 (nibbles 0x01,0x21) resolve through PLTE
        // with per-index tRNS alpha — same treatment as the 8-bit
        // palette path, just unpacked from nibbles.
        let palette = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
        let trns = [255, 128, 0];
        let png = craft_png(4, 1, 4, 3, 0, Some(&palette), Some(&trns), &[0, 0x01, 0x21]);
        let image = decode_png(&png).expect("4-bit palette decodes");
        let want: Vec<u8> =
            [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0], [0, 255, 0, 128]].concat();
        assert_eq!(image.data, want);
    }

    #[test]
    fn decodes_adam7_rgb_5x3() {
        // Hand-crafted Adam7 for a 5x3 RGB image. Pass lattice for
        // 5x3 (PNG §4D.2): P1 {(0,0)}, P2 {(4,0)}, P3 empty (height
        // 3 <= y_start 4), P4 {(2,0)}, P5 row y=2 x∈{0,2,4}, P6 rows
        // y∈{0,2} x∈{1,3}, P7 row y=1 x∈{0..4}. Each pass gets a
        // distinct colour so a mis-scatter shows up as a wrong pixel.
        let pass1 = [10u8, 0, 0];
        let pass2 = [10, 10, 0];
        let pass4 = [0, 0, 10];
        let pass5 = [0, 10, 10];
        let pass6_top = [0, 10, 0];
        let pass6_bottom = [10, 0, 10];
        let pass7 = [10, 10, 10];
        let mut stream = Vec::new();
        stream.push(0); // P1: 1x1
        stream.extend(pass1);
        stream.push(0); // P2: 1x1
        stream.extend(pass2);
        // P3: zero scanlines — no bytes at all.
        stream.push(0); // P4: 1x1
        stream.extend(pass4);
        stream.push(0); // P5: 3x1
        stream.extend(pass5);
        stream.extend(pass5);
        stream.extend(pass5);
        stream.push(0); // P6 row 0: 2x1 → (1,0),(3,0)
        stream.extend(pass6_top);
        stream.extend(pass6_top);
        stream.push(0); // P6 row 1: 2x1 → (1,2),(3,2)
        stream.extend(pass6_bottom);
        stream.extend(pass6_bottom);
        stream.push(0); // P7: 5x1 → y=1
        for _ in 0..5 {
            stream.extend(pass7);
        }
        let png = craft_png(5, 3, 8, 2, 1, None, None, &stream);
        let image = decode_png(&png).expect("adam7 rgb decodes");
        assert_eq!((image.width, image.height), (5, 3));
        let grid = [
            [pass1, pass6_top, pass4, pass6_top, pass2],
            [pass7, pass7, pass7, pass7, pass7],
            [pass5, pass6_bottom, pass5, pass6_bottom, pass5],
        ];
        let want: Vec<u8> = grid
            .iter()
            .flat_map(|row| row.iter().flat_map(|px| px.iter().copied().chain([255])))
            .collect();
        assert_eq!(image.data, want);
        // A truncated stream (P7's row amputated) is malformed.
        let short = craft_png(5, 3, 8, 2, 1, None, None, &stream[..stream.len() - 3]);
        assert_eq!(decode_png(&short), None);
    }

    #[test]
    fn decodes_adam7_one_bit_gray_3x3() {
        // The ①×② interaction: interlaced sub-byte rows unpack at
        // each pass's own width, bit-aligned per pass row. Image:
        // a 3x3 checkerboard with corners lit.
        //   1 0 1
        //   0 1 0
        //   1 0 1
        // Pass layout for 3x3: P1 {(0,0)}; P2/P3 empty; P4 {(2,0)};
        // P5 {(0,2),(2,2)}; P6 two 1x1 rows {(1,0)} and {(1,2)};
        // P7 row y=1 x∈{0,1,2}.
        // P1: (0,0) = 1 → filter byte + one bit, MSB first.
        let mut stream = vec![0u8, 0b1000_0000];
        stream.push(0); // P4: (2,0) = 1
        stream.push(0b1000_0000);
        stream.push(0); // P5: (0,2)=1,(2,2)=1 → 0b1100_0000
        stream.push(0b1100_0000);
        stream.push(0); // P6 row 0: (1,0) = 0
        stream.push(0b0000_0000);
        stream.push(0); // P6 row 1: (1,2) = 0
        stream.push(0b0000_0000);
        stream.push(0); // P7: (0,1)=0,(1,1)=1,(2,1)=0 → 0b0100_0000
        stream.push(0b0100_0000);
        let png = craft_png(3, 3, 1, 0, 1, None, None, &stream);
        let image = decode_png(&png).expect("adam7 1-bit gray decodes");
        let want: Vec<u8> = [1u8, 0, 1, 0, 1, 0, 1, 0, 1]
            .iter()
            .flat_map(|&bit| [bit * 255, bit * 255, bit * 255, 255])
            .collect();
        assert_eq!(image.data, want);
    }

    #[test]
    fn decodes_sixteen_bit_taking_the_high_byte() {
        // Pins the unchanged 16-bit path: gray+alpha, samples
        // 0xABCD/0x1122 and 0x1234/0x5678 → high bytes only.
        let stream = [0, 0xAB, 0xCD, 0x11, 0x22, 0x12, 0x34, 0x56, 0x78];
        let png = craft_png(2, 1, 16, 4, 0, None, None, &stream);
        let image = decode_png(&png).expect("16-bit decodes");
        assert_eq!(image.data, vec![0xAB, 0xAB, 0xAB, 0x11, 0x12, 0x12, 0x12, 0x56]);
    }

    #[test]
    fn adam7_rgba_round_trips_with_every_filter() {
        // Interlacing smoke test, real-world-ish shape: a 9x9
        // pseudo-random RGBA image Adam7-encoded with every row
        // filter exercised across the passes. The encode side
        // filters against each pass's *own* previous row, so a
        // decoder that unfiltered against output-grid neighbours
        // (or forgot the empty-pass skips) cannot round-trip.
        let (w, h) = (9usize, 9usize);
        let mut lcg = 0x1234_5678u32;
        let mut next = move || {
            lcg = lcg.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (lcg >> 16) as u8
        };
        let pixels: Vec<u8> = (0..w * h * 4).map(|_| next()).collect();
        let bpp = 4usize; // filter unit for 8-bit RGBA
        let mut stream = Vec::new();
        let mut row_index = 0usize;
        for &(x_start, y_start, x_step, y_step) in &ADAM7_PASSES {
            let (pw, ph) = pass_dimensions(w, h, (x_start, y_start, x_step, y_step));
            if pw == 0 || ph == 0 {
                continue;
            }
            let row_bytes = pw * bpp;
            let mut prev_row = vec![0u8; row_bytes];
            for j in 0..ph {
                let y = y_start + j * y_step;
                let mut row = Vec::with_capacity(row_bytes);
                for i in 0..pw {
                    let x = x_start + i * x_step;
                    row.extend_from_slice(&pixels[(y * w + x) * 4..(y * w + x) * 4 + 4]);
                }
                let filter = (row_index % 5) as u8;
                row_index += 1;
                let mut encoded = Vec::with_capacity(row_bytes);
                for col in 0..row_bytes {
                    let value = row[col];
                    let left = if col >= bpp { row[col - bpp] } else { 0 };
                    let up = prev_row[col];
                    let up_left = if col >= bpp { prev_row[col - bpp] } else { 0 };
                    let out = match filter {
                        0 => value,
                        1 => value.wrapping_sub(left),
                        2 => value.wrapping_sub(up),
                        3 => value.wrapping_sub(
                            u8::try_from(u16::midpoint(u16::from(left), u16::from(up)))
                                .expect("midpoint of u8s"),
                        ),
                        _ => value.wrapping_sub(
                            u8::try_from(paeth_predictor(
                                i16::from(left),
                                i16::from(up),
                                i16::from(up_left),
                            ))
                            .expect("predictor in range"),
                        ),
                    };
                    encoded.push(out);
                }
                stream.push(filter);
                stream.extend_from_slice(&encoded);
                prev_row = row;
            }
        }
        let png = craft_png(w, h, 8, 6, 1, None, None, &stream);
        let image = decode_png(&png).expect("adam7 rgba decodes");
        assert_eq!((image.width, image.height), (w, h));
        assert_eq!(image.data, pixels);
    }
}
