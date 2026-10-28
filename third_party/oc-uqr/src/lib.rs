//! Faithful Rust port of the `uqr` 0.1.3 QR encoder (vendored — see
//! ../README.md). The encoder core is Nayuki's QR Code generator
//! library algorithm, as shipped in uqr 0.1.3's
//! `dist/index.mjs`; every table, loop bound and bit order below
//! mirrors that file so module matrices are byte-identical. The
//! only surface ported is what ref consumes:
//! `encode(text, { border })` + `renderUnicodeCompact`.

// Vendored, port-faithful code: clippy style lints (manual_div_ceil)
// are suppressed crate-wide to keep the port byte-identical to uqr.
#![allow(clippy::manual_div_ceil)]

/// ECC level order/format pair bits (`EccMap`, index.mjs:11-16).
/// (`ecl[0]` indexes the tables, `ecl[1]` is the format-bit value.)
/// The full ladder is kept for port fidelity though ref only uses L.
#[allow(dead_code)]
const LOW: [u8; 2] = [0, 1];
#[allow(dead_code)]
const MEDIUM: [u8; 2] = [1, 0];
#[allow(dead_code)]
const QUARTILE: [u8; 2] = [2, 3];
#[allow(dead_code)]
const HIGH: [u8; 2] = [3, 2];

const ALPHANUMERIC_CHARSET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

const MIN_VERSION: u8 = 1;
const MAX_VERSION: u8 = 40;

const PENALTY_N1: u32 = 3;
const PENALTY_N2: u32 = 3;
const PENALTY_N3: u32 = 40;
const PENALTY_N4: u32 = 10;

