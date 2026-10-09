//! TUI golden-frame loading and comparison primitives (wave 7.0).
//!
//! The golden corpus under `goldens/tui-frames/` is exported from the
//! reference v2.0.24 TUI by `/tmp/opencode/tui-dump/export.ts` (bun), which
//! drives the reference opentui renderer headless through its own test
//! fixture harness. Each scenario is exported at two widths, as:
//!
//! - `<id>.w<width>.txt` — the exact `captureCharFrame()` text grid bytes
//!   (rows joined with `\n`, one trailing newline);
//! - `<id>.w<width>.ansi` — a canonical styled byte stream: one line per
//!   row, a single full-state SGR (`ESC [ 0 ; attrs ; fg ; bg m`) whenever
//!   the cell style changes, `\x1b[0m` at end of a non-default-styled line.
//!
//! This module mirrors the export format exactly (`parse_ansi` /
//! `render_ansi` are inverse on goldens) and provides the comparison
//! primitives the Phase 7 acceptance gate will use:
//!
//! - [`compare_text_grids`] — first-divergent-cell diff of two text grids;
//! - [`compare_ansi_bytes`] — first-divergent-offset byte diff;
//! - [`compare_ansi_frames`] / [`FrameFidelity`] — cell-wise glyph/style
//!   fidelity with the first divergence classified as dimensions / glyph /
//!   style;
//! - [`degrade_frame`] + [`ColorDepth`] — the TERM degradation matrix
//!   (truecolor → 256 → 16) used to compare renders across terminal
//!   capabilities;
//! - [`load_manifest`] / [`verify_goldens`] — manifest loading and SHA-256
//!   verification of the corpus on disk.
//!
//! Format reference (keep in sync): `/tmp/opencode/tui-dump/frame.ts` and
//! `crates/oc-tui/src/spike` (renderer-substrate spike serializer).
//!
//! Percentages convert terminal-bounded cell counts to floats, and SGR
//! parameter arithmetic is bounded by the parsed digit ranges, where the
//! pedantic cast lints carry no real risk.
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]

use std::fmt;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// sha256 helper (shared hex digest format with the exporter).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub generator: String,
    pub ansi_format: String,
    pub scenarios: Vec<ManifestScenario>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestScenario {
    pub id: String,
    pub source: String,
    pub description: String,
    pub component_class: String,
    pub variants: Vec<ManifestVariant>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestVariant {
    pub width: usize,
    pub height: usize,
    pub rows: usize,
    pub cols: usize,
    pub txt: ManifestBlob,
    pub ansi: ManifestBlob,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestBlob {
    pub bytes: u64,
    pub sha256: String,
}

impl Manifest {
    /// Locate a scenario variant by `(scenario id, width)`.
    #[must_use]
    pub fn variant(&self, id: &str, width: usize) -> Option<&ManifestVariant> {
        self.scenarios
            .iter()
            .find(|scenario| scenario.id == id)?
            .variants
            .iter()
            .find(|variant| variant.width == width)
    }
}

/// Load `manifest.json` from a golden corpus directory.
pub fn load_manifest(dir: &Path) -> Result<Manifest> {
    let bytes = std::fs::read(dir.join("manifest.json"))
        .with_context(|| format!("reading tui-frame manifest in {}", dir.display()))?;
    serde_json::from_slice(&bytes).with_context(|| "parsing tui-frame manifest")
}

/// Outcome of verifying one golden blob against the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoldenCheck {
    pub scenario: String,
    pub width: usize,
    pub kind: &'static str,
    pub ok: bool,
    pub detail: String,
}

/// Verify every `.txt` / `.ansi` file referenced by the manifest: present on
/// disk, byte length and SHA-256 match. Returns one check per blob.
pub fn verify_goldens(dir: &Path) -> Result<Vec<GoldenCheck>> {
    let manifest = load_manifest(dir)?;
    let mut checks = Vec::new();
    for scenario in &manifest.scenarios {
        for variant in &scenario.variants {
            for (kind, blob) in [("txt", &variant.txt), ("ansi", &variant.ansi)] {
                let path = dir.join(format!("{}.w{}.{}", scenario.id, variant.width, kind));
                let ok;
                let detail;
                match std::fs::read(&path) {
                    Err(error) => {
                        ok = false;
                        detail = format!("read error: {error}");
                    }
                    Ok(bytes) => {
                        let actual_sha = sha256_hex(&bytes);
                        if bytes.len() as u64 != blob.bytes {
                            ok = false;
                            detail = format!("length {} != manifest {}", bytes.len(), blob.bytes);
                        } else if actual_sha != blob.sha256 {
                            ok = false;
                            detail = format!("sha256 {actual_sha} != manifest {}", blob.sha256);
                        } else {
                            ok = true;
                            detail = String::new();
                        }
                    }
                }
                checks.push(GoldenCheck {
                    scenario: scenario.id.clone(),
                    width: variant.width,
                    kind,
                    ok,
                    detail,
                });
            }
        }
    }
    Ok(checks)
}

/// Load one golden text grid by `(scenario id, width)`.
pub fn load_text_grid(dir: &Path, id: &str, width: usize) -> Result<TextGrid> {
    let manifest = load_manifest(dir)?;
    let variant = manifest
        .variant(id, width)
        .with_context(|| format!("scenario {id}@{width} missing from manifest"))?;
    let bytes = std::fs::read(dir.join(format!("{id}.w{width}.txt")))?;
    let text = String::from_utf8(bytes)?;
    Ok(parse_text_grid(&text, Some((variant.rows, variant.cols))))
}

/// Load one golden styled frame by `(scenario id, width)`.
pub fn load_ansi_frame(dir: &Path, id: &str, width: usize) -> Result<AnsiFrame> {
    let manifest = load_manifest(dir)?;
    let variant = manifest
        .variant(id, width)
        .with_context(|| format!("scenario {id}@{width} missing from manifest"))?;
    let bytes = std::fs::read(dir.join(format!("{id}.w{width}.ansi")))?;
    let text = String::from_utf8(bytes)?;
    parse_ansi(&text, Some((variant.rows, variant.cols))).map_err(|error| anyhow::anyhow!(error))
}

// ---------------------------------------------------------------------------
// Text grid
// ---------------------------------------------------------------------------

/// A frame as a grid of characters (one `String` per row).
///
/// Rows are *not* padded to a uniform width; indexing past the end of a row
/// yields a blank (`' '`), matching how a terminal displays the remainder of
/// a short line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextGrid {
    pub rows: Vec<String>,
    pub height: usize,
    pub width: usize,
}

