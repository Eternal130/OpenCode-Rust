//! TUI golden-frame loading and comparison primitives (wave 7.0; gate-8 P1
//! + findings 2/5 in wave 7.1).
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
//! **Corpus theme (gate-8 hard precondition P1)**: every scenario of the
//! `tui-frame-golden/2` corpus renders under the reference's default theme —
//! the built-in `opencode` v2 theme resolved in dark mode (manifest
//! `theme: { id: "opencode", mode: "dark" }`). The pre-P1 corpus, which
//! mixed three style bases (opentui preload defaults, mini
//! `RUN_THEME_FALLBACK`, v2-dark `emptyThemeSource`), is kept under
//! `goldens/tui-frames/legacy-mixed-theme/` with its manifest marked
//! `superseded: true` — diffing reference only, never an acceptance target.
//!
//! **Width-edge scenarios (finding 2)**: `width-edge-*` pin opentui's
//! actual display-width behavior for CJK text, emoji + ZWJ sequences +
//! skin-tone modifiers, and combining marks / East-Asian-ambiguous
//! characters, at 80 and 40 columns. Each also ships a `<id>.widths.json`
//! sidecar with the probe strings measured by the reference's own width
//! function (`string-width@7.2.0` bundled by `@opentui/core`; ambiguous
//! chars narrow, emoji 2 cells) — ground truth for unit-testing the Rust
//! `display_width` without going through frames.
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
//! - [`compare_ansi_frames_at`] / [`compare_ansi_bytes_at`] +
//!   [`ComparisonDepth`] — the same comparisons at an explicit style depth
//!   (truecolor-canonical, or both-sides-degraded 256; see the
//!   [`ComparisonDepth`] docs for the approximation caveat and
//!   `tools/parity/NORMALIZATION.md` §10);
//! - [`degrade_frame`] + [`ColorDepth`] — the TERM degradation matrix
//!   (truecolor → 256 → 16) used to compare renders across terminal
//!   capabilities;
//! - [`load_manifest`] / [`verify_goldens`] / [`load_text_grid`] /
//!   [`load_ansi_frame`] / [`load_widths_sidecar`] — the Phase 7 gate's
//!   ingestion API.
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

/// Corpus theme descriptor (gate-8 hard precondition P1): every scenario of
/// a `tui-frame-golden/2` corpus renders under this theme — the reference's
/// default built-in `opencode` v2 theme (`src/theme/assets/v2/opencode.json`)
/// resolved in the given mode.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestTheme {
    pub id: String,
    pub label: String,
    pub mode: String,
}

/// Style comparison depth a scenario is judged at (oracle-gate8 finding 5).
///
/// Goldens are exported headless (TERM-free) in truecolor. Tier-A scenarios
/// compare the canonical streams byte-for-byte in that form
/// ([`ComparisonDepth::TruecolorCanonical`]). Tier-B scenarios tolerate exact
/// hue drift by degrading **both** sides to the xterm-256 palette before the
/// byte/style comparison ([`ComparisonDepth::Degraded256`]) — this
/// degradation is **our approximation** of the reference's native
/// TERM=xterm-256color rendering, not a capture of it: the goldens never
/// passed through a 256-color terminal, so the nearest-palette mapping (see
/// [`nearest_ansi256`], documented tie-breaking) stands in for whatever the
/// reference would emit natively. Good enough to assert "same perceived
/// color class"; not a byte-parity claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonDepth {
    #[serde(rename = "truecolor-canonical")]
    TruecolorCanonical,
    #[serde(rename = "degraded-256")]
    Degraded256,
}

impl ComparisonDepth {
    /// The TERM color depth this comparison mode degrades frames to.
    #[must_use]
    pub fn color_depth(self) -> ColorDepth {
        match self {
            ComparisonDepth::TruecolorCanonical => ColorDepth::TrueColor,
            ComparisonDepth::Degraded256 => ColorDepth::Ansi256,
        }
    }
}

impl fmt::Display for ComparisonDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComparisonDepth::TruecolorCanonical => write!(f, "truecolor-canonical"),
            ComparisonDepth::Degraded256 => write!(f, "degraded-256"),
        }
    }
}

