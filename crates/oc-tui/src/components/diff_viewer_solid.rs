//! The diff viewer family on the shared tree (lane D3.5): the VCS
//! route's full-screen surface — toolbar, file tree, per-file card
//! headers and the patch body — mounted as family `diff` under the
//! shared-tree hub (`surface-diff`, skeleton index 0, the BASE seat:
//! every other family wrapper paints above it, exactly the old route
//! frame's paste order).
//!
//! ## The patch body is opentui's `DiffRenderable`
//!
//! Every hunk is one `DiffRenderable` (the ref's `PatchDiff` per-hunk
//! `<diff>` nodes, patch-diff.tsx:97-116) constructed through
//! [`CliRenderer::make_diff`](opentui_core::renderer::CliRenderer::make_diff):
//! the hunk patch text feeds `build_diff_view`, the gutter is the
//! ported `LineNumberRenderable` (the local row painter and its
//! hand-painted number/sign slot are retired), and the row colors map
//! 1:1 from `theme.diff.*`. `syncGutters` (patch-diff.tsx:60-89) is
//! ported as the `syncGutters` pass (below): the per-hunk side gutters are
//! widened to one file-wide number field (the injected `after` pad),
//! which is what makes the per-hunk `DiffRenderable` bytes coincide
//! with the ref's settled gutters.
//!
//! ## Virtualization
//!
//! [`DiffFile::hunks`] already chunks a >3000-line added-file patch
//! into 128-line `-0,0` hunks (`split_added_patch`, the ref's
//! `VIRTUAL_MIN_LINES`/`VIRTUAL_CHUNK_LINES`, patch-diff.tsx:24-27 +
//! util/diff.ts). The shared-tree renderer consumes that SAME hunk
//! list — the row set is identical; the ref's lazy mount window is
//! the recorded G-130 bounded deviation (the Rust side renders the
//! chunk list directly).
//!
//! ## Scroll
//!
//! The patch column lives in an `overflow: hidden` clip box whose
//! content offsets by `-scroll` — the old painter's per-column crop
//! (`render_diff_scrolled`) as a tree clip; the toolbar and the tree
//! pane stay pinned.

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::too_many_lines)]

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use opentui_core::color::{parse_color, Rgba};
use opentui_core::layout::Dimension;
use opentui_core::renderable::Content;
use opentui_core::renderables::diff::{DiffOptions, DiffView as OtDiffView};
use opentui_core::renderables::text::WrapMode;
use opentui_core::styled_text::{StyledText, TextChunk};
use opentui_core::syntax_style::{StyleDefinitionInput, SyntaxStyle};
use opentui_solid::element::{Children, PropValue};
use opentui_solid::extend::{extend, CustomElement};
use opentui_solid::{box_, custom, text, Element};

use crate::components::chrome_solid::{
    blank_frame, composite_chrome_tree, family_wrapper_live, mount_family_wrapper, reset_hub,
};
use crate::components::diff_viewer::{
    build_file_tree, diff_layout, split_added_patch, tint, DiffFile, DiffView, DiffViewerOptions,
    DiffViewerState, FileStatus, Hunk, PatchRow, VIRTUAL_CHUNK_LINES, VIRTUAL_MIN_LINES,
};
use crate::components::diff_viewer_mount::DiffImageHit;
use crate::render::Frame;
use crate::style::Color;
use crate::theme::ThemeStyles;

/// The family's seat under the shared tree's root: the BASE position
/// (the fullscreen route frame is pasted first; every family wrapper
/// — chrome overlays, dialogs, toasts — seats above it).
const DIFF_SKELETON_INDEX: usize = 0;

/// `isDiffImageFile` extension set — kept local to avoid a cycle with
/// the mount module's copy (identical list).
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

// ---------------------------------------------------------------------------
// colors
// ---------------------------------------------------------------------------

fn rgba(color: Color) -> Option<Rgba> {
    match color {
        Color::Rgb(r, g, b) => Some(Rgba::rgb(r, g, b)),
        Color::Indexed(_) | Color::Reset => None,
    }
}

fn hex(color: Color) -> Option<String> {
    match color {
        Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
        Color::Indexed(_) | Color::Reset => None,
    }
}

fn rgba_hex(color: Rgba) -> String {
    let channel = |index: usize| u8::try_from(color.0[index]).unwrap_or(0);
    format!("#{:02x}{:02x}{:02x}", channel(0), channel(1), channel(2))
}

fn chunk(text: impl Into<String>, fg: Option<Rgba>, bg: Option<Rgba>) -> TextChunk {
    TextChunk { text: text.into(), fg, bg, attributes: 0, link: None }
}

fn bold_chunk(text: impl Into<String>, fg: Option<Rgba>, bg: Option<Rgba>) -> TextChunk {
    TextChunk { text: text.into(), fg, bg, attributes: 1, link: None }
}

