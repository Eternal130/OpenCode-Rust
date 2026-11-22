//! TUI-16 — mouse text selection → clipboard (util/selection.ts:44-98
//! with app.tsx:546-571): a left-drag over screen cells captures a
//! selection; on release `terminal.copy = "select"` (the non-win32
//! default) writes the selected text to the clipboard with a
//! `Copied to clipboard` toast. `"manual"` (the win32 default) keeps
//! the selection for an explicit copy: ctrl+y (the renderer console's
//! `copy-selection` binding, app.tsx:243) or ctrl+c
//! (selection.ts:82-91), and any other stroke clears it.

use crate::render::{CellColor, Frame};

/// `SelectionBehavior` (types.d.ts:35): `"cell" | "word" | "line"` —
/// the drag model's endpoint extension mode. Cell is the plain anchor
/// → focus rect; word/line extend EACH endpoint through the
/// renderer's native `selectWord`/`selectLine` (the union model of
/// `expandSelectionRange`, text-buffer-view.zig:871-891): the anchor
/// endpoint expands at press, the focus endpoint on every drag, and
/// the selection is the union of the two expanded endpoints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectionBehavior {
    #[default]
    Cell,
    Word,
    Line,
}

/// `nextClickBehavior` (renderer.d.ts:617's private helper): the
/// click-count → behavior cycle. The ref's renderer tracks multi
/// clicks natively; the canonical model (and the one the port pins)
/// cycles cell → word → line → cell… on rapid same-cell presses.
#[must_use]
pub fn behavior_for_click_count(count: u32) -> SelectionBehavior {
    match count % 3 {
        1 => SelectionBehavior::Cell,
        2 => SelectionBehavior::Word,
        _ => SelectionBehavior::Line,
    }
}

/// `isWordCodepoint` (utf8.zig:270-272 via editor.rs:1387): ASCII
/// word bytes AND CJK word codepoints — the `selectWord` boundary
/// rule the endpoints extend over.
fn is_word_codepoint(ch: char) -> bool {
    if (ch as u32) <= 0x7F {
        return ch.is_ascii_lowercase()
            || ch.is_ascii_uppercase()
            || ch.is_ascii_digit()
            || ch == '_';
    }
    opentui_core::text_buffer::is_cjk_word_codepoint(ch as u32)
}

/// The row's text extent: one past the last non-blank cell (the
/// char-range model's line end — `coordsToCharOffset`'s clamp).
fn row_extent(row: &[crate::render::Cell]) -> usize {
    row.iter().rposition(|cell| cell.ch != ' ').map_or(0, |at| at + 1)
}

/// The word span (exclusive end) around column `x` on row `y`, when
/// the cell lands on a word codepoint — `selectWord`'s both-direction
/// walk (`get_prev_word_boundary`/`get_next_word_boundary`,
/// edit-buffer.zig:804-946). Blank cells select nothing.
fn word_at(frame: &Frame, x: usize, y: usize) -> Option<(usize, usize)> {
    let row = frame.cells.get(y)?;
    let x = x.min(row.len().saturating_sub(1));
    if !row.get(x).is_some_and(|cell| is_word_codepoint(cell.ch)) {
        return None;
    }
    let mut start = x;
    while start > 0 && row.get(start - 1).is_some_and(|cell| is_word_codepoint(cell.ch)) {
        start -= 1;
    }
    let mut end = x + 1;
    while row.get(end).is_some_and(|cell| is_word_codepoint(cell.ch)) {
        end += 1;
    }
    Some((start, end))
}

/// A resolved selection: the bounding rect plus the per-endpoint
/// row clamps (the first row's highlight starts at `first_row_start`,
/// the last row's ends at `last_row_end` — interior rows span the
/// full rect width; `usize::MAX` = to the row's text extent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSelection {
    pub rect: SelectionRect,
    pub first_row_start: usize,
    pub last_row_end: usize,
}

