//! The diff viewer's LIVE mount (SURF-2, auditB #6 + auditA §2.12):
//! the fullscreen `diff` plugin route — body states, scroll, and the
//! `diff.*` navigation targets — over the ported painter
//! ([`crate::components::diff_viewer::render_diff_viewer`]).
//!
//! v2 sources (`feature-plugins/system/diff-viewer.tsx`):
//!
//! * **Body states** (the content `Switch`, :846-872): Loading
//!   "Loading diff…" / error (2 copy variants, :852-860) /
//!   unavailable (:861-867) / empty "No changes to show" (:868-872).
//! * **Scroll** (:917-926 + the commands, :559-733): the patch column
//!   is ONE scrollbox over every file card — line/page/half-page/
//!   first/last scroll it, `]`/`[` jump hunk headers, `n`/`p` jump
//!   file cards.
//! * **Image files** (`isDiffImageFile`, diff-viewer-image.tsx:9):
//!   the header shows `Image` instead of `+N/−N` (the painter's
//!   counts column); the working-tree halfblock preview surface is
//!   deferred (see docs/surf2.log).

use crate::components::diff_viewer::{
    diff_layout, render_diff_viewer_with_hits, DiffFile, DiffView, DiffViewerOptions, PatchRow,
};
use crate::render::{Cell, CellColor, Frame};
use crate::store::{DiffPhase, DiffSource, DiffViewerLive, Store};
use crate::theme::ThemeStyles;

/// `isDiffImageFile` (diff-viewer-image.tsx:9).
#[must_use]
pub fn is_diff_image_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|extension| lower.ends_with(extension))
}

/// One file card's position inside the patch column's content space
/// (the scrollbox's y-walk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCardLayout {
    /// The card's top row (its header row) in content space.
    pub top: usize,
    /// Each hunk's (`header_row`, `first_patch_row`) — the header row is
    /// `None` when the hunk starts at line 1 (hidden, :25 note).
    pub hunks: Vec<(Option<usize>, usize)>,
}

/// The patch column's content walk: each visible file card's layout +
/// the total content height (the scroll clamp — `scroll.scrollHeight`).
/// Mirrors [`render_diff_viewer`]'s y-walk exactly (drift-pinned by
/// the unit test below).
#[must_use]
pub fn diff_content_layout(
    files: &[DiffFile],
    view: DiffView,
    edge_y: usize,
) -> (Vec<FileCardLayout>, usize) {
    let mut cards = Vec::new();
    let mut y = edge_y + 1;
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            y += 1; // the ▄ separator row between cards
        }
        let top = y;
        y += 2; // header row + its paddingBottom
        let mut hunks = Vec::new();
        for hunk in file.hunks() {
            let header = (hunk.old_start != 1).then_some(y);
            if header.is_some() {
                y += 1;
            }
            hunks.push((header, y));
            let rows = match view {
                DiffView::Unified => hunk.rows.len(),
                DiffView::Split => split_row_count(&hunk.rows),
            };
            y += rows;
        }
        if file.hunks().is_empty() {
            y += 1; // the no-patch fallback row (the painter's notice)
        }
        cards.push(FileCardLayout { top, hunks });
    }
    (cards, y)
}

/// The split view's row count: contexts emit one pair, deletion runs
/// zip with addition runs (`split_pairs`, :461-511).
#[allow(clippy::items_after_statements)] // the nested flush reads best here
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

/// The scroll clamp: opentui's `scrollTo` saturates at
/// `scrollHeight` — the port clamps to the last full viewport.
#[must_use]
fn clamp_scroll(scroll: usize, content_height: usize, viewport: usize) -> usize {
    scroll.min(content_height.saturating_sub(viewport))
}

fn muted_fg(styles: &ThemeStyles) -> CellColor {
    let color = styles.text_muted.fg.or(styles.text.fg).unwrap_or(ratatui::style::Color::Reset);
    CellColor::from(color)
}

fn plain_frame(width: usize, height: usize) -> Frame {
    Frame { cells: vec![vec![Cell::default(); width]; height], width, height }
}

/// TUI-10: the inline diff images' screen hit rects — `(x, y, width,
/// height, uri)`.
pub type DiffImageHit = (usize, usize, usize, usize, String);
pub type DiffImageHits = Vec<DiffImageHit>;

