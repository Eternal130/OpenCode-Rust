//! The diff viewer (`feature-plugins/system/diff-viewer.tsx` +
//! `diff-viewer-file-tree.tsx`, the plugin-shipped VCS route) — 7.2
//! wave 2, **tier B** (`diff-viewer-split` @80/160 at the manifest's
//! `degraded-256` depth).
//!
//! ## Layout (`diff-viewer.tsx` constants, golden-pinned)
//!
//! * The file tree shows at `width ≥ 90` (and `files > 0`), width
//!   `clamp(floor(width/4), 30, 40)`.
//! * `patchPaneWidth = width − tree − 4` (the patch column's 2-col
//!   side padding each way).
//! * Split view needs `patchPaneWidth ≥ 100`; below that (and at 80
//!   cols: no tree, 76-wide pane) the unified list renders.
//! * Split: old pane `[x, x+w/2)`, new pane the rest, both gutters
//!   identical to the unified column.
//!
//! ## Gutter (golden bytes)
//!
//! One row is a space, the line number (left-aligned, natural width —
//! the field sized to the file's widest number), the sign
//! (` -`/` +` right after the number) and the text at
//! `pane + 1 + digits + 3`. Removed/added gutters paint
//! `diff.lineNumber.background.*`, their text `diff.background.*`;
//! the signs paint `diff.highlight.*`. A hunk header hides while its
//! hunk starts at line 1 (the golden's first `@@` never shows).
//!
//! ## Keys
//!
//! The `diff` layer of the v2 default table ([`diff_command_for_key`]):
//! `n/p` file jumps, `b` tree, `s` single patch, `d` source, `v`
//! split/unified, `enter/space` toggle, `tab` focus, `]/[` hunks,
//! `?` help, `escape/q` close.
//!
//! ## Syntax (R-01b)
//!
//! The text column highlights through oc-syntax (the file's extension
//! → filetype, one parse per file via the engine's content-keyed
//! cache; rows map to highlighted lines). The gutter and the diff row
//! backgrounds paint exactly as before — highlight spans recolor the
//! foreground only, their background pinned to the row's diff bg.
//! Unknown extensions render the original plain rows.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::too_many_lines)]

use crate::layout::{Placed, PlacedLine, StyledSpan};
use crate::render::{paint_placed_with_fills, Fill, Frame};
use crate::theme::{DiffStyles, ThemeStyles};
use oc_syntax::HighlightedBlock;
use ratatui::style::{Color, Modifier, Style};

/// `MIN_SPLIT_WIDTH` — the patch pane needs this many columns before
/// the split view is offered.
pub const MIN_SPLIT_WIDTH: usize = 100;
/// `FILE_TREE_MIN_WIDTH`.
pub const FILE_TREE_MIN_WIDTH: usize = 30;
/// `FILE_TREE_MAX_WIDTH`.
pub const FILE_TREE_MAX_WIDTH: usize = 40;
/// The tree renders only at/above this terminal width.
pub const FILE_TREE_BREAKPOINT: usize = 90;

/// A file's diff status (`vcs.diff` entries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
}

impl FileStatus {
    /// The tree row's status letter (`fileTreeRowStatus`).
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
        }
    }
}

/// One file of the diff (`vcs.diff` response entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub file: String,
    pub additions: u64,
    pub deletions: u64,
    pub status: FileStatus,
    /// The raw unified patch (`--- a/…`, `+++ b/…`, hunks).
    pub patch: String,
    /// `isDiffImageFile` (diff-viewer-image.tsx:9): the header shows
    /// `Image` instead of `+N/−N` and the body carries the
    /// no-preview fallback copy.
    pub image: bool,
    /// TUI-10: the working-tree bytes for a previewable image
    /// (`client.file.read`'s port-side stand-in — the fetch worker
    /// reads `directory/file` for non-deleted images in non-committed
    /// modes; `None` keeps the fallback copy).
    pub image_data: Option<Vec<u8>>,
}

/// One parsed hunk row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchRow {
    Context { old: u64, new: u64, text: String },
    Deleted { old: u64, text: String },
    Added { new: u64, text: String },
}

/// One `@@ -a,b +c,d @@` hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ … @@` body (` -a,b +c,d`).
    pub header: String,
    pub old_start: u64,
    pub new_start: u64,
    pub rows: Vec<PatchRow>,
}

/// Parse a unified patch into hunks (row numbers pre-assigned).
///
/// G-11 (framework convergence): patch PARSING is delegated to
/// opentui-core's jsdiff-9.0.0-faithful
/// [`opentui_core::renderables::diff::parse_patch`] — the single
/// source of truth for unified-diff parsing. This thin adapter maps
/// its [`StructuredPatch`](opentui_core::renderables::diff::StructuredPatch)
/// onto the viewer's pre-numbered row model:
///
/// * context lines drop the unified marker space (rendered text);
/// * `\ No newline at end of file` markers are dropped — jsdiff keeps
///   them in `hunk.lines` but upstream Diff.ts renders none of them;
/// * a jsdiff `Err` (count mismatch, invalid body line, …) yields no
///   hunks → the "No patch available for this file." fallback, the
///   same disposition as upstream's error view. The legacy scanner
///   tolerated malformed patches; upstream rejects them, and the
///   vcs.diff endpoint only emits valid unified diffs.
#[must_use]
pub fn parse_patch(patch: &str) -> Vec<Hunk> {
    let Ok(patches) = opentui_core::renderables::diff::parse_patch(patch) else {
        return Vec::new();
    };
    let total: usize = patches.iter().map(|structured| structured.hunks.len()).sum();
    // Display header text + starts re-derived from the source: jsdiff's
    // model keeps only the numbers, and it bumps a `,0` count's start by
    // one (jsdiff's "chunk size 0" quirk, parse.js:458-463) — the
    // viewer's header-hiding heuristic (`old_start != 1`) and golden
    // bytes read the RAW start, so recover it from the header text when
    // the scan lines up 1:1 with the parsed hunks (the ordinary case).
    let headers = hunk_header_texts(patch);
    let mut header_index = 0usize;
    let aligned = headers.len() == total;
    let mut out: Vec<Hunk> = Vec::new();
    for structured in &patches {
        for hunk in &structured.hunks {
            let parsed = aligned.then(|| headers.get(header_index)).flatten().cloned();
            header_index += 1;
            let (header, old_start, new_start) = parsed.unwrap_or_else(|| {
                let old = num(hunk.old_start);
                let new = num(hunk.new_start);
                (
                    format!(
                        "@@ -{},{} +{},{} @@",
                        num(hunk.old_start),
                        count(hunk.old_lines),
                        num(hunk.new_start),
                        count(hunk.new_lines),
                    ),
                    old,
                    new,
                )
            });
            let mut old_line = old_start;
            let mut new_line = new_start;
            let mut rows: Vec<PatchRow> = Vec::new();
            for line in &hunk.lines {
                if let Some(text) = line.strip_prefix('-') {
                    rows.push(PatchRow::Deleted { old: old_line, text: text.to_owned() });
                    old_line = old_line.saturating_add(1);
                } else if let Some(text) = line.strip_prefix('+') {
                    rows.push(PatchRow::Added { new: new_line, text: text.to_owned() });
                    new_line = new_line.saturating_add(1);
                } else if line.starts_with('\\') {
                    // jsdiff keeps the no-newline marker; Diff.ts renders
                    // none of it (upstream disposition — metadata, not a row).
                } else {
                    // Context lines carry the leading marker space in the
                    // unified patch — the rendered text drops it. jsdiff
                    // reads an empty non-last body line as ' ' context.
                    let text = line.strip_prefix(' ').unwrap_or(line).to_owned();
                    rows.push(PatchRow::Context { old: old_line, new: new_line, text });
                    old_line = old_line.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                }
            }
            out.push(Hunk { header, old_start, new_start, rows });
        }
    }
    out
}

