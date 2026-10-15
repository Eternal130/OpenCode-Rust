//! The opentui-core EDITOR adapter (lane M-engine): the composer's
//! editing engine over the REAL `@opentui/core` textarea line —
//! `opentui-core`'s [`EditBuffer`] (edit-buffer.zig: cursor, clusters,
//! word boundaries, undo/redo) + [`EditorView`] (editor-view.zig: the
//! wrap/visual-line layer, Word wrap — `TextareaRenderable`'s default,
//! EditBufferRenderable.ts:166) — selected by `OC_ENGINE_OPENTUI`.
//! The hand-rolled [`crate::text_edit::TextArea`] stays the `=0`
//! fallback for one release.
//!
//! ## The contract (identical observable surface to `TextArea`)
//!
//! * Char-offset `text`/`caret`/`anchor` mirrors — the composer's
//!   model. The buffer's coordinate space is display-width offsets
//!   (cluster widths summed, one unit per line break); the adapter
//!   converts through the buffer's own cluster table, so wide/CJK
//!   clusters move exactly like upstream's editor.
//! * [`EditCommand`] — the full `input.*` surface, driven through the
//!   buffer/view methods `EditBufferRenderable` itself uses
//!   (renderables/textarea.ts ports: `moveCursorLeft`,
//!   `gotoLineHome`, `deleteWordForward`, …), including the readline
//!   `col==0`/`col==eol` line-home/end jumps to the neighboring line.
//! * Keyboard selection = the anchor model
//!   (`_keyboardSelectionActive` + the focus offset): select motions
//!   keep the anchor and move the caret; plain motions collapse to
//!   the selection's edge WITHOUT taking another step
//!   (`collapseSelectionToEdge`, EditBufferRenderable.ts:727-734).
//! * Undo/redo: the buffer's rope snapshots restore text + cursor
//!   (upstream does NOT restore the selection — the anchor clears).
//!
//! ## Deviations from the legacy engine (upstream-faithful, logged)
//!
//! * Visual-line motions follow the WRAP model (`EditorView` vlines at
//!   the composer's inner width) — legacy aliased logical lines.
//! * Word boundaries are the buffer's script-transition rules
//!   (`getNextWordBoundary`, edit-buffer.zig:821-946) — legacy used
//!   run-class rules; punctuation/whitespace stops can differ.
//! * `deleteForward` stores two undo snapshots upstream (ported
//!   faithfully); the adapter's [`SnapshotKind`] reports per-op, and
//!   same-state undo restores (text unchanged) report
//!   [`SnapshotKind::None`] so the composer's mention-sidecar mirror
//!   stays keyed to observable text changes.

use std::ops::Range;

use opentui_core::editor::{EditBuffer, EditorView};
use opentui_core::renderables::text::WrapMode;

use crate::text_edit::{diff_range, EditCommand, EditOutcome, SnapshotKind};

/// A tall-enough viewport height for the per-op view (motions never
/// depend on the viewport height; only the wrap width matters).
const VIEW_HEIGHT: u32 = 1000;

/// The composer editing engine over the real opentui-core textarea
/// line. Same observable surface as [`crate::text_edit::TextArea`].
#[derive(Clone)]
pub struct TextAreaOpentui {
    buffer: EditBuffer,
    /// The selection anchor (char offset); `None` = no selection.
    anchor: Option<usize>,
    /// The wrap width for visual-line motions (the composer's inner
    /// text width; the pump keeps it fresh on resize).
    viewport_width: u32,
}

impl Default for TextAreaOpentui {
    fn default() -> Self {
        Self::new()
    }
}

impl TextAreaOpentui {
    #[must_use]
    pub fn new() -> Self {
        Self { buffer: EditBuffer::new(), anchor: None, viewport_width: 80 }
    }

    /// The wrap width for visual-line motions (composer inner width —
    /// the same formula `mount_height` uses; the pump sets it on
    /// resize/boot).
    pub fn set_viewport_width(&mut self, width: u32) {
        self.viewport_width = width.max(1);
    }

    #[must_use]
    pub fn viewport_width(&self) -> u32 {
        self.viewport_width
    }

    // -- observable mirrors ------------------------------------------------