impl TextGrid {
    /// Character at `(row, col)`; blanks outside the written extent.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> char {
        self.rows.get(row).and_then(|line| line.chars().nth(col)).unwrap_or(' ')
    }
}

/// Parse golden text-grid bytes. With `dims = Some((rows, cols))` the parser
/// asserts the parsed grid matches the manifest dimensions.
#[must_use]
pub fn parse_text_grid(text: &str, dims: Option<(usize, usize)>) -> TextGrid {
    let mut rows: Vec<String> = text.split('\n').map(str::to_string).collect();
    if rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    let height = rows.len();
    let width = rows.iter().map(|row| row.chars().count()).max().unwrap_or(0);
    let grid = TextGrid { rows, height, width };
    if let Some((expected_rows, expected_cols)) = dims {
        assert_eq!(
            (grid.height, grid.width),
            (expected_rows, expected_cols),
            "text grid dims do not match manifest"
        );
    }
    grid
}

/// First divergent cell between two text grids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellDivergence {
    pub row: usize,
    pub col: usize,
    pub expected: char,
    pub actual: char,
}

/// Result of comparing two text grids cell-by-cell over the union extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextGridDiff {
    pub expected_height: usize,
    pub actual_height: usize,
    pub expected_width: usize,
    pub actual_width: usize,
    pub first_divergence: Option<CellDivergence>,
    /// Cells with equal characters (blanks outside a grid count as blanks).
    pub matching_cells: usize,
    /// Union-extent cell count (`max_height * max_width`).
    pub total_cells: usize,
}

impl TextGridDiff {
    /// Percentage of matching cells over the compared extent (0–100).
    #[must_use]
    pub fn match_pct(&self) -> f64 {
        if self.total_cells == 0 {
            return 100.0;
        }
        self.matching_cells as f64 * 100.0 / self.total_cells as f64
    }
}

#[must_use]
pub fn compare_text_grids(expected: &TextGrid, actual: &TextGrid) -> TextGridDiff {
    let height = expected.height.max(actual.height);
    let width = expected.width.max(actual.width);
    let mut diff = TextGridDiff {
        expected_height: expected.height,
        actual_height: actual.height,
        expected_width: expected.width,
        actual_width: actual.width,
        first_divergence: None,
        matching_cells: 0,
        total_cells: height * width,
    };
    for row in 0..height {
        for col in 0..width {
            let expected_cell = expected.cell(row, col);
            let actual_cell = actual.cell(row, col);
            if expected_cell == actual_cell {
                diff.matching_cells += 1;
            } else if diff.first_divergence.is_none() {
                diff.first_divergence =
                    Some(CellDivergence { row, col, expected: expected_cell, actual: actual_cell });
            }
        }
    }
    diff
}

