//! The composer's editing engine — the REAL `@opentui/core` textarea
//! line: `opentui-core`'s [`EditBuffer`] (edit-buffer.zig: cursor,
//! clusters, word boundaries, undo/redo) + [`EditorView`]
//! (editor-view.zig: the wrap/visual-line layer, Word wrap —
//! `TextareaRenderable`'s default, EditBufferRenderable.ts:166).
//! Since the M-engine-retire lane this is the ONLY editing engine —
//! the hand-rolled predecessor is gone. This module also owns the
//! command/outcome data contracts ([`EditCommand`],
//! [`EditOutcome`], [`SnapshotKind`]) and the composer-facing
//! [`EditCore`] seam.
//!
//! ## The contract
//!
//! * Char-offset `text`/`caret`/`anchor` mirrors — the composer's
//!   model. The buffer's coordinate space is display-width offsets
//!   (cluster widths summed, one unit per line break); the adapter
//!   converts through the buffer's own cluster table, so wide/CJK
//!   clusters move exactly like upstream's editor.
//! * [`EditCommand`] — the full `input.*` surface
//!   (`config/keybind.ts:206-243`), driven through the buffer/view
//!   methods `EditBufferRenderable` itself uses
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
//! ## Upstream-faithful behaviors (each pinned by a test below)
//!
//! * Visual-line motions follow the WRAP model (`EditorView` vlines at
//!   the composer's inner width).
//! * Word boundaries are the buffer's script-transition rules
//!   (`getNextWordBoundary`, edit-buffer.zig:821-946).
//! * `deleteForward` stores two undo snapshots upstream (ported
//!   faithfully); the adapter's [`SnapshotKind`] reports per-op, and
//!   same-state undo restores (text unchanged) report
//!   [`SnapshotKind::None`] so the composer's mention-sidecar mirror
//!   stays keyed to observable text changes.

use std::ops::Range;

use opentui_core::editor::{EditBuffer, EditorView};
use opentui_core::renderables::text::WrapMode;

/// The `input.*` command set — one variant per binding in
/// `config/keybind.ts:206-243` (the textarea slice of the table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    /// `input.move.left`
    MoveLeft,
    /// `input.move.right`
    MoveRight,
    /// `input.move.up`
    MoveUp,
    /// `input.move.down`
    MoveDown,
    /// `input.select.left`
    SelectLeft,
    /// `input.select.right`
    SelectRight,
    /// `input.select.up`
    SelectUp,
    /// `input.select.down`
    SelectDown,
    /// `input.line.home`
    LineHome,
    /// `input.line.end`
    LineEnd,
    /// `input.select.line.home`
    SelectLineHome,
    /// `input.select.line.end`
    SelectLineEnd,
    /// `input.visual.line.home`
    VisualLineHome,
    /// `input.visual.line.end`
    VisualLineEnd,
    /// `input.select.visual.line.home`
    SelectVisualLineHome,
    /// `input.select.visual.line.end`
    SelectVisualLineEnd,
    /// `input.buffer.home`
    BufferHome,
    /// `input.buffer.end`
    BufferEnd,
    /// `input.select.buffer.home`
    SelectBufferHome,
    /// `input.select.buffer.end`
    SelectBufferEnd,
    /// `input.delete.line`
    DeleteLine,
    /// `input.delete.to.line.end`
    DeleteToLineEnd,
    /// `input.delete.to.line.start`
    DeleteToLineStart,
    /// `input.backspace`
    Backspace,
    /// `input.delete`
    Delete,
    /// `input.newline`
    Newline,
    /// `input.undo`
    Undo,
    /// `input.redo`
    Redo,
    /// `input.word.forward`
    WordForward,
    /// `input.word.backward`
    WordBackward,
    /// `input.select.word.forward`
    SelectWordForward,
    /// `input.select.word.backward`
    SelectWordBackward,
    /// `input.delete.word.forward`
    DeleteWordForward,
    /// `input.delete.word.backward`
    DeleteWordBackward,
    /// `input.select.all`
    SelectAll,
}