    /// The buffer text (LF-joined — the composer's draft form).
    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.get_text()
    }

    /// Replace the whole state (the legacy `set_text` observable:
    /// caret at the END, no selection, history cleared — the
    /// composer's draft-restore path pins the caret-at-end form).
    pub fn set_text(&mut self, text: &str) {
        self.buffer.set_text(text);
        let end = self.buffer.text_end_offset();
        self.buffer.set_cursor_by_offset(end);
        self.anchor = None;
    }

    /// Caret offset in chars.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.offset_to_char(self.buffer.primary_cursor().offset)
    }

    /// Place the caret (clamped); the selection clears.
    pub fn set_caret(&mut self, caret: usize) {
        self.place_caret(caret);
        self.anchor = None;
    }

    /// Place the caret without touching the anchor (the internal
    /// post-edit form).
    fn place_caret(&mut self, caret: usize) {
        let clamped = caret.min(self.char_len());
        self.buffer.set_cursor_by_offset(self.char_to_offset(clamped));
    }

    /// Place the caret keeping a selection anchor (the store's mirror
    /// path preserves an active anchor across the round-trip).
    pub fn set_caret_keep_anchor(&mut self, caret: usize) {
        let clamped = caret.min(self.char_len());
        self.buffer.set_cursor_by_offset(self.char_to_offset(clamped));
        if self.anchor == Some(clamped) {
            self.anchor = None;
        }
    }

    /// The anchor offset (chars), when a selection is armed.
    #[must_use]
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Arm the selection anchor directly (tests + the store mirror).
    pub fn set_anchor(&mut self, anchor: Option<usize>) {
        self.anchor = anchor;
    }

    fn char_len(&self) -> usize {
        self.buffer.get_text().chars().count()
    }

    /// The active selection range (normalized, `Some` only when
    /// non-empty) — `getSelection()`.
    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        let caret = self.caret();
        if anchor == caret {
            return None;
        }
        Some(anchor.min(caret)..anchor.max(caret))
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| {
            let text = self.buffer.get_text();
            let start = char_index_to_byte(&text, range.start);
            let end = char_index_to_byte(&text, range.end);
            text[start..end].to_owned()
        })
    }

    // -- coordinate conversion (char offsets ↔ display-width offsets) ------

    /// char offset → the buffer's display-width offset (through the
    /// row/cluster walk).
    #[must_use]
    fn char_to_offset(&self, chars: usize) -> u32 {
        let mut remaining = chars;
        let lines = self.buffer.lines_for_render();
        for (row, line) in lines.iter().enumerate() {
            let line_chars = line.text.chars().count();
            if remaining <= line_chars {
                // width of the first `remaining` chars on this row
                let mut chars_seen = 0usize;
                let mut width = 0u32;
                for cluster in &line.clusters {
                    let cluster_chars = line.text
                        [cluster.byte_start as usize..cluster.byte_end() as usize]
                        .chars()
                        .count();
                    if chars_seen >= remaining {
                        break;
                    }
                    chars_seen += cluster_chars;
                    width += cluster.width;
                }
                let _ = row;
                let row = u32::try_from(row).unwrap_or_default();
                return self.buffer.get_line_start_offset(row) + width;
            }
            remaining -= line_chars + 1; // + the line break
        }
        self.buffer.text_end_offset()
    }

    /// display-width offset → char offset.
    #[must_use]
    fn offset_to_char(&self, offset: u32) -> usize {
        let Some((row, width_col)) = self.buffer.offset_to_coords(offset) else {
            return 0;
        };
        let mut chars = 0usize;
        let lines = self.buffer.lines_for_render();
        for line in &lines[..(row as usize).min(lines.len())] {
            chars += line.text.chars().count() + 1; // the row + its break
        }
        let Some(line) = lines.get(row as usize) else { return chars };
        let mut width = 0u32;
        for cluster in &line.clusters {
            if width >= width_col {
                break;
            }
            width += cluster.width;
            chars +=
                line.text[cluster.byte_start as usize..cluster.byte_end() as usize].chars().count();
        }
        chars
    }

    // -- the edit bridge ----------------------------------------------------

    /// Run one op with the tracked outcome (the legacy
    /// `apply_tracked` contract: the replaced char range, the inserted
    /// length, the history event). Rejected = nothing observable
    /// changed AND no history pop happened (the stroke falls through).
    fn tracked(
        &mut self,
        kind: SnapshotKind,
        history_pop: bool,
        run: impl FnOnce(&mut Self),
    ) -> EditOutcome {
        let before = self.buffer.get_text();
        let before_caret = self.caret();
        let before_anchor = self.anchor;
        run(self);
        let after = self.buffer.get_text();
        let observable =
            before != after || self.caret() != before_caret || self.anchor != before_anchor;
        if !observable && !history_pop {
            return EditOutcome::Rejected;
        }
        let (replaced, inserted) = diff_range(&before, &after);
        EditOutcome::Applied { replaced, inserted, snapshot: kind }
    }

    /// Build the wrap-aware view for visual motions (per-op; the
    /// buffer clones back after).
    fn view(&self) -> EditorView {
        let mut view = EditorView::new(self.buffer.clone(), self.viewport_width, VIEW_HEIGHT);
        view.set_wrap_mode(WrapMode::Word);
        view
    }

    fn take_view(&mut self, mut view: EditorView) {
        self.buffer = std::mem::take(&mut view.buffer);
    }

    // -- selection glue (EditBufferRenderable's keyboard path) --------------

    /// `collapseSelectionToEdge` (727-734): a plain (non-select)
    /// motion first collapses an active selection to its edge —
    /// WITHOUT taking another step. Returns whether it collapsed.
    fn collapse_selection_to_edge(&mut self, edge: &str) -> bool {
        let Some(range) = self.selection() else { return false };
        let edge = if edge == "start" { range.start } else { range.end };
        self.set_caret(edge);
        true
    }

    /// The select-motion wrapper: keep/arms the anchor, run the move,
    /// then the focus lands on the new caret — the observable
    /// anchor/focus model of `updateSelectionForMovement` plus the
    /// local selection delivery.
    fn select_motion(&mut self, run: impl FnOnce(&mut Self)) {
        let caret = self.caret();
        if self.anchor.is_none() {
            self.anchor = Some(caret);
        }
        run(self);
        if self.anchor == Some(self.caret()) {
            self.anchor = None;
        }
    }

    // -- mutations ----------------------------------------------------------

    /// `insertText` at the caret (selection replaced first —
    /// `deleteSelectedText`).
    pub fn insert(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.delete_selection_if_any();
        self.buffer.insert_text(text);
        true
    }

    fn delete_selection_if_any(&mut self) {
        if let Some(range) = self.selection() {
            self.delete_char_range(range);
            self.anchor = None;
        }
    }

    /// Delete a char range directly (the mention-chip paths); the
    /// caret lands at the range start in the NEW text (char-rebased —
    /// width offsets shift across the delete).
    fn delete_char_range(&mut self, range: Range<usize>) {
        let start = self.char_to_offset(range.start);
        let end = self.char_to_offset(range.end);
        let (start_row, start_col) = self.buffer.offset_to_coords(start).unwrap_or((0, 0));
        let (end_row, end_col) = self.buffer.offset_to_coords(end).unwrap_or((0, 0));
        self.buffer.delete_range(start_row, start_col, end_row, end_col);
        self.place_caret(range.start);
    }

    // -- the v2 command surface ---------------------------------------------

    /// Apply one `input.*` command; the tracked outcome reports the
    /// delta (Rejected when nothing observable happened).
    #[allow(clippy::too_many_lines)] // one arm per v2 `input.*` command
    #[allow(clippy::match_same_arms)] // faithful per-arm ports
    pub fn apply_tracked(&mut self, command: EditCommand) -> EditOutcome {
        let snapshot = |command: EditCommand| match command {
            EditCommand::Undo => SnapshotKind::Undone,
            EditCommand::Redo => SnapshotKind::Redone,
            EditCommand::MoveLeft
            | EditCommand::MoveRight
            | EditCommand::MoveUp
            | EditCommand::MoveDown
            | EditCommand::SelectLeft
            | EditCommand::SelectRight
            | EditCommand::SelectUp
            | EditCommand::SelectDown
            | EditCommand::LineHome
            | EditCommand::LineEnd
            | EditCommand::SelectLineHome
            | EditCommand::SelectLineEnd
            | EditCommand::VisualLineHome
            | EditCommand::VisualLineEnd
            | EditCommand::SelectVisualLineHome
            | EditCommand::SelectVisualLineEnd
            | EditCommand::BufferHome
            | EditCommand::BufferEnd
            | EditCommand::SelectBufferHome
            | EditCommand::SelectBufferEnd
            | EditCommand::SelectAll => SnapshotKind::None,
            _ => SnapshotKind::Pushed,
        };
        let kind = snapshot(command);
        // Undo/redo applied = the stack had an entry to pop (upstream
        // restores even a same-state snapshot — the composer's mention
        // mirror is command-keyed, so both stay in step); selectAll
        // runs on every non-empty buffer even when the observable is
        // unchanged (upstream's handler returns true).
        let history_pop = match command {
            EditCommand::Undo => self.can_undo(),
            EditCommand::Redo => self.can_redo(),
            EditCommand::SelectAll => self.char_len() > 0,
            _ => false,
        };
        self.tracked(kind, history_pop, |area| area.apply(command))
    }

    #[allow(clippy::too_many_lines)] // one arm per v2 `input.*` command (the EditBufferRenderable port)
    #[allow(clippy::match_same_arms)] // faithful per-arm ports
    fn apply(&mut self, command: EditCommand) {
        match command {
            EditCommand::MoveLeft => {
                if !self.collapse_selection_to_edge("start") {
                    self.buffer.move_left();
                }
            }
            EditCommand::MoveRight => {
                if !self.collapse_selection_to_edge("end") {
                    self.buffer.move_right();
                }
            }
            EditCommand::MoveUp => {
                let mut view = self.view();
                view.move_up_visual();
                self.take_view(view);
            }
            EditCommand::MoveDown => {
                let mut view = self.view();
                view.move_down_visual();
                self.take_view(view);
            }
            EditCommand::SelectLeft => self.select_motion(|area| area.buffer.move_left()),
            EditCommand::SelectRight => self.select_motion(|area| area.buffer.move_right()),
            EditCommand::SelectUp => self.select_motion(|area| {
                let mut view = area.view();
                view.move_up_visual();
                area.take_view(view);
            }),
            EditCommand::SelectDown => self.select_motion(|area| {
                let mut view = area.view();
                view.move_down_visual();
                area.take_view(view);
            }),
            EditCommand::LineHome => {
                let cursor = self.buffer.primary_cursor();
                if cursor.col == 0 && cursor.row > 0 {
                    // The readline form: home at col 0 lands on the
                    // previous line's end (gotoLineHome, 791-807).
                    self.buffer.set_cursor(cursor.row - 1, 0);
                    let eol = self.buffer.get_eol();
                    self.buffer.set_cursor(eol.row, eol.col);
                } else {
                    self.buffer.set_cursor(cursor.row, 0);
                }
            }
            EditCommand::LineEnd => {
                let cursor = self.buffer.primary_cursor();
                let eol = self.buffer.get_eol();
                let line_count = self.buffer.line_count();
                if cursor.col == eol.col && cursor.row + 1 < line_count {
                    // end at EOL lands on the next line's start.
                    self.buffer.set_cursor(cursor.row + 1, 0);
                } else {
                    self.buffer.set_cursor(eol.row, eol.col);
                }
            }
            EditCommand::SelectLineHome => {
                self.select_motion(|area| area.apply(EditCommand::LineHome));
            }
            EditCommand::SelectLineEnd => {
                self.select_motion(|area| area.apply(EditCommand::LineEnd));
            }
            EditCommand::SelectVisualLineHome => self.select_motion(|area| {
                let mut view = area.view();
                let sol = view.get_visual_sol();
                let (row, col) = (sol.logical_row, sol.logical_col);
                area.take_view(view);
                area.buffer.set_cursor(row, col);
            }),
            EditCommand::SelectVisualLineEnd => self.select_motion(|area| {
                let mut view = area.view();
                view.goto_visual_line_end();
                area.take_view(view);
            }),
            EditCommand::VisualLineHome => {
                let mut view = self.view();
                let sol = view.get_visual_sol();
                let (row, col) = (sol.logical_row, sol.logical_col);
                self.take_view(view);
                self.buffer.set_cursor(row, col);
            }
            EditCommand::VisualLineEnd => {
                let mut view = self.view();
                view.goto_visual_line_end();
                self.take_view(view);
            }
            EditCommand::BufferHome => self.buffer.set_cursor(0, 0),
            EditCommand::BufferEnd => {
                let end = self.buffer.text_end_offset();
                self.buffer.set_cursor_by_offset(end);
            }
            EditCommand::SelectBufferHome => {
                self.select_motion(|area| area.buffer.set_cursor(0, 0));
            }
            EditCommand::SelectBufferEnd => {
                self.select_motion(|area| {
                    let end = area.buffer.text_end_offset();
                    area.buffer.set_cursor_by_offset(end);
                });
            }
            EditCommand::DeleteLine => {
                self.buffer.delete_line();
            }
            EditCommand::DeleteToLineEnd => {
                let cursor = self.buffer.primary_cursor();
                let eol = self.buffer.get_eol();
                if eol.col > cursor.col {
                    self.buffer.delete_range(cursor.row, cursor.col, eol.row, eol.col);
                }
            }
            EditCommand::DeleteToLineStart => {
                let cursor = self.buffer.primary_cursor();
                if cursor.col > 0 {
                    self.buffer.delete_range(cursor.row, 0, cursor.row, cursor.col);
                } else if cursor.row > 0 {
                    self.buffer.backspace();
                }
            }
            EditCommand::Backspace => {
                self.delete_selection_if_any_else(|area| area.buffer.backspace());
            }
            EditCommand::Delete => {
                self.delete_selection_if_any_else(|area| area.buffer.delete_forward());
            }
            EditCommand::Newline => {
                self.buffer.new_line();
            }
            EditCommand::Undo => {
                self.buffer.undo();
                self.anchor = None;
            }
            EditCommand::Redo => {
                self.buffer.redo();
                self.anchor = None;
            }
            EditCommand::WordForward => {
                if !self.collapse_selection_to_edge("end") {
                    let next = self.buffer.get_next_word_boundary();
                    self.buffer.set_cursor_by_offset(next.offset);
                }
            }
            EditCommand::WordBackward => {
                if !self.collapse_selection_to_edge("start") {
                    let prev = self.buffer.get_prev_word_boundary();
                    self.buffer.set_cursor_by_offset(prev.offset);
                }
            }
            EditCommand::SelectWordForward => self.select_motion(|area| {
                let next = area.buffer.get_next_word_boundary();
                area.buffer.set_cursor_by_offset(next.offset);
            }),
            EditCommand::SelectWordBackward => self.select_motion(|area| {
                let prev = area.buffer.get_prev_word_boundary();
                area.buffer.set_cursor_by_offset(prev.offset);
            }),
            EditCommand::DeleteWordForward => {
                self.delete_selection_if_any();
                let cursor = self.buffer.cursor_position();
                let next = self.buffer.get_next_word_boundary();
                if next.offset > cursor.offset {
                    self.buffer.delete_range(cursor.row, cursor.col, next.row, next.col);
                    self.buffer.set_cursor_by_offset(cursor.offset);
                }
            }
            EditCommand::DeleteWordBackward => {
                self.delete_selection_if_any();
                let cursor = self.buffer.cursor_position();
                let prev = self.buffer.get_prev_word_boundary();
                if prev.offset < cursor.offset {
                    self.buffer.delete_range(prev.row, prev.col, cursor.row, cursor.col);
                    self.buffer.set_cursor_by_offset(prev.offset);
                }
            }
            EditCommand::SelectAll => {
                if self.char_len() == 0 {
                    return;
                }
                self.anchor = Some(0);
                let end = self.buffer.text_end_offset();
                self.buffer.set_cursor_by_offset(end);
            }
        }
    }

    fn delete_selection_if_any_else(&mut self, plain: impl FnOnce(&mut Self)) {
        if self.selection().is_some() {
            self.delete_selection_if_any();
        } else {
            plain(self);
        }
    }

    /// Host-facing insert with the same delta/history plumbing as
    /// [`Self::apply_tracked`] (the composer's paste/char paths).
    pub fn insert_tracked(&mut self, text: &str) -> EditOutcome {
        if text.is_empty() {
            return EditOutcome::Rejected;
        }
        self.tracked(SnapshotKind::Pushed, false, |area| {
            area.delete_selection_if_any();
            area.buffer.insert_text(text);
        })
    }

    /// Host-facing range delete (the composer's atomic mention-chip
    /// deletion).
    pub fn delete_range_tracked(&mut self, range: Range<usize>) -> EditOutcome {
        let start = range.start.min(self.char_len());
        let end = range.end.min(self.char_len());
        if start >= end {
            return EditOutcome::Rejected;
        }
        self.tracked(SnapshotKind::Pushed, false, |area| area.delete_char_range(start..end))
    }

    /// Offset of the next word start after `from` (char units — the
    /// buffer's boundary rules; the probe moves the buffer cursor).
    #[must_use]
    pub fn next_word_start(&mut self, from: usize) -> usize {
        self.buffer.set_cursor_by_offset(self.char_to_offset(from));
        let next = self.buffer.get_next_word_boundary();
        self.offset_to_char(next.offset)
    }

    /// Offset of the word start at/before `from` (char units).
    #[must_use]
    pub fn prev_word_start(&mut self, from: usize) -> usize {
        self.buffer.set_cursor_by_offset(self.char_to_offset(from));
        let prev = self.buffer.get_prev_word_boundary();
        self.offset_to_char(prev.offset)
    }

    /// `input.undo` availability.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    /// History depth (the parity display + the files-mirror check).
    #[must_use]
    pub fn undo_depth(&self) -> (usize, usize) {
        // The buffer exposes availability only; depth parity uses the
        // observable can_undo/can_redo pair.
        (usize::from(self.can_undo()), usize::from(self.can_redo()))
    }
}