/// `f64` (JS number) → `u64`: NaN/negative → 0, saturating otherwise.
#[allow(clippy::cast_sign_loss)] // NaN-guarded JS-number model values
fn num(value: f64) -> u64 {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value as u64
    }
}

/// A hunk count for the fallback header rebuild (NaN → 0).
fn count(value: f64) -> u64 {
    if value.is_nan() {
        0
    } else {
        num(value)
    }
}

/// The legacy `@@ … @@` display strings plus their RAW starts, scanned
/// from the source (`@@ -a[,b] +c[,d] @@`). Display text and the
/// un-bumped starts only — the structural parsing is opentui-core's.
fn hunk_header_texts(patch: &str) -> Vec<(String, u64, u64)> {
    patch
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("@@")?;
            // ` -a[,b] +c[,d] @@ …`
            let raw = rest.split("@@").next().unwrap_or_default();
            let body = raw.trim();
            let mut numbers = body.split_whitespace();
            let old = numbers.next().unwrap_or("-0").trim_start_matches('-');
            let new = numbers.next().unwrap_or("+0").trim_start_matches('+');
            let old_start: u64 = old.split(',').next().unwrap_or("0").parse().unwrap_or(0);
            let new_start: u64 = new.split(',').next().unwrap_or("0").parse().unwrap_or(0);
            Some((format!("@@ {body} @@"), old_start, new_start))
        })
        .collect()
}

impl DiffFile {
    /// The file's parsed hunks.
    #[must_use]
    pub fn hunks(&self) -> Vec<Hunk> {
        parse_patch(&self.patch)
    }

    /// The gutter's number field width: the widest line number in the
    /// file (`patch-diff.tsx` sizes the gutter for the whole file).
    #[must_use]
    pub fn number_digits(&self) -> usize {
        let mut max = 0u64;
        for hunk in self.hunks() {
            for row in &hunk.rows {
                let value = match row {
                    PatchRow::Context { old, new, .. } => (*old).max(*new),
                    PatchRow::Deleted { old, .. } => *old,
                    PatchRow::Added { new, .. } => *new,
                };
                max = max.max(value);
            }
        }
        max.to_string().len()
    }
}

/// One file-tree row (`buildFileTree`): a directory (expanded `▾`) or
/// a file (`≡` with its status letter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub name: String,
    pub file_index: Option<usize>,
}

/// Build the tree rows: every directory on the file paths (depth =
/// segment position, sorted), then the file rows.
#[must_use]
pub fn build_file_tree(files: &[DiffFile]) -> Vec<TreeRow> {
    let mut paths: Vec<String> = Vec::new();
    for file in files {
        let segments: Vec<&str> = file.file.split('/').collect();
        for depth in 0..segments.len().saturating_sub(1) {
            let prefix = segments[..=depth].join("/");
            if !paths.contains(&prefix) {
                paths.push(prefix);
            }
        }
    }
    paths.sort();
    let mut rows: Vec<TreeRow> = paths
        .iter()
        .map(|path| TreeRow {
            depth: path.split('/').count() - 1,
            name: path.rsplit('/').next().unwrap_or_default().to_owned(),
            file_index: None,
        })
        .collect();
    for (index, file) in files.iter().enumerate() {
        rows.push(TreeRow {
            depth: file.file.split('/').count().saturating_sub(1),
            name: file.file.rsplit('/').next().unwrap_or_default().to_owned(),
            file_index: Some(index),
        });
    }
    rows
}

/// The effective layout at one terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLayout {
    pub show_tree: bool,
    pub tree_width: usize,
    pub patch_pane_width: usize,
    pub split_available: bool,
}

/// `diff-viewer.tsx`'s layout memos as one function.
#[must_use]
pub fn diff_layout(width: usize, tree_enabled: bool, file_count: usize) -> DiffLayout {
    let show_tree = width >= FILE_TREE_BREAKPOINT && tree_enabled && file_count > 0;
    let tree_width =
        if show_tree { (width / 4).clamp(FILE_TREE_MIN_WIDTH, FILE_TREE_MAX_WIDTH) } else { 0 };
    let patch_pane_width = width.saturating_sub(if show_tree { tree_width + 4 } else { 4 }).max(1);
    let split_available = patch_pane_width >= MIN_SPLIT_WIDTH;
    DiffLayout { show_tree, tree_width, patch_pane_width, split_available }
}

/// `DiffView = "split" | "unified"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffView {
    Split,
    Unified,
}

/// The viewer's interactive state (the key layer's target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffViewerState {
    pub tree_enabled: bool,
    pub view_override: Option<DiffView>,
    pub single_patch: bool,
    pub selected_file: Option<usize>,
    /// Reviewed file names (`diff.mark_reviewed`).
    pub reviewed: Vec<String>,
}

impl Default for DiffViewerState {
    fn default() -> Self {
        Self {
            tree_enabled: true,
            view_override: None,
            single_patch: false,
            selected_file: None,
            reviewed: Vec::new(),
        }
    }
}

impl DiffViewerState {
    /// The active view: unified when split is unavailable; else the
    /// override or split (`view()` memo).
    #[must_use]
    pub fn view(&self, layout: &DiffLayout) -> DiffView {
        if !layout.split_available {
            return DiffView::Unified;
        }
        self.view_override.unwrap_or(DiffView::Split)
    }

    /// Run one `diff.*` command (the key layer's dispatch target).
    /// Returns whether the state changed (a redraw would follow).
    pub fn run_command(&mut self, command: &str, layout: &DiffLayout, files: &[DiffFile]) -> bool {
        match command {
            "diff.toggle_file_tree" => {
                self.tree_enabled = !self.tree_enabled;
                true
            }
            "diff.toggle_view" => {
                if !layout.split_available {
                    return false;
                }
                self.view_override = Some(match self.view(layout) {
                    DiffView::Split => DiffView::Unified,
                    DiffView::Unified => DiffView::Split,
                });
                true
            }
            "diff.single_patch" => {
                if !self.single_patch && self.selected_file.is_none() {
                    self.selected_file = Some(0);
                }
                self.single_patch = !self.single_patch;
                true
            }
            "diff.next_file" => self.jump_relative_file(1, files.len()),
            "diff.previous_file" => self.jump_relative_file(-1, files.len()),
            "diff.mark_reviewed" => {
                let Some(index) = self.selected_file else { return false };
                let Some(file) = files.get(index) else { return false };
                if self.reviewed.contains(&file.file) {
                    self.reviewed.retain(|name| name != &file.file);
                } else {
                    self.reviewed.push(file.file.clone());
                }
                true
            }
            _ => false,
        }
    }

    fn jump_relative_file(&mut self, offset: i32, file_count: usize) -> bool {
        if file_count == 0 {
            return false;
        }
        let current = self.selected_file.unwrap_or(0);
        let next = if offset >= 0 {
            current.saturating_add(offset.unsigned_abs() as usize).min(file_count - 1)
        } else {
            current.saturating_sub(offset.unsigned_abs() as usize)
        };
        if self.selected_file == Some(next) {
            return false;
        }
        self.selected_file = Some(next);
        true
    }
}