impl EditCommand {
    /// The dot command id of this command (`input.move.left`).
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::MoveLeft => "input.move.left",
            Self::MoveRight => "input.move.right",
            Self::MoveUp => "input.move.up",
            Self::MoveDown => "input.move.down",
            Self::SelectLeft => "input.select.left",
            Self::SelectRight => "input.select.right",
            Self::SelectUp => "input.select.up",
            Self::SelectDown => "input.select.down",
            Self::LineHome => "input.line.home",
            Self::LineEnd => "input.line.end",
            Self::SelectLineHome => "input.select.line.home",
            Self::SelectLineEnd => "input.select.line.end",
            Self::VisualLineHome => "input.visual.line.home",
            Self::VisualLineEnd => "input.visual.line.end",
            Self::SelectVisualLineHome => "input.select.visual.line.home",
            Self::SelectVisualLineEnd => "input.select.visual.line.end",
            Self::BufferHome => "input.buffer.home",
            Self::BufferEnd => "input.buffer.end",
            Self::SelectBufferHome => "input.select.buffer.home",
            Self::SelectBufferEnd => "input.select.buffer.end",
            Self::DeleteLine => "input.delete.line",
            Self::DeleteToLineEnd => "input.delete.to.line.end",
            Self::DeleteToLineStart => "input.delete.to.line.start",
            Self::Backspace => "input.backspace",
            Self::Delete => "input.delete",
            Self::Newline => "input.newline",
            Self::Undo => "input.undo",
            Self::Redo => "input.redo",
            Self::WordForward => "input.word.forward",
            Self::WordBackward => "input.word.backward",
            Self::SelectWordForward => "input.select.word.forward",
            Self::SelectWordBackward => "input.select.word.backward",
            Self::DeleteWordForward => "input.delete.word.forward",
            Self::DeleteWordBackward => "input.delete.word.backward",
            Self::SelectAll => "input.select.all",
        }
    }

    /// Parse a dot command id (`composer_command`'s table).
    #[must_use]
    pub fn parse(command: &str) -> Option<Self> {
        Some(match command {
            "input.move.left" => Self::MoveLeft,
            "input.move.right" => Self::MoveRight,
            "input.move.up" => Self::MoveUp,
            "input.move.down" => Self::MoveDown,
            "input.select.left" => Self::SelectLeft,
            "input.select.right" => Self::SelectRight,
            "input.select.up" => Self::SelectUp,
            "input.select.down" => Self::SelectDown,
            "input.line.home" => Self::LineHome,
            "input.line.end" => Self::LineEnd,
            "input.select.line.home" => Self::SelectLineHome,
            "input.select.line.end" => Self::SelectLineEnd,
            "input.visual.line.home" => Self::VisualLineHome,
            "input.visual.line.end" => Self::VisualLineEnd,
            "input.select.visual.line.home" => Self::SelectVisualLineHome,
            "input.select.visual.line.end" => Self::SelectVisualLineEnd,
            "input.buffer.home" => Self::BufferHome,
            "input.buffer.end" => Self::BufferEnd,
            "input.select.buffer.home" => Self::SelectBufferHome,
            "input.select.buffer.end" => Self::SelectBufferEnd,
            "input.delete.line" => Self::DeleteLine,
            "input.delete.to.line.end" => Self::DeleteToLineEnd,
            "input.delete.to.line.start" => Self::DeleteToLineStart,
            "input.backspace" => Self::Backspace,
            "input.delete" => Self::Delete,
            "input.newline" => Self::Newline,
            "input.undo" => Self::Undo,
            "input.redo" => Self::Redo,
            "input.word.forward" => Self::WordForward,
            "input.word.backward" => Self::WordBackward,
            "input.select.word.forward" => Self::SelectWordForward,
            "input.select.word.backward" => Self::SelectWordBackward,
            "input.delete.word.forward" => Self::DeleteWordForward,
            "input.delete.word.backward" => Self::DeleteWordBackward,
            "input.select.all" => Self::SelectAll,
            _ => return None,
        })
    }
}