fn char_index_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(byte, _)| byte)
}

impl std::fmt::Debug for TextAreaOpentui {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextAreaOpentui")
            .field("text", &self.buffer.get_text())
            .field("caret", &self.caret())
            .field("anchor", &self.anchor)
            .field("can_undo", &self.can_undo())
            .field("can_redo", &self.can_redo())
            .finish_non_exhaustive()
    }
}

impl PartialEq for TextAreaOpentui {
    fn eq(&self, other: &Self) -> bool {
        self.buffer.get_text() == other.buffer.get_text()
            && self.caret() == other.caret()
            && self.anchor == other.anchor
            && self.can_undo() == other.can_undo()
            && self.can_redo() == other.can_redo()
    }
}

// ---------------------------------------------------------------------------
// The engine-selection facade (composer seam)
// ---------------------------------------------------------------------------

/// The composer's editing engine, flag-selected once at construction
/// ([`crate::opentui_engine_enabled`]): the hand-rolled
/// [`crate::text_edit::TextArea`] or the opentui-core-backed
/// [`TextAreaOpentui`]. One internal surface — the composer's code
/// path is identical modulo the hop.
#[derive(Clone)]
pub enum EditCore {
    Legacy(crate::text_edit::TextArea),
    Opentui(TextAreaOpentui),
}