/// The `diff` layer of the v2 default key table: one key's canonical
/// command (`n` → `diff.next_file`, …, `escape`/`q` → `diff.close`).
#[must_use]
pub fn diff_command_for_key(key: &str) -> Option<&'static str> {
    for def in crate::keymap::default_table() {
        if def.layer != "diff" {
            continue;
        }
        if def.defaults.split(',').map(str::trim).any(|spec| spec == key && spec != "none") {
            return Some(def.command);
        }
    }
    None
}

/// `tint(color, background, t)` — `color` pulled toward `background`
/// by factor `t` (the file tree's rail/marker colors:
/// `tint(muted, raised, 0.45)` / `0.7`).
fn tint(color: Color, background: Color, t: f32) -> Color {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mix =
        |a: u8, b: u8| -> u8 { (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8 };
    match (color, background) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bg_, bb)) => {
            Color::Rgb(mix(r, br), mix(g, bg_), mix(b, bb))
        }
        (other, _) => other,
    }
}

/// The toolbar / tree inputs.
#[derive(Debug, Clone)]
pub struct DiffViewerOptions<'a> {
    /// `DIFF_SOURCES[mode].label` (bold, `text.action.secondary`).
    pub source_label: &'a str,
    /// `props.sourceDetail` — the base ref shown after ` · `
    /// (`vs {name}` of the chosen base).
    pub source_detail: &'a str,
    /// The VIEWPORT height (the inline image's `Math.max(3, Math.min(
    /// 8, floor(height / 4)))`, diff-viewer-image.tsx:21).
    pub viewport_height: usize,
}

impl Default for DiffViewerOptions<'_> {
    fn default() -> Self {
        Self { source_label: "All", source_detail: "vs v2", viewport_height: 24 }
    }
}

/// The golden fixture (`tui-dump/scenarios/diff_viewer.tsx`): one
/// modified file, three 3-line hunks (+3/−3).
#[must_use]
pub fn diff_viewer_fixture() -> Vec<DiffFile> {
    vec![DiffFile {
        file: "src/file.txt".into(),
        additions: 3,
        deletions: 3,
        status: FileStatus::Modified,
        patch: "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1,3 +1,3 @@\n const first = true\n-const oldFirst = true\n+const newFirst = true\n const afterFirst = true\n@@ -20,3 +20,3 @@\n const second = true\n-const oldSecond = true\n+const newSecond = true\n const afterSecond = true\n@@ -40,3 +40,3 @@\n const third = true\n-const oldThird = true\n+const newThird = true\n const afterThird = true".into(),
        image: false,
        image_data: None,
    }]
}

// -- rendering ----------------------------------------------------------------

/// One hunk row's paint colors (gutter background, sign, text
/// background, number, text) plus the row's line index into the file's
/// highlighted source (R-01b: the text column highlights through
/// oc-syntax by file extension).
#[derive(Debug, Clone)]
struct RowColors {
    gutter_bg: Color,
    sign_fg: Option<Color>,
    sign: &'static str,
    text_bg: Color,
    number: u64,
    text: String,
    /// This row's line index into the file-wide joined row text (the
    /// `oc_syntax::HighlightedBlock` line it renders as).
    line: usize,
}

/// One hunk row pair for the split view (either side may be absent —
/// unpaired deletions/additions render with a blank counterpart).
struct SplitPair {
    left: Option<RowColors>,
    right: Option<RowColors>,
}

/// Emit the buffered del/add run as aligned row pairs (either side
/// may run out first — the remainder renders with a blank counterpart).
fn flush_split_pairs(
    pairs: &mut Vec<SplitPair>,
    dels: &mut Vec<RowColors>,
    adds: &mut Vec<RowColors>,
) {
    for index in 0..dels.len().max(adds.len()) {
        pairs.push(SplitPair { left: dels.get(index).cloned(), right: adds.get(index).cloned() });
    }
    dels.clear();
    adds.clear();
}

/// A hunk row's text (the payload shared by every row shape).
fn row_text(row: &PatchRow) -> &str {
    match row {
        PatchRow::Context { text, .. }
        | PatchRow::Deleted { text, .. }
        | PatchRow::Added { text, .. } => text,
    }
}

/// Pair hunk rows for the split panes: contexts go to both sides,
/// deletion runs zip with the following addition run. `base_line` is
/// the hunk's first row's line index into the file's highlighted
/// source; each source row consumes one line (a context pair paints
/// the same line on both sides).
fn split_pairs(rows: &[PatchRow], diff: &DiffStyles, base_line: usize) -> Vec<SplitPair> {
    fn row_colors(row: &PatchRow, diff: &DiffStyles, deleted: bool, line: usize) -> RowColors {
        match row {
            PatchRow::Context { old, new, text } => RowColors {
                gutter_bg: diff.background_context,
                sign_fg: None,
                sign: "   ",
                text_bg: diff.background_context,
                number: if deleted { *old } else { *new },
                text: text.clone(),
                line,
            },
            PatchRow::Deleted { old, text } => RowColors {
                gutter_bg: diff.line_number_removed,
                sign_fg: Some(diff.highlight_removed),
                sign: " -",
                text_bg: diff.background_removed,
                number: *old,
                text: text.clone(),
                line,
            },
            PatchRow::Added { new, text } => RowColors {
                gutter_bg: diff.line_number_added,
                sign_fg: Some(diff.highlight_added),
                sign: " +",
                text_bg: diff.background_added,
                number: *new,
                text: text.clone(),
                line,
            },
        }
    }
    let mut pairs = Vec::new();
    let mut pending_dels: Vec<RowColors> = Vec::new();
    let mut pending_adds: Vec<RowColors> = Vec::new();
    for (offset, row) in rows.iter().enumerate() {
        let line = base_line + offset;
        match row {
            PatchRow::Context { .. } => {
                flush_split_pairs(&mut pairs, &mut pending_dels, &mut pending_adds);
                pairs.push(SplitPair {
                    left: Some(row_colors(row, diff, true, line)),
                    right: Some(row_colors(row, diff, false, line)),
                });
            }
            PatchRow::Deleted { .. } => pending_dels.push(row_colors(row, diff, true, line)),
            PatchRow::Added { .. } => pending_adds.push(row_colors(row, diff, false, line)),
        }
    }
    flush_split_pairs(&mut pairs, &mut pending_dels, &mut pending_adds);
    pairs
}

/// Render the diff viewer fullscreen.
#[must_use]
pub fn render_diff_viewer(
    files: &[DiffFile],
    state: &DiffViewerState,
    options: &DiffViewerOptions,
    theme: &ThemeStyles,
    width: usize,
    height: usize,
) -> Frame {
    render_diff_viewer_with_hits(files, state, options, theme, width, height, None)
}

