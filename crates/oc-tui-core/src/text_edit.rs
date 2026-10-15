//! Text-edit primitives — v2's textarea semantics for the composer.
//!
//! The command surface is EXACTLY the `input.*` table
//! (`config/keybind.ts:206-243`); the operations mirror
//! `editBufferCommandHandlers` (`@opentui/keymap` addons/opentui/
//! index.js — `editBufferCommandHandlers`): every `input.*` command
//! maps one-to-one onto an `EditBufferRenderable` method
//! (`@opentui/core` renderables/EditBufferRenderable.d.ts:140-205):
//!
//! ```text
//! input.move.left    → moveCursorLeft()            (select: shift+…)
//! input.line.home    → gotoLineHome({select})
//! input.visual.line… → gotoVisualLineHome/End({select})
//! input.buffer.…     → gotoBufferHome/End({select})
//! input.delete.line  → deleteLine()
//! input.word.forward → moveWordForward({select})
//! input.delete.word.backward → deleteWordBackward()
//! input.select.all   → selectAll()
//! input.undo/redo    → undo()/redo()
//! …
//! ```
//!
//! A command that cannot act (no selection to delete, caret at the
//! buffer edge with nothing to undo) returns `false` — v2's REJECTED
//! command: the keymap binding counts as unhandled and the stroke
//! falls through (`runBinding`, index.js:2006-2029).
//!
//! Offsets are CHAR offsets over the draft text (the composer's
//! caret model). Cluster motion (grapheme clusters) is host-pluggable
//! through [`ClusterBounds`] so the composer keeps its wrap-derived
//! cluster behavior (`wrap::flatten`) without this crate depending
//! on the renderer.

use std::ops::Range;

/// The host's grapheme-cluster model: where cluster boundaries sit
/// (char offsets, ascending, starting at 0, ending at char count).
pub trait ClusterBounds {
    fn cluster_starts(&self, text: &str) -> Vec<usize>;
}

/// The default model: every char is its own cluster (unit tests and
/// ASCII hosts).
#[derive(Debug, Clone, Copy, Default)]
pub struct CharClusters;

impl ClusterBounds for CharClusters {
    fn cluster_starts(&self, text: &str) -> Vec<usize> {
        (0..=text.chars().count()).collect()
    }
}

/// One undo/redo snapshot (text + caret + anchor).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: Option<usize>,
}

/// The undo stack bound (v2's native buffer keeps full history; the
/// port bounds it — a draft is small, the bound only stops pathology).
const HISTORY_LIMIT: usize = 512;

/// The text-edit state machine: text + caret + selection anchor +
/// undo/redo stacks.
#[derive(Clone)]
pub struct TextArea {
    pub text: String,
    /// Caret offset in chars.
    pub caret: usize,
    /// The selection anchor (`_keyboardSelectionActive` +
    /// `lastLocalSelection`): `None` = no selection; Some(anchor)
    /// with anchor ≠ caret = an active range.
    pub anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    clusters: std::rc::Rc<dyn ClusterBounds>,
}

impl Default for TextArea {
    fn default() -> Self {
        Self::with_clusters(std::rc::Rc::new(CharClusters))
    }
}

impl TextArea {
    #[must_use]
    pub fn with_clusters(clusters: std::rc::Rc<dyn ClusterBounds>) -> Self {
        Self {
            text: String::new(),
            caret: 0,
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clusters,
        }
    }