/// The opentui `SyntaxStyle` for the diff body's code panes — the SAME
/// generated theme rule table the oc-syntax seam consumes
/// (markdown.rs's construction).
fn syntax_style(styles: &ThemeStyles) -> SyntaxStyle {
    let defs = styles
        .syntax
        .iter()
        .map(|(scope, def)| {
            (
                scope.as_str(),
                StyleDefinitionInput {
                    fg: def.fg.and_then(|c| hex(Color::Rgb(c.r, c.g, c.b))),
                    bg: def.bg.and_then(|c| hex(Color::Rgb(c.r, c.g, c.b))),
                    bold: def.bold,
                    italic: def.italic,
                    underline: def.underline,
                    dim: def.dim,
                },
            )
        })
        .collect::<Vec<_>>();
    SyntaxStyle::from_styles(defs)
}

// ---------------------------------------------------------------------------
// the custom `diff` element (opentui DiffRenderable + syncGutters)
// ---------------------------------------------------------------------------

fn prop_str(props: &opentui_solid::props::PropMap, name: &str) -> Option<String> {
    props.iter().find(|(n, _)| n == name).and_then(|(_, v)| match v {
        PropValue::Str(s) | PropValue::Text(s) => Some(s.clone()),
        _ => None,
    })
}

fn prop_num(props: &opentui_solid::props::PropMap, name: &str) -> Option<f32> {
    props.iter().find(|(n, _)| n == name).and_then(|(_, v)| match v {
        PropValue::Num(n) => Some(*n),
        _ => None,
    })
}

fn prop_color(props: &opentui_solid::props::PropMap, name: &str) -> Option<Rgba> {
    props.iter().find(|(n, _)| n == name).and_then(|(_, v)| match v {
        PropValue::Color(s) | PropValue::Str(s) => Some(parse_color(s)),
        _ => None,
    })
}

fn build_diff_element(
    renderer: &mut opentui_core::renderer::CliRenderer,
    props: &opentui_solid::props::PropMap,
    id: &str,
    syntax: &SyntaxStyle,
) -> opentui_core::renderable::Renderable {
    let opts = DiffOptions {
        diff: prop_str(props, "diff").unwrap_or_default(),
        view: match prop_str(props, "view").as_deref() {
            Some("split") => OtDiffView::Split,
            _ => OtDiffView::Unified,
        },
        filetype: prop_str(props, "filetype"),
        // The viewer has always truncated to the pane width (the old
        // row painter clipped); `none` keeps one logical line = one
        // visual row, so the row model and the scroll anchors are
        // exact.
        wrap_mode: Some(WrapMode::None),
        conceal: false,
        fg: prop_color(props, "fg"),
        show_line_numbers: true,
        line_number_fg: prop_color(props, "lineNumberFg").unwrap_or(parse_color("#888888")),
        line_number_bg: prop_color(props, "lineNumberBg").unwrap_or(parse_color("transparent")),
        added_bg: prop_color(props, "addedBg").unwrap_or(parse_color("#1a4d1a")),
        removed_bg: prop_color(props, "removedBg").unwrap_or(parse_color("#4d1a1a")),
        context_bg: prop_color(props, "contextBg").unwrap_or(parse_color("transparent")),
        added_content_bg: None,
        removed_content_bg: None,
        context_content_bg: None,
        added_sign_color: prop_color(props, "addedSignColor").unwrap_or(parse_color("#22c55e")),
        removed_sign_color: prop_color(props, "removedSignColor").unwrap_or(parse_color("#ef4444")),
        added_line_number_bg: prop_color(props, "addedLineNumberBg")
            .unwrap_or(parse_color("transparent")),
        removed_line_number_bg: prop_color(props, "removedLineNumberBg")
            .unwrap_or(parse_color("transparent")),
        width: Dimension::Percent(1.0),
        height: Dimension::Points(prop_num(props, "rows").unwrap_or(1.0)),
    };
    let mut root = renderer.make_diff(id, &opts, syntax);
    // The LineNumber wrapper's content-background pass uses the PORT's
    // layout-relative `x`/`y` (upstream's getters are absolute), so a
    // nested Diff would stamp its row backgrounds at the canvas top.
    // The family paints those rows itself (screen-exact fills) and
    // clears the wrapper map — the gutter's own backgrounds keep
    // painting through its screen-anchored path.
    for side in &mut root.children {
        if let Content::LineNumber(state) = &mut side.content {
            state.line_colors_content.clear();
        }
    }
    let pads: Vec<u32> = ["gutterAfterLeft", "gutterAfterRight"]
        .iter()
        .filter_map(|name| prop_num(props, name))
        .map(|n| n.max(0.0) as u32)
        .collect();
    for (index, side) in root.children.iter_mut().enumerate() {
        let Some(pad) = pads.get(index) else { continue };
        let Some(gutter) = side.children.first_mut() else { continue };
        let Content::Gutter(state) = &mut gutter.content else { continue };
        state.max_after_width = *pad;
        let node = gutter.node;
        if let Some(ctx) = renderer.tree.get_node_context_mut(node) {
            if let Some(measure) = &mut ctx.gutter {
                measure.max_after_width = *pad;
            }
        }
        let _ = renderer.tree.mark_dirty(node);
    }
    root
}