/// [`render_diff_viewer`] plus the inline images' CONTENT-coordinate
/// hit rects (TUI-10 — a release inside one opens the preview; the
/// mount maps them through the scroll into screen cells).
#[must_use]
#[allow(clippy::too_many_arguments)] // the painter's full geometry
pub fn render_diff_viewer_with_hits(
    files: &[DiffFile],
    state: &DiffViewerState,
    options: &DiffViewerOptions,
    theme: &ThemeStyles,
    width: usize,
    height: usize,
    mut image_hits: Option<&mut Vec<crate::components::diff_viewer_mount::DiffImageHit>>,
) -> Frame {
    let styles = theme;
    let diff = styles.diff;
    let layout = diff_layout(width, state.tree_enabled, files.len());
    let view = state.view(&layout);
    let mut placed: Vec<Placed> = Vec::new();
    let mut fill_rects: Vec<Fill> = Vec::new();
    let base = crate::components::tool_rows::fill_style(styles);
    let fg_base = styles.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
    let fg_muted = styles.text_muted.fg.unwrap_or(fg_base);

    // The source toolbar: row 1 inside the tree pane when it shows,
    // else row 0 over the base background (plus the absolute `?`).
    let toolbar_width = if layout.show_tree { layout.tree_width } else { width };
    let toolbar_row = usize::from(layout.show_tree);
    let toolbar_bg = if layout.show_tree { styles.background_raised } else { styles.background };
    placed.push(Placed {
        x: 2,
        y: toolbar_row,
        width: crate::wrap::display_width(options.source_label),
        lines: vec![PlacedLine::plain(
            options.source_label,
            Style::new().fg(styles.action_secondary).bg(toolbar_bg).add_modifier(Modifier::BOLD),
        )],
        tag: Some("diff-source-switch".into()),
    });
    let detail = format!(" · {}", options.source_detail);
    placed.push(Placed {
        x: 2 + crate::wrap::display_width(options.source_label),
        y: toolbar_row,
        width: crate::wrap::display_width(&detail),
        lines: vec![PlacedLine::plain(detail, Style::new().fg(fg_muted).bg(toolbar_bg))],
        tag: None,
    });
    let reviewed = files.iter().filter(|file| state.reviewed.contains(&file.file)).count();
    let count = format!("{reviewed}/{}", files.len());
    placed.push(Placed {
        x: toolbar_width - 2 - count.chars().count(),
        y: toolbar_row,
        width: count.chars().count(),
        lines: vec![PlacedLine::plain(count, Style::new().fg(fg_muted).bg(toolbar_bg))],
        tag: Some("diff-review-count".into()),
    });
    if !layout.show_tree {
        // The help shortcut's absolute square (top-right).
        placed.push(Placed {
            x: width - 1,
            y: 0,
            width: 1,
            lines: vec![PlacedLine::plain("?", Style::new().fg(fg_base).bg(styles.background))],
            tag: Some("diff-help".into()),
        });
    }

    // The tree pane: raised background, one blank under the toolbar,
    // rows from y=3, help footer pinned to the bottom.
    if layout.show_tree {
        fill_rects.push(Fill {
            x: 0,
            y: 0,
            width: layout.tree_width as u16,
            height: height as u16,
            style: Style::new()
                .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                .bg(styles.background_raised),
        });
        let faint = tint(fg_muted, styles.background_raised, 0.45);
        let rail = tint(fg_muted, styles.background_raised, 0.7);
        for (offset, row) in build_file_tree(files).into_iter().enumerate() {
            let y = 3 + offset;
            let indent = "│ ".repeat(row.depth);
            let marker = if row.file_index.is_some() { "≡ " } else { "▾ " };
            let prefix = format!("{indent}{marker}");
            placed.push(Placed {
                x: 2,
                y,
                width: crate::wrap::display_width(&prefix),
                lines: vec![PlacedLine {
                    runs: (if indent.is_empty() {
                        vec![]
                    } else {
                        vec![StyledSpan {
                            link: None,
                            text: indent,
                            style: Style::new().fg(rail).bg(styles.background_raised),
                        }]
                    })
                    .into_iter()
                    .chain([StyledSpan {
                        link: None,
                        text: marker.to_owned(),
                        style: Style::new().fg(faint).bg(styles.background_raised),
                    }])
                    .collect(),
                }],
                tag: row.file_index.map(|index| format!("diff-file-row-{index}")),
            });
            let name_style = if row.file_index.is_some() {
                Style::new().fg(fg_base).bg(styles.background_raised)
            } else {
                Style::new().fg(fg_muted).bg(styles.background_raised)
            };
            placed.push(Placed {
                x: 2 + 2 * row.depth + 2,
                y,
                width: crate::wrap::display_width(&row.name),
                lines: vec![PlacedLine::plain(row.name.clone(), name_style)],
                tag: None,
            });
            if let Some(index) = row.file_index {
                let status_style = match files[index].status {
                    FileStatus::Added => {
                        Style::new().fg(diff.text_added).bg(styles.background_raised)
                    }
                    FileStatus::Deleted => {
                        Style::new().fg(diff.text_removed).bg(styles.background_raised)
                    }
                    FileStatus::Modified => Style::new().fg(fg_muted).bg(styles.background_raised),
                };
                placed.push(Placed {
                    x: layout.tree_width - 3,
                    y,
                    width: 1,
                    lines: vec![PlacedLine::plain(
                        files[index].status.letter().to_string(),
                        status_style,
                    )],
                    tag: None,
                });
            }
        }
        // The help footer: padding 1, `? help`, padding 1.
        // The footer sits one row above the pane's closing pad
        // (`? help` at height − 3 in the golden).
        let help_y = height.saturating_sub(3);
        placed.push(Placed {
            x: 2,
            y: help_y,
            width: 1,
            lines: vec![PlacedLine::plain(
                "?",
                Style::new().fg(fg_base).bg(styles.background_raised),
            )],
            tag: Some("diff-help".into()),
        });
        placed.push(Placed {
            x: 3,
            y: help_y,
            width: 5,
            lines: vec![PlacedLine::plain(
                " help",
                Style::new().fg(fg_muted).bg(styles.background_raised),
            )],
            tag: None,
        });
    }

    // The patch column: fixed top edge, then the file cards.
    let patch_x = if layout.show_tree { layout.tree_width + 2 } else { 2 };
    let patch_w = layout.patch_pane_width;
    let edge_y = usize::from(!layout.show_tree);
    fill_rects.push(Fill {
        x: patch_x as u16,
        y: edge_y as u16,
        width: patch_w as u16,
        height: 1,
        style: Style::new()
            .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
            .bg(diff.background_context),
    });
    let mut y = edge_y + 1;
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            y += 1; // the ▄ separator row between cards (single-file golden)
        }
        // File header: `␣` + dir muted + basename, counts right-aligned.
        let (dir, basename) = match file.file.rsplit_once('/') {
            Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
            None => (String::new(), file.file.clone()),
        };
        fill_rects.push(Fill {
            x: patch_x as u16,
            y: y as u16,
            width: patch_w as u16,
            height: 2,
            style: Style::new()
                .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                .bg(diff.background_context),
        });
        placed.push(Placed {
            x: patch_x + 1,
            y,
            width: crate::wrap::display_width(&dir),
            lines: vec![PlacedLine::plain(
                dir.clone(),
                Style::new().fg(fg_muted).bg(diff.background_context),
            )],
            tag: None,
        });
        placed.push(Placed {
            x: patch_x + 1 + crate::wrap::display_width(&dir),
            y,
            width: crate::wrap::display_width(&basename),
            lines: vec![PlacedLine::plain(
                basename,
                Style::new().fg(fg_base).bg(diff.background_context),
            )],
            tag: Some(format!("diff-file-header-{index}")),
        });
        // The counts: "+N" (added), one frame-default space, "-N"
        // (removed — the minus belongs to the count, the gap to the
        // fill). Image files carry the `Image` label instead
        // (diff-viewer.tsx:990-997, `countsWidth` = 6).
        if file.image {
            placed.push(Placed {
                x: patch_x + patch_w - 1 - 5,
                y,
                width: 5,
                lines: vec![PlacedLine::plain(
                    "Image".to_owned(),
                    Style::new().fg(fg_muted).bg(diff.background_context),
                )],
                tag: None,
            });
        } else {
            let added = format!("+{}", file.additions);
            let removed = format!("-{}", file.deletions);
            let counts_w = added.chars().count() + 1 + removed.chars().count();
            placed.push(Placed {
                x: patch_x + patch_w - 1 - counts_w,
                y,
                width: counts_w,
                lines: vec![PlacedLine {
                    runs: vec![
                        StyledSpan {
                            link: None,
                            text: added,
                            style: Style::new().fg(diff.text_added).bg(diff.background_context),
                        },
                        StyledSpan {
                            link: None,
                            text: " ".to_owned(),
                            style: Style::new()
                                .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                                .bg(diff.background_context),
                        },
                        StyledSpan {
                            link: None,
                            text: removed,
                            style: Style::new().fg(diff.text_removed).bg(diff.background_context),
                        },
                    ],
                }],
                tag: None,
            });
        }
        y += 2; // header row + its paddingBottom

        // The hunks. R-01b: the file's rows highlight through oc-syntax
        // (file extension → filetype, one parse per file, content-keyed
        // engine cache); every hunk row is one line of the joined row
        // text, so `file_line` maps rows to highlighted lines. Unknown
        // extensions (`.txt`, …) leave `highlighted` None — today's
        // plain rows, byte-exact.
        let hunks = file.hunks();
        if hunks.is_empty() {
            // TUI-10: a previewable image renders the inline
            // DiffViewerImage (diff-viewer-image.tsx:24-80): the
            // "Working tree preview" label, the blocks-quantized
            // image (`max(3, min(8, h/4))` rows), the decoded `WxH`
            // size row. Only non-committed modes with working-tree
            // bytes (deleted files keep the fallback copy).
            let label_y = y;
            let previewable = file.image
                && options.source_label != "Committed"
                && file.status != FileStatus::Deleted
                && file.image_data.is_some();
            if previewable {
                let bytes = file.image_data.clone().unwrap_or_default();
                let decoded = crate::image_blocks::decode_png(&bytes);
                let image_rows = (options.viewport_height / 4).clamp(3, 8);
                placed.push(Placed {
                    x: patch_x + 1,
                    y,
                    width: 20,
                    lines: vec![PlacedLine::plain(
                        "Working tree preview".to_owned(),
                        Style::new().fg(fg_muted).bg(diff.background_context),
                    )],
                    tag: None,
                });
                y += 2; // label + its gap 1
                if let Some(image) = decoded.as_ref() {
                    let inner_w = patch_w.saturating_sub(2).max(1);
                    let (cols, rows) = crate::image_blocks::fit_cells(
                        image.width,
                        image.height,
                        inner_w,
                        image_rows,
                    );
                    let cells = crate::image_blocks::blocks_cells(image, cols, rows);
                    let offset_x = patch_x + 1 + inner_w.saturating_sub(cols) / 2;
                    for (line_index, line) in cells.iter().enumerate() {
                        let runs = line
                            .iter()
                            .map(|cell| StyledSpan {
                                link: None,
                                text: cell.glyph.to_string(),
                                style: Style::new()
                                    .fg(Color::Rgb(cell.fg[0], cell.fg[1], cell.fg[2]))
                                    .bg(Color::Rgb(cell.bg[0], cell.bg[1], cell.bg[2])),
                            })
                            .collect();
                        placed.push(Placed {
                            x: offset_x,
                            y: y + line_index,
                            width: cols,
                            lines: vec![PlacedLine { runs }],
                            tag: None,
                        });
                    }
                    let size = format!("{}x{}", image.width, image.height);
                    placed.push(Placed {
                        x: patch_x + 1,
                        y: y + image_rows,
                        width: crate::wrap::display_width(&size),
                        lines: vec![PlacedLine::plain(
                            size,
                            Style::new().fg(fg_muted).bg(diff.background_context),
                        )],
                        tag: None,
                    });
                    if let Some(hits) = image_hits.as_deref_mut() {
                        // The click payload: the file's bytes as the
                        // ref's octet-stream data URI (the mention
                        // carries the path).
                        let mime = image_mime(&file.file);
                        let encoded = crate::clipboard::base64_encode::encode(&bytes);
                        let uri = format!("data:{mime};base64,{encoded}");
                        hits.push((
                            patch_x + 1,
                            label_y,
                            patch_w.saturating_sub(2),
                            image_rows + 3,
                            uri,
                        ));
                    }
                    y += image_rows + 2;
                } else {
                    // `image.error` → "Could not load image".
                    placed.push(Placed {
                        x: patch_x + 1,
                        y,
                        width: 21,
                        lines: vec![PlacedLine::plain(
                            "Could not load image".to_owned(),
                            Style::new().fg(styles.error).bg(diff.background_context),
                        )],
                        tag: None,
                    });
                    y += 2;
                }
                continue;
            }
            // The no-patch fallback row (diff-viewer.tsx:1012-1024):
            // image files carry their mode-specific copy, anything
            // else the generic notice.
            let text = if file.image {
                if options.source_label == "Committed" {
                    "Committed image preview unavailable. The working-tree image is not shown."
                } else if file.status == FileStatus::Deleted {
                    "Deleted image. The previous revision is not available for preview."
                } else {
                    "No patch available for this file."
                }
            } else {
                "No patch available for this file."
            };
            placed.push(Placed {
                x: patch_x + 1,
                y,
                width: crate::wrap::display_width(text),
                lines: vec![PlacedLine::plain(
                    text.to_owned(),
                    Style::new().fg(fg_muted).bg(diff.background_context),
                )],
                tag: None,
            });
            y += 1;
            continue;
        }
        let row_texts: Vec<&str> =
            hunks.iter().flat_map(|hunk| hunk.rows.iter().map(row_text)).collect();
        let source = row_texts.join("\n");
        let highlighted = crate::components::syntax::highlight_for_path(&source, &file.file);
        let mut file_line = 0usize;
        let digits = file.number_digits();
        for hunk in &hunks {
            if hunk.old_start != 1 {
                let header = format!(" {}", hunk.header);
                fill_rects.push(Fill {
                    x: patch_x as u16,
                    y: y as u16,
                    width: patch_w as u16,
                    height: 1,
                    style: Style::new()
                        .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                        .bg(diff.background_context),
                });
                placed.push(Placed {
                    x: patch_x,
                    y,
                    width: crate::wrap::display_width(&header),
                    lines: vec![PlacedLine::plain(
                        header,
                        Style::new().fg(diff.hunk_header).bg(diff.background_context),
                    )],
                    tag: None,
                });
                y += 1;
            }
            match view {
                DiffView::Unified => {
                    for row in &hunk.rows {
                        let colors = unified_colors(row, &diff, file_line);
                        paint_diff_row(
                            &mut placed,
                            &mut fill_rects,
                            patch_x,
                            patch_w,
                            digits,
                            y,
                            &colors,
                            fg_base,
                            &diff,
                            highlighted.as_ref(),
                            &styles.syntax,
                        );
                        file_line += 1;
                        y += 1;
                    }
                }
                DiffView::Split => {
                    let pane = patch_w / 2;
                    for pair in split_pairs(&hunk.rows, &diff, file_line) {
                        match pair.left {
                            Some(left) => paint_diff_row(
                                &mut placed,
                                &mut fill_rects,
                                patch_x,
                                pane,
                                digits,
                                y,
                                &left,
                                fg_base,
                                &diff,
                                highlighted.as_ref(),
                                &styles.syntax,
                            ),
                            None => fill_rects.push(Fill {
                                x: patch_x as u16,
                                y: y as u16,
                                width: pane as u16,
                                height: 1,
                                style: Style::new()
                                    .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                                    .bg(diff.background_context),
                            }),
                        }
                        match pair.right {
                            Some(right) => paint_diff_row(
                                &mut placed,
                                &mut fill_rects,
                                patch_x + pane,
                                patch_w - pane,
                                digits,
                                y,
                                &right,
                                fg_base,
                                &diff,
                                highlighted.as_ref(),
                                &styles.syntax,
                            ),
                            None => fill_rects.push(Fill {
                                x: (patch_x + pane) as u16,
                                y: y as u16,
                                width: (patch_w - pane) as u16,
                                height: 1,
                                style: Style::new()
                                    .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
                                    .bg(diff.background_context),
                            }),
                        }
                        y += 1;
                    }
                    file_line += hunk.rows.len();
                }
            }
        }
    }

    paint_placed_with_fills(&placed, &fill_rects, (width as u16, height as u16), base)
}