/// Acceptance tier of a scenario (oracle-gate8 approval table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioClass {
    TierA,
    TierB,
    TierC1,
    WidthEdge,
}

/// A measured-width sidecar (`<id>.widths.json`) of a `width-edge`
/// scenario: the exact probe strings plus their display widths as measured
/// by the reference's own width function (`string-width@7.2.0` bundled by
/// `@opentui/core`, the renderer's "unicode" width method). The Rust
/// `display_width` is unit-tested against these numbers directly, without
/// going through frames.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSidecar {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub samples: usize,
}

/// One measured sample of a widths sidecar.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WidthSample {
    pub text: String,
    pub utf16: usize,
    pub codepoints: usize,
    pub width: usize,
}

/// The parsed content of a `<id>.widths.json` sidecar.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WidthsSidecar {
    pub id: String,
    pub measured_by: String,
    pub width_method: String,
    pub samples: Vec<WidthSample>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub generator: String,
    pub ansi_format: String,
    /// Corpus theme; absent on legacy (pre-P1) `tui-frame-golden/1` manifests.
    #[serde(default)]
    pub theme: Option<ManifestTheme>,
    /// Default comparison depth for scenarios without an explicit override.
    #[serde(default)]
    pub comparison_depth_default: Option<ComparisonDepth>,
    /// True on superseded legacy corpora kept for diffing reference only.
    #[serde(default)]
    pub superseded: bool,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub superseded_reason: Option<String>,
    pub scenarios: Vec<ManifestScenario>,
    /// Keypress-replay scripts (gate-8 non-frame acceptance track) pinned
    /// under `replay/`; absent on corpora predating the second export wave.
    #[serde(default)]
    pub replay: Vec<ManifestReplay>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestScenario {
    pub id: String,
    pub source: String,
    pub description: String,
    pub component_class: String,
    /// Acceptance tier; absent on legacy manifests.
    #[serde(default)]
    pub class: Option<ScenarioClass>,
    /// Explicit comparison-depth override for this scenario.
    #[serde(default)]
    pub comparison_depth: Option<ComparisonDepth>,
    /// Virtual-scroll scenarios: the pinned scroll offset this frame records
    /// (`vscroll-*` family — one scenario per offset of the same fixture).
    #[serde(default)]
    pub scroll_offset: Option<String>,
    pub variants: Vec<ManifestVariant>,
    /// Measured-width sidecar (width-edge scenarios).
    #[serde(default)]
    pub widths: Option<ManifestSidecar>,
}