impl Default for EditCore {
    fn default() -> Self {
        Self::fresh()
    }
}

impl std::fmt::Debug for EditCore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy(area) => area.fmt(formatter),
            Self::Opentui(area) => area.fmt(formatter),
        }
    }
}

impl PartialEq for EditCore {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Legacy(left), Self::Legacy(right)) => left == right,
            (Self::Opentui(left), Self::Opentui(right)) => left == right,
            _ => false,
        }
    }
}

impl EditCore {
    /// A fresh engine on the active path.
    #[must_use]
    pub fn fresh() -> Self {
        if crate::opentui_engine_enabled() {
            Self::Opentui(TextAreaOpentui::new())
        } else {
            Self::Legacy(crate::text_edit::TextArea::default())
        }
    }

    /// Whether this is the opentui engine (the viewport-width seam is
    /// a no-op on the legacy path).
    #[must_use]
    pub fn is_opentui(&self) -> bool {
        matches!(self, Self::Opentui(_))
    }

    /// The wrap width for visual-line motions (opentui path only).
    pub fn set_viewport_width(&mut self, width: u32) {
        if let Self::Opentui(area) = self {
            area.set_viewport_width(width);
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Legacy(area) => area.text.clone(),
            Self::Opentui(area) => area.text(),
        }
    }