/// `ECC_CODEWORDS_PER_BLOCK` (index.mjs:26-33) — index 0 padding.
#[rustfmt::skip]
const ECC_CODEWORDS_PER_BLOCK: [[i16; 41]; 4] = [
    [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
    [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];

/// `NUM_ERROR_CORRECTION_BLOCKS` (index.mjs:35-40) — index 0 padding.
#[rustfmt::skip]
const NUM_ERROR_CORRECTION_BLOCKS: [[i16; 41]; 4] = [
    [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
    [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
    [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];

/// `QrCodeDataType` (`index.mjs:1-9`) — only the nonzero-ness of the
/// function-module mark matters to the encoder (`types[y][x]` truthy
/// test in `drawCodewords`/`applyMask`); `Border` (−1) is nonzero
/// too but never coexists with mask/draw phases.
#[allow(dead_code)]
const TYPE_BORDER: u8 = 5; // uqr QrCodeDataType.Border (port fidelity)
const TYPE_DATA: u8 = 0;
const TYPE_FUNCTION: u8 = 1;
const TYPE_POSITION: u8 = 2;
const TYPE_TIMING: u8 = 3;
const TYPE_ALIGNMENT: u8 = 4;

/// `encode(data, options)` result after `addBorder` (`index.mjs:502-532`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrResult {
    pub version: u8,
    pub mask_pattern: u8,
    pub size: usize,
    /// `false` = light, `true` = dark (`result.data`).
    pub data: Vec<Vec<bool>>,
}

struct QrCode {
    version: u8,
    ecc: [u8; 2],
    size: usize,
    mask: u8,
    modules: Vec<Vec<bool>>,
    types: Vec<Vec<u8>>,
}

impl QrCode {
    /// `new QrCode(version, ecc, dataCodewords, msk)` (index.mjs:68-106).
    fn new(version: u8, ecc: [u8; 2], data_codewords: &[u8], mut msk: i8) -> QrCode {
        assert!((MIN_VERSION..=MAX_VERSION).contains(&version), "Version value out of range");
        assert!((-1..=7).contains(&msk), "Mask value out of range");
        let size = (version as usize) * 4 + 17;
        let mut code = QrCode {
            version,
            ecc,
            size,
            mask: 0,
            modules: vec![vec![false; size]; size],
            types: vec![vec![TYPE_DATA; size]; size],
        };
        code.draw_function_patterns();
        let all_codewords = code.add_ecc_and_interleave(data_codewords);
        code.draw_codewords(&all_codewords);
        if msk == -1 {
            // Automatic mask choice: the lowest penalty wins (first on
            // ties, since `<` is strict — index.mjs:92-101).
            let mut min_penalty = u32::MAX;
            let mut best = 0i8;
            for i in 0..8i8 {
                code.apply_mask(i as u8);
                code.draw_format_bits(i as u8);
                let penalty = code.get_penalty_score();
                if penalty < min_penalty {
                    best = i;
                    min_penalty = penalty;
                }
                code.apply_mask(i as u8);
            }
            msk = best;
        }
        code.mask = msk as u8;
        code.apply_mask(msk as u8);
        code.draw_format_bits(msk as u8);
        code
    }

    /// `drawFunctionPatterns` (index.mjs:117-141).
    fn draw_function_patterns(&mut self) {
        for i in 0..self.size {
            self.set_function_module(6, i, i % 2 == 0, TYPE_TIMING);
            self.set_function_module(i, 6, i % 2 == 0, TYPE_TIMING);
        }
        self.draw_finder_pattern(3, 3);
        self.draw_finder_pattern(self.size - 4, 3);
        self.draw_finder_pattern(3, self.size - 4);
        let align_pat_pos = self.get_alignment_pattern_positions();
        let num_align = align_pat_pos.len();
        for (i, &pos_i) in align_pat_pos.iter().enumerate() {
            for (j, &pos_j) in align_pat_pos.iter().enumerate() {
                if !((i == 0 && j == 0)
                    || (i == 0 && j == num_align - 1)
                    || (i == num_align - 1 && j == 0))
                {
                    self.draw_alignment_pattern(pos_i, pos_j);
                }
            }
        }
        self.draw_format_bits(0);
        self.draw_version();
    }

    /// `drawFormatBits(mask)` (index.mjs:143-165).
    fn draw_format_bits(&mut self, mask: u8) {
        let data: u32 = ((self.ecc[1] as u32) << 3) | mask as u32;
        let mut rem: u32 = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 1335);
        }
        let bits: u32 = ((data << 10) | rem) ^ 21522;
        for i in 0..=5u32 {
            self.set_function_module(8, i as usize, get_bit(bits, i), TYPE_FUNCTION);
        }
        self.set_function_module(8, 7, get_bit(bits, 6), TYPE_FUNCTION);
        self.set_function_module(8, 8, get_bit(bits, 7), TYPE_FUNCTION);
        self.set_function_module(7, 8, get_bit(bits, 8), TYPE_FUNCTION);
        for i in 9..15u32 {
            self.set_function_module((14 - i) as usize, 8, get_bit(bits, i), TYPE_FUNCTION);
        }
        for i in 0..8u32 {
            self.set_function_module(
                self.size - 1 - i as usize,
                8,
                get_bit(bits, i),
                TYPE_FUNCTION,
            );
        }
        for i in 8..15u32 {
            self.set_function_module(
                8,
                self.size - 15 + i as usize,
                get_bit(bits, i),
                TYPE_FUNCTION,
            );
        }
        // This dark module is always present (index.mjs:164).
        self.set_function_module(8, self.size - 8, true, TYPE_FUNCTION);
    }

    /// `drawVersion` (index.mjs:167-182).
    fn draw_version(&mut self) {
        if self.version < 7 {
            return;
        }
        let mut rem: u32 = self.version as u32;
        for _ in 0..12 {
            rem = (rem << 1) ^ ((rem >> 11) * 7973);
        }
        let bits: u32 = ((self.version as u32) << 12) | rem;
        for i in 0..18u32 {
            let color = get_bit(bits, i);
            let a = self.size - 11 + (i % 3) as usize;
            let b = (i / 3) as usize;
            self.set_function_module(a, b, color, TYPE_FUNCTION);
            self.set_function_module(b, a, color, TYPE_FUNCTION);
        }
    }

    /// `drawFinderPattern(x, y)` (index.mjs:184-196).
    fn draw_finder_pattern(&mut self, x: usize, y: usize) {
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let dist = dx.unsigned_abs().max(dy.unsigned_abs());
                let xx = x as i64 + dx as i64;
                let yy = y as i64 + dy as i64;
                if xx >= 0 && xx < self.size as i64 && yy >= 0 && yy < self.size as i64 {
                    self.set_function_module(
                        xx as usize,
                        yy as usize,
                        dist != 2 && dist != 4,
                        TYPE_POSITION,
                    );
                }
            }
        }
    }

    /// `drawAlignmentPattern(x, y)` (index.mjs:198-211).
    fn draw_alignment_pattern(&mut self, x: usize, y: usize) {
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                self.set_function_module(
                    (x as i64 + dx as i64) as usize,
                    (y as i64 + dy as i64) as usize,
                    dx.unsigned_abs().max(dy.unsigned_abs()) != 1,
                    TYPE_ALIGNMENT,
                );
            }
        }
    }

    /// `setFunctionModule` (index.mjs:213-217).
    fn set_function_module(&mut self, x: usize, y: usize, is_dark: bool, kind: u8) {
        self.modules[y][x] = is_dark;
        self.types[y][x] = kind;
    }

    /// `addEccAndInterleave(data)` (index.mjs:223-252).
    fn add_ecc_and_interleave(&self, data: &[u8]) -> Vec<u8> {
        let ver = self.version as usize;
        let ecl = self.ecc[0] as usize;
        assert_eq!(data.len(), get_num_data_codewords(self.version, self.ecc), "Invalid argument");
        let num_blocks = NUM_ERROR_CORRECTION_BLOCKS[ecl][ver] as usize;
        let block_ecc_len = ECC_CODEWORDS_PER_BLOCK[ecl][ver] as usize;
        let raw_codewords = get_num_raw_data_modules(self.version) as usize / 8;
        let num_short_blocks = num_blocks - raw_codewords % num_blocks;
        let short_block_len = raw_codewords / num_blocks;
        let rs_div = reed_solomon_compute_divisor(block_ecc_len);
        let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks);
        let mut k = 0usize;
        for i in 0..num_blocks {
            let dat_len = short_block_len - block_ecc_len + usize::from(i >= num_short_blocks);
            let mut dat = data[k..k + dat_len].to_vec();
            k += dat_len;
            let ecc = reed_solomon_compute_remainder(&dat, &rs_div);
            if i < num_short_blocks {
                dat.push(0);
            }
            dat.extend(ecc);
            blocks.push(dat);
        }
        let mut result = Vec::with_capacity(raw_codewords);
        for i in 0..blocks[0].len() {
            for (j, block) in blocks.iter().enumerate() {
                if i != short_block_len - block_ecc_len || j >= num_short_blocks {
                    result.push(block[i]);
                }
            }
        }
        result
    }

    /// `drawCodewords(data)` (index.mjs:258-278).
    fn draw_codewords(&mut self, data: &[u8]) {
        assert_eq!(
            data.len(),
            get_num_raw_data_modules(self.version) as usize / 8,
            "Invalid argument"
        );
        let mut i = 0usize;
        let mut right = self.size as isize - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..self.size {
                for j in 0..2isize {
                    let x = (right - j) as usize;
                    let upward = ((right + 1) & 2) == 0;
                    let y = if upward { self.size - 1 - vert } else { vert };
                    if self.types[y][x] == TYPE_DATA && i < data.len() * 8 {
                        self.modules[y][x] = get_bit(data[i >> 3] as u32, 7 - (i & 7) as u32);
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    /// `applyMask(mask)` (index.mjs:284-323).
    fn apply_mask(&mut self, mask: u8) {
        for y in 0..self.size {
            for x in 0..self.size {
                let invert = match mask {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => (x * y % 2 + x * y % 3) == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    7 => ((x + y) % 2 + x * y % 3) % 2 == 0,
                    _ => unreachable!("Mask value out of range"),
                };
                if self.types[y][x] == TYPE_DATA && invert {
                    self.modules[y][x] = !self.modules[y][x];
                }
            }
        }
    }

    /// `getPenaltyScore` (index.mjs:329-383).
    fn get_penalty_score(&self) -> u32 {
        let mut result: u32 = 0;
        for y in 0..self.size {
            let mut run_color = false;
            let mut run_x = 0usize;
            let mut run_history = [0usize; 7];
            for x in 0..self.size {
                if self.modules[y][x] == run_color {
                    run_x += 1;
                    if run_x == 5 {
                        result += PENALTY_N1;
                    } else if run_x > 5 {
                        result += 1;
                    }
                } else {
                    finder_penalty_add_history(run_x, &mut run_history, self.size);
                    if !run_color {
                        result += finder_penalty_count_patterns(&run_history) * PENALTY_N3;
                    }
                    run_color = self.modules[y][x];
                    run_x = 1;
                }
            }
            result +=
                finder_penalty_terminate_and_count(run_color, run_x, &mut run_history, self.size)
                    * PENALTY_N3;
        }
        for x in 0..self.size {
            let mut run_color = false;
            let mut run_y = 0usize;
            let mut run_history = [0usize; 7];
            for y in 0..self.size {
                if self.modules[y][x] == run_color {
                    run_y += 1;
                    if run_y == 5 {
                        result += PENALTY_N1;
                    } else if run_y > 5 {
                        result += 1;
                    }
                } else {
                    finder_penalty_add_history(run_y, &mut run_history, self.size);
                    if !run_color {
                        result += finder_penalty_count_patterns(&run_history) * PENALTY_N3;
                    }
                    run_color = self.modules[y][x];
                    run_y = 1;
                }
            }
            result +=
                finder_penalty_terminate_and_count(run_color, run_y, &mut run_history, self.size)
                    * PENALTY_N3;
        }
        for y in 0..self.size - 1 {
            for x in 0..self.size - 1 {
                let color = self.modules[y][x];
                if color == self.modules[y][x + 1]
                    && color == self.modules[y + 1][x]
                    && color == self.modules[y + 1][x + 1]
                {
                    result += PENALTY_N2;
                }
            }
        }
        let mut dark: usize = 0;
        for row in &self.modules {
            dark += row.iter().filter(|&&c| c).count();
        }
        let total = self.size * self.size;
        let k = ((dark * 20).abs_diff(total * 10) + total - 1) / total;
        result += k.saturating_sub(1) as u32 * PENALTY_N4;
        result
    }

    /// `getAlignmentPatternPositions` (index.mjs:388-402).
    fn get_alignment_pattern_positions(&self) -> Vec<usize> {
        if self.version == 1 {
            Vec::new()
        } else {
            let num_align = (self.version as usize / 7) + 2;
            let step = if self.version == 32 {
                26
            } else {
                (((self.version as usize * 4 + 4) + (num_align * 2 - 2) - 1) / (num_align * 2 - 2))
                    * 2
            };
            let mut result = vec![6usize];
            let mut pos = self.size - 7;
            while result.len() < num_align {
                result.insert(1, pos);
                pos -= step;
            }
            result
        }
    }
}

/// `finderPenaltyCountPatterns` (index.mjs:407-413).
fn finder_penalty_count_patterns(run_history: &[usize; 7]) -> u32 {
    let n = run_history[1];
    let core = n > 0
        && run_history[2] == n
        && run_history[3] == n * 3
        && run_history[4] == n
        && run_history[5] == n;
    u32::from(core && run_history[0] >= n * 4 && run_history[6] >= n)
        + u32::from(core && run_history[6] >= n * 4 && run_history[0] >= n)
}

/// `finderPenaltyTerminateAndCount` (index.mjs:415-425).
fn finder_penalty_terminate_and_count(
    current_run_color: bool,
    mut current_run_length: usize,
    run_history: &mut [usize; 7],
    size: usize,
) -> u32 {
    if current_run_color {
        finder_penalty_add_history(current_run_length, run_history, size);
        current_run_length = 0;
    }
    current_run_length += size;
    finder_penalty_add_history(current_run_length, run_history, size);
    finder_penalty_count_patterns(run_history)
}

/// `finderPenaltyAddHistory` (index.mjs:427-432).
fn finder_penalty_add_history(
    mut current_run_length: usize,
    run_history: &mut [usize; 7],
    size: usize,
) {
    if run_history[0] == 0 {
        current_run_length += size;
    }
    run_history.copy_within(0..6, 1);
    run_history[0] = current_run_length;
}

fn get_bit(x: u32, i: u32) -> bool {
    ((x >> i) & 1) != 0
}

/// Mode char-count-bit tables (`MODE_*`, index.mjs:448-450): index 0
/// is the mode selector bits, 1..3 the count widths by version band.
const MODE_NUMERIC: [u32; 4] = [1, 10, 12, 14];
const MODE_ALPHANUMERIC: [u32; 4] = [2, 9, 11, 13];
const MODE_BYTE: [u32; 4] = [4, 8, 16, 16];

fn num_char_count_bits(mode: &[u32; 4], ver: u8) -> u32 {
    mode[(ver as usize + 7) / 17 + 1]
}

struct QrSegment {
    mode: [u32; 4],
    num_chars: usize,
    bit_data: Vec<bool>,
}

fn append_bits(val: u32, len: u32, bb: &mut Vec<bool>) {
    assert!(len <= 31 && (val >> len) == 0, "Value out of range");
    for i in (0..len).rev() {
        bb.push(((val >> i) & 1) != 0);
    }
}

fn make_bytes(data: &[u8]) -> QrSegment {
    let mut bb = Vec::new();
    for &b in data {
        append_bits(b as u32, 8, &mut bb);
    }
    QrSegment { mode: MODE_BYTE, num_chars: data.len(), bit_data: bb }
}

/// `makeNumeric` (index.mjs:459-471).
fn make_numeric(digits: &str) -> QrSegment {
    assert!(is_numeric(digits), "String contains non-numeric characters");
    let mut bb = Vec::new();
    let mut i = 0usize;
    while i < digits.len() {
        let n = (digits.len() - i).min(3);
        let val: u32 = digits[i..i + n].parse().expect("digits");
        append_bits(val, (n as u32) * 3 + 1, &mut bb);
        i += n;
    }
    QrSegment { mode: MODE_NUMERIC, num_chars: digits.len(), bit_data: bb }
}

/// `makeAlphanumeric` (index.mjs:473-485).
fn make_alphanumeric(text: &str) -> QrSegment {
    assert!(is_alphanumeric(text), "String contains unencodable characters in alphanumeric mode");
    let bytes = text.as_bytes();
    let index_of = |c: u8| ALPHANUMERIC_CHARSET.iter().position(|&x| x == c).expect("charset");
    let mut bb = Vec::new();
    let mut i = 0usize;
    while i + 2 <= bytes.len() {
        let temp = index_of(bytes[i]) as u32 * 45 + index_of(bytes[i + 1]) as u32;
        append_bits(temp, 11, &mut bb);
        i += 2;
    }
    if i < bytes.len() {
        append_bits(index_of(bytes[i]) as u32, 6, &mut bb);
    }
    QrSegment { mode: MODE_ALPHANUMERIC, num_chars: bytes.len(), bit_data: bb }
}

/// `makeSegments` (index.mjs:487-494) — single-segment mode choice.
fn make_segments(text: &str) -> Vec<QrSegment> {
    if text.is_empty() {
        Vec::new()
    } else if is_numeric(text) {
        vec![make_numeric(text)]
    } else if is_alphanumeric(text) {
        vec![make_alphanumeric(text)]
    } else {
        // JS `toUtf8ByteArray` (encodeURI percent-decoding) == UTF-8 bytes.
        vec![make_bytes(text.as_bytes())]
    }
}

/// `NUMERIC_REGEX` (`^\d*$`, index.mjs:18).
fn is_numeric(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_digit())
}

/// `ALPHANUMERIC_REGEX` (`^[A-Z0-9 $%*+./:-]*$`, index.mjs:19).
fn is_alphanumeric(text: &str) -> bool {
    text.bytes().all(|b| {
        b.is_ascii_uppercase()
            || b.is_ascii_digit()
            || matches!(b, b' ' | b'$' | b'%' | b'*' | b'+' | b'.' | b'/' | b':')
    })
}

/// `getTotalBits` (index.mjs:501-511).
fn get_total_bits(segs: &[QrSegment], version: u8) -> Option<u32> {
    let mut result = 0u32;
    for seg in segs {
        let ccbits = num_char_count_bits(&seg.mode, version);
        if seg.num_chars >= (1usize << ccbits) {
            return None;
        }
        result += 4 + ccbits + seg.bit_data.len() as u32;
    }
    Some(result)
}

/// `getNumRawDataModules` (index.mjs:536-549).
fn get_num_raw_data_modules(ver: u8) -> u32 {
    assert!((MIN_VERSION..=MAX_VERSION).contains(&ver), "Version number out of range");
    let ver = ver as u32;
    let mut result = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let num_align = ver / 7 + 2;
        result -= (25 * num_align - 10) * num_align - 55;
        if ver >= 7 {
            result -= 36;
        }
    }
    result
}

/// `getNumDataCodewords` (index.mjs:551-553).
fn get_num_data_codewords(ver: u8, ecl: [u8; 2]) -> usize {
    (get_num_raw_data_modules(ver) / 8) as usize
        - (ECC_CODEWORDS_PER_BLOCK[ecl[0] as usize][ver as usize]
            * NUM_ERROR_CORRECTION_BLOCKS[ecl[0] as usize][ver as usize]) as usize
}

/// `reedSolomonComputeDivisor` (index.mjs:555-573).
fn reed_solomon_compute_divisor(degree: usize) -> Vec<u8> {
    assert!((1..=255).contains(&degree), "Degree out of range");
    let mut result = vec![0u8; degree - 1];
    result.push(1);
    let mut root: u8 = 1;
    for _ in 0..degree {
        for j in 0..result.len() {
            result[j] = reed_solomon_multiply(result[j], root);
            if j + 1 < result.len() {
                result[j] ^= result[j + 1];
            }
        }
        root = reed_solomon_multiply(root, 2);
    }
    result
}

/// `reedSolomonComputeRemainder` (index.mjs:575-585).
fn reed_solomon_compute_remainder(data: &[u8], divisor: &[u8]) -> Vec<u8> {
    let mut result = vec![0u8; divisor.len()];
    for &b in data {
        let factor = b ^ result.remove(0);
        result.push(0);
        for (i, &coef) in divisor.iter().enumerate() {
            result[i] ^= reed_solomon_multiply(coef, factor);
        }
    }
    result
}

/// `reedSolomonMultiply` (index.mjs:587-598) — GF(2^8) with the QR
/// field polynomial 0x11D (the JS `(z>>>7)*285` reduces to XOR 0x1D).
fn reed_solomon_multiply(x: u8, y: u8) -> u8 {
    let mut z: u8 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ (u8::from(z >> 7 == 1) * 0x1D);
        z ^= u8::from((y >> i) & 1 == 1) * x;
    }
    z
}