/// A normalized cell rect (inclusive corners).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl SelectionRect {
    fn new(anchor: (usize, usize), head: (usize, usize)) -> Self {
        let x0 = anchor.0.min(head.0);
        let x1 = anchor.0.max(head.0);
        let y0 = anchor.1.min(head.1);
        let y1 = anchor.1.max(head.1);
        Self { x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 }
    }

    /// Whether `cell` sits inside the rect.
    #[must_use]
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// The drag state machine (`renderer.getSelection()`'s cell model):
/// press arms the anchor, a drag that crosses cells marks the
/// selection, the release resolves it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenSelection {
    anchor: Option<(usize, usize)>,
    head: Option<(usize, usize)>,
    dragged: bool,
    /// Whether the release KEPT the selection (manual copy mode —
    /// v2's renderer keeps the painted selection until the next
    /// stroke).
    retained: bool,
    /// The drag's endpoint-extension mode (`SelectionBehavior`,
    /// types.d.ts:35) — set at press from the click count.
    behavior: SelectionBehavior,
    /// The release-resolved selection manual mode keeps (the
    /// renderer's painted selection until the next stroke).
    retained_selection: Option<ResolvedSelection>,
}

impl ScreenSelection {
    /// A left press arms the anchor (every press starts a potential
    /// selection; the release only resolves one when the pointer
    /// crossed cells). `click_count` picks the behavior
    /// ([`behavior_for_click_count`]): the plain press keeps the
    /// cell model; a double press selects the WORD at the anchor, a
    /// triple the LINE (the renderer's `nextClickBehavior` cycle).
    pub fn press(&mut self, x: usize, y: usize, click_count: u32) {
        self.anchor = Some((x, y));
        self.head = None;
        self.dragged = false;
        self.retained = false;
        self.behavior = behavior_for_click_count(click_count);
        if self.behavior != SelectionBehavior::Cell {
            // word/line selections arm on the press itself: the
            // expanded anchor endpoint already spans cells (a
            // double-click with no drag move still selects).
            self.head = Some((x, y));
            self.dragged = true;
        }
    }

    /// A left drag extends the head; the first crossing of the anchor
    /// cell marks the drag.
    pub fn drag(&mut self, x: usize, y: usize) {
        if let Some(anchor) = self.anchor {
            self.head = Some((x, y));
            if (x, y) != anchor || self.behavior != SelectionBehavior::Cell {
                // word/line selections arm on the press itself (the
                // expanded anchor endpoint already spans cells).
                self.dragged = true;
            }
        }
    }

    /// The drag's behavior mode.
    #[must_use]
    pub fn behavior(&self) -> SelectionBehavior {
        self.behavior
    }

    /// The release resolves the drag against `frame` (word/line
    /// endpoints expand over its rows; `None` resolves the cell model
    /// only): `Some(resolved)` when a real selection exists. The
    /// state machine resets its drag arm either way; `retained` keeps
    /// the RESOLVED selection queryable for the manual-mode copy
    /// (ctrl+c/ctrl+y over a kept selection — selection.ts:78-84).
    pub fn release(&mut self, frame: Option<&Frame>) -> Option<ResolvedSelection> {
        let resolved = self.resolve(frame);
        self.anchor = None;
        self.head = None;
        self.dragged = false;
        self.retained = resolved.is_some();
        self.retained_selection = resolved;
        resolved
    }