fn register_diff_element(styles: &ThemeStyles) {
    // The diff body highlights through opentui's TreeSitterClient;
    // the oc-syntax catalog registration is the process-wide startup
    // seam (idempotent) — headless tests may mount before app boot.
    crate::components::syntax::register_parsers();
    let syntax = syntax_style(styles);
    extend(vec![CustomElement::new("diff-pane", move |renderer, props, id| {
        build_diff_element(renderer, props, id, &syntax)
    })]);
}

// ---------------------------------------------------------------------------
// paint-unit helpers (absolute-positioned tree nodes)
// ---------------------------------------------------------------------------

fn text_at(key: impl Into<String>, x: usize, y: isize, chunks: Vec<TextChunk>) -> Element {
    text()
        .key(key)
        .position("absolute")
        .left(x as f32)
        .top(y as f32)
        .wrap_mode("none")
        .flex_shrink(0.0)
        .prop("content", PropValue::Styled(StyledText::new(chunks)))
        .into_element()
}

fn fill_at(
    key: impl Into<String>,
    x: usize,
    y: isize,
    width: usize,
    height: usize,
    bg: Rgba,
) -> Element {
    box_()
        .key(key)
        .position("absolute")
        .left(x as f32)
        .top(y as f32)
        .width(width.max(1) as f32)
        .height(height.max(1) as f32)
        .background_color(rgba_hex(bg))
        .into_element()
}

/// The patch column's row model: the viewer's visual row count for a
/// hunk (unified = one row per patch row; split = the del/add run
/// zipped, `split_pairs`' old pairing).
fn split_row_count(rows: &[PatchRow]) -> usize {
    let mut pairs = 0usize;
    let mut pending_dels = 0usize;
    let mut pending_adds = 0usize;
    fn flush(pairs: &mut usize, dels: &mut usize, adds: &mut usize) {
        *pairs += (*dels).max(*adds);
        *dels = 0;
        *adds = 0;
    }
    for row in rows {
        match row {
            PatchRow::Context { .. } => {
                flush(&mut pairs, &mut pending_dels, &mut pending_adds);
                pairs += 1;
            }
            PatchRow::Deleted { .. } => pending_dels += 1,
            PatchRow::Added { .. } => pending_adds += 1,
        }
    }
    flush(&mut pairs, &mut pending_dels, &mut pending_adds);
    pairs
}

fn hunk_rows(hunk: &Hunk, view: DiffView) -> usize {
    match view {
        DiffView::Unified => hunk.rows.len(),
        DiffView::Split => split_row_count(&hunk.rows),
    }
}

fn digits(value: u64) -> usize {
    value.max(1).to_string().len()
}

/// One side's widest line number (0 when the side has none).
fn side_digits(rows: &[PatchRow], left: bool) -> usize {
    let mut max = 0u64;
    for row in rows {
        let value = match row {
            PatchRow::Context { old, new, .. } => {
                if left {
                    *old
                } else {
                    *new
                }
            }
            PatchRow::Deleted { old, .. } => *old,
            PatchRow::Added { new, .. } => *new,
        };
        max = max.max(value);
    }
    digits(max)
}

/// One file's `syncGutters` inputs: `(global width, max after pad)`.
/// `min_digits` is the virtual-chunk gutter floor (the whole-file line
/// count, patch-diff.tsx:44-48).
fn gutter_geometry(file: &DiffFile, hunks: &[Hunk], view: DiffView) -> (usize, usize) {
    let virtual_chunks = split_added_patch(&file.patch, VIRTUAL_CHUNK_LINES)
        .map(|chunks| chunks.iter().map(|c| c.rows).sum::<usize>())
        .filter(|total| *total > VIRTUAL_MIN_LINES);
    let min_digits = virtual_chunks.map_or(0, |total| digits(total as u64));
    let mut max_digits = min_digits;
    let mut max_after = 0usize;
    for hunk in hunks {
        let sides: &[bool] = match view {
            DiffView::Unified => &[false],
            DiffView::Split => &[true, false],
        };
        for &left in sides {
            max_digits = max_digits.max(side_digits(&hunk.rows, left));
        }
        if hunk
            .rows
            .iter()
            .any(|row| matches!(row, PatchRow::Deleted { .. } | PatchRow::Added { .. }))
        {
            max_after = 2;
        }
    }
    (max_digits, max_after)
}

// ---------------------------------------------------------------------------
// inputs / mount
// ---------------------------------------------------------------------------

/// The route body the family paints (`diff-viewer.tsx:846-872`).
pub(crate) enum DiffBody<'a> {
    Loading,
    Failed { needs_base: bool },
    Unavailable,
    Empty,
    Loaded { files: &'a [DiffFile], state: &'a DiffViewerState, options: &'a DiffViewerOptions<'a> },
}

struct Built {
    content: Element,
    hits: Vec<DiffImageHit>,
    key: String,
}