/// `encodeSegments` (index.mjs:600-641) with uqr `encode()`'s fixed
/// defaults: `minVersion 1`, `maxVersion 40`, `mask -1`,
/// `boostEcc false` (ref never overrides any of them).
fn encode_segments(segs: &[QrSegment], ecl: [u8; 2]) -> QrCode {
    let mut version = MIN_VERSION;
    let data_used_bits;
    loop {
        let data_capacity_bits = get_num_data_codewords(version, ecl) as u32 * 8;
        if let Some(used) = get_total_bits(segs, version) {
            if used <= data_capacity_bits {
                data_used_bits = used;
                break;
            }
        }
        assert!(version < MAX_VERSION, "Data too long");
        version += 1;
    }
    let _ = data_used_bits; // boostEcl is false — no ECL promotion.
    let mut bb: Vec<bool> = Vec::new();
    for seg in segs {
        append_bits(seg.mode[0], 4, &mut bb);
        append_bits(seg.num_chars as u32, num_char_count_bits(&seg.mode, version), &mut bb);
        bb.extend(seg.bit_data.iter().copied());
    }
    let data_capacity_bits = get_num_data_codewords(version, ecl) as u32 * 8;
    append_bits(0, (data_capacity_bits - bb.len() as u32).min(4), &mut bb);
    append_bits(0, (8 - bb.len() as u32 % 8) % 8, &mut bb);
    let mut pad_byte = 236u8;
    while (bb.len() as u32) < data_capacity_bits {
        append_bits(pad_byte as u32, 8, &mut bb);
        pad_byte ^= 236 ^ 17;
    }
    let mut data_codewords = vec![0u8; (bb.len() + 7) / 8];
    for (i, b) in bb.iter().enumerate() {
        if *b {
            data_codewords[i >> 3] |= 1 << (7 - (i & 7));
        }
    }
    QrCode::new(version, ecl, &data_codewords, -1)
}