    /// Replace the whole state (v2's `setText`: resets history).
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.caret = self.char_len();
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
    }

    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    fn clamp(&self, offset: usize) -> usize {
        offset.min(self.char_len())
    }

    /// The active selection range (normalized, `Some` only when
    /// non-empty) — `getSelection()`.
    #[must_use]
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(anchor.min(self.caret)..anchor.max(self.caret))
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| {
            let start = char_to_byte(&self.text, range.start);
            let end = char_to_byte(&self.text, range.end);
            self.text[start..end].to_owned()
        })
    }

    /// Push an undo snapshot before a mutation (v2's native buffer
    /// creates undo points per edit; the snapshot is the observable
    /// equivalent for `undo()`/`redo()`).
    fn snapshot(&mut self) {
        self.redo.clear();
        self.undo.push(Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }

    /// Delete a char range (clamped); the caret lands at the range
    /// start.
    fn delete_range(&mut self, range: Range<usize>) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end);
        if start >= end {
            return;
        }
        let start_b = char_to_byte(&self.text, start);
        let end_b = char_to_byte(&self.text, end);
        self.text.replace_range(start_b..end_b, "");
        self.caret = start;
    }

    /// Delete the active selection and place the caret at its start
    /// (`deleteSelectedText`) — every insert/delete op runs it first.
    fn collapse_selection_edit(&mut self) {
        if let Some(range) = self.selection() {
            self.delete_range(range);
        }
        self.anchor = None;
    }

    /// `insertText`.
    pub fn insert(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.snapshot();
        self.collapse_selection_edit();
        let byte = char_to_byte(&self.text, self.caret);
        self.text.insert_str(byte, text);
        self.caret += text.chars().count();
        true
    }

    /// Movement core: `select` extends the selection (keeping the
    /// anchor), plain movement collapses it
    /// (`updateSelectionForMovement`). Returns whether anything
    /// observable changed (v2's boolean returns — a move that cannot
    /// move REJECTS the binding).
    fn move_caret(&mut self, to: usize, select: bool) -> bool {
        let target = self.clamp(to);
        let before_caret = self.caret;
        if !select {
            self.anchor = None;
        } else if self.anchor.is_none() {
            self.anchor = Some(self.caret);
        }
        self.caret = target;
        self.caret != before_caret || (select && self.selection().is_some())
    }

    /// Cluster length ending at `caret`.
    fn prev_cluster_len(&self, caret: usize) -> usize {
        let starts = self.clusters.cluster_starts(&self.text);
        let mut prev = 0;
        for start in starts {
            if start >= caret {
                break;
            }
            prev = start;
        }
        caret - prev
    }

    /// Cluster length starting at `caret`.
    fn next_cluster_len(&self, caret: usize) -> usize {
        let starts = self.clusters.cluster_starts(&self.text);
        match starts.iter().find(|start| **start > caret) {
            Some(next) => next - caret,
            None => 1,
        }
    }

    /// The current logical line's start offset (chars).
    fn line_start(&self) -> usize {
        let byte = char_to_byte(&self.text, self.caret);
        self.text[..byte].rfind('\n').map_or(0, |pos| self.text[..pos].chars().count() + 1)
    }

    /// The current logical line's end offset (chars, before `\n`).
    fn line_end(&self) -> usize {
        let byte = char_to_byte(&self.text, self.caret);
        let rest = &self.text[byte..];
        let nl = rest.find('\n').unwrap_or(rest.len());
        self.text[..byte + nl].chars().count()
    }

    // -- word boundaries -------------------------------------------------
    //
    // v2 computes these in the native (zig) EditBuffer
    // (`getNextWordBoundary`/`getPrevWordBoundary`,
    // edit-buffer.d.ts:81-82). The Rust port uses the standard
    // run-based rule: a "word" is a maximal run of word chars
    // (`[alnum_]`), a "run" is any maximal same-class stretch, and a
    // boundary move crosses the current run plus following
    // whitespace to the next run's start (readline/codemirror
    // behavior — the observable results the unit tests pin).

    fn is_word(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn class(ch: char) -> u8 {
        if ch == '\n' {
            0
        } else if ch.is_whitespace() {
            1
        } else if Self::is_word(ch) {
            2
        } else {
            3
        }
    }

    fn char_at(&self, offset: usize) -> Option<char> {
        self.text.chars().nth(offset)
    }

    /// Offset of the next word start after `from`
    /// (`getNextWordBoundary`).
    #[must_use]
    pub fn next_word_start(&self, from: usize) -> usize {
        let len = self.char_len();
        let mut at = from;
        // Over the current run…
        if let Some(ch) = self.char_at(at) {
            let class = Self::class(ch);
            while at < len && self.char_at(at).is_some_and(|ch| Self::class(ch) == class) {
                at += 1;
            }
        }
        // …then any whitespace (not crossing a line break start):
        while at < len && self.char_at(at).is_some_and(|ch| ch != '\n' && ch.is_whitespace()) {
            at += 1;
        }
        at
    }

    /// Offset of the word start at/before `from`
    /// (`getPrevWordBoundary`).
    #[must_use]
    pub fn prev_word_start(&self, from: usize) -> usize {
        let mut at = from.min(self.char_len());
        // Back over whitespace…
        while at > 0 && self.char_at(at - 1).is_some_and(|ch| ch != '\n' && ch.is_whitespace()) {
            at -= 1;
        }
        // …then over the current run.
        if at > 0 {
            let class = self.char_at(at - 1).map_or(3, Self::class);
            while at > 0 && self.char_at(at - 1).is_some_and(|ch| Self::class(ch) == class) {
                at -= 1;
            }
        }
        at
    }

    // -- the v2 command surface ------------------------------------------

    /// Apply one `input.*` command; `false` = the command rejected
    /// (nothing to do — the stroke falls through).
    #[allow(clippy::too_many_lines)] // one arm per v2 `input.*` command
    #[allow(clippy::match_same_arms)] // visual-line aliases share the logical arm (no soft wrap yet)
    pub fn apply(&mut self, command: EditCommand) -> bool {
        match command {
            EditCommand::MoveLeft => {
                self.move_caret(self.caret.saturating_sub(self.prev_cluster_len(self.caret)), false)
            }
            EditCommand::MoveRight => {
                self.move_caret(self.caret + self.next_cluster_len(self.caret), false)
            }
            EditCommand::MoveUp => {
                // Logical-row move (the visual-row move is caret
                // internals — C2): to the previous line's end.
                let start = self.line_start();
                if start == 0 {
                    return false;
                }
                self.move_caret(start - 1, false) && {
                    self.caret = self.line_end();
                    true
                }
            }
            EditCommand::MoveDown => {
                let byte = char_to_byte(&self.text, self.caret);
                if self.text[byte..].find('\n').is_none() {
                    return false;
                }
                self.move_caret(self.text[..=byte].chars().count(), false) && {
                    self.caret = self.line_end();
                    true
                }
            }
            EditCommand::SelectLeft => {
                self.move_caret(self.caret.saturating_sub(self.prev_cluster_len(self.caret)), true)
            }
            EditCommand::SelectRight => {
                self.move_caret(self.caret + self.next_cluster_len(self.caret), true)
            }
            EditCommand::SelectUp => {
                let start = self.line_start();
                if start == 0 {
                    return self.move_caret(0, true);
                }
                self.move_caret(start - 1, true) && {
                    self.caret = self.line_end();
                    true
                }
            }
            EditCommand::SelectDown => {
                let byte = char_to_byte(&self.text, self.caret);
                if self.text[byte..].find('\n').is_none() {
                    return self.move_caret(self.char_len(), true);
                }
                self.move_caret(self.text[..=byte].chars().count(), true) && {
                    self.caret = self.line_end();
                    true
                }
            }
            EditCommand::LineHome => self.move_caret(self.line_start(), false),
            EditCommand::LineEnd => self.move_caret(self.line_end(), false),
            EditCommand::SelectLineHome => self.move_caret(self.line_start(), true),
            EditCommand::SelectLineEnd => self.move_caret(self.line_end(), true),
            // No soft wrap in the composer's model yet: visual-line
            // motions alias the logical ones (the wrap renderer keeps
            // its own row math — C2).
            EditCommand::VisualLineHome => self.move_caret(self.line_start(), false),
            EditCommand::VisualLineEnd => self.move_caret(self.line_end(), false),
            EditCommand::SelectVisualLineHome => self.move_caret(self.line_start(), true),
            EditCommand::SelectVisualLineEnd => self.move_caret(self.line_end(), true),
            EditCommand::BufferHome => self.move_caret(0, false),
            EditCommand::BufferEnd => self.move_caret(self.char_len(), false),
            EditCommand::SelectBufferHome => self.move_caret(0, true),
            EditCommand::SelectBufferEnd => self.move_caret(self.char_len(), true),
            EditCommand::DeleteLine => {
                let start = self.line_start();
                let end = self.line_end();
                if start == end {
                    // v2's deleteLine on an empty line removes the
                    // line itself (the trailing newline).
                    let next = (end + 1).min(self.char_len());
                    if next == end {
                        return false;
                    }
                    self.snapshot();
                    self.delete_range(start..next);
                    return true;
                }
                self.snapshot();
                self.delete_range(start..end);
                true
            }
            EditCommand::DeleteToLineEnd => {
                let (start, end) = (self.caret, self.line_end());
                if start >= end {
                    return false;
                }
                self.snapshot();
                self.delete_range(start..end);
                true
            }
            EditCommand::DeleteToLineStart => {
                let (start, end) = (self.line_start(), self.caret);
                if start >= end {
                    return false;
                }
                self.snapshot();
                self.delete_range(start..end);
                true
            }
            EditCommand::Backspace => {
                if self.selection().is_some() {
                    self.snapshot();
                    self.collapse_selection_edit();
                    return true;
                }
                if self.caret == 0 {
                    return false;
                }
                self.snapshot();
                let start = self.caret - self.prev_cluster_len(self.caret);
                self.delete_range(start..self.caret);
                true
            }
            EditCommand::Delete => {
                if self.selection().is_some() {
                    self.snapshot();
                    self.collapse_selection_edit();
                    return true;
                }
                if self.caret >= self.char_len() {
                    return false;
                }
                self.snapshot();
                let end = self.caret + self.next_cluster_len(self.caret);
                self.delete_range(self.caret..end);
                true
            }
            EditCommand::Newline => self.insert("\n"),
            EditCommand::Undo => self.undo(),
            EditCommand::Redo => self.redo(),
            EditCommand::WordForward => self.move_caret(self.next_word_start(self.caret), false),
            EditCommand::WordBackward => self.move_caret(self.prev_word_start(self.caret), false),
            EditCommand::SelectWordForward => {
                self.move_caret(self.next_word_start(self.caret), true)
            }
            EditCommand::SelectWordBackward => {
                self.move_caret(self.prev_word_start(self.caret), true)
            }
            EditCommand::DeleteWordForward => {
                if self.selection().is_some() {
                    self.snapshot();
                    self.collapse_selection_edit();
                    return true;
                }
                let end = self.next_word_start(self.caret);
                if end == self.caret {
                    return false;
                }
                self.snapshot();
                self.delete_range(self.caret..end);
                true
            }
            EditCommand::DeleteWordBackward => {
                if self.selection().is_some() {
                    self.snapshot();
                    self.collapse_selection_edit();
                    return true;
                }
                let start = self.prev_word_start(self.caret);
                if start == self.caret {
                    return false;
                }
                self.snapshot();
                self.delete_range(start..self.caret);
                true
            }
            EditCommand::SelectAll => {
                if self.char_len() == 0 {
                    return false;
                }
                self.anchor = Some(0);
                self.caret = self.char_len();
                true
            }
        }
    }

    /// Apply one command, reporting the exact text delta (the char
    /// range replaced and the inserted length — a prefix/suffix diff)
    /// plus the history event. Hosts with sidecar state keyed to text
    /// offsets use this to adjust their bookkeeping.
    pub fn apply_tracked(&mut self, command: EditCommand) -> EditOutcome {
        let before_text = self.text.clone();
        let before_caret = self.caret;
        if !self.apply(command) {
            return EditOutcome::Rejected;
        }
        let snapshot = match command {
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
        let (replaced, inserted) = diff_range(&before_text, &self.text);
        let _ = before_caret;
        EditOutcome::Applied { replaced, inserted, snapshot }
    }

    /// Host-facing insert with the same delta/history plumbing as
    /// [`Self::apply_tracked`] (v2's `insertText` — the composer's
    /// paste/char paths).
    pub fn insert_tracked(&mut self, text: &str) -> EditOutcome {
        let before = self.text.clone();
        if !self.insert(text) {
            return EditOutcome::Rejected;
        }
        let (replaced, inserted) = diff_range(&before, &self.text);
        EditOutcome::Applied { replaced, inserted, snapshot: SnapshotKind::Pushed }
    }

    /// Host-facing range delete (v2's `deleteRange` — the composer's
    /// atomic mention-chip deletion).
    pub fn delete_range_tracked(&mut self, range: Range<usize>) -> EditOutcome {
        let before = self.text.clone();
        let start = range.start.min(self.char_len());
        let end = range.end.min(self.char_len());
        if start >= end {
            return EditOutcome::Rejected;
        }
        self.snapshot();
        self.delete_range(start..end);
        let (replaced, inserted) = diff_range(&before, &self.text);
        EditOutcome::Applied { replaced, inserted, snapshot: SnapshotKind::Pushed }
    }

    /// `input.undo` — restores the last snapshot; `false` when the
    /// stack is empty.
    pub fn undo(&mut self) -> bool {
        let snapshot = self.undo.pop();
        self.restore(snapshot, true)
    }

    /// `input.redo`.
    pub fn redo(&mut self) -> bool {
        let snapshot = self.redo.pop();
        self.restore(snapshot, false)
    }

    fn restore(&mut self, snapshot: Option<Snapshot>, onto_redo: bool) -> bool {
        let Some(snapshot) = snapshot else { return false };
        let current = Snapshot { text: self.text.clone(), caret: self.caret, anchor: self.anchor };
        self.text = snapshot.text;
        self.caret = snapshot.caret;
        self.anchor = snapshot.anchor;
        if onto_redo {
            self.redo.push(current);
        } else {
            self.undo.push(current);
        }
        true
    }
}

/// Byte offset of a char index.
fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(byte, _)| byte)
}