fn body_line(styles: &ThemeStyles, width: usize, height: usize, text: &str, error: bool) -> Built {
    let bg = rgba(styles.background).unwrap_or(Rgba::rgb(0, 0, 0));
    let fg = if error {
        rgba(styles.error).unwrap_or(Rgba::rgb(255, 0, 0))
    } else {
        rgba(styles.text_muted.fg.unwrap_or(styles.text.fg.unwrap_or(Color::Reset)))
            .unwrap_or(Rgba::rgb(255, 255, 255))
    };
    let content = box_()
        .key("diff-root")
        .position("absolute")
        .left(0.0)
        .top(0.0)
        .width(width.max(1) as f32)
        .height(height.max(1) as f32)
        .background_color(rgba_hex(bg))
        .children(Children::Static(vec![text_at(
            "diff-body",
            2,
            2,
            vec![chunk(text, Some(fg), Some(bg))],
        )]))
        .into_element();
    Built { content, hits: Vec::new(), key: format!("diff-body@{width}x{height}:{text}") }
}

fn default_fg(styles: &ThemeStyles) -> Rgba {
    rgba(styles.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND))
        .unwrap_or(Rgba::rgb(255, 255, 255))
}

fn build_loaded(
    files: &[DiffFile],
    state: &DiffViewerState,
    options: &DiffViewerOptions<'_>,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    scroll: usize,
) -> Built {
    let diff = &styles.diff;
    let layout = diff_layout(width, state.tree_enabled, files.len());
    let view = state.view(&layout);
    let bg = rgba(styles.background).unwrap_or(Rgba::rgb(0, 0, 0));
    let raised = rgba(styles.background_raised).unwrap_or(bg);
    let context_bg = rgba(diff.background_context).unwrap_or(bg);
    let fg_base = default_fg(styles);
    let fg_muted = rgba(styles.text_muted.fg.unwrap_or(Color::Reset)).unwrap_or(fg_base);
    let edge_y = usize::from(!layout.show_tree);
    let mut hits: Vec<DiffImageHit> = Vec::new();
    let mut children: Vec<Element> = Vec::new();

    // -- the tree pane -------------------------------------------------
    let toolbar_width = if layout.show_tree { layout.tree_width } else { width };
    let toolbar_row = usize::from(layout.show_tree);
    let toolbar_bg = if layout.show_tree { raised } else { bg };
    if layout.show_tree {
        children.push(fill_at("diff-tree", 0, 0, layout.tree_width, height, raised));
        let faint =
            tint(styles.text_muted.fg.unwrap_or(Color::Reset), styles.background_raised, 0.45);
        let rail =
            tint(styles.text_muted.fg.unwrap_or(Color::Reset), styles.background_raised, 0.7);
        let faint = rgba(faint).unwrap_or(fg_muted);
        let rail = rgba(rail).unwrap_or(fg_muted);
        for (offset, row) in build_file_tree(files).into_iter().enumerate() {
            let y = 3 + offset;
            let indent = "│ ".repeat(row.depth);
            let marker = if row.file_index.is_some() { "≡ " } else { "▾ " };
            let mut runs = Vec::new();
            if !indent.is_empty() {
                runs.push(chunk(indent.clone(), Some(rail), Some(raised)));
            }
            runs.push(chunk(marker, Some(faint), Some(raised)));
            children.push(text_at(format!("diff-tree-prefix-{offset}"), 2, y as isize, runs));
            let name_fg = if row.file_index.is_some() { fg_base } else { fg_muted };
            children.push(text_at(
                format!("diff-tree-name-{offset}"),
                2 + 2 * row.depth + 2,
                y as isize,
                vec![chunk(row.name, Some(name_fg), Some(raised))],
            ));
            if let Some(index) = row.file_index {
                let status_fg = match files[index].status {
                    FileStatus::Added => rgba(diff.text_added).unwrap_or(fg_base),
                    FileStatus::Deleted => rgba(diff.text_removed).unwrap_or(fg_base),
                    FileStatus::Modified => fg_muted,
                };
                children.push(text_at(
                    format!("diff-tree-status-{offset}"),
                    layout.tree_width.saturating_sub(3),
                    y as isize,
                    vec![chunk(
                        files[index].status.letter().to_string(),
                        Some(status_fg),
                        Some(raised),
                    )],
                ));
            }
        }
        let help_y = height.saturating_sub(3);
        children.push(text_at(
            "diff-help",
            2,
            help_y as isize,
            vec![chunk("?", Some(fg_base), Some(raised))],
        ));
        children.push(text_at(
            "diff-help-label",
            3,
            help_y as isize,
            vec![chunk(" help", Some(fg_muted), Some(raised))],
        ));
    }

    // -- the toolbar ---------------------------------------------------
    let label = options.source_label;
    children.push(text_at(
        "diff-source",
        2,
        toolbar_row as isize,
        vec![bold_chunk(label, rgba(styles.action_secondary).or(Some(fg_base)), Some(toolbar_bg))],
    ));
    let detail = format!(" · {}", options.source_detail);
    children.push(text_at(
        "diff-source-detail",
        2 + crate::wrap::display_width(label),
        toolbar_row as isize,
        vec![chunk(detail, Some(fg_muted), Some(toolbar_bg))],
    ));
    let reviewed = files.iter().filter(|file| state.reviewed.contains(&file.file)).count();
    let count = format!("{reviewed}/{}", files.len());
    children.push(text_at(
        "diff-review-count",
        toolbar_width.saturating_sub(2 + count.chars().count()),
        toolbar_row as isize,
        vec![chunk(count, Some(fg_muted), Some(toolbar_bg))],
    ));
    if !layout.show_tree {
        children.push(text_at(
            "diff-help",
            width.saturating_sub(1),
            0,
            vec![chunk("?", Some(fg_base), Some(bg))],
        ));
    }

    // -- the patch column ---------------------------------------------
    let patch_x = if layout.show_tree { layout.tree_width + 2 } else { 2 };
    let patch_w = layout.patch_pane_width;
    children.push(fill_at("diff-edge", patch_x, edge_y as isize, patch_w, 1, context_bg));
    let clip_top = edge_y + 1;
    let clip_h = height.saturating_sub(clip_top).max(1);
    let mut patch: Vec<Element> = Vec::new();
    let mut y = clip_top;
    let rel = |content_y: usize| -> isize { content_y as isize - (clip_top + scroll) as isize };

    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            y += 1; // the separator row between cards
        }
        // File header: `␣` + dir muted + basename, counts right-aligned.
        patch.push(fill_at(format!("diff-header-fill-{index}"), 0, rel(y), patch_w, 2, context_bg));
        let (dir, basename) = match file.file.rsplit_once('/') {
            Some((dir, name)) => (format!("{dir}/"), name.to_owned()),
            None => (String::new(), file.file.clone()),
        };
        patch.push(text_at(
            format!("diff-header-dir-{index}"),
            1,
            rel(y),
            vec![chunk(dir.clone(), Some(fg_muted), Some(context_bg))],
        ));
        patch.push(text_at(
            format!("diff-header-name-{index}"),
            1 + crate::wrap::display_width(&dir),
            rel(y),
            vec![chunk(basename, Some(fg_base), Some(context_bg))],
        ));
        if file.image {
            patch.push(text_at(
                format!("diff-header-image-{index}"),
                patch_w.saturating_sub(1 + 5),
                rel(y),
                vec![chunk("Image", Some(fg_muted), Some(context_bg))],
            ));
        } else {
            let added = format!("+{}", file.additions);
            let removed = format!("-{}", file.deletions);
            let counts_w = added.chars().count() + 1 + removed.chars().count();
            patch.push(text_at(
                format!("diff-header-counts-{index}"),
                patch_w.saturating_sub(1 + counts_w),
                rel(y),
                vec![
                    chunk(added, rgba(diff.text_added), Some(context_bg)),
                    chunk(" ", Some(Rgba::rgb(255, 255, 255)), Some(context_bg)),
                    chunk(removed, rgba(diff.text_removed), Some(context_bg)),
                ],
            ));
        }
        y += 2;

        let hunks = file.hunks();
        if hunks.is_empty() {
            let label_y = y;
            let previewable = file.image
                && options.source_label != "Committed"
                && file.status != FileStatus::Deleted
                && file.image_data.is_some();
            if previewable {
                let bytes = file.image_data.clone().unwrap_or_default();
                let decoded = crate::image_codec::decode_png(&bytes);
                let image_rows = (options.viewport_height / 4).clamp(3, 8);
                patch.push(text_at(
                    format!("diff-image-label-{index}"),
                    1,
                    rel(y),
                    vec![chunk("Working tree preview", Some(fg_muted), Some(context_bg))],
                ));
                y += 2;
                if let Some(image) = decoded.as_ref() {
                    let inner_w = patch_w.saturating_sub(2).max(1);
                    let (cols, rows) = crate::image_view::fit_cells(
                        image.width,
                        image.height,
                        inner_w,
                        image_rows,
                    );
                    let cells = crate::image_view::blocks_cells(image, cols, rows);
                    let offset_x = 1 + inner_w.saturating_sub(cols) / 2;
                    for (line_index, line) in cells.iter().enumerate() {
                        let runs = line
                            .iter()
                            .map(|cell| TextChunk {
                                text: cell.glyph.to_string(),
                                fg: Some(Rgba::rgb(cell.fg[0], cell.fg[1], cell.fg[2])),
                                bg: Some(Rgba::rgb(cell.bg[0], cell.bg[1], cell.bg[2])),
                                attributes: 0,
                                link: None,
                            })
                            .collect();
                        patch.push(text_at(
                            format!("diff-image-row-{index}-{line_index}"),
                            offset_x,
                            rel(y + line_index),
                            runs,
                        ));
                    }
                    let size = format!("{}x{}", image.width, image.height);
                    patch.push(text_at(
                        format!("diff-image-size-{index}"),
                        1,
                        rel(y + image_rows),
                        vec![chunk(size, Some(fg_muted), Some(context_bg))],
                    ));
                    let mime = image_mime(&file.file);
                    let encoded = crate::clipboard::base64_encode::encode(&bytes);
                    let uri = format!("data:{mime};base64,{encoded}");
                    hits.push((
                        patch_x + 1,
                        label_y.saturating_sub(scroll),
                        patch_w.saturating_sub(2),
                        image_rows + 3,
                        uri,
                    ));
                    y += image_rows + 2;
                } else {
                    patch.push(text_at(
                        format!("diff-image-error-{index}"),
                        1,
                        rel(y),
                        vec![chunk(
                            "Could not load image",
                            Some(rgba(styles.error).unwrap_or(fg_base)),
                            Some(context_bg),
                        )],
                    ));
                    y += 2;
                }
                continue;
            }
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
            patch.push(text_at(
                format!("diff-fallback-{index}"),
                1,
                rel(y),
                vec![chunk(text, Some(fg_muted), Some(context_bg))],
            ));
            y += 1;
            continue;
        }

        let (max_digits, max_after) = gutter_geometry(file, &hunks, view);
        for (hunk_index, hunk) in hunks.iter().enumerate() {
            if hunk.old_start != 1 {
                let header = format!(" {}", hunk.header);
                patch.push(fill_at(
                    format!("diff-hunk-fill-{index}-{hunk_index}"),
                    0,
                    rel(y),
                    patch_w,
                    1,
                    context_bg,
                ));
                patch.push(text_at(
                    format!("diff-hunk-{index}-{hunk_index}"),
                    0,
                    rel(y),
                    vec![chunk(header, rgba(diff.hunk_header), Some(context_bg))],
                ));
                y += 1;
            }
            let rows = hunk_rows(hunk, view);
            let (left_digits, right_digits) = match view {
                DiffView::Unified => (side_digits(&hunk.rows, false), 0),
                DiffView::Split => (side_digits(&hunk.rows, true), side_digits(&hunk.rows, false)),
            };
            let pad = |side_digits: usize| -> u32 {
                (max_after + max_digits.saturating_sub(side_digits)) as u32
            };
            let view_name = match view {
                DiffView::Unified => "unified",
                DiffView::Split => "split",
            };
            let mut diff_el = custom("diff-pane")
                .key(format!("diff-pane-{index}-{hunk_index}"))
                .width(patch_w as f32)
                .height(rows.max(1) as f32)
                .prop("diff", hunk_patch(hunk, &file.patch))
                .prop("view", view_name.to_owned())
                .prop("rows", rows.max(1) as f32)
                .prop("gutterAfterLeft", pad(left_digits));
            if view == DiffView::Split {
                diff_el = diff_el.prop("gutterAfterRight", pad(right_digits));
            }
            if let Some(filetype) = crate::components::syntax::filetype_for_path(&file.file) {
                diff_el = diff_el.prop("filetype", filetype.to_owned());
            }
            let fg =
                rgba(styles.text.fg.unwrap_or(crate::components::tool_rows::DEFAULT_FOREGROUND));
            let color_props: [(&str, Option<String>); 9] = [
                ("fg", fg.map(rgba_hex)),
                ("lineNumberFg", hex(diff.line_number)),
                ("lineNumberBg", hex(diff.background_context)),
                ("addedBg", hex(diff.background_added)),
                ("removedBg", hex(diff.background_removed)),
                ("contextBg", hex(diff.background_context)),
                ("addedSignColor", hex(diff.highlight_added)),
                ("removedSignColor", hex(diff.highlight_removed)),
                ("addedLineNumberBg", hex(diff.line_number_added)),
            ];
            for (name, value) in color_props {
                if let Some(value) = value {
                    diff_el = diff_el.prop(name, value);
                }
            }
            if let Some(value) = hex(diff.line_number_removed) {
                diff_el = diff_el.prop("removedLineNumberBg", value);
            }
            let backgrounds = row_backgrounds(hunk, view, diff);
            let pane = patch_w / 2;
            for (row_index, sides) in backgrounds.iter().enumerate() {
                if sides.len() == 1 {
                    patch.push(fill_at(
                        format!("diff-row-fill-{index}-{hunk_index}-{row_index}"),
                        0,
                        rel(y + row_index),
                        patch_w,
                        1,
                        sides[0],
                    ));
                } else {
                    patch.push(fill_at(
                        format!("diff-row-fill-l-{index}-{hunk_index}-{row_index}"),
                        0,
                        rel(y + row_index),
                        pane,
                        1,
                        sides[0],
                    ));
                    patch.push(fill_at(
                        format!("diff-row-fill-r-{index}-{hunk_index}-{row_index}"),
                        pane,
                        rel(y + row_index),
                        patch_w - pane,
                        1,
                        sides[1],
                    ));
                }
            }
            patch.push(
                box_()
                    .key(format!("diff-pane-box-{index}-{hunk_index}"))
                    .position("absolute")
                    .left(0.0)
                    .top(rel(y) as f32)
                    .width(patch_w as f32)
                    .height(rows.max(1) as f32)
                    .child(diff_el)
                    .into_element(),
            );
            y += rows;
        }
    }
    children.push(
        box_()
            .key("diff-clip")
            .position("absolute")
            .left(patch_x as f32)
            .top(clip_top as f32)
            .width(patch_w as f32)
            .height(clip_h as f32)
            .overflow("hidden")
            .background_color(rgba_hex(bg))
            .children(Children::Static(patch))
            .into_element(),
    );

    let content = box_()
        .key("diff-root")
        .position("absolute")
        .left(0.0)
        .top(0.0)
        .width(width.max(1) as f32)
        .height(height.max(1) as f32)
        .background_color(rgba_hex(bg))
        .children(Children::Static(children))
        .into_element();

    let mut hasher = DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    scroll.hash(&mut hasher);
    state.tree_enabled.hash(&mut hasher);
    state.single_patch.hash(&mut hasher);
    state.selected_file.hash(&mut hasher);
    state.reviewed.hash(&mut hasher);
    format!("{view:?}").hash(&mut hasher);
    options.source_label.hash(&mut hasher);
    options.source_detail.hash(&mut hasher);
    files.len().hash(&mut hasher);
    for file in files {
        file.file.hash(&mut hasher);
        file.patch.hash(&mut hasher);
        file.status.letter().hash(&mut hasher);
        file.additions.hash(&mut hasher);
        file.deletions.hash(&mut hasher);
    }
    Built {
        content,
        hits,
        key: format!("diff-content@{width}x{height}@{scroll}@{:x}", hasher.finish()),
    }
}

