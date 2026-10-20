//! TUI-4 — the embedded terminal pane (`component/terminal-pane.tsx`
//! + `context/session-terminals.tsx`, lane-tui2):
//!
//! * [`interaction_frame`] — the attach protocol's input framing
//!   (`[type u8][cols u16be][rows u16be][payload]`, input protocol 1
//!   — handlers/persistent_pty.rs §3; the ref's `interactionFrame`,
//!   terminal-pane.tsx:395-403);
//! * [`encode_stroke`] — one keyboard stroke as terminal input bytes
//!   (the embedded terminal's `handleKeyPress` arm the ref delegates
//!   to opentui; the port encodes the same xterm sequences);
//! * [`TerminalPaneState`] — the stream machine: the snapshot
//!   checkpoint fast-forward, the WS output queue held until the VT
//!   grid matches the canonical size, the resize checkpoints
//!   (`ESC c` + base64 checkpoint, terminal-pane.tsx:109-118), the
//!   pending-input backlog until `replay_complete`, and the failure
//!   surface (`Terminal connection failed` / `disconnected`);
//! * [`render_terminal_pane`] — the VT grid as a paint [`Frame`]
//!   (cell chars + colors + bold/underline/reverse attributes).
//!
//! The pane mounts in the session frame's right pane
//! (`components/session.rs` — v2's `rightPane() === "terminal"`),
//! sized by the layout's `terminalWidth` (default half the frame).

use crate::render::{Cell, CellColor, Frame};
use crate::keymap::KeyStroke;

/// The input frame's HAS-INPUT flag value (kind 1 = input; kind 0 =
/// a pure resize control, §3).
pub const FRAME_INPUT: u8 = 1;
/// The control-only frame kind (a resize with no payload).
pub const FRAME_CONTROL: u8 = 0;

/// The attach protocol's binary interaction frame
/// (`[type][cols u16be][rows u16be][payload?]`, ≥5 bytes).
#[must_use]
pub fn interaction_frame(kind: u8, cols: u16, rows: u16, data: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(5 + data.len());
    frame.push(kind);
    frame.extend_from_slice(&cols.to_be_bytes());
    frame.extend_from_slice(&rows.to_be_bytes());
    frame.extend_from_slice(data);
    frame
}