/// What an [`EditCore::apply_tracked`] did to the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// The command rejected (nothing changed, no snapshot pushed).
    Rejected,
    /// The edit applied.
    Applied {
        /// The char range of the buffer the edit replaced.
        replaced: Range<usize>,
        /// How many chars replaced it.
        inserted: usize,
        /// The history effect (hosts with sidecar state — the
        /// composer's file mentions — mirror their stacks onto
        /// it).
        snapshot: SnapshotKind,
    },
}

/// The history effect of one applied command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
    /// No history movement (motions/selection).
    None,
    /// An undo snapshot was pushed (a mutating edit).
    Pushed,
    /// `undo()` restored a snapshot.
    Undone,
    /// `redo()` restored a snapshot.
    Redone,
}

/// Prefix/suffix diff of two texts, in CHAR units: the replaced range
/// of `before` and how many chars `after` put there.
fn diff_range(before: &str, after: &str) -> (Range<usize>, usize) {
    let before_chars: Vec<char> = before.chars().collect();
    let after_chars: Vec<char> = after.chars().collect();
    let mut prefix = 0usize;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix] == after_chars[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < before_chars.len() - prefix
        && suffix < after_chars.len() - prefix
        && before_chars[before_chars.len() - 1 - suffix]
            == after_chars[after_chars.len() - 1 - suffix]
    {
        suffix += 1;
    }
    (prefix..before_chars.len() - suffix, after_chars.len() - prefix - suffix)
}

/// A tall-enough viewport height for the per-op view (motions never
/// depend on the viewport height; only the wrap width matters).
const VIEW_HEIGHT: u32 = 1000;

/// The composer editing engine over the real opentui-core textarea
/// line — the implementation behind [`EditCore`].
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
// The composer seam
// ---------------------------------------------------------------------------

/// The composer's editing engine: the opentui-core-backed
/// [`TextAreaOpentui`] (the only path since the M-engine-retire
/// lane). A named wrapper so the composer/store keep their field
/// vocabulary; every method delegates 1:1.
#[derive(Clone, Debug, PartialEq)]
pub struct EditCore(TextAreaOpentui);

impl Default for EditCore {
    fn default() -> Self {
        Self::fresh()
    }
}

impl EditCore {
    /// A fresh engine.
    #[must_use]
    pub fn fresh() -> Self {
        Self(TextAreaOpentui::new())
    }