/// `encode(data, { border })` (index.mjs:643-676 + `addBorder`
/// :678-700). Fixed as uqr's defaults: ecc `L`, no boost, auto mask.
#[must_use]
pub fn encode(data: &str, border: usize) -> QrResult {
    let qr = encode_segments(&make_segments(data), LOW);
    // addBorder: pad all sides with light (`false`) modules.
    let size = qr.size + border * 2;
    let mut rows: Vec<Vec<bool>> = Vec::with_capacity(size);
    for _ in 0..border {
        rows.push(vec![false; size]);
    }
    for row in &qr.modules {
        let mut padded = vec![false; border];
        padded.extend(row.iter().copied());
        padded.extend(vec![false; border]);
        rows.push(padded);
    }
    for _ in 0..border {
        rows.push(vec![false; size]);
    }
    QrResult { version: qr.version, mask_pattern: qr.mask, size, data: rows }
}

/// `renderUnicodeCompact` (index.mjs:681-706): two rows per text line
/// via half blocks. NOTE the palette polarity — a LIGHT module renders
/// as the solid `█` (the block chars draw dark-on-light terminals)
/// and out-of-bounds reads as BLACK (`getDataAt(..., true)`), so the
/// bottom half of an odd-sized code renders blank.
#[must_use]
pub fn render_unicode_compact(data: &str, border: usize) -> String {
    const WHITE_ALL: char = '\u{2588}'; // █
    const WHITE_BLACK: char = '\u{2580}'; // ▀
    const BLACK_WHITE: char = '\u{2584}'; // ▄
    const BLACK_ALL: char = ' ';
    let result = encode(data, border);
    let at = |x: i64, y: i64| -> bool {
        // getDataAt with default `true` (BLACK) out of bounds.
        if x < 0 || y < 0 || x >= result.size as i64 || y >= result.size as i64 {
            true
        } else {
            result.data[y as usize][x as usize]
        }
    };
    let mut lines = Vec::new();
    let mut row = 0usize;
    while row < result.size {
        let mut line = String::with_capacity(result.size);
        for col in 0..result.size {
            let (top, bottom) = (at(col as i64, row as i64), at(col as i64, row as i64 + 1));
            // WHITE == false, BLACK == true (module polarity).
            line.push(match (top, bottom) {
                (false, false) => WHITE_ALL,
                (false, true) => WHITE_BLACK,
                (true, false) => BLACK_WHITE,
                (true, true) => BLACK_ALL,
            });
        }
        lines.push(line);
        row += 2;
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden vectors generated from the ref-pinned uqr 0.1.3 itself
    /// (see ../README.md for the regen script).
    const GOLDEN: &str = include_str!("golden.json");

    #[derive(serde::Deserialize)]
    struct Golden {
        text: Option<String>,
        #[serde(rename = "compactFor")]
        compact_for: Option<String>,
        border: usize,
        version: Option<u8>,
        size: Option<usize>,
        mask: Option<u8>,
        data: Option<Vec<Vec<bool>>>,
        compact: Option<String>,
    }

    #[test]
    fn matrices_match_uqr_goldens() {
        let golden: Vec<Golden> = serde_json::from_str(GOLDEN).expect("golden.json parses");
        let mut checked = 0;
        for g in &golden {
            let Some(text) = &g.text else { continue };
            let r = encode(text, g.border);
            assert_eq!(r.version, g.version.expect("version"), "version for {text:?}");
            assert_eq!(r.size, g.size.expect("size"), "size for {text:?}");
            if let Some(mask) = g.mask {
                assert_eq!(r.mask_pattern, mask, "mask for {text:?}");
            }
            assert_eq!(r.data, g.data.clone().expect("data"), "module matrix for {text:?}");
            checked += 1;
        }
        assert!(checked >= 7, "golden coverage: {checked}");
    }

    #[test]
    fn compact_render_matches_uqr_goldens() {
        let golden: Vec<Golden> = serde_json::from_str(GOLDEN).expect("golden.json parses");
        let mut checked = 0;
        for g in &golden {
            let Some(text) = &g.compact_for else { continue };
            let compact = render_unicode_compact(text, g.border);
            assert_eq!(compact, g.compact.clone().expect("compact"), "compact render for {text:?}");
            checked += 1;
        }
        assert!(checked >= 7, "golden coverage: {checked}");
    }

    #[test]
    fn compact_shape_is_half_the_rows() {
        let compact = render_unicode_compact("http://localhost:4096/auth/connect/pair-code-123", 1);
        let lines: Vec<&str> = compact.split('\n').collect();
        // v3 → 29 modules + 2 border = 31 wide, ceil(31/2) = 16 rows.
        assert_eq!(lines.len(), 16);
        assert!(lines.iter().all(|l| l.chars().count() == 31));
        // Only the four palette characters appear.
        let set: std::collections::HashSet<char> = compact.chars().filter(|c| *c != '\n').collect();
        assert!(set.iter().all(|c| ['█', '▀', '▄', ' '].contains(c)), "{set:?}");
    }
}