    /// The store's defensive resync (an EXTERNAL draft change — submit
    /// clears, prompt.set): the text rebases and the history resets
    /// (v2's `setText` clears the rope history — undo never restores
    /// across an external replace). A MATCHING text is a no-op — the
    /// per-op bridge calls this on every keystroke, and the caret/
    /// history must survive (the legacy arm's plain field copy was
    /// naturally inert).
    pub fn mirror_text(&mut self, text: &str) {
        match self {
            Self::Legacy(area) => area.text.clone_from(&text.to_owned()),
            Self::Opentui(area) => {
                if area.text() != text {
                    area.set_text(text);
                }
            }
        }
    }

    pub fn set_text(&mut self, text: &str) {
        match self {
            Self::Legacy(area) => area.set_text(text),
            Self::Opentui(area) => area.set_text(text),
        }
    }

    #[must_use]
    pub fn caret(&self) -> usize {
        match self {
            Self::Legacy(area) => area.caret,
            Self::Opentui(area) => area.caret(),
        }
    }

    pub fn set_caret(&mut self, caret: usize) {
        match self {
            Self::Legacy(area) => {
                area.caret = caret.min(area.text.chars().count());
                area.anchor = None;
            }
            Self::Opentui(area) => area.set_caret(caret),
        }
    }