    /// The current selection resolved against `frame` (drag in flight
    /// or retained) — `expandSelectionRange`'s union model
    /// (text-buffer-view.zig:871-891): cell = the plain rect; word =
    /// the union of the anchor/focus word spans; line = the whole
    /// anchor→focus rows.
    #[must_use]
    pub fn resolve(&self, frame: Option<&Frame>) -> Option<ResolvedSelection> {
        if !self.dragged {
            return self.retained_selection;
        }
        let anchor = self.anchor?;
        let head = self.head.or(self.anchor)?;
        match self.behavior {
            SelectionBehavior::Cell => {
                if anchor == head {
                    return None;
                }
                let rect = SelectionRect::new(anchor, head);
                Some(ResolvedSelection {
                    rect,
                    first_row_start: rect.x,
                    last_row_end: rect.x + rect.width,
                })
            }
            SelectionBehavior::Line => {
                let frame = frame?;
                let y0 = anchor.1.min(head.1);
                let y1 = anchor.1.max(head.1);
                let height = y1 - y0 + 1;
                let width = frame.width.max(1);
                Some(ResolvedSelection {
                    rect: SelectionRect { x: 0, y: y0, width, height },
                    first_row_start: 0,
                    last_row_end: usize::MAX,
                })
            }
            SelectionBehavior::Word => {
                let frame = frame?;
                let (ax0, ax1) = word_at(frame, anchor.0, anchor.1)?;
                let (hx0, hx1) = word_at(frame, head.0, head.1)?;
                if anchor.1 == head.1 {
                    let (start, end) = (ax0.min(hx0), ax1.max(hx1));
                    if start == end {
                        return None;
                    }
                    Some(ResolvedSelection {
                        rect: SelectionRect {
                            x: start,
                            y: anchor.1,
                            width: end - start,
                            height: 1,
                        },
                        first_row_start: start,
                        last_row_end: end,
                    })
                } else {
                    let (top, bottom) =
                        if anchor.1 < head.1 { (anchor, head) } else { (head, anchor) };
                    let (top_start, _) = word_at(frame, top.0, top.1)?;
                    let (_, bottom_end) = word_at(frame, bottom.0, bottom.1)?;
                    let width = frame.width.max(1);
                    Some(ResolvedSelection {
                        rect: SelectionRect { x: 0, y: top.1, width, height: bottom.1 - top.1 + 1 },
                        first_row_start: top_start,
                        last_row_end: bottom_end,
                    })
                }
            }
        }
    }

    /// The current selection rect (cell model; drag in flight or
    /// retained).
    #[must_use]
    pub fn rect(&self) -> Option<SelectionRect> {
        if !self.dragged {
            return None;
        }
        let anchor = self.anchor?;
        let head = self.head?;
        if anchor == head {
            return None;
        }
        Some(SelectionRect::new(anchor, head))
    }

    /// Whether a selection is live (dragged or retained).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.dragged || self.retained
    }

    /// Clear the selection (`renderer.clearSelection()` — also resets
    /// the multi-click history: `copy`'s comment, selection.ts:60).
    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragged = false;
        self.retained = false;
        self.retained_selection = None;
    }
}

/// The selected cells' text (`getSelectedText`): each row's run
/// right-trimmed, rows joined with `\n` — trailing blank rows drop.
#[must_use]
pub fn selected_text(frame: &Frame, rect: SelectionRect) -> String {
    let resolved =
        ResolvedSelection { rect, first_row_start: rect.x, last_row_end: rect.x + rect.width };
    selected_text_resolved(frame, &resolved)
}