/// The inline diff image's MIME (isDiffImageFile's extension set).
fn image_mime(path: &str) -> &'static str {
    let ends_with = |suffix: &str| {
        path.len() >= suffix.len() && path[path.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    };
    if ends_with(".png") {
        "image/png"
    } else if ends_with(".jpg") || ends_with(".jpeg") {
        "image/jpeg"
    } else if ends_with(".webp") {
        "image/webp"
    } else {
        "image/gif"
    }
}

fn unified_colors(row: &PatchRow, diff: &DiffStyles, line: usize) -> RowColors {
    match row {
        PatchRow::Context { old, new, text } => RowColors {
            gutter_bg: diff.background_context,
            sign_fg: None,
            sign: "   ",
            text_bg: diff.background_context,
            number: (*old).max(*new),
            text: text.clone(),
            line,
        },
        PatchRow::Deleted { old, text } => RowColors {
            gutter_bg: diff.line_number_removed,
            sign_fg: Some(diff.highlight_removed),
            sign: " -",
            text_bg: diff.background_removed,
            number: *old,
            text: text.clone(),
            line,
        },
        PatchRow::Added { new, text } => RowColors {
            gutter_bg: diff.line_number_added,
            sign_fg: Some(diff.highlight_added),
            sign: " +",
            text_bg: diff.background_added,
            number: *new,
            text: text.clone(),
            line,
        },
    }
}