// ---------------------------------------------------------------------------
// ANSI styled frame
// ---------------------------------------------------------------------------

/// Cell color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Text attributes — ECMA-48 bit values, matching the exporter
/// (`@opentui/core` `TextAttributes`).
pub mod attrs {
    pub const BOLD: u16 = 1 << 0;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const BLINK: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const HIDDEN: u16 = 1 << 6;
    pub const STRIKETHROUGH: u16 = 1 << 7;
}

/// Full style of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellStyle {
    pub attrs: u16,
    pub fg: Color,
    pub bg: Color,
}

impl CellStyle {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.attrs == 0 && self.fg == Color::Default && self.bg == Color::Default
    }
}

/// One styled cell (single scalar character; grapheme clusters are stored
/// as leading char + space continuations by the exporter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyledCell {
    pub ch: char,
    pub style: CellStyle,
}

/// A parsed golden ANSI stream: one row of styled cells per screen row,
/// padded to the frame width with default-styled blanks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiFrame {
    pub rows: Vec<Vec<StyledCell>>,
    pub height: usize,
    pub width: usize,
}

impl AnsiFrame {
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> StyledCell {
        self.rows
            .get(row)
            .and_then(|cells| cells.get(col))
            .copied()
            .unwrap_or(StyledCell { ch: ' ', style: CellStyle::default() })
    }
}

#[derive(Debug, Clone)]
pub struct AnsiParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for AnsiParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ansi parse error at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for AnsiParseError {}

/// Parse a canonical (or general SGR) ANSI byte stream into a styled frame.
///
/// Supported: SGR sequences (`ESC [ ... m`, parameter subset used by the
/// exporter plus the common off-codes), text, and `\n` row breaks. Any other
/// escape sequence is rejected so silent corruption cannot pass as a match.
/// Short rows are padded with default-styled blanks to the frame width.
pub fn parse_ansi(text: &str, dims: Option<(usize, usize)>) -> Result<AnsiFrame, AnsiParseError> {
    let mut rows: Vec<Vec<StyledCell>> = Vec::new();
    let mut current: Vec<StyledCell> = Vec::new();
    let mut style = CellStyle::default();
    let mut offset = 0usize;
    let chars: Vec<char> = text.chars().collect();

    let err =
        |offset: usize, message: &str| AnsiParseError { offset, message: message.to_string() };

    while offset < chars.len() {
        let ch = chars[offset];
        if ch == '\x1b' {
            if chars.get(offset + 1) != Some(&'[') {
                return Err(err(offset, "expected CSI '[' after ESC"));
            }
            let mut end = offset + 2;
            let mut params = String::new();
            while end < chars.len() && chars[end].is_ascii_digit()
                || end < chars.len() && chars[end] == ';'
            {
                params.push(chars[end]);
                end += 1;
            }
            if end >= chars.len() || chars[end] != 'm' {
                return Err(err(offset, "only SGR (…m) sequences are allowed"));
            }
            style = apply_sgr(&style, &parse_sgr_params(&params));
            offset = end + 1;
            continue;
        }
        match ch {
            '\n' => {
                rows.push(std::mem::take(&mut current));
            }
            '\r' => {}
            _ => current.push(StyledCell { ch, style }),
        }
        offset += 1;
    }
    if !current.is_empty() {
        rows.push(current);
    }
    if let Some((expected_rows, _)) = dims {
        if rows.len() != expected_rows {
            return Err(err(
                text.len(),
                &format!("row count {} != manifest {expected_rows}", rows.len()),
            ));
        }
    }
    let height = rows.len();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        if row.len() < width {
            row.resize(width, StyledCell { ch: ' ', style: CellStyle::default() });
        }
    }
    Ok(AnsiFrame { rows, height, width })
}

fn parse_sgr_params(params: &str) -> Vec<u16> {
    if params.is_empty() {
        return vec![0];
    }
    params.split(';').map(|p| p.parse::<u16>().unwrap_or(0)).collect()
}