/// Manifest entry for one keypress-replay script (`replay/<id>.json`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ManifestReplay {
    pub id: String,
    pub description: String,
    pub source: String,
    /// Key-event token encoding the steps use ("kitty").
    pub keys: String,
    pub steps: usize,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
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

    /// Effective comparison depth for a scenario: the scenario's explicit
    /// override, else the corpus default, else
    /// [`ComparisonDepth::TruecolorCanonical`].
    #[must_use]
    pub fn comparison_depth_for(&self, scenario: &ManifestScenario) -> ComparisonDepth {
        scenario
            .comparison_depth
            .or(self.comparison_depth_default)
            .unwrap_or(ComparisonDepth::TruecolorCanonical)
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

/// Verify every `.txt` / `.ansi` blob (and, for `width-edge` scenarios, the
/// `.widths.json` sidecar; plus every `replay/*.json` script) referenced by
/// the manifest: present on disk, byte length and SHA-256 match. Returns one
/// check per blob.
pub fn verify_goldens(dir: &Path) -> Result<Vec<GoldenCheck>> {
    let manifest = load_manifest(dir)?;
    let mut checks = Vec::new();
    for scenario in &manifest.scenarios {
        for variant in &scenario.variants {
            for (kind, blob) in [("txt", &variant.txt), ("ansi", &variant.ansi)] {
                let path = dir.join(format!("{}.w{}.{}", scenario.id, variant.width, kind));
                checks.push(verify_blob(dir, &path, &scenario.id, variant.width, kind, blob));
            }
        }
        if let Some(sidecar) = &scenario.widths {
            let path = dir.join(&sidecar.file);
            let mut check = verify_blob(
                dir,
                &path,
                &scenario.id,
                0,
                "widths",
                &ManifestBlob { bytes: sidecar.bytes, sha256: sidecar.sha256.clone() },
            );
            check.detail = if check.detail.is_empty() {
                sidecar.file.clone()
            } else {
                format!("{}: {}", sidecar.file, check.detail)
            };
            checks.push(check);
        }
    }
    for replay in &manifest.replay {
        let path = dir.join(&replay.file);
        let mut check = verify_blob(
            dir,
            &path,
            &replay.id,
            0,
            "replay",
            &ManifestBlob { bytes: replay.bytes, sha256: replay.sha256.clone() },
        );
        check.detail = if check.detail.is_empty() {
            replay.file.clone()
        } else {
            format!("{}: {}", replay.file, check.detail)
        };
        checks.push(check);
    }
    Ok(checks)
}

fn verify_blob(
    _dir: &Path,
    path: &Path,
    scenario: &str,
    width: usize,
    kind: &'static str,
    blob: &ManifestBlob,
) -> GoldenCheck {
    let ok;
    let detail;
    match std::fs::read(path) {
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
    GoldenCheck { scenario: scenario.to_string(), width, kind, ok, detail }
}

/// Load and parse one scenario's measured-width sidecar (`<id>.widths.json`).
pub fn load_widths_sidecar(dir: &Path, id: &str) -> Result<WidthsSidecar> {
    let bytes = std::fs::read(dir.join(format!("{id}.widths.json")))
        .with_context(|| format!("reading widths sidecar for {id} in {}", dir.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing widths sidecar for {id}"))
}

/// One step of a keypress-replay script: the key tokens to replay plus the
/// state the reference asserts afterwards.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayStep {
    pub keys: Vec<String>,
    pub expect: ReplayExpect,
}

/// The asserted post-state of a replay step. Frame checks use the stable
/// labels the reference tests match on; `exit` means the `app.exit` binding
/// ran (renderer torn down).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReplayExpect {
    #[serde(default)]
    pub frame_includes: Option<Vec<String>>,
    #[serde(default)]
    pub frame_excludes: Option<Vec<String>>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub route: Option<ReplayRoute>,
    #[serde(default)]
    pub dialogs: Option<usize>,
    #[serde(default)]
    pub exit: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayRoute {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, rename = "sessionID")]
    pub session_id: Option<String>,
}

/// The parsed content of a `replay/<id>.json` script.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayScript {
    pub id: String,
    pub description: String,
    pub source: String,
    /// Key-event token encoding ("kitty").
    pub keys: String,
    pub steps: Vec<ReplayStep>,
}