/// Paint one gutter+text row into the pane `[x, x + pane_w)`:
/// `␣` + line number (left-aligned, natural width) + sign (`␣−`/`␣+`
/// immediately after the number) + padding — text starts at
/// `x + 1 + digits + 3`.
///
/// R-01b: with a highlighted block the text column renders the row's
/// oc-syntax spans (theme fg, bg pinned to the row's diff background —
/// the gutter and diff-bg painting are untouched by highlighting);
/// without one it is exactly the original plain single-run row.
#[allow(clippy::too_many_arguments)] // an internal painter over precomputed inputs
fn paint_diff_row(
    placed: &mut Vec<Placed>,
    fills: &mut Vec<Fill>,
    x: usize,
    pane_w: usize,
    digits: usize,
    y: usize,
    colors: &RowColors,
    text_fg: Color,
    diff: &DiffStyles,
    highlighted: Option<&HighlightedBlock>,
    syntax: &oc_syntax::SyntaxStyles,
) {
    // The gutter: `␣` + number field + sign column + `␣` (text starts
    // 1 + digits + 3 columns in).
    let gutter_w = 1 + digits + 3;
    // The gutter's spaces carry the frame-default fg (255) — the
    // number and sign runs paint their own colors.
    fills.push(Fill {
        x: x as u16,
        y: y as u16,
        width: gutter_w as u16,
        height: 1,
        style: Style::new()
            .fg(crate::components::tool_rows::DEFAULT_FOREGROUND)
            .bg(colors.gutter_bg),
    });
    let number = colors.number.to_string();
    let number_width = number.chars().count();
    placed.push(Placed {
        x: x + 1,
        y,
        width: number_width,
        lines: vec![PlacedLine::plain(
            number,
            Style::new().fg(diff.line_number).bg(colors.gutter_bg),
        )],
        tag: None,
    });
    if let Some(sign_fg) = colors.sign_fg {
        placed.push(Placed {
            x: x + 1 + number_width,
            y,
            width: 2,
            lines: vec![PlacedLine::plain(
                colors.sign,
                Style::new().fg(sign_fg).bg(colors.gutter_bg),
            )],
            tag: None,
        });
    }
    // The text: from the gutter's end to the pane's right edge, in the
    // row's own background.
    let text_x = x + gutter_w;
    let text_w = pane_w.saturating_sub(gutter_w);
    fills.push(Fill {
        x: text_x as u16,
        y: y as u16,
        width: text_w as u16,
        height: 1,
        style: Style::new().fg(crate::components::tool_rows::DEFAULT_FOREGROUND).bg(colors.text_bg),
    });
    if colors.text.is_empty() {
        return;
    }
    match highlighted {
        Some(block) => {
            let base = Style::new().fg(text_fg).bg(colors.text_bg);
            let runs = crate::components::syntax::line_spans(block, colors.line, base, syntax);
            placed.push(Placed {
                x: text_x,
                y,
                width: text_w,
                lines: vec![PlacedLine {
                    runs: crate::components::syntax::clip_spans(runs, text_w),
                }],
                tag: None,
            });
        }
        // The original plain row (unknown extensions): unchanged bytes.
        None => placed.push(Placed {
            x: text_x,
            y,
            width: crate::wrap::display_width(&colors.text).min(text_w),
            lines: vec![PlacedLine::plain(
                colors.text.clone(),
                Style::new().fg(text_fg).bg(colors.text_bg),
            )],
            tag: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// The patch parses into three hunks with pre-assigned numbers;
    /// the first starts at old line 1 (its header hides).
    #[test]
    fn patch_parses_with_line_numbers() {
        let files = diff_viewer_fixture();
        let hunks = files[0].hunks();
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[1].header, "@@ -20,3 +20,3 @@");
        assert_eq!(hunks[0].rows.len(), 4);
        assert!(matches!(&hunks[0].rows[0], PatchRow::Context { old: 1, new: 1, .. }));
        assert!(matches!(&hunks[0].rows[1], PatchRow::Deleted { old: 2, .. }));
        assert!(matches!(&hunks[0].rows[2], PatchRow::Added { new: 2, .. }));
        assert!(matches!(&hunks[0].rows[3], PatchRow::Context { old: 3, new: 3, .. }));
        assert!(matches!(&hunks[2].rows[0], PatchRow::Context { old: 40, new: 40, .. }));
        assert_eq!(files[0].number_digits(), 2, "widest number 42");
    }

    /// The layout rules: tree at ≥90 (width/4 clamped 30..40), patch
    /// pane = width − tree − 4, split needs ≥100.
    #[test]
    fn layout_rules_match_the_reference() {
        let wide = diff_layout(160, true, 1);
        assert!(wide.show_tree);
        assert_eq!(wide.tree_width, 40);
        assert_eq!(wide.patch_pane_width, 116);
        assert!(wide.split_available);
        let narrow = diff_layout(80, true, 1);
        assert!(!narrow.show_tree, "no tree below 90");
        assert_eq!(narrow.patch_pane_width, 76);
        assert!(!narrow.split_available);
        // The clamps: 30..40 of width/4.
        assert_eq!(diff_layout(140, true, 1).tree_width, 35);
        assert_eq!(diff_layout(400, true, 1).tree_width, FILE_TREE_MAX_WIDTH);
        // No files: no tree.
        assert!(!diff_layout(160, true, 0).show_tree);
    }

    /// The v2 `diff` key layer: n/p/b/s/d/v/enter/tab/]/[/? map to
    /// their canonical commands.
    #[test]
    fn diff_key_layer_from_the_v2_table() {
        let cases = [
            ("n", "diff.next_file"),
            ("p", "diff.previous_file"),
            ("b", "diff.toggle_file_tree"),
            ("s", "diff.single_patch"),
            ("d", "diff.switch_source"),
            ("v", "diff.toggle_view"),
            ("j", "diff.down"),
            ("k", "diff.up"),
            ("m", "diff.mark_reviewed"),
            ("ctrl+d", "diff.half_page.down"),
            ("]", "diff.next_hunk"),
            ("[", "diff.previous_hunk"),
            ("?", "diff.help"),
            ("escape", "diff.close"),
            ("q", "diff.close"),
        ];
        for (key, command) in cases {
            assert_eq!(diff_command_for_key(key), Some(command), "key {key}");
        }
        // The five deprecated v1-era tree bindings ship "none" — the
        // file tree is mouse-controlled in v2 (keybind.ts:75-79).
        for key in ["enter", "space", "tab", "right", "left", "E"] {
            assert_eq!(diff_command_for_key(key), None, "deprecated key {key} unbound");
        }
        assert_eq!(diff_command_for_key("x"), None);
        assert_eq!(diff_command_for_key("none"), None);
    }

    /// The state machine: v toggles only when split is available, b
    /// toggles the tree, n/p clamp the selection.
    #[test]
    fn commands_drive_the_state() {
        let files = diff_viewer_fixture();
        let wide = diff_layout(160, true, 1);
        let mut state = DiffViewerState::default();
        assert!(state.run_command("diff.toggle_view", &wide, &files));
        assert_eq!(state.view(&wide), DiffView::Unified);
        let narrow = diff_layout(80, true, 1);
        assert!(!state.run_command("diff.toggle_view", &narrow, &files), "no split below 100");
        assert!(state.run_command("diff.toggle_file_tree", &narrow, &files));
        assert!(!state.tree_enabled);
        assert!(state.run_command("diff.next_file", &narrow, &files));
        assert_eq!(state.selected_file, Some(0));
        assert!(!state.run_command("diff.next_file", &narrow, &files), "clamped at one file");
    }

    /// The tree builds one directory row plus the file row.
    #[test]
    fn file_tree_rows() {
        let rows = build_file_tree(&diff_viewer_fixture());
        assert_eq!(
            rows,
            vec![
                TreeRow { depth: 0, name: "src".into(), file_index: None },
                TreeRow { depth: 1, name: "file.txt".into(), file_index: Some(0) },
            ]
        );
    }

    /// Split pairing zips the del/add runs onto one row.
    #[test]
    fn split_pairs_zip_dels_and_adds() {
        let diff = theme().diff;
        let rows = diff_viewer_fixture()[0].hunks().remove(0).rows;
        let pairs = split_pairs(&rows, &diff, 0);
        assert_eq!(pairs.len(), 3);
        assert!(pairs[0].left.is_some() && pairs[0].right.is_some(), "context both sides");
        assert!(matches!(&pairs[1].left, Some(c) if c.sign == " -"));
        assert!(matches!(&pairs[1].right, Some(c) if c.sign == " +"));
        // Line indices: one per source row, contexts shared by both
        // sides, the del/add pair on rows 1 and 2 of the joined text.
        assert_eq!(pairs[0].left.as_ref().map(|c| c.line), Some(0));
        assert_eq!(pairs[1].left.as_ref().map(|c| c.line), Some(1));
        assert_eq!(pairs[1].right.as_ref().map(|c| c.line), Some(2));
    }

    /// R-01b: a rust file's text column renders oc-syntax spans — the
    /// comment row carries a themed fg (≠ the base text color) while
    /// the diff backgrounds and gutter stay exactly the plain-file
    /// colors; a `.txt` file keeps the original single-run plain rows.
    #[test]
    fn rust_file_text_column_highlights_and_txt_stays_plain() {
        let theme = theme();
        let patch = "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,2 +1,2 @@\n // keep\n-fn old() {}\n+fn new() {}\n";
        let files = vec![DiffFile {
            file: "src/main.rs".into(),
            additions: 1,
            deletions: 1,
            status: FileStatus::Modified,
            patch: patch.into(),
            image: false,
            image_data: None,
        }];
        let frame = render_diff_viewer(
            &files,
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme,
            80,
            24,
        );
        let grid = frame.text_grid();
        // Row 4 is the context comment; glyphs unchanged (this file's
        // widest number is one digit, so the text starts at col 7).
        assert!(grid[4].starts_with("   1   // keep"), "row 4: {:?}", grid[4]);
        let fg_base = theme.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND);
        let base_cell = crate::render::CellColor::from(fg_base);
        assert_ne!(frame.cell(4, 7).fg, base_cell, "comment themed, not base");
        // The diff row background is untouched by highlighting.
        assert_eq!(
            frame.cell(4, 7).bg,
            crate::render::CellColor::from(theme.diff.background_context)
        );
        // The deleted row keeps its removal background under spans.
        assert!(grid[5].starts_with("   2 - fn old() {}"), "row 5: {:?}", grid[5]);
        assert_eq!(
            frame.cell(5, 7).bg,
            crate::render::CellColor::from(theme.diff.background_removed)
        );
        assert_ne!(frame.cell(5, 7).fg, base_cell, "fn keyword themed");
        // Plain fixture (.txt): every text cell stays the base fg.
        let plain = render_diff_viewer(
            &diff_viewer_fixture(),
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme,
            80,
            24,
        );
        let grid = plain.text_grid();
        assert!(grid[4].starts_with("   1    const first = true"));
        assert_eq!(plain.cell(4, 8).fg, base_cell, "plain rows keep base fg");
    }

    /// The unified render at 80: toolbar + card rows + the `?` square.
    #[test]
    fn unified_render_shape_at_80() {
        let theme = theme();
        let files = diff_viewer_fixture();
        let frame = render_diff_viewer(
            &files,
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme,
            80,
            24,
        );
        let grid = frame.text_grid();
        assert!(grid[0].starts_with("  All · vs v2"), "row 0: {:?}", grid[0]);
        assert!(grid[0].trim_end().ends_with("0/1 ?"), "row 0: {:?}", grid[0]);
        assert!(grid[2].contains("src/file.txt"), "row 2: {:?}", grid[2]);
        assert!(grid[2].trim_end().ends_with("+3 -3"), "row 2: {:?}", grid[2]);
        assert!(grid[4].starts_with("   1    const first = true"), "row 4: {:?}", grid[4]);
        assert!(grid[5].starts_with("   2 -  const oldFirst = true"), "row 5: {:?}", grid[5]);
        assert!(grid[8].starts_with("   @@ -20,3 +20,3 @@"), "row 8: {:?}", grid[8]);
    }

    /// G-11 adapter contract: the viewer's model agrees with
    /// opentui-core's `parse_patch` on a nontrivial fixture — multi-
    /// hunk, context lines, mixed add/delete runs, a no-newline
    /// marker, and a git rename (extended headers, a similarity-100
    /// rename carries no hunks; a rename-with-edit carries them).
    #[test]
    fn diff_model_agrees_with_opentui_parse_patch() {
        let fixture = concat!(
            "--- a/src/lib.rs\n",
            "+++ b/src/lib.rs\n",
            "@@ -1,3 +1,4 @@\n",
            " mod lib;\n",
            "-fn old() {}\n",
            "\\ No newline at end of file\n",
            "+fn new() {}\n",
            "+fn added() {}\n",
            " // tail\n",
            "@@ -20,3 +21,1 @@\n",
            " ctx\n",
            "-removed\n",
            "-also-removed\n",
        );
        // The opentui side (jsdiff semantics): two hunks, the marker
        // kept in lines, counts validated.
        let structured = opentui_core::renderables::diff::parse_patch(fixture).expect("valid");
        assert_eq!(structured.len(), 1);
        assert_eq!(structured[0].hunks.len(), 2);
        assert_eq!(structured[0].hunks[0].lines.len(), 6, "marker kept upstream");
        // The oc-tui side: same hunks, pre-numbered rows, marker dropped.
        let hunks = parse_patch(fixture);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].header, "@@ -1,3 +1,4 @@");
        assert_eq!((hunks[0].old_start, hunks[0].new_start), (1, 1));
        assert_eq!(
            hunks[0].rows,
            vec![
                PatchRow::Context { old: 1, new: 1, text: "mod lib;".into() },
                PatchRow::Deleted { old: 2, text: "fn old() {}".into() },
                PatchRow::Added { new: 2, text: "fn new() {}".into() },
                PatchRow::Added { new: 3, text: "fn added() {}".into() },
                PatchRow::Context { old: 3, new: 4, text: "// tail".into() },
            ],
            "marker dropped, numbers follow upstream counting"
        );
        assert_eq!(hunks[1].header, "@@ -20,3 +21,1 @@");
        assert_eq!((hunks[1].old_start, hunks[1].new_start), (20, 21));
        assert_eq!(
            hunks[1].rows,
            vec![
                PatchRow::Context { old: 20, new: 21, text: "ctx".into() },
                PatchRow::Deleted { old: 21, text: "removed".into() },
                PatchRow::Deleted { old: 22, text: "also-removed".into() },
            ]
        );
        // A pure git rename (similarity 100%): no hunks on either side.
        let rename = concat!(
            "diff --git a/old_name.rs b/new_name.rs\n",
            "similarity index 100%\n",
            "rename from old_name.rs\n",
            "rename to new_name.rs\n",
        );
        let renamed = opentui_core::renderables::diff::parse_patch(rename).unwrap();
        assert!(renamed[0].hunks.is_empty(), "upstream: pure rename carries no hunks");
        assert_eq!(parse_patch(rename), [] as [Hunk; 0]);
        // A malformed patch (count mismatch) is REJECTED upstream → no
        // hunks (the "No patch available" fallback).
        let broken = "--- a\n+++ b\n@@ -1,2 +1,2 @@\n x";
        assert!(opentui_core::renderables::diff::parse_patch(broken).is_err());
        assert_eq!(parse_patch(broken), [] as [Hunk; 0]);
    }

    /// R3 byte-parity spike (FRAMEWORK-CONVERGENCE.md, line-number section):
    /// opentui-core's `LineNumberRenderable` gutter vs the viewer's
    /// pinned inline gutter (`paint_diff_row`). Verdict recorded:
    /// **permanent local keep** — the gutter WIDTH models coincide
    /// (digits + 4 columns) but the internal alignment does not:
    ///
    /// * the viewer paints the number LEFT-aligned at `x+1` with the
    ///   sign `" -"`/`" +"` immediately after the natural-width
    ///   number, so the sign column VARIES per row (golden bytes:
    ///   `   2 -  const oldFirst` vs `   21 - const oldSecond`);
    /// * opentui right-aligns the number
    ///   (`line_num_x = maxBefore + 1 + availableSpace − numWidth − 1`,
    ///   LineNumberRenderable.ts:324-337) and pins the sign at
    ///   `gutter_width − padding_right − max_after_width` (:339-344),
    ///   a FIXED column.
    ///
    /// Every row whose number is narrower than the file's widest
    /// number diverges — the dominant case (visible in the primary
    /// golden fixture itself) — so adoption is neither byte-identical
    /// nor a provably-limited re-pin.
    #[test]
    fn r3_line_number_gutter_diverges_from_opentui() {
        use opentui_core::buffer::OptimizedBuffer;
        use opentui_core::color::parse_color;
        use opentui_core::renderables::line_number::{
            calculate_sign_widths, paint_gutter, GutterState, LineNumberOptions, LineSign,
        };

        // The golden's del rows: a 2-digit field (widest number 42)
        // carrying the numbers 2 and 21, sign " -" after the number.
        let mut opts = LineNumberOptions::default();
        for (line, number) in [(0i64, 2i64), (1, 21)] {
            opts.line_signs.insert(
                line,
                LineSign {
                    after: Some(" -".to_owned()),
                    after_color: Some(parse_color("#ef4444")),
                    ..Default::default()
                },
            );
            opts.line_numbers.insert(line, number);
        }
        let state = GutterState::new(&opts);
        let (before, after) = calculate_sign_widths(&state.line_signs);
        assert_eq!((before, after), (0, 2), "sign width model matches the spike brief");
        // Width model: opentui max(min_width 3, digits 2 + pad 1 + 1) + after 2
        // == the viewer's 1 + digits + 3 == 6. The widths coincide…
        let opentui_width = state.measure_ctx(2).calculate_width();
        assert_eq!(opentui_width, 6);
        assert_eq!(1 + 2 + 3, opentui_width as usize, "gutter widths agree");
        // …the bytes do not. Paint the opentui gutter for the same rows.
        let mut buffer = OptimizedBuffer::new(6, 2);
        paint_gutter(&state, opentui_width, 2, &[0u32, 1], None, 0, &mut buffer, 0, 0);
        let rows: Vec<String> =
            buffer.char_frame().lines().map(|line| line.chars().take(6).collect()).collect();
        // Number right-aligned, sign at a fixed column (3-4):
        assert_eq!(rows[0], "  2 - ", "opentui row 0: right-aligned");
        assert_eq!(rows[1], " 21 - ", "opentui row 1: sign column fixed");
        // The viewer's pinned model: number left-aligned at col 1, the
        // sign immediately after the natural-width number (col 2-3 for
        // "2", col 3-4 for "21") — the sign column varies per row.
        assert_eq!(" 2 -  ".chars().take(6).collect::<String>(), " 2 -  ");
        assert_ne!(rows[0], " 2 -  ", "row 0 bytes diverge (number alignment)");
        assert_ne!(rows[1], "21 -  ", "row 1 bytes diverge (leading pad column)");
        // And the divergence is pinned in the golden corpus itself: the
        // w80 text grid carries ` 2 - `-shaped rows next to ` 21 - `
        // ones (sign right after the number, not column-aligned).
        let frame = render_diff_viewer(
            &diff_viewer_fixture(),
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme(),
            80,
            24,
        );
        let grid = frame.text_grid();
        assert!(grid[5].starts_with("   2 -  const oldFirst"), "golden model: {:?}", grid[5]);
        assert!(grid[10].starts_with("   21 - const oldSecond"), "golden model: {:?}", grid[10]);
    }
}