/// One stroke as terminal input bytes (`None` = not representable —
/// the caller drops the stroke). The xterm sequences a VT expects:
/// printable text as UTF-8, `return`/`enter` as CR, backspace 0x7f,
/// the arrows/home/end/page/delete family as SS3/CSI, F1-F4 as SS3,
/// ctrl+letter as the 0x01-0x1a control range, alt as the ESC prefix.
#[must_use]
pub fn encode_stroke(stroke: &KeyStroke) -> Option<Vec<u8>> {
    let key = stroke.key.as_str();
    // Ctrl+letter (and ctrl+@ .. ctrl+_) map onto the C0 range; the
    // terminal side reads them raw.
    if stroke.ctrl && !stroke.alt {
        let byte = match key {
            "@" | " " => 0x00,
            "[" => 0x1b,
            "\\" => 0x1c,
            "]" => 0x1d,
            "^" => 0x1e,
            "_" => 0x1f,
            letter if letter.len() == 1 => {
                let lower = letter.to_ascii_lowercase();
                let ch = lower.chars().next()?;
                if ch.is_ascii_lowercase() {
                    u8::try_from(ch as u32 - 'a' as u32 + 1).ok()?
                } else if ch.is_ascii_digit() {
                    // The historical digit controls (ctrl+2 = NUL …).
                    match ch {
                        '2' => 0x00,
                        '3'..='8' => u8::try_from(ch as u32 - '0' as u32 + 0x1c - 2).ok()?,
                        _ => return None,
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        return Some(vec![byte]);
    }
    let base: Vec<u8> = match key {
        "return" | "enter" => b"\r".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => b"\x1b[15~".to_vec(),
        "f6" => b"\x1b[17~".to_vec(),
        "f7" => b"\x1b[18~".to_vec(),
        "f8" => b"\x1b[19~".to_vec(),
        "f9" => b"\x1b[20~".to_vec(),
        "f10" => b"\x1b[21~".to_vec(),
        "f11" => b"\x1b[23~".to_vec(),
        "f12" => b"\x1b[24~".to_vec(),
        // A single grapheme types as-is (shift folded into the char
        // by the terminal input layer already).
        single if single.chars().count() == 1 => {
            let mut bytes = Vec::new();
            use std::io::Write as _;
            write!(bytes, "{}", single.chars().next()?).ok()?;
            bytes
        }
        _ => return None,
    };
    if stroke.alt {
        let mut prefixed = vec![0x1b];
        prefixed.extend_from_slice(&base);
        return Some(prefixed);
    }
    Some(base)
}

/// One queued stream item (the ref's `StreamItem` union).
#[derive(Debug, Clone)]
enum StreamItem {
    Output(Vec<u8>),
    Resize { cols: u16, rows: u16, checkpoint: Option<Vec<u8>> },
    Ready,
}

/// The pane's live state: the VT grid plus the attach stream machine.
pub struct TerminalPaneState {
    parser: vt100::Parser,
    /// The server's canonical size (set by `attached.info.size` and
    /// each `resized` frame; the grid only applies output while it
    /// MATCHES, terminal-pane.tsx:94).
    canonical: (u16, u16),
    stream: Vec<StreamItem>,
    /// `restored` — `replay_complete` arrived; input may flow
    /// (pending backlog flushes then).
    restored: bool,
    controller: bool,
    wants_control: bool,
    pending_input: Vec<Vec<u8>>,
    /// The pane's own box size (cols-2 for the box's left pad,
    /// terminal-pane.tsx:289) — frames carry THIS size.
    pane_size: (u16, u16),
    /// The last size sent in a frame (suppresses duplicate controls).
    sent_size: Option<(u16, u16)>,
    failure: Option<String>,
}

impl TerminalPaneState {
    /// A fresh pane at the snapshot's size with the checkpoint
    /// fast-forwarded into the grid (`connect()`,
    /// terminal-pane.tsx:175-182).
    pub fn from_snapshot(size: (u16, u16), checkpoint: &[u8]) -> Self {
        let mut parser = vt100::Parser::new(size.1.max(1), size.0.max(1), 0);
        parser.process(checkpoint);
        Self {
            parser,
            canonical: size,
            stream: Vec::new(),
            restored: false,
            controller: false,
            wants_control: false,
            pending_input: Vec::new(),
            pane_size: size,
            sent_size: None,
            failure: None,
        }
    }

    /// The failure surface text (the ref's two connection strings).
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Set the failure surface (`Terminal connection failed` /
    /// `Terminal disconnected`, terminal-pane.tsx:267/275).
    pub fn set_failure(&mut self, message: String) {
        self.failure = Some(message);
    }

    /// Whether the pane owns the terminal's input (controller +
    /// restored — the interaction frames' gate).
    #[must_use]
    pub fn accepts_input(&self) -> bool {
        self.restored && self.controller
    }

    /// The pane box's content size (frames carry it).
    #[must_use]
    pub fn pane_size(&self) -> (u16, u16) {
        self.pane_size
    }

    /// The layout pass: recompute the pane's box size (the renderable
    /// follows the box, `onSizeChange` — terminal-pane.tsx:288-291);
    /// returns the control frame to send when the pane is the
    /// controller and the size changed (`interact()`).
    pub fn resize_pane(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.pane_size = (cols, rows);
        self.parser.set_size(rows, cols);
        self.interact()
    }

    /// A controller interaction frame for the current pane size
    /// (`interact()` — none while not controller/restored, or when
    /// the size already went out).
    #[must_use]
    pub fn interact(&mut self) -> Option<Vec<u8>> {
        if !self.restored || !self.controller {
            return None;
        }
        if self.sent_size == Some(self.pane_size) {
            return None;
        }
        self.sent_size = Some(self.pane_size);
        Some(interaction_frame(FRAME_CONTROL, self.pane_size.0, self.pane_size.1, &[]))
    }

    /// Queue one WS output chunk (the `processStream` gating: output
    /// only applies while the grid matches the canonical size).
    pub fn push_output(&mut self, data: &[u8]) {
        self.stream.push(StreamItem::Output(data.to_vec()));
        self.drain();
    }

    /// Queue a server resize (the checkpoint replaces the grid via
    /// `ESC c` + bytes, terminal-pane.tsx:113-116).
    pub fn push_resize(&mut self, cols: u16, rows: u16, checkpoint: Option<Vec<u8>>) {
        self.stream.push(StreamItem::Resize { cols, rows, checkpoint });
        self.drain();
    }

    /// `replay_complete` — the input gate opens; the pending backlog
    /// flushes as ready-to-send frames (terminal-pane.tsx:120-123's
    /// `input.forEach(sendInput)`).
    pub fn mark_ready(&mut self) -> Vec<Vec<u8>> {
        self.stream.push(StreamItem::Ready);
        self.drain();
        let backlog = std::mem::take(&mut self.pending_input);
        backlog
            .into_iter()
            .map(|data| interaction_frame(FRAME_INPUT, self.pane_size.0, self.pane_size.1, &data))
            .collect()
    }

    /// `controller_changed` (takeover): becoming the controller
    /// sends one interaction frame when already restored.
    pub fn set_controller(&mut self, controller: bool) {
        let previous = self.controller;
        self.controller = controller;
        if controller && !previous && self.restored {
            self.wants_control = true;
            self.drain();
        }
    }

    /// Queue typed input (backlogged until restored,
    /// terminal-pane.tsx:85-91); returns the frames to send now.
    #[must_use]
    pub fn push_input(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        if !self.restored {
            self.pending_input.push(data.to_vec());
            return Vec::new();
        }
        vec![interaction_frame(FRAME_INPUT, self.pane_size.0, self.pane_size.1, data)]
    }

    /// The attach landed (`attached` frame's info size).
    pub fn attached(&mut self, size: (u16, u16), controller: bool) {
        self.controller = controller;
        self.stream.push(StreamItem::Resize { cols: size.0, rows: size.1, checkpoint: None });
        self.drain();
    }

    /// The stream machine's drain loop (`processStream`,
    /// terminal-pane.tsx:93-126): items apply in order — a resize
    /// FORCES the grid to the canonical size
    /// (`setCanonicalSize` writes `terminal.width/height`,
    /// terminal-pane.tsx:57-62) with the checkpoint reset (`ESC c` +
    /// bytes), output writes straight into the grid, and ready opens
    /// the input gate (flushing the pending backlog as frames).
    fn drain(&mut self) {
        while let Some(item) = self.stream.first().cloned() {
            match item {
                StreamItem::Output(data) => {
                    self.stream.remove(0);
                    self.parser.process(&data);
                }
                StreamItem::Resize { cols, rows, checkpoint } => {
                    self.stream.remove(0);
                    self.canonical = (cols, rows);
                    self.parser.set_size(rows, cols);
                    if let Some(checkpoint) = checkpoint {
                        self.parser.process(b"\x1bc");
                        self.parser.process(&checkpoint);
                    }
                }
                StreamItem::Ready => {
                    self.stream.remove(0);
                    self.restored = true;
                    if self.controller || self.wants_control {
                        self.wants_control = false;
                        self.sent_size = None;
                    }
                }
            }
        }
    }
}

/// The VT grid as a paint frame (the pane's whole box).
#[must_use]
pub fn render_terminal_pane(state: &TerminalPaneState, width: u16, height: u16) -> Frame {
    let (width, height) = (usize::from(width), usize::from(height));
    let blank = || {
        Cell { ch: ' ', attrs: 0, fg: CellColor::Default, bg: CellColor::Default }
    };
    let mut cells: Vec<Vec<Cell>> = vec![vec![blank(); width]; height];
    if let Some(failure) = state.failure() {
        let row = &mut cells[0];
        let mut at = 1usize;
        for atom in failure.chars() {
            if at >= width.saturating_sub(1) {
                break;
            }
            row[at] = Cell {
                ch: atom,
                attrs: 0,
                fg: CellColor::Rgb(251, 146, 146),
                bg: CellColor::Default,
            };
            at += atom.len_utf8().max(1);
        }
        return Frame { cells, width, height };
    }
    let screen = state.parser.screen();
    let (grid_rows, grid_cols) = screen.size();
    // The embedded terminal paints at the box's left pad (1).
    for row in 0..usize::from(grid_rows).min(height) {
        for col in 0..usize::from(grid_cols).min(width.saturating_sub(1)) {
            let Some(cell) = screen.cell(
                u16::try_from(row).unwrap_or(0),
                u16::try_from(col).unwrap_or(0),
            ) else {
                continue;
            };
            let ch = cell.contents().chars().next().unwrap_or(' ');
            let fg = vt_color(cell.fgcolor(), false);
            let bg = vt_color(cell.bgcolor(), true);
            let mut attrs = 0u16;
            if cell.bold() {
                attrs |= 1;
            }
            if cell.underline() {
                attrs |= 2;
            }
            if cell.inverse() {
                attrs |= 4;
            }
            cells[row][col + 1] = Cell { ch, attrs, fg, bg };
        }
    }
    Frame { cells, width, height }
}

/// Map the emulator's color onto the paint cell (default/256/RGB).
fn vt_color(color: vt100::Color, background: bool) -> CellColor {
    // SGR 10 default foreground/background distinction is not tracked
    // by the emulator; the paint side treats both as the terminal
    // default.
    let _ = background;
    match color {
        vt100::Color::Default => CellColor::Default,
        vt100::Color::Idx(index) => CellColor::Indexed(index),
        vt100::Color::Rgb(r, g, b) => CellColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str) -> KeyStroke {
        KeyStroke::new(key)
    }

    /// The frame layout (§3): kind, u16be cols/rows, payload.
    #[test]
    fn interaction_frames_are_byte_exact() {
        assert_eq!(
            interaction_frame(FRAME_CONTROL, 40, 24, &[]),
            vec![0, 0, 40, 0, 24]
        );
        assert_eq!(
            interaction_frame(FRAME_INPUT, 80, 30, b"hi"),
            vec![1, 0, 80, 0, 30, b'h', b'i']
        );
    }

    /// The stroke encoder's xterm sequences.
    #[test]
    fn strokes_encode_to_terminal_bytes() {
        assert_eq!(encode_stroke(&stroke("a")).as_deref(), Some(b"a".as_slice()));
        assert_eq!(encode_stroke(&stroke("return")).as_deref(), Some(b"\r".as_slice()));
        assert_eq!(encode_stroke(&stroke("backspace")).as_deref(), Some(b"\x7f".as_slice()));
        assert_eq!(encode_stroke(&stroke("up")).as_deref(), Some(b"\x1b[A".as_slice()));
        assert_eq!(encode_stroke(&stroke("f5")).as_deref(), Some(b"\x1b[15~".as_slice()));
        let mut ctrl_c = stroke("c");
        ctrl_c.ctrl = true;
        assert_eq!(encode_stroke(&ctrl_c).as_deref(), Some(&[0x03][..]));
        let mut alt_a = stroke("a");
        alt_a.alt = true;
        assert_eq!(encode_stroke(&alt_a).as_deref(), Some(b"\x1ba".as_slice()));
        assert_eq!(encode_stroke(&stroke("f13")).is_none(), true);
    }

    /// The stream machine: snapshot fast-forward, the ordered output
    /// application, the resize checkpoint reset (which FORCES the
    /// grid to the server's size), and the input backlog until ready.
    #[test]
    fn stream_machine_gates_output_and_backlogs_input() {
        let mut pane = TerminalPaneState::from_snapshot((10, 3), b"hello");
        assert_eq!(pane.accepts_input(), false);
        // Input before replay_complete backlogs.
        assert!(pane.push_input(b"x").is_empty());
        // Output applies immediately (the grid matches the snapshot).
        pane.push_output(b" there");
        // Attach at a DIFFERENT size: the resize forces the grid.
        pane.attached((20, 5), true);
        pane.push_output(b"!");
        // Not ready yet — replay_complete opens the input gate and
        // flushes the backlog as frames.
        let backlog = pane.mark_ready();
        assert_eq!(backlog.len(), 1);
        assert_eq!(backlog[0][0], FRAME_INPUT);
        assert!(pane.accepts_input());
        let frames = pane.push_input(b"x");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0][0], FRAME_INPUT);
        // A resize checkpoint resets the grid (ESC c + checkpoint)
        // and forces the canonical size.
        pane.push_resize(30, 10, Some(b"reset-paint".to_vec()));
        let frame = render_terminal_pane(&pane, 30, 10);
        let text = frame.text_grid().join("\n");
        assert!(text.contains("reset-paint"), "checkpoint repainted: {text}");
    }

    /// The control frames only flow while controller + restored, and
    /// only once per size.
    #[test]
    fn control_frames_flow_once_per_size() {
        let mut pane = TerminalPaneState::from_snapshot((10, 3), b"");
        assert!(pane.interact().is_none());
        pane.attached((10, 3), true);
        pane.mark_ready();
        let first = pane.interact();
        assert_eq!(first.map(|frame| frame[0]), Some(FRAME_CONTROL));
        assert!(pane.interact().is_none(), "duplicate control suppressed");
        let resize_frame = pane.resize_pane(12, 4);
        assert_eq!(resize_frame.map(|frame| frame[0]), Some(FRAME_CONTROL), "a new size re-sends");
        assert!(pane.interact().is_none(), "the re-sent control is once per size");
    }
}
