//! TUI-16 — mouse text selection → clipboard (util/selection.ts:44-98
//! with app.tsx:546-571): a left-drag over screen cells captures a
//! selection; on release `terminal.copy = "select"` (the non-win32
//! default) writes the selected text to the clipboard with a
//! `Copied to clipboard` toast. `"manual"` (the win32 default) keeps
//! the selection for an explicit copy: ctrl+y (the renderer console's
//! `copy-selection` binding, app.tsx:243) or ctrl+c
//! (selection.ts:82-91), and any other stroke clears it.

use crate::render::Frame;

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
}

impl ScreenSelection {
    /// A left press arms the anchor (every press starts a potential
    /// selection; the release only resolves one when the pointer
    /// crossed cells).
    pub fn press(&mut self, x: usize, y: usize) {
        self.anchor = Some((x, y));
        self.head = None;
        self.dragged = false;
        self.retained = false;
    }

    /// A left drag extends the head; the first crossing of the anchor
    /// cell marks the drag.
    pub fn drag(&mut self, x: usize, y: usize) {
        if let Some(anchor) = self.anchor {
            self.head = Some((x, y));
            if (x, y) != anchor {
                self.dragged = true;
            }
        }
    }

    /// The release resolves the drag: `Some(rect)` when the pointer
    /// crossed cells (a real selection). `retained` keeps the rect
    /// queryable for the manual-mode copy (the state machine resets
    /// its drag arm either way).
    pub fn release(&mut self) -> Option<SelectionRect> {
        let rect = self.rect();
        let resolved = self.dragged.then_some(rect).flatten();
        self.anchor = None;
        self.head = None;
        self.dragged = false;
        self.retained = resolved.is_some();
        resolved
    }

    /// The current selection rect (drag in flight or retained).
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

    /// Clear the selection (`renderer.clearSelection()`).
    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragged = false;
        self.retained = false;
    }
}

/// The selected cells' text (`getSelectedText`): each row's run
/// right-trimmed, rows joined with `\n` — trailing blank rows drop.
#[must_use]
pub fn selected_text(frame: &Frame, rect: SelectionRect) -> String {
    let mut lines: Vec<String> = Vec::new();
    for y in rect.y..rect.y.saturating_add(rect.height).min(frame.cells.len()) {
        let Some(row) = frame.cells.get(y) else { continue };
        let mut text = String::new();
        for x in rect.x..rect.x.saturating_add(rect.width) {
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

/// `handleSelectionKey` (selection.ts:62-98) — the priority-101
/// intercept's verdict for one stroke over a live selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKeyAction {
    /// Copy the selection (`ctrl+y`, or `ctrl+c` in manual mode) and
    /// consume the stroke.
    CopyAndConsume,
    /// Clear the selection and consume the stroke (`ctrl+c` in
    /// copy-on-select mode; `escape` with text).
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
                // (selection.ts:82-86's first arm).
                SelectionKeyAction::ClearAndConsume
            } else {
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
        Frame { width, height: rows.len(), cells }
    }

    #[test]
    fn drag_state_machine_resolves_crossed_cells_only() {
        let mut selection = ScreenSelection::default();
        // A bare click (press + release on the same cell) never
        // selects.
        selection.press(5, 2);
        assert_eq!(selection.release(), None);
        // A crossing drag resolves the normalized rect.
        selection.press(8, 1);
        selection.drag(9, 1);
        selection.drag(5, 3);
        let rect = selection.release().expect("the drag rect");
        assert_eq!(rect, SelectionRect { x: 5, y: 1, width: 4, height: 3 });
        // The release retained it (manual mode's copy window).
        assert!(selection.is_active());
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

    #[test]
    fn key_actions_follow_the_copy_mode() {
        // Copy-on-select: ctrl+c clears (the release already copied).
        assert_eq!(selection_key_action("ctrl+c", true), SelectionKeyAction::ClearAndConsume);
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