/// The diff rows' CONTENT backgrounds (the wrapper's cleared
/// `line_colors_content` pass, restored as screen-exact fills behind
/// the DiffRenderable). One entry per visual row; unified carries one
/// side, split carries `[left, right]` (an empty pairing side is the
/// card's context color).
fn row_backgrounds(hunk: &Hunk, view: DiffView, diff: &crate::theme::DiffStyles) -> Vec<Vec<Rgba>> {
    let context = rgba(diff.background_context).unwrap_or(Rgba::rgb(0, 0, 0));
    let removed = rgba(diff.background_removed).unwrap_or(context);
    let added = rgba(diff.background_added).unwrap_or(context);
    match view {
        DiffView::Unified => hunk
            .rows
            .iter()
            .map(|row| {
                vec![match row {
                    PatchRow::Deleted { .. } => removed,
                    PatchRow::Added { .. } => added,
                    PatchRow::Context { .. } => context,
                }]
            })
            .collect(),
        DiffView::Split => {
            let mut out: Vec<Vec<Rgba>> = Vec::new();
            let mut dels: Vec<Rgba> = Vec::new();
            let mut adds: Vec<Rgba> = Vec::new();
            let flush = |out: &mut Vec<Vec<Rgba>>, dels: &mut Vec<Rgba>, adds: &mut Vec<Rgba>| {
                for index in 0..dels.len().max(adds.len()) {
                    out.push(vec![
                        dels.get(index).copied().unwrap_or(context),
                        adds.get(index).copied().unwrap_or(context),
                    ]);
                }
                dels.clear();
                adds.clear();
            };
            for row in &hunk.rows {
                match row {
                    PatchRow::Context { .. } => {
                        flush(&mut out, &mut dels, &mut adds);
                        out.push(vec![context, context]);
                    }
                    PatchRow::Deleted { .. } => dels.push(removed),
                    PatchRow::Added { .. } => adds.push(added),
                }
            }
            flush(&mut out, &mut dels, &mut adds);
            out
        }
    }
}