    /// Place the caret keeping an armed selection anchor (the store's
    /// round-trip mirror preserves it).
    pub fn set_caret_keep_anchor(&mut self, caret: usize) {
        match self {
            Self::Legacy(area) => {
                let clamped = caret.min(area.text.chars().count());
                if area.anchor == Some(clamped) {
                    area.anchor = None;
                }
                area.caret = clamped;
            }
            Self::Opentui(area) => area.set_caret_keep_anchor(caret),
        }
    }

    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        match self {
            Self::Legacy(area) => area.selection(),
            Self::Opentui(area) => area.selection(),
        }
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        match self {
            Self::Legacy(area) => area.selected_text(),
            Self::Opentui(area) => area.selected_text(),
        }
    }

    pub fn insert_tracked(&mut self, text: &str) -> EditOutcome {
        match self {
            Self::Legacy(area) => area.insert_tracked(text),
            Self::Opentui(area) => area.insert_tracked(text),
        }
    }

    pub fn delete_range_tracked(&mut self, range: Range<usize>) -> EditOutcome {
        match self {
            Self::Legacy(area) => area.delete_range_tracked(range),
            Self::Opentui(area) => area.delete_range_tracked(range),
        }
    }

    pub fn apply_tracked(&mut self, command: EditCommand) -> EditOutcome {
        match self {
            Self::Legacy(area) => area.apply_tracked(command),
            Self::Opentui(area) => area.apply_tracked(command),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_edit::{CharClusters, EditCommand as Cmd, TextArea};

    fn legacy(text: &str) -> TextArea {
        let mut area = TextArea::with_clusters(std::rc::Rc::new(CharClusters));
        area.set_text(text);
        area
    }

    fn opentui(text: &str) -> TextAreaOpentui {
        let mut area = TextAreaOpentui::new();
        area.set_text(text);
        area
    }

    /// The observable triple both engines expose.
    type Observed = (String, usize, Option<Range<usize>>);

    fn observe(area: &TextAreaOpentui) -> Observed {
        (area.text(), area.caret(), area.selection())
    }

    fn observe_legacy(area: &TextArea) -> (String, usize, Option<Range<usize>>) {
        (area.text.clone(), area.caret, area.selection())
    }

    /// One op applied to BOTH engines; both observables returned.
    fn step(
        legacy: &mut TextArea,
        opentui: &mut TextAreaOpentui,
        command: Cmd,
    ) -> (Observed, Observed) {
        let _ = legacy.apply(command);
        let _ = opentui.apply_tracked(command);
        (observe_legacy(legacy), observe(opentui))
    }

    /// Insert applied to both.
    fn step_insert(
        legacy: &mut TextArea,
        opentui: &mut TextAreaOpentui,
        text: &str,
    ) -> (Observed, Observed) {
        let _ = legacy.insert(text);
        let _ = opentui.insert(text);
        (observe_legacy(legacy), observe(opentui))
    }

    #[test]
    fn motions_and_selection_parity() {
        let mut left = legacy("hello world");
        let mut right = opentui("hello world");
        // Caret to 5 (both engines' placement path).
        left.caret = 5;
        right.set_caret(5);
        // Plain motions run only over NO selection here — the
        // collapse-without-step rule is an upstream delta (its own
        // test below).
        for command in [
            Cmd::SelectRight,
            Cmd::SelectRight,
            Cmd::SelectLeft,
            Cmd::SelectAll,
            Cmd::SelectLeft,
            Cmd::SelectBufferHome,
            Cmd::MoveRight,
        ] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} diverged");
        }
        // Buffer bounds.
        let mut left = legacy("one\ntwo\nthree");
        let mut right = opentui("one\ntwo\nthree");
        for command in [Cmd::BufferHome, Cmd::BufferEnd, Cmd::SelectBufferHome, Cmd::MoveLeft] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} diverged");
        }
    }

    #[test]
    fn edit_ops_parity() {
        // Insert over a selection, backspace, delete, newline, undo/redo.
        let mut left = legacy("hello world");
        let mut right = opentui("hello world");
        let _ = left.apply(Cmd::SelectAll);
        let _ = right.apply_tracked(Cmd::SelectAll);
        let (l, r) = step_insert(&mut left, &mut right, "replaced");
        assert_eq!(l, r);
        for command in [Cmd::Backspace, Cmd::Undo, Cmd::Redo, Cmd::Delete] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} diverged");
        }
        // Backspace joining lines + cluster deletes.
        let mut left = legacy("ab\ncd");
        let mut right = opentui("ab\ncd");
        left.caret = 3;
        right.set_caret(3);
        for command in [Cmd::Backspace, Cmd::Backspace, Cmd::Backspace, Cmd::Undo] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} diverged");
        }
    }

    #[test]
    fn undo_redo_round_trip_and_rejection() {
        let mut left = legacy("");
        let mut right = opentui("");
        // Rejections at the buffer edges (motions only — the delete
        // family's phantom-snapshot behavior is an upstream delta
        // with its own test below).
        for command in [Cmd::MoveLeft, Cmd::MoveRight] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} rejected-observable diverged");
        }
        let _ = step_insert(&mut left, &mut right, "a");
        let _ = step_insert(&mut left, &mut right, "b");
        for command in [Cmd::Undo, Cmd::Undo, Cmd::Undo, Cmd::Redo, Cmd::Redo, Cmd::Redo] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} diverged");
        }
    }

    #[test]
    fn wide_clusters_move_by_grapheme() {
        // CJK clusters are one motion unit on both engines (the
        // opentui engine's cluster widths; the legacy wrap clusters).
        let text = "a日本語b";
        let mut left = legacy(text);
        let mut right = opentui(text);
        for command in [Cmd::MoveRight, Cmd::MoveRight, Cmd::MoveRight, Cmd::MoveLeft] {
            let (l, r) = step(&mut left, &mut right, command);
            assert_eq!(l, r, "{command:?} wide-cluster diverged");
        }
        // Caret conversions survive the round trip across wide rows.
        for caret in 0..=text.chars().count() {
            right.set_caret(caret);
            assert_eq!(right.caret(), caret, "caret round trip at {caret}");
        }
    }

    /// **Engine delta (upstream-faithful): word boundaries.** The
    /// buffer's `getNextWordBoundary` (script-transition rules,
    /// edit-buffer.zig:821-946) vs the legacy run-class rule — the
    /// punctuation/whitespace stops differ; the opentui engine is
    /// v2's behavior (the `editor_tests` suite pins it there).
    #[test]
    fn delta_word_boundaries_follow_upstream() {
        let mut left = legacy("fn foo()");
        let mut right = opentui("fn foo()");
        left.caret = 0;
        right.set_caret(0);
        // Hop trajectories: legacy's run-class stops [3, 6, 8]
        // (inside the paren run); upstream's script-transition
        // boundaries [3, 7, 8] (the parenthesis run is its own unit).
        let _ = left.apply(Cmd::WordForward);
        let _ = right.apply_tracked(Cmd::WordForward);
        assert_eq!(left.caret, 3);
        assert_eq!(right.caret(), 3, "the first hop agrees (the space)");
        let _ = left.apply(Cmd::WordForward);
        let _ = right.apply_tracked(Cmd::WordForward);
        assert_eq!(left.caret, 6, "legacy: stops before the paren run");
        assert_eq!(right.caret(), 7, "upstream: crosses through it");
        // Both reach the buffer end in a bounded number of hops.
        for _ in 0..8 {
            let _ = left.apply(Cmd::WordForward);
        }
        for _ in 0..8 {
            let _ = right.apply_tracked(Cmd::WordForward);
        }
        assert_eq!(left.caret, left.text.chars().count());
        assert_eq!(right.caret(), right.char_len());
    }

    /// **Engine delta (upstream-faithful): line home/end readline
    /// jumps.** `gotoLineHome` at col 0 lands on the PREVIOUS line's
    /// end; `gotoLineEnd` at EOL lands on the NEXT line's start
    /// (EditBufferRenderable.ts:791-825). The legacy engine moved to
    /// the current line's bounds only.
    #[test]
    fn delta_line_home_end_readline_jumps() {
        let mut left = legacy("one\ntwo\nthree");
        let mut right = opentui("one\ntwo\nthree");
        // Caret at the start of "two" (row 1, col 0).
        left.caret = 4;
        right.set_caret(4);
        let _ = left.apply(Cmd::LineHome);
        let _ = right.apply_tracked(Cmd::LineHome);
        assert_eq!(left.caret, 4, "legacy: stays at the line start");
        assert_eq!(right.caret(), 3, "upstream: previous line's end");
        // LineEnd at the row's EOL → next line's start.
        right.set_caret(7); // the true EOL of "two"
        let _ = right.apply_tracked(Cmd::LineEnd);
        assert_eq!(right.caret(), 8, "upstream: next line's start");
        // Mid-row LineEnd still goes to the current EOL.
        right.set_caret(5);
        let _ = right.apply_tracked(Cmd::LineEnd);
        assert_eq!(right.caret(), 7);
    }

    /// **Engine delta (upstream-faithful): visual motions follow the
    /// wrap model.** With a narrow viewport, up/down cross WRAPPED
    /// rows (`EditorView` vlines) — the legacy engine aliased logical
    /// lines.
    #[test]
    fn delta_visual_motions_follow_wrap() {
        let mut right = opentui("aaa bbb ccc ddd");
        right.set_viewport_width(7); // wraps after "aaa " etc.
        right.set_caret(15); // the end
        let _ = right.apply_tracked(Cmd::MoveUp);
        assert!(right.caret() < 15, "visual up leaves the last wrapped row");
        let visual_stop = right.caret();
        let mut left = legacy("aaa bbb ccc ddd");
        left.caret = 15;
        let _ = left.apply(Cmd::MoveUp);
        assert_eq!(left.caret, 15, "legacy: single logical row — up REJECTS");
        // The donor's word-wrap vline ends at col 6 (the boundary
        // affinity keeps the cursor on the last grapheme, not past
        // the break's trailing space) — editor-view.zig's rule.
        assert_eq!(visual_stop, 6, "visual up lands on the previous WRAPPED row");
    }

    #[test]
    fn mention_chip_ranges_delete_atomically() {
        // The composer's delete_range_tracked path (atomic chips).
        let mut right = opentui("keep [Image 1] tail");
        let outcome = right.delete_range_tracked(5..14);
        assert!(matches!(outcome, EditOutcome::Applied { .. }));
        assert_eq!(right.text(), "keep  tail");
        assert_eq!(right.caret(), 5);
    }

    /// **Engine delta (upstream-faithful): `deleteForward` stores an
    /// undo point even when it cannot delete** (the ported
    /// edit-buffer.zig double-snapshot: `autoStoreUndo` runs before
    /// the guard — one extra undo press walks a same-state restore).
    /// The legacy engine only snapshotted real deletions.
    #[test]
    fn delta_delete_forward_phantom_snapshot() {
        let mut right = opentui("ab");
        right.set_caret(2); // the end
        let _ = right.apply_tracked(Cmd::Delete); // no-op delete…
        assert!(right.can_undo(), "upstream stored the undo point");
        // The first undo restores the SAME state (a same-state walk);
        // the text never changed.
        let outcome = right.apply_tracked(Cmd::Undo);
        assert!(matches!(outcome, EditOutcome::Applied { .. }));
        assert_eq!(right.text(), "ab");
    }

    /// **Engine delta (upstream-faithful): plain moves over a
    /// selection collapse to the edge WITHOUT another step**
    /// (`collapseSelectionToEdge`, EditBufferRenderable.ts:727-734 —
    /// moveCursorLeft's first arm). The legacy engine cleared the
    /// selection AND moved.
    #[test]
    fn delta_plain_moves_collapse_without_stepping() {
        let mut left = legacy("hello world");
        let mut right = opentui("hello world");
        left.caret = 5;
        right.set_caret(5);
        let _ = left.apply(Cmd::SelectRight); // 5..6
        let _ = right.apply_tracked(Cmd::SelectRight);
        let _ = left.apply(Cmd::MoveLeft); // legacy: 6 -> 5 (cleared + moved)
        let _ = right.apply_tracked(Cmd::MoveLeft); // upstream: collapse to 5
        assert_eq!(left.caret, 5);
        assert_eq!(right.caret(), 5, "both land on 5 — but upstream did NOT step");
        // From a selection whose start is 0: left collapses to 0 on
        // both, but the NEXT left differs (legacy already moved).
        let _ = left.apply(Cmd::SelectAll);
        let _ = right.apply_tracked(Cmd::SelectAll);
        let _ = left.apply(Cmd::MoveLeft);
        let _ = right.apply_tracked(Cmd::MoveLeft);
        assert_eq!((left.caret, left.selection()), (10, None), "legacy: cleared AND moved");
        assert_eq!(
            (right.caret(), right.selection()),
            (0, None),
            "upstream: collapsed to the edge"
        );
    }

    #[test]
    fn selection_restores_and_collapses_like_upstream() {
        let mut right = opentui("hello world");
        right.set_caret(0);
        let _ = right.apply_tracked(Cmd::SelectRight);
        let _ = right.apply_tracked(Cmd::SelectRight);
        assert_eq!(right.selection(), Some(0..2));
        assert_eq!(right.selected_text().as_deref(), Some("he"));
        // A plain move collapses to the EDGE without another step.
        let _ = right.apply_tracked(Cmd::MoveRight);
        assert_eq!(right.caret(), 2, "collapse-to-end, no extra step");
        assert_eq!(right.selection(), None);
    }
}