/// [`selected_text`] over a fully resolved selection (word/line rows
/// take their endpoint clamps — the first row starts at the anchor
/// word's start, the last ends at the focus word's end / the row's
/// text extent).
#[must_use]
pub fn selected_text_resolved(frame: &Frame, resolved: &ResolvedSelection) -> String {
    let rect = resolved.rect;
    let mut lines: Vec<String> = Vec::new();
    for y in rect.y..rect.y.saturating_add(rect.height).min(frame.cells.len()) {
        let Some(row) = frame.cells.get(y) else { continue };
        let last_row = y + 1 == rect.y + rect.height;
        let first_row = y == rect.y;
        let start = if first_row { resolved.first_row_start } else { rect.x }.max(rect.x);
        let end = if last_row {
            resolved.last_row_end.min(row_extent(row))
        } else {
            (rect.x + rect.width).min(row.len())
        };
        let mut text = String::new();
        for x in start..end {
            let ch = row.get(x).map_or(' ', |cell| cell.ch);
            text.push(ch);
        }
        lines.push(text.trim_end().to_owned());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

/// W6 symptom ⑧ / real-machine ⑧ residue — paint the live selection's highlight over
/// the final frame: opentui's selection is a CHARACTER-offset range
/// over the text renderables (`TextBufferRenderable.onSelectionChanged`
/// → text-buffer-view.zig `setLocalSelectionStyle`: anchor/focus cell
/// coordinates convert through `coordsToCharOffset` and the highlighted
/// cells are exactly those the range's characters occupy — a drag past
/// a line's text end never paints the trailing background blanks, the
/// notepad-like extent real-machine ⑧ observed). The frame-level port clamps
/// each row's highlight to that row's text extent (the last non-blank
/// cell), so the rect's background-blank tail stays unselected.
/// Selected cells render as REVERSE VIDEO — each cell's fg and bg swap
/// (a `Default` side resolves against the terminal's own default
/// pair, exactly like the renderer's unresolved cells). Called from
/// [`crate::app::App::frame`] AFTER every tree layer composited, so
/// the highlight rides above the whole screen (the renderer's own
/// post-grid pass z).
pub fn paint_highlight(frame: &mut Frame, rect: SelectionRect) {
    let resolved =
        ResolvedSelection { rect, first_row_start: rect.x, last_row_end: rect.x + rect.width };
    paint_resolved(frame, &resolved);
}

/// [`paint_highlight`] over a fully resolved selection: word/line
/// rows take their endpoint clamps (the first row's highlight starts
/// at the anchor word's start; the last ends at the focus word's end
/// or the row's text extent).
pub fn paint_resolved(frame: &mut Frame, resolved: &ResolvedSelection) {
    let rect = resolved.rect;
    for y in rect.y..rect.y.saturating_add(rect.height).min(frame.cells.len()) {
        let Some(row) = frame.cells.get_mut(y) else { continue };
        // real-machine ⑧ — the row's text extent: one past the last non-blank
        // cell of the whole row (the char-range model's line end).
        let extent = row.iter().rposition(|cell| cell.ch != ' ').map_or(0, |at| at + 1);
        let first_row = y == rect.y;
        let last_row = y + 1 == rect.y + rect.height;
        let start = if first_row { resolved.first_row_start } else { rect.x }.max(rect.x);
        let last = if last_row {
            resolved.last_row_end.min(extent)
        } else {
            (rect.x + rect.width).min(extent)
        };
        for x in start..last {
            if let Some(cell) = row.get_mut(x) {
                // W7 Minor ② (W6 review): a Default bg is the emitter's
                // TRANSPARENT black; swapped into the fg slot it renders
                // with an edge color diff vs ref's opaque resolution —
                // materialize it as opaque black first (one line, the
                // ref drawText swap's own default-pair resolution).
                if cell.bg == CellColor::Default {
                    cell.bg = CellColor::Rgb(0, 0, 0);
                }
                std::mem::swap(&mut cell.fg, &mut cell.bg);
            }
        }
    }
}

/// `handleSelectionKey` (selection.ts:62-98) — the priority-101
/// intercept's verdict for one stroke over a live selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKeyAction {
    /// Copy the selection (`ctrl+y`, or `ctrl+c` in manual mode) and
    /// consume the stroke.
    CopyAndConsume,
    /// Clear the selection and consume the stroke (`escape` with
    /// text).
    ClearAndConsume,
    /// Clear the selection, let the stroke through (any other key —
    /// selection.ts:96-98 clears then falls through).
    ClearAndPass,
    /// The stroke does not concern the selection.
    Pass,
}

/// Resolve one stroke over a live selection (`handleSelectionKey`):
/// `ctrl+c` (with the Kitty `baseCode` C variants) copies or clears
/// per the copy mode, `escape` clears (consumed when text was held),
/// every other stroke clears and passes through.
#[must_use]
pub fn selection_key_action(spec: &str, copy_on_select: bool) -> SelectionKeyAction {
    match spec {
        "ctrl+c" | "ctrl+C" | "ctrl+c:99" | "ctrl+c:67" => {
            if copy_on_select {
                // The release already copied: the stroke only clears
                // and FALLS THROUGH (selection.ts:76-79's first arm —
                // no preventDefault, so ctrl+c still reaches the app
                // bindings and interrupts).
                SelectionKeyAction::ClearAndPass
            } else {
                // Manual mode: the copy consumed the stroke
                // (selection.ts:80-84's preventDefault).
                SelectionKeyAction::CopyAndConsume
            }
        }
        "ctrl+y" | "ctrl+Y" => SelectionKeyAction::CopyAndConsume,
        "escape" => SelectionKeyAction::ClearAndConsume,
        _ => SelectionKeyAction::ClearAndPass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(rows: &[&str]) -> Frame {
        let width = rows.iter().map(|row| row.chars().count()).max().unwrap_or(0);
        let mut cells = Vec::with_capacity(rows.len());
        for row in rows {
            let mut line: Vec<crate::render::Cell> = row
                .chars()
                .map(|ch| crate::render::Cell { ch, ..crate::render::Cell::default() })
                .collect();
            line.resize(width, crate::render::Cell::default());
            cells.push(line);
        }
        Frame { width, height: rows.len(), cells, clusters: None }
    }

    #[test]
    fn drag_state_machine_resolves_crossed_cells_only() {
        let mut selection = ScreenSelection::default();
        // A bare click (press + release on the same cell) never
        // selects.
        selection.press(5, 2, 1);
        assert_eq!(selection.release(None), None);
        // A crossing drag resolves the normalized rect.
        selection.press(8, 1, 1);
        selection.drag(9, 1);
        selection.drag(5, 3);
        let resolved = selection.release(None).expect("the drag rect");
        assert_eq!(resolved.rect, SelectionRect { x: 5, y: 1, width: 4, height: 3 });
        // The release retained it (manual mode's copy window).
        assert!(selection.is_active());
        assert_eq!(
            selection.resolve(None).map(|r| r.rect),
            Some(SelectionRect { x: 5, y: 1, width: 4, height: 3 })
        );
        selection.clear();
        assert!(!selection.is_active());
    }

    #[test]
    fn selected_text_trims_rows_and_tail() {
        let frame = frame_of(&["let a = 1;", "  let b = 2;  ", ""]);
        let rect = SelectionRect { x: 0, y: 0, width: 13, height: 3 };
        assert_eq!(selected_text(&frame, rect), "let a = 1;\n  let b = 2;");
        // A column slice takes the run's cells (row 1's col 4 is 't').
        let rect = SelectionRect { x: 4, y: 0, width: 1, height: 2 };
        assert_eq!(selected_text(&frame, rect), "a\nt");
    }

    /// W6 symptom ⑧ — the highlight paints reverse video over exactly the
    /// selected rect (opentui's post-grid selection pass).
    #[test]
    fn highlight_swaps_fg_bg_inside_the_rect_only() {
        #![allow(clippy::cast_possible_truncation, clippy::similar_names, clippy::redundant_clone)]
        let mut frame = frame_of(&["abcdef", "ghijkl"]);
        for (row_y, row) in frame.cells.iter_mut().enumerate() {
            for (col_x, cell) in row.iter_mut().enumerate() {
                cell.fg = crate::render::CellColor::Rgb(
                    (200 + col_x).min(255) as u8,
                    10,
                    (10 + row_y).min(255) as u8,
                );
                cell.bg = crate::render::CellColor::Rgb(1, 2, 3);
            }
        }
        let expected_fg_at = |col_x: usize, row_y: usize, inside: bool| {
            if inside {
                crate::render::CellColor::Rgb(1, 2, 3)
            } else {
                crate::render::CellColor::Rgb(
                    (200 + col_x).min(255) as u8,
                    10,
                    (10 + row_y).min(255) as u8,
                )
            }
        };
        paint_highlight(&mut frame, SelectionRect { x: 2, y: 0, width: 2, height: 2 });
        for (row_y, row) in frame.cells.iter().enumerate().take(2) {
            for (col_x, cell) in row.iter().enumerate().take(6) {
                let inside = (2..4).contains(&col_x);
                assert_eq!(
                    cell.fg,
                    expected_fg_at(col_x, row_y, inside),
                    "fg at ({col_x},{row_y})"
                );
                assert_eq!(
                    cell.bg,
                    crate::render::CellColor::Rgb(
                        if inside { (200 + col_x).min(255) as u8 } else { 1 },
                        if inside { 10 } else { 2 },
                        if inside { (10 + row_y).min(255) as u8 } else { 3 },
                    ),
                    "bg at ({col_x},{row_y})"
                );
            }
        }
    }

    /// G-457 — the click-count cycle (`nextClickBehavior`).
    #[test]
    fn click_count_cycles_cell_word_line() {
        assert_eq!(behavior_for_click_count(1), SelectionBehavior::Cell);
        assert_eq!(behavior_for_click_count(2), SelectionBehavior::Word);
        assert_eq!(behavior_for_click_count(3), SelectionBehavior::Line);
        assert_eq!(behavior_for_click_count(4), SelectionBehavior::Cell);
    }

    /// G-457 — double-click selects the word: single row, the union
    /// of the anchor word span; a drag to another row unions the
    /// words on both rows (expandSelectionRange's word branch,
    /// text-buffer-view.zig:880-884).
    #[test]
    fn word_selection_expands_endpoints() {
        let frame = frame_of(&["let value_two = 2;", "return value_two;", ""]);
        let mut selection = ScreenSelection::default();
        // Double-click inside `value_two` on row 0.
        selection.press(8, 0, 2);
        let resolved = selection.release(Some(&frame)).expect("the word selection");
        assert_eq!(selected_text_resolved(&frame, &resolved), "value_two");
        // Drag the double-click down to `value_two` on row 1: rows
        // between fully selected, endpoints clamped to the words.
        selection.press(8, 0, 2);
        selection.drag(12, 1);
        let resolved = selection.release(Some(&frame)).expect("the cross-row word selection");
        let text = selected_text_resolved(&frame, &resolved);
        assert_eq!(text, "value_two = 2;\nreturn value_two");
    }

    /// G-457 — triple-click selects the whole line(s).
    #[test]
    fn line_selection_takes_full_rows() {
        let frame = frame_of(&["let a = 1;", "  let b = 2;", ""]);
        let mut selection = ScreenSelection::default();
        selection.press(3, 1, 3);
        let resolved = selection.release(Some(&frame)).expect("the line selection");
        assert_eq!(selected_text_resolved(&frame, &resolved), "  let b = 2;");
        selection.press(3, 0, 3);
        selection.drag(1, 1);
        let resolved = selection.release(Some(&frame)).expect("the two-line selection");
        assert_eq!(selected_text_resolved(&frame, &resolved), "let a = 1;\n  let b = 2;");
    }

    /// A double-click on a blank cell selects nothing (selectWord at
    /// a non-word codepoint).
    #[test]
    fn word_selection_on_blank_selects_nothing() {
        let frame = frame_of(&["let a = 1;"]);
        let mut selection = ScreenSelection::default();
        selection.press(9, 0, 2);
        assert_eq!(selection.release(Some(&frame)), None);
    }

    #[test]
    fn key_actions_follow_the_copy_mode() {
        // Copy-on-select: ctrl+c clears AND falls through (the
        // release already copied; selection.ts:76-79 returns without
        // preventDefault so the interrupt still fires).
        assert_eq!(selection_key_action("ctrl+c", true), SelectionKeyAction::ClearAndPass);
        // Manual: ctrl+c COPIES.
        assert_eq!(selection_key_action("ctrl+c", false), SelectionKeyAction::CopyAndConsume);
        // ctrl+y (the console's copy-selection binding) always copies.
        assert_eq!(selection_key_action("ctrl+y", true), SelectionKeyAction::CopyAndConsume);
        // Escape clears; other keys clear and pass.
        assert_eq!(selection_key_action("escape", true), SelectionKeyAction::ClearAndConsume);
        assert_eq!(selection_key_action("a", true), SelectionKeyAction::ClearAndPass);
        assert_eq!(selection_key_action("return", true), SelectionKeyAction::ClearAndPass);
    }
}