/// The hunk's own patch text: the file prefix (`---`/`+++`) plus this
/// hunk's body, the ref's `splitPatchHunks` slice (util/diff.ts:40-56).
fn hunk_patch(hunk: &Hunk, file_patch: &str) -> String {
    // The prefix is everything before the first hunk header; the body
    // is the header line plus the rows' original markers.
    let prefix_end = file_patch.find("\n@@ ").map_or(0, |at| at + 1);
    let prefix = &file_patch[..prefix_end.min(file_patch.len())];
    let mut text = String::from(prefix);
    text.push_str("@@ ");
    text.push_str(hunk.header.strip_prefix("@@ ").unwrap_or(&hunk.header));
    text.push('\n');
    for row in &hunk.rows {
        match row {
            PatchRow::Context { text: body, .. } => {
                text.push(' ');
                text.push_str(body);
            }
            PatchRow::Deleted { text: body, .. } => {
                text.push('-');
                text.push_str(body);
            }
            PatchRow::Added { text: body, .. } => {
                text.push('+');
                text.push_str(body);
            }
        }
        text.push('\n');
    }
    text
}

// ---------------------------------------------------------------------------
// the mount entry
// ---------------------------------------------------------------------------

thread_local! {
    static DIFF_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn build(
    body: DiffBody<'_>,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    scroll: usize,
) -> Built {
    match body {
        DiffBody::Loading => body_line(styles, width, height, "Loading diff…", false),
        DiffBody::Failed { needs_base } => body_line(
            styles,
            width,
            height,
            if needs_base {
                "Could not load diff. Choose a base branch from Diff source, or select Uncommitted."
            } else {
                "Could not load diff. Reopen the diff viewer to try again."
            },
            true,
        ),
        DiffBody::Unavailable => body_line(
            styles,
            width,
            height,
            "Committed comparison unavailable without base metadata. Choose a base branch from Diff source.",
            false,
        ),
        DiffBody::Empty => body_line(styles, width, height, "No changes to show", false),
        DiffBody::Loaded { files, state, options } => {
            build_loaded(files, state, options, styles, width, height, scroll)
        }
    }
}

/// Mount (or sync) the diff route's family subtree into the shared
/// tree. Returns the inline image hit rects (screen coordinates).
pub(crate) fn mount_diff_route(
    body: DiffBody<'_>,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    scroll: usize,
) -> Vec<DiffImageHit> {
    register_diff_element(styles);
    let width = width.max(1);
    let height = height.max(1);
    let built = build(body, styles, width, height, scroll);
    let rebuild = !family_wrapper_live("diff")
        || DIFF_KEY.with_borrow(|slot| slot.as_deref() != Some(built.key.as_str()));
    let (content, content_key) = if rebuild {
        DIFF_KEY.with_borrow_mut(|slot| *slot = Some(built.key.clone()));
        (Some(built.content), Some(built.key))
    } else {
        (None, None)
    };
    mount_family_wrapper(
        "diff",
        DIFF_SKELETON_INDEX,
        0,
        0,
        width,
        height,
        content_key,
        content,
        &[],
        &[],
        false,
    );
    built.hits
}

/// The headless twin (the golden/unit harness shape): mount the
/// loaded viewer into a fresh hub and read the pixels back through the
/// one composite seam — the bytes `render_diff_viewer` returns.
pub(crate) fn twin_loaded(
    files: &[DiffFile],
    state: &DiffViewerState,
    options: &DiffViewerOptions<'_>,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> (Frame, Vec<DiffImageHit>) {
    reset_hub();
    DIFF_KEY.with_borrow_mut(|slot| *slot = None);
    let hits =
        mount_diff_route(DiffBody::Loaded { files, state, options }, styles, width, height, 0);
    let mut frame = blank_frame(width.max(1), height.max(1));
    composite_chrome_tree(&mut frame);
    (frame, hits)
}

/// The route's frame source: mount with the live scroll and compose
/// the family pixels into the route's blank base.
pub(crate) fn route_frame(
    body: DiffBody<'_>,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
    scroll: usize,
) -> (Frame, Vec<DiffImageHit>) {
    let hits = mount_diff_route(body, styles, width, height, scroll);
    let mut frame = blank_frame(width.max(1), height.max(1));
    composite_chrome_tree(&mut frame);
    (frame, hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::diff_viewer::{diff_viewer_fixture, DiffViewerState};
    use crate::theme::ThemeState;

    #[test]
    fn split_row_count_matches_the_pairing() {
        let rows = diff_viewer_fixture()[0].hunks().remove(0).rows;
        assert_eq!(split_row_count(&rows), 3, "context, del/add pair, context");
    }

    #[test]
    fn hunk_patch_rebuilds_the_ref_hunk_slice() {
        let file = diff_viewer_fixture().remove(0);
        let hunks = file.hunks();
        let patch = hunk_patch(&hunks[1], &file.patch);
        assert!(patch.starts_with("--- a/src/file.txt\n+++ b/src/file.txt\n@@ -20,3 +20,3 @@\n"));
        assert!(patch.contains("\n const second = true\n"));
        assert!(patch.contains("-const oldSecond = true\n"));
        assert!(patch.contains("+const newSecond = true\n"));
        assert!(!patch.contains("newFirst"), "only this hunk's body");
    }

    #[test]
    fn gutter_geometry_pads_small_hunks_to_the_file_width() {
        let file = diff_viewer_fixture().remove(0);
        let hunks = file.hunks();
        let (max_digits, max_after) = gutter_geometry(&file, &hunks, DiffView::Unified);
        assert_eq!((max_digits, max_after), (2, 2));
    }

    #[test]
    fn twin_render_matches_the_pinned_rows() {
        let theme = ThemeState::v2_dark().styles();
        let (frame, hits) = twin_loaded(
            &diff_viewer_fixture(),
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme,
            80,
            24,
        );
        let grid = frame.text_grid();
        assert!(grid[0].starts_with("  All · vs v2"));
        assert!(grid[4].starts_with("   1    const first = true"), "row 4: {:?}", grid[4]);
        assert!(grid[5].starts_with("   2 -  const oldFirst = true"), "row 5: {:?}", grid[5]);
        assert!(hits.is_empty());
    }
}