/// Prefix/suffix diff of two texts, in CHAR units: the replaced range
/// of `before` and how many chars `after` put there.
pub(crate) fn diff_range(before: &str, after: &str) -> (Range<usize>, usize) {
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

impl std::fmt::Debug for TextArea {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextArea")
            .field("text", &self.text)
            .field("caret", &self.caret)
            .field("anchor", &self.anchor)
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for TextArea {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.caret == other.caret
            && self.anchor == other.anchor
            && self.undo == other.undo
            && self.redo == other.redo
    }
}

/// What an [`TextArea::apply_tracked`] did to the buffer.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn area(text: &str) -> TextArea {
        let mut edit = TextArea::default();
        edit.set_text(text);
        edit
    }

    #[test]
    fn selection_extends_and_collapses() {
        let mut edit = area("hello world");
        edit.caret = 5;
        // shift+right twice extends the selection left-to-right.
        assert!(edit.apply(EditCommand::SelectRight));
        assert_eq!(edit.selection(), Some(5..6));
        assert!(edit.apply(EditCommand::SelectRight));
        assert_eq!(edit.selection(), Some(5..7));
        assert_eq!(edit.selected_text().as_deref(), Some(" w"));
        // A plain move collapses.
        assert!(edit.apply(EditCommand::MoveLeft));
        assert_eq!(edit.selection(), None);
        // shift+left from the middle selects backwards.
        edit.caret = 5;
        assert!(edit.apply(EditCommand::SelectLeft));
        assert_eq!(edit.selection(), Some(4..5));
    }

    #[test]
    fn select_all_and_edit_deletes_the_selection_first() {
        let mut edit = area("hello world");
        assert!(edit.apply(EditCommand::SelectAll));
        assert_eq!(edit.selection(), Some(0..11));
        assert!(edit.apply(EditCommand::Delete));
        assert_eq!(edit.text, "");
        // Undo restores the deleted text AND the selection.
        assert!(edit.apply(EditCommand::Undo));
        assert_eq!(edit.text, "hello world");
        // Redo re-deletes.
        assert!(edit.apply(EditCommand::Redo));
        assert_eq!(edit.text, "");
    }

    #[test]
    fn word_motions_follow_run_boundaries() {
        let mut edit = area("foo bar  baz");
        edit.apply(EditCommand::BufferHome);
        // alt+f from 0 → 4 (start of "bar").
        assert!(edit.apply(EditCommand::WordForward));
        assert_eq!(edit.caret, 4);
        assert!(edit.apply(EditCommand::WordForward));
        assert_eq!(edit.caret, 9, "double space crossed to baz");
        // alt+b back → 4.
        assert!(edit.apply(EditCommand::WordBackward));
        assert_eq!(edit.caret, 4);
        // Punctuation is its own run: "fn foo()" — alt+f from 0 stops
        // after "fn", at the space before "foo".
        let mut edit = area("fn foo()");
        edit.apply(EditCommand::BufferHome);
        edit.apply(EditCommand::WordForward);
        assert_eq!(edit.caret, 3);
        edit.apply(EditCommand::WordForward);
        assert_eq!(edit.caret, 6, "stops at the paren run (no whitespace between)");
        edit.apply(EditCommand::WordForward);
        assert_eq!(edit.caret, 8, "crosses the paren run to the end");
    }

    #[test]
    fn delete_word_ops() {
        // alt+d (delete word forward): deletes through the next word
        // boundary — the word AND the whitespace before the next one.
        let mut edit = area("foo bar baz");
        edit.apply(EditCommand::BufferHome);
        edit.apply(EditCommand::WordForward); // caret 4
        assert!(edit.apply(EditCommand::DeleteWordForward));
        assert_eq!(edit.text, "foo baz");
        assert_eq!(edit.caret, 4);
        // ctrl+w (delete word backward): the boundary rules mirror.
        let mut edit = area("foo bar baz");
        edit.apply(EditCommand::BufferHome);
        edit.apply(EditCommand::WordForward);
        edit.apply(EditCommand::WordForward); // caret 8
        assert!(edit.apply(EditCommand::DeleteWordBackward));
        assert_eq!(edit.text, "foo baz");
        assert_eq!(edit.caret, 4);
        // Both reject at the buffer edges (nothing to do).
        let mut edit = area("one");
        edit.caret = 3;
        assert!(!edit.apply(EditCommand::DeleteWordForward));
        edit.caret = 0;
        assert!(!edit.apply(EditCommand::DeleteWordBackward));
    }

    #[test]
    fn delete_to_line_boundaries_and_delete_line() {
        // ctrl+k (to line end).
        let mut edit = area("hello");
        edit.caret = 2;
        assert!(edit.apply(EditCommand::DeleteToLineEnd));
        assert_eq!(edit.text, "he");
        // ctrl+u (to line start).
        let mut edit = area("hello");
        edit.caret = 3;
        assert!(edit.apply(EditCommand::DeleteToLineStart));
        assert_eq!(edit.text, "lo");
        // ctrl+shift+d deletes the WHOLE line (multi-line text).
        let mut edit = area("one\ntwo\nthree");
        edit.caret = 5; // inside "two"
        assert!(edit.apply(EditCommand::DeleteLine));
        assert_eq!(edit.text, "one\n\nthree");
        // On an empty line it removes the line itself.
        let mut edit = area("one\n\nthree");
        edit.caret = 4;
        assert!(edit.apply(EditCommand::DeleteLine));
        assert_eq!(edit.text, "one\nthree");
    }

    #[test]
    fn line_and_buffer_motions_with_selection() {
        let mut edit = area("one\ntwo\nthree");
        edit.caret = 5;
        // ctrl+a / ctrl+e (line home/end).
        assert!(edit.apply(EditCommand::LineHome));
        assert_eq!(edit.caret, 4);
        assert!(edit.apply(EditCommand::LineEnd));
        assert_eq!(edit.caret, 7);
        // shift+home / shift+end select to the buffer bounds (v2
        // binds the buffer-select variants there — keybind.ts:226-227).
        // The ANCHOR stays at the motion's origin (5's line end = 7):
        // successive select-motions keep the original anchor.
        assert!(edit.apply(EditCommand::SelectBufferHome));
        assert_eq!(edit.selection(), Some(0..7));
        assert!(edit.apply(EditCommand::SelectBufferEnd));
        assert_eq!(edit.selection(), Some(7..13));
        // alt+shift+a / alt+shift+e select to the line bounds.
        let mut edit = area("one\ntwo\nthree");
        edit.caret = 5;
        assert!(edit.apply(EditCommand::SelectLineHome));
        assert_eq!(edit.selection(), Some(4..5));
        assert!(edit.apply(EditCommand::SelectLineEnd));
        assert_eq!(edit.selection(), Some(5..7));
    }

    #[test]
    fn undo_redo_round_trip_rejects_when_empty() {
        let mut edit = area("");
        assert!(!edit.apply(EditCommand::Undo));
        assert!(!edit.apply(EditCommand::Redo));
        assert!(edit.insert("a"));
        assert!(edit.insert("b"));
        assert_eq!(edit.text, "ab");
        assert!(edit.apply(EditCommand::Undo));
        assert_eq!(edit.text, "a");
        assert!(edit.apply(EditCommand::Undo));
        assert_eq!(edit.text, "");
        assert!(!edit.apply(EditCommand::Undo));
        assert!(edit.apply(EditCommand::Redo));
        assert_eq!(edit.text, "a");
        assert!(edit.apply(EditCommand::Redo));
        assert_eq!(edit.text, "ab");
        assert!(!edit.apply(EditCommand::Redo));
    }

    #[test]
    fn rejected_commands_report_false() {
        let mut edit = area("");
        assert!(!edit.apply(EditCommand::MoveLeft));
        assert!(!edit.apply(EditCommand::MoveRight));
        assert!(!edit.apply(EditCommand::Backspace));
        assert!(!edit.apply(EditCommand::Delete));
        assert!(!edit.apply(EditCommand::DeleteToLineEnd));
        assert!(!edit.apply(EditCommand::DeleteToLineStart));
        let mut edit = area("one");
        edit.caret = 0;
        assert!(!edit.apply(EditCommand::MoveUp));
        edit.caret = 3;
        assert!(!edit.apply(EditCommand::MoveDown));
        edit.caret = 0;
        assert!(!edit.apply(EditCommand::LineHome), "already at the line start");
    }

    #[test]
    fn newline_inserts_and_multiline_motions() {
        let mut edit = area("");
        assert!(edit.apply(EditCommand::Newline));
        assert!(edit.apply(EditCommand::Newline));
        assert_eq!(edit.text, "\n\n");
        assert_eq!(edit.caret, 2);
        // Up lands on the previous row's end (the composer's pinned
        // row-move behavior — to the previous line's end).
        assert!(edit.apply(EditCommand::MoveUp));
        assert_eq!(edit.caret, 1);
    }
}