/// Apply one SGR parameter list to a style (full-state sequences produced by
/// the exporter and incremental sequences accepted for robustness).
fn apply_sgr(style: &CellStyle, params: &[u16]) -> CellStyle {
    let mut next = *style;
    let mut index = 0;
    while index < params.len() {
        let param = params[index];
        match param {
            0 => next = CellStyle::default(),
            1 => next.attrs |= attrs::BOLD,
            2 => next.attrs |= attrs::DIM,
            3 => next.attrs |= attrs::ITALIC,
            4 => next.attrs |= attrs::UNDERLINE,
            5 => next.attrs |= attrs::BLINK,
            7 => next.attrs |= attrs::INVERSE,
            8 => next.attrs |= attrs::HIDDEN,
            9 => next.attrs |= attrs::STRIKETHROUGH,
            21 | 22 => next.attrs &= !(attrs::BOLD | attrs::DIM),
            23 => next.attrs &= !attrs::ITALIC,
            24 => next.attrs &= !attrs::UNDERLINE,
            25 => next.attrs &= !attrs::BLINK,
            27 => next.attrs &= !attrs::INVERSE,
            28 => next.attrs &= !attrs::HIDDEN,
            29 => next.attrs &= !attrs::STRIKETHROUGH,
            30..=37 => next.fg = Color::Indexed((param - 30) as u8),
            39 => next.fg = Color::Default,
            40..=47 => next.bg = Color::Indexed((param - 40) as u8),
            49 => next.bg = Color::Default,
            90..=97 => next.fg = Color::Indexed((param - 90 + 8) as u8),
            100..=107 => next.bg = Color::Indexed((param - 100 + 8) as u8),
            38 | 48 => {
                let target = if param == 38 { &mut next.fg } else { &mut next.bg };
                match params.get(index + 1) {
                    Some(5) => {
                        if let Some(value) = params.get(index + 2) {
                            *target = Color::Indexed(*value as u8);
                        }
                        index += 2;
                    }
                    Some(2) => {
                        if let (Some(r), Some(g), Some(b)) =
                            (params.get(index + 2), params.get(index + 3), params.get(index + 4))
                        {
                            *target = Color::Rgb(*r as u8, *g as u8, *b as u8);
                        }
                        index += 4;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        index += 1;
    }
    next
}

/// Serialize a styled frame to the canonical byte format (byte-inverse of
/// [`parse_ansi`] for goldens; also the output format of the oc-tui spike).
#[must_use]
pub fn render_ansi(frame: &AnsiFrame) -> String {
    let mut out = String::new();
    for row in &frame.rows {
        let mut current = CellStyle::default();
        let mut line = String::new();
        for cell in row {
            if cell.style != current {
                line.push_str(&sgr(&cell.style));
                current = cell.style;
            }
            line.push(cell.ch);
        }
        if !current.is_default() {
            line.push_str("\x1b[0m");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Full-state SGR for a style, identical to the exporter's `sgr()`.
fn sgr(style: &CellStyle) -> String {
    let mut params: Vec<String> = vec!["0".into()];
    if style.attrs & attrs::BOLD != 0 {
        params.push("1".into());
    }
    if style.attrs & attrs::DIM != 0 {
        params.push("2".into());
    }
    if style.attrs & attrs::ITALIC != 0 {
        params.push("3".into());
    }
    if style.attrs & attrs::UNDERLINE != 0 {
        params.push("4".into());
    }
    if style.attrs & attrs::BLINK != 0 {
        params.push("5".into());
    }
    if style.attrs & attrs::INVERSE != 0 {
        params.push("7".into());
    }
    if style.attrs & attrs::HIDDEN != 0 {
        params.push("8".into());
    }
    if style.attrs & attrs::STRIKETHROUGH != 0 {
        params.push("9".into());
    }
    params.extend(color_params(style.fg, 30));
    params.extend(color_params(style.bg, 40));
    format!("\x1b[{}m", params.join(";"))
}

fn color_params(color: Color, base: u8) -> Vec<String> {
    match color {
        Color::Default => vec![(base + 9).to_string()],
        Color::Indexed(index) => vec![(base + 8).to_string(), "5".into(), index.to_string()],
        Color::Rgb(r, g, b) => {
            vec![(base + 8).to_string(), "2".into(), r.to_string(), g.to_string(), b.to_string()]
        }
    }
}

/// First divergent offset between two ANSI byte streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiByteDiff {
    pub first_divergent_offset: Option<usize>,
    pub expected_len: usize,
    pub actual_len: usize,
    pub equal: bool,
}

#[must_use]
pub fn compare_ansi_bytes(expected: &str, actual: &str) -> AnsiByteDiff {
    let expected_bytes = expected.as_bytes();
    let actual_bytes = actual.as_bytes();
    let mut offset = 0usize;
    while offset < expected_bytes.len() && offset < actual_bytes.len() {
        if expected_bytes[offset] != actual_bytes[offset] {
            return AnsiByteDiff {
                first_divergent_offset: Some(offset),
                expected_len: expected_bytes.len(),
                actual_len: actual_bytes.len(),
                equal: false,
            };
        }
        offset += 1;
    }
    AnsiByteDiff {
        first_divergent_offset: (expected_bytes.len() != actual_bytes.len()).then_some(offset),
        expected_len: expected_bytes.len(),
        actual_len: actual_bytes.len(),
        equal: expected_bytes.len() == actual_bytes.len(),
    }
}

// ---------------------------------------------------------------------------
// Frame fidelity comparison
// ---------------------------------------------------------------------------

/// Class of the first divergence between two styled frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivergenceClass {
    /// Frame dimensions differ.
    Dimensions,
    /// The glyph at the first divergent cell differs (layout/text problem).
    Glyph,
    /// The glyph matches but the style differs (color/attribute problem).
    Style,
}

impl fmt::Display for DivergenceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DivergenceClass::Dimensions => write!(f, "dimensions"),
            DivergenceClass::Glyph => write!(f, "glyph"),
            DivergenceClass::Style => write!(f, "style"),
        }
    }
}

/// Cell-wise fidelity of a rendered frame against a golden.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameFidelity {
    pub glyph_matching_cells: usize,
    pub style_matching_cells: usize,
    /// Cells where glyph AND style both match.
    pub both_matching_cells: usize,
    pub total_cells: usize,
    /// First cell whose glyph differs, if any.
    pub first_glyph_divergence: Option<(usize, usize)>,
    /// First cell whose style differs (glyph may match), if any.
    pub first_style_divergence: Option<(usize, usize)>,
    pub dimension_mismatch: bool,
}

impl FrameFidelity {
    #[must_use]
    pub fn glyph_match_pct(&self) -> f64 {
        pct(self.glyph_matching_cells, self.total_cells)
    }
    #[must_use]
    pub fn style_match_pct(&self) -> f64 {
        pct(self.style_matching_cells, self.total_cells)
    }
    /// Cells where glyph AND style both match.
    #[must_use]
    pub fn cell_match_pct(&self) -> f64 {
        pct(self.both_matching_cells, self.total_cells)
    }
    /// First divergence across glyph and style, classified.
    #[must_use]
    pub fn first_divergence(&self) -> Option<(usize, usize, DivergenceClass)> {
        if self.dimension_mismatch {
            return Some((0, 0, DivergenceClass::Dimensions));
        }
        match (self.first_glyph_divergence, self.first_style_divergence) {
            (Some(glyph), Some(style)) => {
                if glyph <= style {
                    Some((glyph.0, glyph.1, DivergenceClass::Glyph))
                } else {
                    Some((style.0, style.1, DivergenceClass::Style))
                }
            }
            (Some(glyph), None) => Some((glyph.0, glyph.1, DivergenceClass::Glyph)),
            (None, Some(style)) => Some((style.0, style.1, DivergenceClass::Style)),
            (None, None) => None,
        }
    }
    #[must_use]
    pub fn all_match(&self) -> bool {
        self.both_matching_cells == self.total_cells
    }
}

fn pct(part: usize, total: usize) -> f64 {
    if total == 0 {
        100.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

/// Compare two styled frames cell-by-cell over the union extent.
#[must_use]
pub fn compare_ansi_frames(expected: &AnsiFrame, actual: &AnsiFrame) -> FrameFidelity {
    let height = expected.height.max(actual.height);
    let width = expected.width.max(actual.width);
    let dimension_mismatch = expected.height != actual.height || expected.width != actual.width;
    let mut fidelity = FrameFidelity {
        glyph_matching_cells: 0,
        style_matching_cells: 0,
        both_matching_cells: 0,
        total_cells: height * width,
        first_glyph_divergence: None,
        first_style_divergence: None,
        dimension_mismatch,
    };
    for row in 0..height {
        for col in 0..width {
            let expected_cell = expected.cell(row, col);
            let actual_cell = actual.cell(row, col);
            let glyph_ok = expected_cell.ch == actual_cell.ch;
            // Style comparisons treat out-of-frame cells as default-styled
            // blanks, so trailing padding participates naturally.
            let style_ok = expected_cell.style == actual_cell.style;
            if glyph_ok {
                fidelity.glyph_matching_cells += 1;
            } else if fidelity.first_glyph_divergence.is_none() {
                fidelity.first_glyph_divergence = Some((row, col));
            }
            if style_ok {
                fidelity.style_matching_cells += 1;
            } else if fidelity.first_style_divergence.is_none() {
                fidelity.first_style_divergence = Some((row, col));
            }
            if glyph_ok && style_ok {
                fidelity.both_matching_cells += 1;
            }
        }
    }
    fidelity
}

/// Compare a golden text grid (glyphs only) against a styled render.
#[must_use]
pub fn compare_text_grid_to_frame(grid: &TextGrid, frame: &AnsiFrame) -> FrameFidelity {
    let height = grid.height.max(frame.height);
    let width = grid.width.max(frame.width);
    let dimension_mismatch = grid.height != frame.height || grid.width != frame.width;
    let mut fidelity = FrameFidelity {
        glyph_matching_cells: 0,
        style_matching_cells: 0,
        both_matching_cells: 0,
        total_cells: height * width,
        first_glyph_divergence: None,
        first_style_divergence: None,
        dimension_mismatch,
    };
    for row in 0..height {
        for col in 0..width {
            let expected = grid.cell(row, col);
            let actual = frame.cell(row, col).ch;
            if expected == actual {
                fidelity.glyph_matching_cells += 1;
                fidelity.style_matching_cells += 1;
                fidelity.both_matching_cells += 1;
            } else if fidelity.first_glyph_divergence.is_none() {
                fidelity.first_glyph_divergence = Some((row, col));
            }
        }
    }
    fidelity
}

// ---------------------------------------------------------------------------
// TERM degradation matrix
// ---------------------------------------------------------------------------

/// Target color depth for degradation comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Standard xterm 16-color palette (indices 0–15 of the 256 palette).
pub const ANSI16_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (128, 0, 0),
    (0, 128, 0),
    (128, 128, 0),
    (0, 0, 128),
    (128, 0, 128),
    (0, 128, 128),
    (192, 192, 192),
    (128, 128, 128),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (0, 0, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// RGB value of an xterm-256 palette index.
#[must_use]
pub fn xterm256_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => ANSI16_RGB[index as usize],
        16..=231 => {
            let steps = [0u8, 95, 135, 175, 215, 255];
            let index = index - 16;
            let r = steps[(index / 36) as usize];
            let g = steps[((index / 6) % 6) as usize];
            let b = steps[(index % 6) as usize];
            (r, g, b)
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        }
    }
}

fn color_distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let dr = u32::from(a.0.abs_diff(b.0));
    let dg = u32::from(a.1.abs_diff(b.1));
    let db = u32::from(a.2.abs_diff(b.2));
    dr * dr + dg * dg + db * db
}

/// Nearest xterm-256 palette index for an RGB triple.
#[must_use]
pub fn nearest_ansi256(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for index in 0..=255u8 {
        let distance = color_distance((r, g, b), xterm256_rgb(index));
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}

/// Nearest ANSI-16 palette index for an RGB triple.
#[must_use]
pub fn nearest_ansi16(r: u8, g: u8, b: u8) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for (index, rgb) in ANSI16_RGB.iter().enumerate() {
        let distance = color_distance((r, g, b), *rgb);
        if distance < best_distance {
            best_distance = distance;
            best = index as u8;
        }
    }
    best
}

/// Degrade one color to the target depth.
#[must_use]
pub fn degrade_color(color: Color, depth: ColorDepth) -> Color {
    match depth {
        ColorDepth::TrueColor => color,
        ColorDepth::Ansi256 => match color {
            Color::Default => Color::Default,
            Color::Indexed(_) => color,
            Color::Rgb(r, g, b) => Color::Indexed(nearest_ansi256(r, g, b)),
        },
        ColorDepth::Ansi16 => match color {
            Color::Default => Color::Default,
            Color::Indexed(index) => {
                let (r, g, b) = xterm256_rgb(index);
                Color::Indexed(nearest_ansi16(r, g, b))
            }
            Color::Rgb(r, g, b) => Color::Indexed(nearest_ansi16(r, g, b)),
        },
    }
}

/// Degrade every cell color of a frame to the target depth (glyphs and
/// attributes untouched) — the TERM degradation matrix.
#[must_use]
pub fn degrade_frame(frame: &AnsiFrame, depth: ColorDepth) -> AnsiFrame {
    AnsiFrame {
        rows: frame
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| StyledCell {
                        ch: cell.ch,
                        style: CellStyle {
                            attrs: cell.style.attrs,
                            fg: degrade_color(cell.style.fg, depth),
                            bg: degrade_color(cell.style.bg, depth),
                        },
                    })
                    .collect()
            })
            .collect(),
        height: frame.height,
        width: frame.width,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_grid_parses_and_pads() {
        let grid = parse_text_grid("ab\ncd\n", None);
        assert_eq!(grid.height, 2);
        assert_eq!(grid.width, 2);
        assert_eq!(grid.cell(0, 0), 'a');
        assert_eq!(grid.cell(1, 1), 'd');
        assert_eq!(grid.cell(5, 5), ' ');
    }

    #[test]
    fn text_grid_diff_finds_first_divergent_cell() {
        let expected = parse_text_grid("abc\ndef\n", None);
        let actual = parse_text_grid("abc\nxef\n", None);
        let diff = compare_text_grids(&expected, &actual);
        let first = diff.first_divergence.expect("divergence");
        assert_eq!((first.row, first.col), (1, 0));
        assert_eq!((first.expected, first.actual), ('d', 'x'));
        assert_eq!(diff.matching_cells, 5);
        assert_eq!(diff.total_cells, 6);
    }

    #[test]
    fn text_grid_diff_handles_dimension_mismatch() {
        let expected = parse_text_grid("ab\n", None);
        let actual = parse_text_grid("ab\ncd\n", None);
        let diff = compare_text_grids(&expected, &actual);
        let first = diff.first_divergence.expect("divergence");
        assert_eq!((first.row, first.col), (1, 0));
        assert_eq!(first.expected, ' ');
        assert_eq!(first.actual, 'c');
    }

    #[test]
    fn ansi_roundtrip_full_state_sgr() {
        let frame = AnsiFrame {
            rows: vec![
                vec![
                    StyledCell {
                        ch: 'a',
                        style: CellStyle { fg: Color::Rgb(1, 2, 3), ..Default::default() },
                    },
                    StyledCell {
                        ch: 'b',
                        style: CellStyle { fg: Color::Rgb(1, 2, 3), ..Default::default() },
                    },
                ],
                vec![
                    StyledCell { ch: 'x', style: CellStyle::default() },
                    StyledCell { ch: 'y', style: CellStyle::default() },
                ],
            ],
            height: 2,
            width: 2,
        };
        let text = render_ansi(&frame);
        // Full-state SGR includes default channels (bg default -> 49).
        assert_eq!(text, "\x1b[0;38;2;1;2;3;49mab\x1b[0m\nxy\n");
        let parsed = parse_ansi(&text, None).expect("parse");
        assert_eq!(parsed, frame);
    }

    #[test]
    fn ansi_parse_attributes_indexed_and_incremental() {
        let text = "\x1b[0;1;3;38;5;17;48;5;4mz\x1b[0m\n\x1b[1mA\x1b[22mB\n";
        let frame = parse_ansi(text, None).expect("parse");
        let z = frame.cell(0, 0);
        assert_eq!(z.ch, 'z');
        assert_eq!(z.style.attrs, attrs::BOLD | attrs::ITALIC);
        assert_eq!(z.style.fg, Color::Indexed(17));
        assert_eq!(z.style.bg, Color::Indexed(4));
        let a = frame.cell(1, 0);
        assert_eq!(a.style.attrs, attrs::BOLD);
        let b = frame.cell(1, 1);
        assert_eq!(b.style.attrs, 0, "22 clears bold");
    }

    #[test]
    fn ansi_byte_diff_finds_first_offset() {
        let a = "\x1b[0;38;2;1;2;3mab\n";
        let b = "\x1b[0;38;2;1;2;4mab\n";
        let diff = compare_ansi_bytes(a, b);
        // Byte 13 is the blue digit of the fg RGB triple.
        assert_eq!(diff.first_divergent_offset, Some(13));
        assert!(!diff.equal);
        assert!(compare_ansi_bytes(a, a).equal);
        // Common-prefix streams of different length diverge at the prefix end.
        let diff = compare_ansi_bytes("ab", "abc");
        assert_eq!(diff.first_divergent_offset, Some(2));
        assert!(!diff.equal);
    }

    #[test]
    fn fidelity_classifies_glyph_vs_style_divergence() {
        let golden = AnsiFrame {
            rows: vec![vec![
                StyledCell {
                    ch: 'a',
                    style: CellStyle { fg: Color::Rgb(9, 9, 9), ..Default::default() },
                },
                StyledCell { ch: 'b', style: CellStyle::default() },
            ]],
            height: 1,
            width: 2,
        };
        let wrong_glyph = AnsiFrame {
            rows: vec![vec![
                StyledCell {
                    ch: 'X',
                    style: CellStyle { fg: Color::Rgb(9, 9, 9), ..Default::default() },
                },
                StyledCell { ch: 'b', style: CellStyle::default() },
            ]],
            height: 1,
            width: 2,
        };
        let wrong_style = AnsiFrame {
            rows: vec![vec![
                StyledCell {
                    ch: 'a',
                    style: CellStyle { fg: Color::Rgb(8, 9, 9), ..Default::default() },
                },
                StyledCell { ch: 'b', style: CellStyle::default() },
            ]],
            height: 1,
            width: 2,
        };
        let (_, _, class) = compare_ansi_frames(&golden, &wrong_glyph).first_divergence().unwrap();
        assert_eq!(class, DivergenceClass::Glyph);
        let (_, _, class) = compare_ansi_frames(&golden, &wrong_style).first_divergence().unwrap();
        assert_eq!(class, DivergenceClass::Style);
        assert!(compare_ansi_frames(&golden, &golden).all_match());
    }

    #[test]
    fn degradation_matrix_reduces_truecolor() {
        // Nearest-palette search walks indices low→high with strict `<`, so
        // the basic 16 win ties against the cube/grayscale (documented).
        let white = Color::Rgb(255, 255, 255);
        assert_eq!(degrade_color(white, ColorDepth::Ansi256), Color::Indexed(15));
        assert_eq!(degrade_color(white, ColorDepth::Ansi16), Color::Indexed(15));
        let near_blue = Color::Rgb(10, 10, 250);
        let indexed = degrade_color(near_blue, ColorDepth::Ansi256);
        assert_eq!(indexed, Color::Indexed(12));
        let reduced = degrade_color(indexed, ColorDepth::Ansi16);
        assert_eq!(reduced, Color::Indexed(12));
        // Default survives degradation untouched.
        assert_eq!(degrade_color(Color::Default, ColorDepth::Ansi16), Color::Default);
        // A cube-only color degrades to the cube, then to its nearest basic.
        let teal = Color::Rgb(0, 135, 135);
        let teal256 = degrade_color(teal, ColorDepth::Ansi256);
        assert_eq!(teal256, Color::Indexed(30), "cube (0,2,2)");
        assert_eq!(degrade_color(teal256, ColorDepth::Ansi16), Color::Indexed(6));
        // Frame degradation only touches colors.
        let frame = AnsiFrame {
            rows: vec![vec![StyledCell {
                ch: 'a',
                style: CellStyle { fg: near_blue, ..Default::default() },
            }]],
            height: 1,
            width: 1,
        };
        let degraded = degrade_frame(&frame, ColorDepth::Ansi16);
        assert_eq!(degraded.cell(0, 0).style.fg, Color::Indexed(12));
        assert_eq!(degraded.cell(0, 0).ch, 'a');
    }

    #[test]
    fn golden_corpus_on_disk_verifies_against_manifest() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames");
        let checks = verify_goldens(&dir).expect("verify goldens");
        assert!(
            checks.len() >= 30,
            "expected at least 15 scenarios × 2 widths × 2 blobs, got {}",
            checks.len()
        );
        let failures: Vec<&GoldenCheck> = checks.iter().filter(|check| !check.ok).collect();
        assert!(failures.is_empty(), "golden verification failures: {failures:?}");
    }

    #[test]
    fn golden_frames_roundtrip_through_the_canonical_format() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames");
        let manifest = load_manifest(&dir).expect("manifest");
        let mut checked = 0;
        for scenario in &manifest.scenarios {
            for variant in &scenario.variants {
                let frame = load_ansi_frame(&dir, &scenario.id, variant.width).expect("load ansi");
                let text = std::fs::read_to_string(
                    dir.join(format!("{}.w{}.ansi", scenario.id, variant.width)),
                )
                .expect("read ansi");
                assert_eq!(render_ansi(&frame), text, "roundtrip failed for {}", scenario.id);
                // Styled-frame glyphs must agree with the text grid golden.
                let grid = load_text_grid(&dir, &scenario.id, variant.width).expect("load txt");
                for row in 0..grid.height {
                    for col in 0..grid.width {
                        assert_eq!(
                            frame.cell(row, col).ch,
                            grid.cell(row, col),
                            "glyph mismatch at {row},{col} in {}",
                            scenario.id
                        );
                    }
                }
                checked += 1;
            }
        }
        assert!(checked >= 36, "expected at least 36 variants, checked {checked}");
    }
}
