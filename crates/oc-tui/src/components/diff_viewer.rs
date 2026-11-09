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
//! ## Rendering (lane D3.5 — shared tree)
//!
//! The route renders through the shared-tree family
//! ([`crate::components::diff_viewer_solid`]): toolbar, tree pane,
//! card headers and the patch body are mounted nodes, and every hunk
//! is one opentui `DiffRenderable` with its built-in
//! `LineNumberRenderable` gutter. The `syncGutters` pass (the ref's
//! patch-diff.tsx:60-89) widens each per-hunk gutter to the file-wide
//! number field, which is what keeps the golden bytes identical.
//! [`render_diff_viewer`] is the headless twin of that mount.
//!
//! A hunk header hides while its hunk starts at line 1 (the golden's
//! first `@@` never shows).
//!
//! ## Keys
//!
//! The `diff` layer of the v2 default table ([`diff_command_for_key`]):
//! `n/p` file jumps, `b` tree, `s` single patch, `d` source, `v`
//! split/unified, `enter/space` toggle, `tab` focus, `]/[` hunks,
//! `?` help, `escape/q` close.
//!
//! ## Syntax (R-01b / D1d)
//!
//! The patch body highlights through opentui's `CodeRenderable` with
//! the file's `filetype` ([`crate::components::syntax::filetype_for_path`])
//! and the theme-derived `SyntaxStyle`; unknown extensions render the
//! plain rows.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::too_many_lines)]

use crate::render::Frame;
use crate::style::Color;
use crate::theme::ThemeStyles;

/// `MIN_SPLIT_WIDTH` — the patch pane needs this many columns before
/// the split view is offered.
pub const MIN_SPLIT_WIDTH: usize = 100;
/// `FILE_TREE_MIN_WIDTH`.
pub const FILE_TREE_MIN_WIDTH: usize = 30;
/// `FILE_TREE_MAX_WIDTH`.
pub const FILE_TREE_MAX_WIDTH: usize = 40;
/// The tree renders only at/above this terminal width.
pub const FILE_TREE_BREAKPOINT: usize = 90;
/// G-130 — `VIRTUAL_MIN_LINES` (patch-diff.tsx:24): only added files
/// this large chunk (smaller ones render fine as one hunk).
pub const VIRTUAL_MIN_LINES: usize = 3000;
/// G-130 — `VIRTUAL_CHUNK_LINES` (patch-diff.tsx:25): the added-file
/// chunk size.
pub const VIRTUAL_CHUNK_LINES: usize = 128;

/// G-130 — one added-file chunk (`AddedPatchChunk`, util/diff.ts:6-10):
/// a valid `-0,0` patch of its own + the raw `+`-lines + the row
/// count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedPatchChunk {
    pub patch: String,
    pub lines: Vec<String>,
    pub rows: usize,
}