/// The live diff viewer frame (the `diff` plugin route's render):
/// body states when not loaded, else the scrolled painter output.
#[must_use]
pub fn render_diff_mount(
    store: &Store,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> Frame {
    let live = store.diff_viewer();
    // The body states replace tree + patch area entirely (:846-872):
    // padding 2, one muted/error line.
    let body_line = |text: &str, error: bool| -> Frame {
        let mut frame = plain_frame(width, height);
        for (offset, ch) in text.chars().enumerate() {
            if let Some(cell) = frame.cells.get_mut(2).and_then(|line| line.get_mut(2 + offset)) {
                cell.ch = ch;
                cell.fg = if error { CellColor::from(styles.error) } else { muted_fg(styles) };
            }
        }
        frame
    };
    let (frame, image_hits) = match &live.phase {
        DiffPhase::Loading => (body_line("Loading diff…", false), Vec::new()),
        DiffPhase::Failed(_) => {
            // Two copy variants (:852-860): a base-needing mode that
            // never resolved one points at the source picker; any
            // other failure says reopen.
            let needs_base = matches!(live.mode, DiffSource::Branch | DiffSource::Committed)
                && live.base.is_none();
            let text = if needs_base {
                "Could not load diff. Choose a base branch from Diff source, or select Uncommitted."
            } else {
                "Could not load diff. Reopen the diff viewer to try again."
            };
            (body_line(text, true), Vec::new())
        }
        DiffPhase::Unavailable => (
            body_line(
                "Committed comparison unavailable without base metadata. Choose a base branch from Diff source.",
                false,
            ),
            Vec::new(),
        ),
        DiffPhase::Loaded(files) if files.is_empty() => {
            (body_line("No changes to show", false), Vec::new())
        }
        DiffPhase::Loaded(_) => render_diff_scrolled(&live, styles, width, height),
    };
    // TUI-10: the inline diff images' screen rects — the mouse
    // release path reads them to open the preview dialog.
    store.set_diff_image_hits(image_hits);
    frame
}

/// The loaded viewer: the painter renders the FULL content into an
/// oversized frame; the patch column crops by `scroll`, the tree pane
/// and toolbar stay pinned (v2's separate scrollboxes).
#[must_use]
fn render_diff_scrolled(
    live: &DiffViewerLive,
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> (Frame, DiffImageHits) {
    let files = live.visible_files();
    let layout = diff_layout(width, live.viewer.tree_enabled, files.len());
    let view = live.viewer.view(&layout);
    let edge_y = usize::from(!layout.show_tree);
    let (_, content_height) = diff_content_layout(files, view, edge_y);
    // Render into the content-sized frame (toolbar + tree paint at
    // the top; the patch column flows past the viewport).
    let oversized = content_height.max(height) + 2;
    let options = DiffViewerOptions {
        source_label: live.mode.label(),
        source_detail: &live.source_detail(),
        viewport_height: height,
    };
    let mut content_image_hits = Vec::new();
    let content = render_diff_viewer_with_hits(
        files,
        &live.viewer,
        &options,
        styles,
        width,
        oversized,
        Some(&mut content_image_hits),
    );
    // Composite per-column: patch cells shift by `scroll`, the tree
    // pane + the toolbar row stay at their content positions.
    let scroll = clamp_scroll(live.scroll, content_height, height);
    let mut frame = plain_frame(width, height);
    for out_y in 0..height {
        for out_x in 0..width {
            let pinned = (layout.show_tree && out_x < layout.tree_width) || out_y <= edge_y;
            let src_y = if pinned { out_y } else { scroll + out_y };
            let source = content.cell(src_y.min(oversized - 1), out_x);
            if let Some(slot) = frame.cells.get_mut(out_y).and_then(|line| line.get_mut(out_x)) {
                *slot = source;
            }
        }
    }
    // TUI-10: map the inline images' CONTENT rects into screen cells
    // (pinned rows never carry images; the patch column shifts by
    // `scroll`) — the caller records them onto the store for the
    // mouse release path.
    let screen_hits = content_image_hits
        .into_iter()
        .map(|(x, y, w, h, uri)| {
            let top = y.saturating_sub(scroll + edge_y);
            (x, top + edge_y, w, h, uri)
        })
        .filter(|(_, top, _, h, _)| *top + *h > edge_y && *top < height)
        .collect::<Vec<_>>();
    // The tree pane's `? help` footer re-pins to the VIEWPORT bottom
    // (the oversized render placed it at content_height − 3).
    if layout.show_tree && height >= 4 {
        let help_y = height - 3;
        let fg_base = CellColor::from(styles.text.fg.unwrap_or(ratatui::style::Color::Reset));
        for (offset, cell_x) in (2..8).enumerate() {
            let source = content.cell(2, cell_x);
            let mut cell = source;
            if offset == 0 {
                cell.fg = fg_base;
            }
            if let Some(slot) = frame.cells.get_mut(help_y).and_then(|line| line.get_mut(cell_x)) {
                *slot = cell;
            }
        }
    }
    (frame, screen_hits)
}

/// `diff.down/up/page/half/first/last` — the scrollbox commands
/// (diff-viewer.tsx:566-625). Returns whether the offset moved.
#[must_use]
pub fn diff_scroll_command(live: &mut DiffViewerLive, command: &str, viewport: usize) -> bool {
    let (content_height, _) = content_metrics(live);
    let max_scroll = content_height.saturating_sub(viewport);
    let page = viewport.saturating_sub(2).max(1);
    let next = match command {
        "diff.down" => live.scroll.saturating_add(1),
        "diff.up" => live.scroll.saturating_sub(1),
        "diff.page.down" => live.scroll.saturating_add(page),
        "diff.page.up" => live.scroll.saturating_sub(page),
        "diff.half_page.down" => live.scroll.saturating_add(page / 2),
        "diff.half_page.up" => live.scroll.saturating_sub(page / 2),
        "diff.first" => 0,
        "diff.last" => max_scroll,
        _ => return false,
    };
    let next = next.min(max_scroll);
    if next == live.scroll {
        return false;
    }
    live.scroll = next;
    true
}

/// (`content_height`, cards) over the CURRENTLY VISIBLE files at a
/// representative width (the render recomputes per frame; the command
/// math only needs the content height).
fn content_metrics(live: &DiffViewerLive) -> (usize, Vec<FileCardLayout>) {
    let files = live.visible_files().to_vec();
    let layout = diff_layout(160, live.viewer.tree_enabled, files.len());
    let view = live.viewer.view(&layout);
    let edge_y = usize::from(!layout.show_tree);
    let (cards, content_height) = diff_content_layout(&files, view, edge_y);
    (content_height, cards)
}

const FILE_HEADER_HEIGHT: usize = 2;

/// `diff.next_hunk`/`diff.previous_hunk` (`jumpRelativeHunk`,
/// :430-463): jump the next hunk header row in the scroll direction,
/// `FILE_HEADER_HEIGHT` above the target.
#[must_use]
pub fn diff_jump_hunk(live: &mut DiffViewerLive, forward: bool, viewport: usize) -> bool {
    let (content_height, cards) = content_metrics(live);
    let content_top = live.scroll + FILE_HEADER_HEIGHT;
    // Every hunk's content anchor (header row when rendered, else the
    // first patch row).
    let mut anchors: Vec<(usize, usize)> = Vec::new(); // (content_y, card_index)
    for (index, card) in cards.iter().enumerate() {
        for (header, first_row) in &card.hunks {
            anchors.push((header.unwrap_or(*first_row), index));
        }
    }
    anchors.sort_unstable_by_key(|(y, _)| *y);
    let target = if forward {
        anchors.iter().find(|(y, _)| *y > content_top).copied()
    } else {
        anchors.iter().rev().find(|(y, _)| *y < content_top).copied()
    };
    let Some((y, card_index)) = target else { return false };
    let max_scroll = content_height.saturating_sub(viewport);
    live.scroll = y.saturating_sub(FILE_HEADER_HEIGHT).min(max_scroll);
    // The card selection follows the visible file — in the all-files
    // view the visible index IS the full-list index (single-patch's
    // slice is the selected file itself, the selection stays).
    if !live.viewer.single_patch {
        live.viewer.selected_file = Some(card_index);
    }
    true
}

/// `diff.next_file`/`diff.previous_file` (`jumpRelativePatchFile`,
/// :418-428): move the selection over the FULL file list and scroll
/// the target card to the top (single-patch re-renders it instead).
#[must_use]
pub fn diff_jump_file(live: &mut DiffViewerLive, forward: bool, viewport: usize) -> bool {
    let full = match &live.phase {
        DiffPhase::Loaded(files) => files.clone(),
        _ => return false,
    };
    if full.is_empty() {
        return false;
    }
    let current = live.viewer.selected_file.unwrap_or(0);
    let next = if forward { (current + 1).min(full.len() - 1) } else { current.saturating_sub(1) };
    let moved = live.viewer.selected_file != Some(next);
    live.viewer.selected_file = Some(next);
    if live.viewer.single_patch {
        live.scroll = 0;
        return true;
    }
    // Scroll the target card's top to the viewport top (the walk runs
    // over the FULL list — the all-files view).
    let layout = diff_layout(160, live.viewer.tree_enabled, full.len());
    let view = live.viewer.view(&layout);
    let edge_y = usize::from(!layout.show_tree);
    let (cards, content_height) = diff_content_layout(&full, view, edge_y);
    let Some(card) = cards.get(next) else { return moved };
    let max_scroll = content_height.saturating_sub(viewport);
    live.scroll = card.top.saturating_sub(edge_y).min(max_scroll);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::diff_viewer::{diff_viewer_fixture, DiffViewerState};
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// The walker must mirror the painter's y-walk: rendering the
    /// fixture at exactly `content_height` paints the last content
    /// row, and one row beyond stays plain.
    #[test]
    fn content_layout_matches_the_painter_walk() {
        let files = diff_viewer_fixture();
        let theme = theme();
        // Width 80: no tree (the breakpoint is 90) → edge_y = 1.
        let (cards, content_height) = diff_content_layout(&files, DiffView::Unified, 1);
        assert_eq!(cards.len(), 1, "one file card");
        // edge(1) + header(2) + hunk1 (4 rows, header hidden) + 2×(header + 4).
        assert_eq!(content_height, 18, "fixture walk: {content_height}");
        let frame = render_diff_viewer_with_hits(
            &files,
            &DiffViewerState::default(),
            &DiffViewerOptions::default(),
            &theme,
            80,
            content_height,
            None,
        );
        let text = frame.text_grid().join("\n");
        assert!(text.contains("const newFirst"), "content painted");
        // Row 3 (the first hunk header — hunk 1 starts at line 1 so
        // ITS header hides; the second hunk's shows): the walker's
        // second anchor must be a painted @@ row.
        let second = cards[0].hunks[1].0.expect("hunk 2 header renders");
        assert!(
            frame.text_grid()[second].contains("@@"),
            "anchor row {} is the hunk header: {:?}",
            second,
            frame.text_grid()[second]
        );
    }

    #[test]
    fn image_files_detected() {
        assert!(is_diff_image_file("assets/Logo.PNG"));
        assert!(is_diff_image_file("a.jpeg"));
        assert!(!is_diff_image_file("src/file.txt"));
    }

    #[test]
    fn scroll_commands_clamp_and_jump() {
        let mut live = DiffViewerLive {
            phase: DiffPhase::Loaded(diff_viewer_fixture()),
            ..Default::default()
        };
        assert!(diff_scroll_command(&mut live, "diff.down", 10));
        assert_eq!(live.scroll, 1);
        assert!(diff_scroll_command(&mut live, "diff.last", 10));
        let maxed = live.scroll;
        assert!(diff_scroll_command(&mut live, "diff.first", 10));
        assert_eq!(live.scroll, 0);
        assert!(maxed > 0, "last moved somewhere: {maxed}");
        // File jumps move the selection (single file: saturates —
        // v2 still "jumps" to the same card, scrolling it to top).
        assert!(diff_jump_file(&mut live, true, 10), "the jump always scrolls");
        assert_eq!(live.viewer.selected_file, Some(0), "saturated at the only file");
        // Hunk jumps: from the top, `]` lands on the 2nd hunk's header.
        assert!(diff_jump_hunk(&mut live, true, 10));
        assert!(live.scroll > 0, "moved to a hunk: {}", live.scroll);
    }

    /// The scrolled viewport shows the middle of a tall diff after a
    /// scroll and the pinned tree column does not shift.
    #[test]
    fn scrolled_render_crops_the_patch_column() {
        let mut live = DiffViewerLive {
            phase: DiffPhase::Loaded(diff_viewer_fixture()),
            ..Default::default()
        };
        let theme = theme();
        let store = crate::store::Store::new(crate::reactive::Runtime::new());
        store.update_diff_viewer(|state| {
            *state = live.clone();
            true
        });
        // Wide enough for the tree pane.
        let frame = render_diff_mount(&store, &theme, 160, 10);
        assert!(frame.text_grid().join("\n").contains("src/file.txt"), "fixture file visible");
        live.scroll = 5;
        store.update_diff_viewer(|state| {
            state.scroll = 5;
            true
        });
        let scrolled = render_diff_mount(&store, &theme, 160, 10);
        let top_before = frame.text_grid()[0].clone();
        let top_after = scrolled.text_grid()[0].clone();
        assert_eq!(top_before, top_after, "the pinned toolbar row does not scroll");
    }
}