/// Load and parse one keypress-replay script by id.
pub fn load_replay_script(dir: &Path, id: &str) -> Result<ReplayScript> {
    let bytes = std::fs::read(dir.join(format!("replay/{id}.json")))
        .with_context(|| format!("reading replay script {id} in {}", dir.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing replay script {id}"))
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
/// asserts the parsed grid has the manifest's row count. Only rows are
/// asserted: `cols` is the renderer's **display** width, while the grid is
/// compared per `char` — wide glyphs occupy multiple columns but one char,
/// and combining marks add chars without columns, so the per-row char count
/// of a width-edge frame legitimately differs from `cols` in both
/// directions (see the `width-edge-*` scenarios and their `.widths.json`
/// sidecars).
#[must_use]
pub fn parse_text_grid(text: &str, dims: Option<(usize, usize)>) -> TextGrid {
    let mut rows: Vec<String> = text.split('\n').map(str::to_string).collect();
    if rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    let height = rows.len();
    let width = rows.iter().map(|row| row.chars().count()).max().unwrap_or(0);
    let grid = TextGrid { rows, height, width };
    if let Some((expected_rows, _expected_cols)) = dims {
        assert_eq!(grid.height, expected_rows, "text grid row count does not match manifest");
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

/// A parsed golden ANSI stream: one row of styled cells per screen row.
/// Rows are **not** padded to a uniform width (wide glyphs and combining
/// marks make per-row char counts vary); out-of-range cells read as
/// default-styled blanks through [`AnsiFrame::cell`].
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
///
/// Rows keep their natural length — they are **not** padded to a uniform
/// width. The exporter emits each line exactly as captured, and with wide
/// glyphs / combining marks the per-row **char** count of a full
/// display-width row varies, so padding to the longest row would invent
/// cells that were never in the stream (and break the byte roundtrip).
/// Out-of-range cells read as default-styled blanks through [`AnsiFrame::cell`].
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

// ---------------------------------------------------------------------------
// Depth-aware comparison (oracle-gate8 finding 5)
// ---------------------------------------------------------------------------

/// Compare two styled frames at an explicit [`ComparisonDepth`].
///
/// `TruecolorCanonical` compares the frames as exported (headless,
/// TERM-free truecolor). `Degraded256` first degrades **both** sides to the
/// xterm-256 palette ([`degrade_frame`]) so exact hue drift is tolerated
/// while color-class drift is not. Important: the degradation is *our*
/// nearest-palette approximation of what the reference would render under
/// `TERM=xterm-256color` — the goldens never passed through such a
/// terminal — so a Degraded256 match asserts perceived-color agreement,
/// not byte parity with reference 256-color output. Glyphs and attributes
/// are compared identically at both depths.
#[must_use]
pub fn compare_ansi_frames_at(
    expected: &AnsiFrame,
    actual: &AnsiFrame,
    depth: ComparisonDepth,
) -> FrameFidelity {
    if depth == ComparisonDepth::TruecolorCanonical {
        return compare_ansi_frames(expected, actual);
    }
    let expected_degraded = degrade_frame(expected, depth.color_depth());
    let actual_degraded = degrade_frame(actual, depth.color_depth());
    compare_ansi_frames(&expected_degraded, &actual_degraded)
}

/// Compare two canonical ANSI byte streams at an explicit
/// [`ComparisonDepth`]: both sides are parsed, degraded (unless
/// truecolor-canonical), re-rendered, and then byte-compared — so the
/// reported first-divergent offset refers to the degraded streams.
///
/// The same caveat as [`compare_ansi_frames_at`] applies: the degradation
/// approximates the reference's native TERM behavior; it does not capture
/// it.
pub fn compare_ansi_bytes_at(
    expected: &str,
    actual: &str,
    depth: ComparisonDepth,
) -> Result<AnsiByteDiff, AnsiParseError> {
    if depth == ComparisonDepth::TruecolorCanonical {
        return Ok(compare_ansi_bytes(expected, actual));
    }
    let expected_frame = parse_ansi(expected, None)?;
    let actual_frame = parse_ansi(actual, None)?;
    let expected_degraded = render_ansi(&degrade_frame(&expected_frame, depth.color_depth()));
    let actual_degraded = render_ansi(&degrade_frame(&actual_frame, depth.color_depth()));
    Ok(compare_ansi_bytes(&expected_degraded, &actual_degraded))
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
        // 37 scenarios × 2 widths × 2 blobs + 3 width sidecars + 3 replay
        // scripts (second export wave included).
        assert!(
            checks.len() >= 40,
            "expected at least 15 scenarios × 2 widths × 2 blobs, got {}",
            checks.len()
        );
        let failures: Vec<&GoldenCheck> = checks.iter().filter(|check| !check.ok).collect();
        assert!(failures.is_empty(), "golden verification failures: {failures:?}");
    }

    /// Second export wave (gate-8 finding 2 / gate-9 additions): the corpus
    /// carries the composer states, message groups, markdown/spinner/toast
    /// C1 frames, sidebar pair, the four pinned virtual-scroll offsets and
    /// the 40-column session screen — every scenario class annotated, the
    /// `vscroll-*` family pinning one scroll offset each, and the replay
    /// scripts parsing with their asserted expectations intact.
    #[test]
    fn second_export_wave_classes_and_replay_scripts_are_pinned() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames");
        let manifest = load_manifest(&dir).expect("manifest");
        let classes: Vec<(String, String)> =
            manifest.scenarios.iter().map(|s| (s.component_class.clone(), s.id.clone())).collect();
        for expected in [
            "composer",
            "message-group",
            "markdown",
            "spinner",
            "toast",
            "sidebar",
            "vscroll",
            "session-screen",
        ] {
            assert!(
                classes.iter().any(|(class, _)| class == expected),
                "scenario class {expected} missing from the corpus"
            );
        }
        // Composer states: all five, both widths.
        let composer: Vec<_> =
            manifest.scenarios.iter().filter(|s| s.component_class == "composer").collect();
        assert_eq!(composer.len(), 5, "five composer states");
        for scenario in &composer {
            assert_eq!(scenario.variants.len(), 2, "{} pinned at 80 + 40", scenario.id);
        }
        // Virtual scroll: four scenarios, one offset each, distinct offsets.
        let vscroll: Vec<_> =
            manifest.scenarios.iter().filter(|s| s.component_class == "vscroll").collect();
        assert_eq!(vscroll.len(), 4, "four pinned scroll offsets");
        let mut offsets: Vec<&str> =
            vscroll.iter().filter_map(|s| s.scroll_offset.as_deref()).collect();
        offsets.sort_unstable();
        assert_eq!(offsets, ["bottom", "middle", "stick-to-bottom", "top"]);
        // C1 wave members (markdown / spinner / toast) stay tier-c1.
        for id in ["markdown-frame", "spinner-frozen", "toast-over-session"] {
            let scenario =
                manifest.scenarios.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(scenario.class, Some(ScenarioClass::TierC1), "{id} is tier-c1");
        }
        // Sidebar pair + 40-column session screen present.
        assert!(manifest.scenarios.iter().any(|s| s.id == "sidebar-expanded"));
        assert!(manifest.scenarios.iter().any(|s| s.id == "sidebar-collapsed"));
        let narrow = manifest
            .scenarios
            .iter()
            .find(|s| s.id == "session-screen-40")
            .expect("session-screen-40");
        assert_eq!(narrow.variants.len(), 1);
        assert_eq!(narrow.variants[0].width, 40);

        // Replay scripts: pinned, parsed, and self-consistent (the
        // wave-1 trio + the wave-3 recorded pair).
        assert_eq!(manifest.replay.len(), 5, "five replay flows");
        let recorded = [
            "exit-leader-q",
            "palette-ctrl-p",
            "permission-once-always-deny",
            "session-list-leader-l",
            "theme-dialog-select",
        ];
        for id in recorded {
            assert!(manifest.replay.iter().any(|entry| entry.id == id), "{id} pinned");
        }
        for entry in &manifest.replay {
            let script = load_replay_script(&dir, &entry.id).expect("replay script parses");
            assert_eq!(script.id, entry.id);
            assert_eq!(script.keys, "kitty");
            assert_eq!(script.steps.len(), entry.steps, "{} step count matches manifest", entry.id);
            assert!(script.steps.len() >= 3, "{} has steps", entry.id);
            for (index, step) in script.steps.iter().enumerate() {
                if let Some(exit) = step.expect.exit {
                    // Only the terminal step of a flow may assert a positive exit.
                    assert_eq!(
                        exit,
                        index + 1 == script.steps.len(),
                        "{} step {index} exit flag must only be true on the last step",
                        entry.id
                    );
                }
                if let Some(route) = &step.expect.route {
                    assert!(matches!(route.kind.as_str(), "home" | "session"));
                }
            }
            // Every step carries at least one asserted expectation.
            assert!(script.steps.iter().all(|step| {
                step.expect.frame_includes.is_some()
                    || step.expect.frame_excludes.is_some()
                    || step.expect.exit.is_some()
                    || step.expect.dialogs.is_some()
                    || step.expect.focus.is_some()
                    || step.expect.route.is_some()
            }));
        }
        // The exit flow ends with the empty-prompt ctrl+c termination
        // (Wave A decollision: <leader>q is the queue manager's chord,
        // never app.exit's).
        let exit = load_replay_script(&dir, "exit-leader-q").expect("exit script");
        let last = exit.steps.last().expect("last step");
        assert_eq!(last.keys, vec!["ctrl+c"]);
        assert_eq!(last.expect.exit, Some(true));
    }

    /// Gate-8 P1: the corpus manifest pins the v2-dark default theme, every
    /// scenario carries an acceptance class, tiers map to comparison
    /// depths, and the width-edge scenarios ship verified sidecars.
    #[test]
    fn corpus_manifest_pins_theme_classes_and_depths() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames");
        let manifest = load_manifest(&dir).expect("manifest");
        assert!(!manifest.superseded, "active corpus must not be superseded");
        assert_eq!(manifest.format, "tui-frame-golden/2");
        let theme = manifest.theme.as_ref().expect("theme pinned");
        assert_eq!((theme.id.as_str(), theme.mode.as_str()), ("opencode", "dark"));
        assert_eq!(manifest.comparison_depth_default, Some(ComparisonDepth::TruecolorCanonical));

        let mut width_edge = 0;
        for scenario in &manifest.scenarios {
            let class = scenario.class.expect("scenario class pinned");
            let depth = manifest.comparison_depth_for(scenario);
            match class {
                ScenarioClass::TierA => assert_eq!(depth, ComparisonDepth::TruecolorCanonical),
                ScenarioClass::TierB => assert_eq!(depth, ComparisonDepth::Degraded256),
                ScenarioClass::TierC1 | ScenarioClass::WidthEdge => {
                    assert_eq!(depth, ComparisonDepth::TruecolorCanonical);
                }
            }
            if class == ScenarioClass::WidthEdge {
                width_edge += 1;
                let sidecar = scenario.widths.as_ref().expect("width-edge has sidecar");
                assert!(sidecar.samples >= 3, "width-edge needs samples");
                let sidecar_file = dir.join(&sidecar.file);
                let bytes = std::fs::read(&sidecar_file).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(bytes.len() as u64, sidecar.bytes);
                assert_eq!(sha256_hex(&bytes), sidecar.sha256);
            } else {
                assert!(scenario.widths.is_none(), "only width-edge has sidecars");
            }
        }
        assert!(width_edge >= 3, "need >= 3 width-edge scenarios, got {width_edge}");
        // Tier-B is exactly the diff viewer until oc-theme token parity lands.
        let diff = manifest
            .scenarios
            .iter()
            .find(|s| s.id == "diff-viewer-split")
            .expect("diff viewer scenario");
        assert_eq!(diff.class, Some(ScenarioClass::TierB));
    }

    /// The pre-P1 corpus stays loadable and on-disk-verified, marked
    /// superseded — diffing reference only.
    #[test]
    fn legacy_mixed_theme_corpus_is_superseded_but_verified() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames/legacy-mixed-theme");
        let manifest = load_manifest(&dir).expect("legacy manifest parses under the v2 struct");
        assert!(manifest.superseded, "legacy corpus must be marked superseded");
        assert!(manifest.superseded_by.is_some());
        assert_eq!(manifest.format, "tui-frame-golden/1");
        assert!(manifest.theme.is_none(), "legacy corpus predates the theme pin");
        let checks = verify_goldens(&dir).expect("verify legacy goldens");
        assert!(
            checks.iter().all(|check| check.ok),
            "legacy blob failures: {:?}",
            checks.iter().filter(|check| !check.ok).collect::<Vec<_>>()
        );
    }

    /// The width-edge sidecars are the ground truth for the Rust
    /// `display_width`: samples parse, agree with their utf16/codepoint
    /// bookkeeping, and pure-ASCII samples measure one cell per char.
    #[test]
    fn width_edge_sidecars_pin_measured_widths() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens/tui-frames");
        let manifest = load_manifest(&dir).expect("manifest");
        let width_edge: Vec<_> = manifest
            .scenarios
            .iter()
            .filter(|s| s.class == Some(ScenarioClass::WidthEdge))
            .collect();
        assert!(width_edge.len() >= 3);
        for scenario in width_edge {
            let sidecar = load_widths_sidecar(&dir, &scenario.id).expect("sidecar");
            assert_eq!(sidecar.id, scenario.id);
            assert_eq!(sidecar.width_method, "unicode");
            assert!(
                sidecar.measured_by.contains("string-width@7.2.0"),
                "measured by the reference's bundled string-width"
            );
            assert!(sidecar.samples.len() >= 6, "each probe has a full sample set");
            for sample in &sidecar.samples {
                assert_ne!(sample.text, "");
                assert_eq!(sample.utf16, sample.text.chars().map(char::len_utf16).sum::<usize>());
                assert_eq!(sample.codepoints, sample.text.chars().count());
                assert!(sample.codepoints <= sample.utf16);
                assert!(sample.width >= 1, "non-empty samples occupy at least one cell");
                if sample.text.is_ascii() {
                    assert_eq!(sample.width, sample.text.chars().count());
                }
            }
        }
    }

    /// Depth-aware comparison: exact-color drift fails truecolor-canonical
    /// but passes after both sides degrade to the 256-color palette, while
    /// color-class drift still fails. The degradation is our approximation
    /// of the reference's TERM behavior (documented on `ComparisonDepth`).
    #[test]
    fn depth_aware_comparison_tolerates_hue_drift_only() {
        let golden = AnsiFrame {
            rows: vec![vec![StyledCell {
                ch: 'a',
                style: CellStyle { fg: Color::Rgb(10, 10, 250), ..Default::default() },
            }]],
            height: 1,
            width: 1,
        };
        // Same glyph, hue-drifted fg: (10,10,250) vs (12,12,248) both
        // degrade to xterm-256 index 12.
        let hue_drift = AnsiFrame {
            rows: vec![vec![StyledCell {
                ch: 'a',
                style: CellStyle { fg: Color::Rgb(12, 12, 248), ..Default::default() },
            }]],
            height: 1,
            width: 1,
        };
        // A different color class entirely.
        let class_drift = AnsiFrame {
            rows: vec![vec![StyledCell {
                ch: 'a',
                style: CellStyle { fg: Color::Rgb(250, 250, 10), ..Default::default() },
            }]],
            height: 1,
            width: 1,
        };
        let strict =
            compare_ansi_frames_at(&golden, &hue_drift, ComparisonDepth::TruecolorCanonical);
        assert!(!strict.all_match(), "exact hue drift fails truecolor-canonical");
        let lenient = compare_ansi_frames_at(&golden, &hue_drift, ComparisonDepth::Degraded256);
        assert!(lenient.all_match(), "hue drift passes after 256 degradation");
        let still_fails =
            compare_ansi_frames_at(&golden, &class_drift, ComparisonDepth::Degraded256);
        assert!(!still_fails.all_match(), "color-class drift fails even degraded");

        // Byte-level variant: degraded streams are byte-equal for hue drift.
        let golden_text = render_ansi(&golden);
        let hue_text = render_ansi(&hue_drift);
        assert!(
            !compare_ansi_bytes_at(&golden_text, &hue_text, ComparisonDepth::TruecolorCanonical)
                .expect("parse")
                .equal
        );
        assert!(
            compare_ansi_bytes_at(&golden_text, &hue_text, ComparisonDepth::Degraded256)
                .expect("parse")
                .equal
        );
        // Degraded comparison maps to the Ansi256 TERM depth.
        assert_eq!(ComparisonDepth::Degraded256.color_depth(), ColorDepth::Ansi256);
        assert_eq!(ComparisonDepth::TruecolorCanonical.color_depth(), ColorDepth::TrueColor);
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
                // (Char-wise agreement: wide/combining graphemes are stored
                // inline identically in both artifacts, so this holds for
                // the width-edge corpus too.)
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
        assert!(checked >= 42, "expected at least 42 variants, checked {checked}");
    }
}