/// G-130 — `splitAddedPatch` (util/diff.ts:18-38): split a new-file
/// patch into `size`-line chunks, each a valid patch with its own
/// `@@ -0,0 +start,count @@` header. `None` for anything else —
/// patches with context or removed lines would need old/new line
/// numbers recomputed at every cut, so they are not split.
#[must_use]
pub fn split_added_patch(patch: &str, size: usize) -> Option<Vec<AddedPatchChunk>> {
    if size == 0 {
        return None;
    }
    // `/^@@ -0,0 \+1,(\d+) @@[^\n]*\n/m` — the first matching header
    // line anywhere in the patch (the file header `--- / +++` lines
    // ride the prefix).
    let mut offset = 0usize;
    let mut line_start = 0usize;
    let mut found: Option<usize> = None;
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("@@ -0,0 +1,") {
            if let Some((digits, tail)) = rest.split_once(' ') {
                if tail.starts_with("@@") {
                    if let Ok(count) = digits.parse::<usize>() {
                        found = Some(count);
                        offset = line_start + line.len() + 1;
                        break;
                    }
                }
            }
        }
        line_start += line.len() + 1;
        offset = line_start;
    }
    let count = found?;
    let prefix = &patch[..line_start];
    let body = &patch[offset..];
    let mut lines: Vec<String> =
        body.strip_suffix('\n').unwrap_or(body).split('\n').map(str::to_owned).collect();
    let marker =
        lines.last().filter(|last| last.starts_with("\\ No newline at end of file")).cloned();
    if marker.is_some() {
        lines.pop();
    }
    if lines.len() != count || lines.iter().any(|line| !line.starts_with('+')) {
        return None;
    }
    let mut chunks = Vec::with_capacity(count.div_ceil(size));
    for index in 0..count.div_ceil(size) {
        let start = index * size;
        let slice = &lines[start..(start + size).min(count)];
        let tail = match &marker {
            Some(marker) if start + size >= count => format!("\n{marker}"),
            _ => String::new(),
        };
        chunks.push(AddedPatchChunk {
            patch: format!(
                "{prefix}@@ -0,0 +{},{} @@\n{}{tail}",
                start + 1,
                slice.len(),
                slice.join("\n")
            ),
            lines: slice.to_vec(),
            rows: slice.len(),
        });
    }
    Some(chunks)
}

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
    ///
    /// G-130 — added-file chunking first (`splitAddedPatch` +
    /// patch-diff.tsx:41-44): a single-hunk `@@ -0,0 +1,N @@` patch
    /// over [`VIRTUAL_MIN_LINES`] renders as
    /// [`VIRTUAL_CHUNK_LINES`]-line chunks, each its own valid
    /// `-0,0` hunk (huge added files chunk like the ref's virtualized
    /// `PatchDiff`; bounded deviation G-130: the Rust viewer renders
    /// the chunk list directly — no lazy mount window, the ROW set is
    /// identical).
    #[must_use]
    pub fn hunks(&self) -> Vec<Hunk> {
        if let Some(chunks) = split_added_patch(&self.patch, VIRTUAL_CHUNK_LINES) {
            let total: usize = chunks.iter().map(|chunk| chunk.rows).sum();
            if total > VIRTUAL_MIN_LINES {
                return chunks
                    .iter()
                    .enumerate()
                    .map(|(index, chunk)| {
                        let new_start = index as u64 * VIRTUAL_CHUNK_LINES as u64 + 1;
                        let rows = chunk
                            .lines
                            .iter()
                            .enumerate()
                            .map(|(row, line)| PatchRow::Added {
                                new: new_start + row as u64,
                                text: line.strip_prefix('+').unwrap_or(line).to_owned(),
                            })
                            .collect();
                        Hunk {
                            header: format!("@@ -0,0 +{},{} @@", new_start, chunk.lines.len()),
                            old_start: 0,
                            new_start,
                            rows,
                        }
                    })
                    .collect();
            }
        }
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
pub(crate) fn tint(color: Color, background: Color, t: f32) -> Color {
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

/// Render the diff viewer fullscreen (the headless twin of the
/// shared-tree family mount — `diff_viewer_solid::twin_loaded`).
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
pub fn render_diff_viewer_with_hits(
    files: &[DiffFile],
    state: &DiffViewerState,
    options: &DiffViewerOptions,
    theme: &ThemeStyles,
    width: usize,
    height: usize,
    image_hits: Option<&mut Vec<crate::components::diff_viewer_mount::DiffImageHit>>,
) -> Frame {
    let (frame, hits) = crate::components::diff_viewer_solid::twin_loaded(
        files, state, options, theme, width, height,
    );
    if let Some(out) = image_hits {
        out.extend(hits);
    }
    frame
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

    /// R3 byte-parity spike (FRAMEWORK-CONVERGENCE.md, line-number
    /// section), kept as the raw-gutter model pin: opentui-core's
    /// `LineNumberRenderable` gutter is right-aligned/fixed-sign on
    /// its own — the shared-tree lane D3.5 restores the golden model
    /// through the `syncGutters` pad (the `-1` sign's trailing spaces
    /// widen the gutter exactly as the ref does). The raw gutter
    /// models coincide in WIDTH (digits + 4 columns) but not
    /// internally:
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
    /// number diverges without the pad — the dominant case — so the
    /// `syncGutters` port (patch-diff.tsx:60-89) is what reconciles
    /// the per-hunk `DiffRenderable` gutters with the file-wide field
    /// (see [`crate::components::diff_viewer_solid`]).
    #[test]
    fn r3_opentui_gutter_aligns_through_the_sync_pad() {
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

// -- G-130: splitAddedPatch ---------------------------------------------------

#[cfg(test)]
mod g130_tests {
    use super::*;
    use std::fmt::Write as _; // format_push_string: write! into the existing String

    fn added_patch(lines: usize, marker: bool) -> String {
        let mut patch = format!("--- /dev/null\n+++ b/big.txt\n@@ -0,0 +1,{lines} @@\n");
        for index in 0..lines {
            let _ = writeln!(patch, "+line {index}");
        }
        if marker {
            // replace the final newline with the no-newline marker
            patch.truncate(patch.trim_end_matches('\n').len() + 1);
            patch.push_str("\\ No newline at end of file\n");
        }
        patch
    }

    #[test]
    fn splits_an_added_file_into_size_line_chunks() {
        let chunks = split_added_patch(&added_patch(300, false), 128).expect("splittable");
        assert_eq!(chunks.len(), 3, "ceil(300/128)");
        assert_eq!(chunks[0].rows, 128);
        assert_eq!(chunks[2].rows, 44);
        assert!(chunks[0]
            .patch
            .starts_with("--- /dev/null\n+++ b/big.txt\n@@ -0,0 +1,128 @@\n+line 0\n"));
        assert!(chunks[1].patch.contains("@@ -0,0 +129,128 @@"));
        assert!(chunks[2].patch.contains("@@ -0,0 +257,44 @@"));
        assert_eq!(chunks.iter().map(|chunk| chunk.rows).sum::<usize>(), 300);
    }

    #[test]
    fn the_no_newline_marker_rides_the_last_chunk() {
        let chunks = split_added_patch(&added_patch(10, true), 4).expect("splittable");
        assert_eq!(chunks.len(), 3);
        assert!(!chunks[0].patch.contains("No newline"));
        assert!(chunks[2].patch.ends_with("\\ No newline at end of file"));
    }

    #[test]
    fn context_or_removed_lines_never_split() {
        let modified = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-a\n+b\n c\n";
        assert_eq!(split_added_patch(modified, 2), None);
        // count mismatch
        let broken = "@@ -0,0 +1,5 @@\n+a\n+b\n";
        assert_eq!(split_added_patch(broken, 2), None);
        // no new-file header at all
        assert_eq!(split_added_patch("no patch here", 2), None);
    }

    #[test]
    fn oversized_added_files_chunk_through_hunks() {
        let file = DiffFile {
            file: "big.txt".to_owned(),
            additions: 4000,
            deletions: 0,
            status: FileStatus::Added,
            patch: added_patch(4000, false),
            image: false,
            image_data: None,
        };
        let hunks = file.hunks();
        assert_eq!(hunks.len(), 32, "ceil(4000/128)");
        assert_eq!(hunks[0].header, "@@ -0,0 +1,128 @@");
        assert_eq!(hunks[31].header, "@@ -0,0 +3969,32 @@");
        match &hunks[31].rows[0] {
            PatchRow::Added { new, text } => {
                assert_eq!(*new, 3969);
                assert_eq!(text, "line 3968");
            }
            other => panic!("expected an added row, got {other:?}"),
        }
        // below the threshold the single hunk parses normally
        let small = DiffFile { patch: added_patch(300, false), ..file };
        assert_eq!(small.hunks().len(), 1);
    }
}