    /// The wrap width for visual-line motions (the composer's inner
    /// text width; the pump keeps it fresh on resize/boot).
    pub fn set_viewport_width(&mut self, width: u32) {
        self.0.set_viewport_width(width);
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// The store's defensive resync (an EXTERNAL draft change — submit
    /// clears, prompt.set): the text rebases and the history resets
    /// (v2's `setText` clears the rope history — undo never restores
    /// across an external replace). A MATCHING text is a no-op — the
    /// per-op bridge calls this on every keystroke, and the caret/
    /// history must survive.
    pub fn mirror_text(&mut self, text: &str) {
        if self.0.text() != text {
            self.0.set_text(text);
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.0.set_text(text);
    }

    #[must_use]
    pub fn caret(&self) -> usize {
        self.0.caret()
    }

    pub fn set_caret(&mut self, caret: usize) {
        self.0.set_caret(caret);
    }

    /// Place the caret keeping an armed selection anchor (the store's
    /// round-trip mirror preserves it).
    pub fn set_caret_keep_anchor(&mut self, caret: usize) {
        self.0.set_caret_keep_anchor(caret);
    }

    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        self.0.selection()
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.0.selected_text()
    }

    pub fn insert_tracked(&mut self, text: &str) -> EditOutcome {
        self.0.insert_tracked(text)
    }

    pub fn delete_range_tracked(&mut self, range: Range<usize>) -> EditOutcome {
        self.0.delete_range_tracked(range)
    }

    pub fn apply_tracked(&mut self, command: EditCommand) -> EditOutcome {
        self.0.apply_tracked(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(text: &str) -> TextAreaOpentui {
        let mut edit = TextAreaOpentui::new();
        edit.set_text(text);
        edit
    }

    /// The observable triple: text, caret, selection.
    type Observed = (String, usize, Option<Range<usize>>);

    fn observe(edit: &TextAreaOpentui) -> Observed {
        (edit.text(), edit.caret(), edit.selection())
    }

    /// One op applied; the observable returned.
    fn step(edit: &mut TextAreaOpentui, command: EditCommand) -> Observed {
        let _ = edit.apply_tracked(command);
        observe(edit)
    }

    /// One insert applied; the observable returned.
    fn step_insert(edit: &mut TextAreaOpentui, text: &str) -> Observed {
        let _ = edit.insert(text);
        observe(edit)
    }

    /// (text, caret, selection) observable.
    fn obs(text: &str, caret: usize, selection: Option<Range<usize>>) -> Observed {
        (text.to_owned(), caret, selection)
    }

    #[test]
    fn motions_and_selection() {
        let mut edit = area("hello world");
        edit.set_caret(5);
        // Plain motions run only over NO selection here — the
        // collapse-without-step rule is pinned in its own test below.
        assert_eq!(step(&mut edit, EditCommand::SelectRight), obs("hello world", 6, Some(5..6)));
        assert_eq!(step(&mut edit, EditCommand::SelectRight), obs("hello world", 7, Some(5..7)));
        assert_eq!(step(&mut edit, EditCommand::SelectLeft), obs("hello world", 6, Some(5..6)));
        assert_eq!(step(&mut edit, EditCommand::SelectAll), obs("hello world", 11, Some(0..11)));
        assert_eq!(step(&mut edit, EditCommand::SelectLeft), obs("hello world", 10, Some(0..10)));
        // The anchor meets the caret: the selection collapses.
        assert_eq!(step(&mut edit, EditCommand::SelectBufferHome), obs("hello world", 0, None));
        assert_eq!(step(&mut edit, EditCommand::MoveRight), obs("hello world", 1, None));
        // Buffer bounds.
        let mut edit = area("one\ntwo\nthree");
        assert_eq!(step(&mut edit, EditCommand::BufferHome), obs("one\ntwo\nthree", 0, None));
        assert_eq!(step(&mut edit, EditCommand::BufferEnd), obs("one\ntwo\nthree", 13, None));
        assert_eq!(
            step(&mut edit, EditCommand::SelectBufferHome),
            obs("one\ntwo\nthree", 0, Some(0..13))
        );
        // Plain left over a selection collapses to the start edge
        // WITHOUT another step.
        assert_eq!(step(&mut edit, EditCommand::MoveLeft), obs("one\ntwo\nthree", 0, None));
    }

    #[test]
    fn edit_ops() {
        // Insert over a selection, backspace, delete, newline, undo/redo.
        let mut edit = area("hello world");
        let _ = edit.apply_tracked(EditCommand::SelectAll);
        let observed = step_insert(&mut edit, "replaced");
        assert_eq!(observed, ("replaced".to_owned(), 8, None));
        let _ = step(&mut edit, EditCommand::Backspace);
        assert_eq!(edit.text(), "replace");
        let _ = step(&mut edit, EditCommand::Undo);
        assert_eq!(edit.text(), "replaced", "undo restored the backspace");
        let _ = step(&mut edit, EditCommand::Redo);
        assert_eq!(edit.text(), "replace", "redo re-deleted");
        // Delete at the buffer END: nothing to delete forward (the
        // phantom-snapshot behavior is pinned in its own test below).
        let _ = step(&mut edit, EditCommand::Delete);
        assert_eq!(edit.text(), "replace");
        // Backspace joining lines + cluster deletes.
        let mut edit = area("ab\ncd");
        edit.set_caret(3);
        for command in [EditCommand::Backspace, EditCommand::Backspace, EditCommand::Backspace] {
            let _ = step(&mut edit, command);
        }
        // join the lines, then delete "b", then "a" — "cd" remains.
        assert_eq!(edit.text(), "cd");
        let _ = step(&mut edit, EditCommand::Undo);
        assert_eq!(edit.text(), "acd", "undo restored one backspace");
    }

    #[test]
    fn undo_redo_round_trip_and_rejection() {
        let mut edit = area("");
        // Rejections at the buffer edges (motions only — the delete
        // family's phantom-snapshot behavior is pinned in its own
        // test below).
        for command in [EditCommand::MoveLeft, EditCommand::MoveRight] {
            assert!(matches!(edit.apply_tracked(command), EditOutcome::Rejected), "{command:?}");
            assert_eq!(observe(&edit), (String::new(), 0, None));
        }
        let _ = step_insert(&mut edit, "a");
        let _ = step_insert(&mut edit, "b");
        assert_eq!(edit.text(), "ab");
        let _ = step(&mut edit, EditCommand::Undo);
        assert_eq!(edit.text(), "a");
        let _ = step(&mut edit, EditCommand::Undo);
        assert_eq!(edit.text(), "");
        let _ = step(&mut edit, EditCommand::Redo);
        assert_eq!(edit.text(), "a");
        let _ = step(&mut edit, EditCommand::Redo);
        assert_eq!(edit.text(), "ab");
    }

    #[test]
    fn wide_clusters_move_by_grapheme() {
        // CJK clusters are one motion unit (the buffer's cluster
        // widths — wide clusters move exactly like upstream's editor).
        let text = "a日本語b";
        let mut edit = area(text);
        edit.set_caret(0);
        assert_eq!(step(&mut edit, EditCommand::MoveRight).1, 1, "one latin char");
        assert_eq!(step(&mut edit, EditCommand::MoveRight).1, 2, "the whole CJK cluster");
        assert_eq!(step(&mut edit, EditCommand::MoveRight).1, 3, "the second CJK cluster");
        assert_eq!(step(&mut edit, EditCommand::MoveRight).1, 4, "the third CJK cluster");
        assert_eq!(step(&mut edit, EditCommand::MoveLeft).1, 3, "and back");
        // Caret conversions survive the round trip across wide rows.
        for caret in 0..=text.chars().count() {
            edit.set_caret(caret);
            assert_eq!(edit.caret(), caret, "caret round trip at {caret}");
        }
    }

    /// Word boundaries follow the buffer's script-transition rules
    /// (`getNextWordBoundary`, edit-buffer.zig:821-946): the
    /// parenthesis run is its own unit.
    #[test]
    fn word_boundaries_follow_upstream() {
        let mut edit = area("fn foo()");
        edit.set_caret(0);
        let _ = edit.apply_tracked(EditCommand::WordForward);
        assert_eq!(edit.caret(), 3, "the first hop agrees (the space)");
        let _ = edit.apply_tracked(EditCommand::WordForward);
        assert_eq!(edit.caret(), 7, "crosses through the paren run");
        let _ = edit.apply_tracked(EditCommand::WordForward);
        assert_eq!(edit.caret(), 8, "the buffer end");
        // A bounded number of hops always reaches the end.
        let mut edit = area("fn foo() bar  baz");
        for _ in 0..12 {
            let _ = edit.apply_tracked(EditCommand::WordForward);
        }
        assert_eq!(edit.caret(), edit.text().chars().count());
    }

    /// Line home/end readline jumps: `gotoLineHome` at col 0 lands on
    /// the PREVIOUS line's end; `gotoLineEnd` at EOL lands on the
    /// NEXT line's start (EditBufferRenderable.ts:791-825).
    #[test]
    fn line_home_end_readline_jumps() {
        let mut edit = area("one\ntwo\nthree");
        // Caret at the start of "two" (row 1, col 0).
        edit.set_caret(4);
        let _ = edit.apply_tracked(EditCommand::LineHome);
        assert_eq!(edit.caret(), 3, "previous line's end");
        // LineEnd at the row's EOL → next line's start.
        edit.set_caret(7); // the true EOL of "two"
        let _ = edit.apply_tracked(EditCommand::LineEnd);
        assert_eq!(edit.caret(), 8, "next line's start");
        // Mid-row LineEnd still goes to the current EOL.
        edit.set_caret(5);
        let _ = edit.apply_tracked(EditCommand::LineEnd);
        assert_eq!(edit.caret(), 7);
    }

    /// Visual motions follow the WRAP model: with a narrow viewport,
    /// up/down cross WRAPPED rows (`EditorView` vlines).
    #[test]
    fn visual_motions_follow_wrap() {
        let mut edit = area("aaa bbb ccc ddd");
        edit.set_viewport_width(7); // wraps after "aaa " etc.
        edit.set_caret(15); // the end
        let _ = edit.apply_tracked(EditCommand::MoveUp);
        assert!(edit.caret() < 15, "visual up leaves the last wrapped row");
        let visual_stop = edit.caret();
        // The donor's word-wrap vline ends at col 6 (the boundary
        // affinity keeps the cursor on the last grapheme, not past
        // the break's trailing space) — editor-view.zig's rule.
        assert_eq!(visual_stop, 6, "visual up lands on the previous WRAPPED row");
    }

    #[test]
    fn mention_chip_ranges_delete_atomically() {
        // The composer's delete_range_tracked path (atomic chips).
        let mut edit = area("keep [Image 1] tail");
        let outcome = edit.delete_range_tracked(5..14);
        assert!(matches!(outcome, EditOutcome::Applied { .. }));
        assert_eq!(edit.text(), "keep  tail");
        assert_eq!(edit.caret(), 5);
    }

    /// `deleteForward` stores an undo point even when it cannot
    /// delete (the ported edit-buffer.zig double-snapshot:
    /// `autoStoreUndo` runs before the guard — one extra undo press
    /// walks a same-state restore).
    #[test]
    fn delete_forward_phantom_snapshot() {
        let mut edit = area("ab");
        edit.set_caret(2); // the end
        let _ = edit.apply_tracked(EditCommand::Delete); // no-op delete…
        assert!(edit.can_undo(), "the undo point was stored");
        // The first undo restores the SAME state (a same-state walk);
        // the text never changed.
        let outcome = edit.apply_tracked(EditCommand::Undo);
        assert!(matches!(outcome, EditOutcome::Applied { .. }));
        assert_eq!(edit.text(), "ab");
    }

    /// Plain moves over a selection collapse to the edge WITHOUT
    /// another step (`collapseSelectionToEdge`,
    /// EditBufferRenderable.ts:727-734 — moveCursorLeft's first arm).
    #[test]
    fn plain_moves_collapse_without_stepping() {
        let mut edit = area("hello world");
        edit.set_caret(5);
        let _ = edit.apply_tracked(EditCommand::SelectRight); // 5..6
        let _ = edit.apply_tracked(EditCommand::MoveLeft);
        assert_eq!(edit.caret(), 5, "collapsed to the selection start, no step");
        // From a whole-buffer selection: left collapses to 0 and the
        // NEXT left is the one that moves.
        let _ = edit.apply_tracked(EditCommand::SelectAll);
        let _ = edit.apply_tracked(EditCommand::MoveLeft);
        assert_eq!((edit.caret(), edit.selection()), (0, None), "collapsed to the edge");
        let _ = edit.apply_tracked(EditCommand::MoveLeft);
        assert_eq!(edit.caret(), 0, "already at 0");
    }

    #[test]
    fn selection_restores_and_collapses_like_upstream() {
        let mut edit = area("hello world");
        edit.set_caret(0);
        let _ = edit.apply_tracked(EditCommand::SelectRight);
        let _ = edit.apply_tracked(EditCommand::SelectRight);
        assert_eq!(edit.selection(), Some(0..2));
        assert_eq!(edit.selected_text().as_deref(), Some("he"));
        // A plain move collapses to the EDGE without another step.
        let _ = edit.apply_tracked(EditCommand::MoveRight);
        assert_eq!(edit.caret(), 2, "collapse-to-end, no extra step");
        assert_eq!(edit.selection(), None);
    }
}
