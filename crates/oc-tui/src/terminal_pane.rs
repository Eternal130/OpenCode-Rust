//! TUI-4 — the embedded terminal pane (`component/terminal-pane.tsx`
//! + `context/session-terminals.tsx`, lane-tui2):
//!
//! * [`interaction_frame`] — the attach protocol's input framing
//!   (`[type u8][cols u16be][rows u16be][payload]`, input protocol 1
//!   — `handlers/persistent_pty.rs` §3; the ref's `interactionFrame`,
//!   terminal-pane.tsx:395-403);
//! * [`TerminalPaneState`] — the stream machine: the snapshot
//!   checkpoint fast-forward, the WS output queue held until the VT
//!   grid matches the canonical size, the resize checkpoints
//!   (`ESC c` + base64 checkpoint, terminal-pane.tsx:109-118), the
//!   pending-input backlog until `replay_complete`, the failure
//!   surface (`Terminal connection failed` / `disconnected`), and the
//!   key encoder (delegated to opentui's ghostty port);
//! * [`render_terminal_pane`] — the VT grid as a paint [`Frame`]
//!   (lane D4.1b: the HEADLESS twin — the live pane mounts its
//!   pixels into the shared tree, `components/terminal_pane_solid`).
//!
//! Lane D3-3b: the VT face (grid, scrollback, selection, key/paste
//! encoders, OSC 4/10/11 palette injection) now lives in opentui's
//! [`EmbeddedTerminalState`]/[`Engine`] (lane D3-3a); oc-tui keeps
//! only the attach/stream machine and the paint conversion. The
//! theme-palette computation stays here (it consumes `oc-theme`
//! tokens) but its bytes are applied through the opentui engine API,
//! not fed back into the PTY.
//!
//! The pane mounts in the session frame's right pane
//! (`components/session.rs` — v2's `rightPane() === "terminal"`),
//! sized by the layout's `terminalWidth` (default half the frame).

use crate::keymap::KeyStroke;
use crate::render::{Cell, CellColor, Frame};
use opentui_core::buffer::OptimizedBuffer;
use opentui_core::embedded_terminal::Engine;
use opentui_core::events::KeyEvent;
use opentui_core::parse_keypress::{KeyEventType, KeySource, ParsedKey};
use opentui_core::renderables::embedded_terminal::{
    EmbeddedTerminalOptions, EmbeddedTerminalState,
};

// -- G-141: the theme palette injection (terminal-pane.tsx:338-385) ---------

/// `rgbToOklch` (theme/tui/color.ts:25-38): the Oklab→OkLCh
/// conversion; only the hue/chroma terms the palette matching needs.
fn rgb_to_oklch(red: f64, green: f64, blue: f64) -> (f64, f64, f64) {
    let srgb_to_linear = |value: f64| -> f64 {
        if value <= 0.04045 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
    };
    let linear_red = srgb_to_linear(red);
    let linear_green = srgb_to_linear(green);
    let linear_blue = srgb_to_linear(blue);
    let l_root = (0.412_221_470_8 * linear_red
        + 0.536_332_536_3 * linear_green
        + 0.051_445_992_9 * linear_blue)
        .cbrt();
    let m_root = (0.211_903_498_2 * linear_red
        + 0.680_699_545_1 * linear_green
        + 0.107_396_956_6 * linear_blue)
        .cbrt();
    let s_root = (0.088_302_461_9 * linear_red
        + 0.281_718_837_6 * linear_green
        + 0.629_978_700_5 * linear_blue)
        .cbrt();
    let lightness = 0.210_454_255_3 * l_root + 0.793_617_785 * m_root - 0.004_072_046_8 * s_root;
    let a = 1.977_998_495_1 * l_root - 2.428_592_205 * m_root + 0.450_593_709_9 * s_root;
    let b = 0.025_904_037_1 * l_root + 0.782_771_766_2 * m_root - 0.808_675_766 * s_root;
    let chroma = (a * a + b * b).sqrt();
    let angle = f64::atan2(b, a).to_degrees();
    let hue = if angle < 0.0 { angle + 360.0 } else { angle };
    (lightness, chroma, hue)
}

/// `hueDistance` (terminal-pane.tsx:381-383): the circular hue gap.
fn hue_distance(first: f64, second: f64) -> f64 {
    let difference = (first - second).abs();
    difference.min(360.0 - difference)
}

/// `terminalHue` (terminal-pane.tsx:370-379): the hue scale whose
/// 200-step sits nearest the target OkLCh hue (chroma ≥ 0.03, alpha
/// > 0), else the accent 200 fallback; matches beyond 30° also fall
/// back.
fn terminal_hue(
    theme: &oc_theme::types::ResolvedThemeTokens,
    target: f64,
) -> oc_theme::color::Rgba {
    let mut nearest: Option<(f64, oc_theme::color::Rgba)> = None;
    for scale in theme.hue.values() {
        let color = scale.s200;
        let [red, green, blue, alpha] = color.to_ints();
        let (_, chroma, hue) =
            rgb_to_oklch(f64::from(red) / 255.0, f64::from(green) / 255.0, f64::from(blue) / 255.0);
        if f64::from(alpha) <= 0.0 || chroma < 0.03 {
            continue;
        }
        let distance = hue_distance(hue, target);
        if nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, color));
        }
    }
    match nearest {
        Some((distance, color)) if distance <= 30.0 => color,
        _ => theme.semantic(oc_theme::types::SemanticHue::Accent).s200,
    }
}

/// G-141 — the active theme's 16-color palette + fg/bg, the structured
/// form of the ref's `terminalPalette` (terminal-pane.tsx:338-366).
///
/// Lane D3-3b: the old byte builder (`terminal_palette()`) retired —
/// the values now go through opentui's engine API
/// ([`Engine::set_palette_color`] + the OSC 10/11 receive path) so the
/// pane's own VT grid resolves SGR colors through the theme, exactly
/// like the ref's embedded terminal. The PTY no longer receives these
/// bytes (the shell's own palette stays its own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalThemePalette {
    /// 16 ANSI slots in index order.
    palette: Vec<(u8, u8, u8)>,
    foreground: (u8, u8, u8),
    background: (u8, u8, u8),
}

impl TerminalThemePalette {
    /// Apply to one opentui engine: OSC 4 via `set_palette_color`,
    /// OSC 10/11 through the engine's `write` parser (the D3-3a fold).
    fn apply(&self, engine: &mut Engine) {
        for (index, color) in self.palette.iter().enumerate() {
            if let Ok(index) = u8::try_from(index) {
                engine.set_palette_color(index, *color);
            }
        }
        let (r, g, b) = self.foreground;
        engine.write(format!("\x1b]10;#{r:02x}{g:02x}{b:02x}\x1b\\").as_bytes());
        let (r, g, b) = self.background;
        engine.write(format!("\x1b]11;#{r:02x}{g:02x}{b:02x}\x1b\\").as_bytes());
    }
}

/// `terminalPalette` (terminal-pane.tsx:338-366): the active theme's
/// 16-color palette + fg/bg as structured RGB.
#[must_use]
pub fn terminal_theme_palette(
    theme: &oc_theme::types::ResolvedThemeTokens,
    mode: oc_theme::types::Mode,
    background: oc_theme::color::Rgba,
) -> TerminalThemePalette {
    let dark = mode == oc_theme::types::Mode::Dark;
    let black = if dark {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s800
    } else {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s200
    };
    let white = if dark {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s200
    } else {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s800
    };
    let bright_white = if dark {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s100
    } else {
        theme.semantic(oc_theme::types::SemanticHue::Neutral).s900
    };
    // Pure blue/magenta hues matched against the theme's hue scales.
    let blue = terminal_hue(theme, rgb_to_oklch(0.0, 0.0, 1.0).2);
    let magenta = terminal_hue(theme, rgb_to_oklch(1.0, 0.0, 1.0).2);
    let normal = [
        theme.text.feedback.error.base,
        theme.text.feedback.success.base,
        theme.text.feedback.warning.base,
        blue,
        magenta,
        theme.text.feedback.info.base,
        white,
    ];
    let mut colors: Vec<oc_theme::color::Rgba> = Vec::with_capacity(16);
    colors.push(black);
    colors.extend_from_slice(&normal);
    colors.push(theme.text.muted);
    // `normal.slice(0, -1).map(decrease)` + brightWhite fills 16.
    for color in &normal[..normal.len() - 1] {
        colors.push(theme.decrease(*color, 1.0));
    }
    colors.push(bright_white);
    let rgb = |color: oc_theme::color::Rgba| {
        let [red, green, blue, _] = color.to_ints();
        (red, green, blue)
    };
    TerminalThemePalette {
        palette: colors.into_iter().map(rgb).collect(),
        foreground: rgb(theme.text.base),
        background: rgb(background),
    }
}

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

/// The legacy C0 arm (`encode_stroke` TUI-4): ctrl+letter and the
/// punctuation/digit controls. Kept as the pre-D3-3b byte contract;
/// opentui's kitty CSI-u path differs for `ctrl+[` and `ctrl+digit`,
/// so those strokes stay on the explicit C0 map.
fn legacy_ctrl_byte(key: &str) -> Option<u8> {
    match key {
        "@" | " " => Some(0x00),
        "[" => Some(0x1b),
        "\\" => Some(0x1c),
        "]" => Some(0x1d),
        "^" => Some(0x1e),
        "_" => Some(0x1f),
        letter if letter.len() == 1 => {
            let lower = letter.to_ascii_lowercase();
            let ch = lower.chars().next()?;
            if ch.is_ascii_lowercase() {
                u8::try_from(ch as u32 - 'a' as u32 + 1).ok()
            } else if ch.is_ascii_digit() {
                // The historical digit controls (ctrl+2 = NUL …).
                match ch {
                    '2' => Some(0x00),
                    '3'..='8' => u8::try_from(ch as u32 - '0' as u32 + 0x1c - 2).ok(),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The physical-key code for F1-F12 (`physicalKey` needs the CamelCase
/// name; a bare `f5` name maps to `Unidentified`).
fn function_key_code(key: &str) -> Option<String> {
    let number: u8 = key.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&number).then(|| format!("F{number}"))
}

/// One [`KeyStroke`] as an opentui [`KeyEvent`] (the renderer's input
/// shape; `sequence` carries printable text, named keys leave it
/// empty so `textualKey` stays blank).
fn key_event(stroke: &KeyStroke) -> KeyEvent {
    let single = stroke.key.chars().count() == 1;
    KeyEvent::new(ParsedKey {
        name: stroke.key.clone(),
        ctrl: stroke.ctrl,
        meta: false,
        shift: stroke.shift,
        option: false,
        sequence: if single { stroke.key.clone() } else { String::new() },
        number: false,
        raw: String::new(),
        event_type: KeyEventType::Press,
        source: KeySource::Raw,
        code: function_key_code(&stroke.key),
        super_key: None,
        hyper: None,
        caps_lock: None,
        num_lock: None,
        base_code: None,
        repeated: None,
    })
}

/// One queued stream item (the ref's `StreamItem` union).
#[derive(Debug, Clone)]
enum StreamItem {
    Output(Vec<u8>),
    Resize { cols: u16, rows: u16, checkpoint: Option<Vec<u8>> },
    Ready,
}

/// The pane's live state: the opentui VT engine plus the attach stream
/// machine.
pub struct TerminalPaneState {
    terminal: EmbeddedTerminalState,
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
    #[must_use]
    pub fn from_snapshot(size: (u16, u16), checkpoint: &[u8]) -> Self {
        let mut terminal = Self::fresh_terminal(size);
        terminal.write(checkpoint);
        Self {
            terminal,
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

    /// A new engine at `size` with the staged theme palette applied.
    fn fresh_terminal(size: (u16, u16)) -> EmbeddedTerminalState {
        let mut terminal = EmbeddedTerminalState::new(&EmbeddedTerminalOptions {
            cols: Some(size.0.max(1)),
            rows: Some(size.1.max(1)),
            max_scrollback: Some(0),
            ..EmbeddedTerminalOptions::default()
        });
        if let Some(palette) = theme_palette() {
            palette.apply(&mut terminal.engine);
        }
        terminal
    }

    /// Apply the active theme's palette to the pane's engine (the
    /// store fans a theme change out to every mounted pane).
    pub fn apply_theme(&mut self, palette: &TerminalThemePalette) {
        palette.apply(&mut self.terminal.engine);
    }

    /// One stroke as terminal input bytes (`None` = not representable —
    /// the caller drops the stroke). Delegates to opentui's ghostty
    /// encoder (`Engine::encode_key`); the legacy C0 and alt-prefix
    /// arms preserve the pre-D3-3b byte contract.
    #[must_use]
    pub fn encode_stroke(&self, stroke: &KeyStroke) -> Option<Vec<u8>> {
        if stroke.ctrl && !stroke.alt {
            return legacy_ctrl_byte(&stroke.key).map(|byte| vec![byte]);
        }
        if stroke.alt {
            // The ghostty encoder's legacyAltPrefix is off in the port;
            // the pane's contract prefixes ESC (and ignores ctrl).
            let plain = KeyStroke {
                alt: false,
                ctrl: false,
                ..stroke.clone()
            };
            let mut bytes = self.encode_stroke(&plain)?;
            bytes.insert(0, 0x1b);
            return Some(bytes);
        }
        let bytes = self.terminal.encode_key(&key_event(stroke));
        (!bytes.is_empty()).then_some(bytes)
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
        self.terminal.engine.resize(cols.max(1), rows.max(1));
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
                    self.terminal.write(&data);
                }
                StreamItem::Resize { cols, rows, checkpoint } => {
                    self.stream.remove(0);
                    self.canonical = (cols, rows);
                    match checkpoint {
                        // `ESC c` (RIS) resets the screen; opentui's
                        // engine leaves RIS to the consumer, so a
                        // checkpoint resize starts from a fresh engine.
                        Some(checkpoint) => {
                            let mut terminal = Self::fresh_terminal((cols, rows));
                            terminal.write(&checkpoint);
                            self.terminal = terminal;
                        }
                        None => self.terminal.engine.resize(cols.max(1), rows.max(1)),
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

/// Map one opentui buffer color onto the paint cell.
fn rgba_color(color: opentui_core::color::Rgba) -> CellColor {
    if color.alpha() == 0 {
        CellColor::Default
    } else {
        CellColor::Rgb(color.red(), color.green(), color.blue())
    }
}

/// The composed cell's ECMA-48 attribute bits. INVERSE is consumed by
/// the engine's compositor (it swaps fg/bg), so it is cleared here to
/// avoid a second SGR-7 swap at emission.
fn paint_attrs(attributes: u32) -> u16 {
    let bits = (attributes & 0xFF) as u16;
    bits & !crate::grid_emitter::text_attributes::INVERSE
}

/// One composed paint cell (the retired Frame conversion's unit —
/// lane D4.1b: the walk feeds BOTH the shared-tree mount's row
/// shaping and the headless twin's Frame, so the two agree by
/// construction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneCell {
    pub ch: char,
    pub attrs: u16,
    pub fg: CellColor,
    pub bg: CellColor,
}

/// The VT grid as paint cells (the pane's whole box) — the engine's
/// composed `OptimizedBuffer` walked cell-by-cell with the paint
/// semantics (`rgba_color`/`paint_attrs`, the box's trailing blank
/// column). Lane D4.1b: no Frame materializes here anymore — the
/// shared-tree mount shapes these into rows
/// (`terminal_pane_solid::terminal_pane_mount`) and the headless
/// twin folds them back into a standalone Frame.
#[must_use]
pub(crate) fn pane_cells(
    state: &TerminalPaneState,
    width: usize,
    height: usize,
) -> Vec<Vec<PaneCell>> {
    let blank = || PaneCell { ch: ' ', attrs: 0, fg: CellColor::Default, bg: CellColor::Default };
    let mut cells: Vec<Vec<PaneCell>> = vec![vec![blank(); width]; height];
    if let Some(failure) = state.failure() {
        let row = &mut cells[0];
        let mut at = 1usize;
        for atom in failure.chars() {
            if at >= width.saturating_sub(1) {
                break;
            }
            row[at] = PaneCell {
                ch: atom,
                attrs: 0,
                fg: CellColor::Rgb(251, 146, 146),
                bg: CellColor::Default,
            };
            at += atom.len_utf8().max(1);
        }
        return cells;
    }
    // The embedded terminal paints at the box's left pad (1).
    let mut buffer = OptimizedBuffer::new(width as u32, height as u32);
    state.terminal.paint(&mut buffer, 0, 0);
    for (row, cells_row) in cells.iter_mut().enumerate().take(height) {
        for (col, slot) in cells_row.iter_mut().enumerate().take(width.saturating_sub(1)) {
            let Some(cell) = buffer.get(col as u32, row as u32) else {
                continue;
            };
            *slot = PaneCell {
                ch: char::from_u32(cell.char).unwrap_or(' '),
                attrs: paint_attrs(cell.attributes),
                fg: rgba_color(cell.fg),
                bg: rgba_color(cell.bg),
            };
        }
    }
    cells
}

/// The VT grid as a paint frame — lane D4.1b's HEADLESS TWIN: the
/// standalone-Frame readback of [`pane_cells`] for the unit
/// harnesses (the live pane rides the shared tree; no Frame comes
/// back to `render_session`).
#[must_use]
pub fn render_terminal_pane(state: &TerminalPaneState, width: u16, height: u16) -> Frame {
    let (width, height) = (usize::from(width), usize::from(height));
    let cells: Vec<Vec<Cell>> = pane_cells(state, width, height)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| Cell { ch: cell.ch, attrs: cell.attrs, fg: cell.fg, bg: cell.bg })
                .collect()
        })
        .collect();
    Frame { cells, width, height, clusters: None }
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
        assert_eq!(interaction_frame(FRAME_CONTROL, 40, 24, &[]), vec![0, 0, 40, 0, 24]);
        assert_eq!(
            interaction_frame(FRAME_INPUT, 80, 30, b"hi"),
            vec![1, 0, 80, 0, 30, b'h', b'i']
        );
    }

    /// The stroke encoder's xterm sequences (opentui engine delegation
    /// with the legacy C0/alt arms).
    #[test]
    fn strokes_encode_to_terminal_bytes() {
        let pane = TerminalPaneState::from_snapshot((80, 24), b"");
        assert_eq!(pane.encode_stroke(&stroke("a")).as_deref(), Some(b"a".as_slice()));
        assert_eq!(pane.encode_stroke(&stroke("return")).as_deref(), Some(b"\r".as_slice()));
        assert_eq!(pane.encode_stroke(&stroke("backspace")).as_deref(), Some(b"\x7f".as_slice()));
        assert_eq!(pane.encode_stroke(&stroke("up")).as_deref(), Some(b"\x1b[A".as_slice()));
        assert_eq!(pane.encode_stroke(&stroke("f5")).as_deref(), Some(b"\x1b[15~".as_slice()));
        let mut ctrl_c = stroke("c");
        ctrl_c.ctrl = true;
        assert_eq!(pane.encode_stroke(&ctrl_c).as_deref(), Some(&[0x03][..]));
        let mut alt_a = stroke("a");
        alt_a.alt = true;
        assert_eq!(pane.encode_stroke(&alt_a).as_deref(), Some(b"\x1ba".as_slice()));
        assert!(pane.encode_stroke(&stroke("f13")).is_none());
    }

    /// The stream machine: snapshot fast-forward, the ordered output
    /// application, the resize checkpoint reset (which FORCES the
    /// grid to the server's size), and the input backlog until ready.
    #[test]
    fn stream_machine_gates_output_and_backlogs_input() {
        let mut pane = TerminalPaneState::from_snapshot((10, 3), b"hello");
        assert!(!pane.accepts_input());
        // Input before replay_complete backlogs.
        assert_eq!(pane.push_input(b"x"), [] as [std::vec::Vec<u8>; 0]);
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

    /// Theme injection rides the opentui engine: OSC 4 sets the ANSI
    /// slot the grid resolves, OSC 10/11 set the pane's defaults, and
    /// the composed frame carries the theme colors.
    #[test]
    fn theme_palette_injects_through_the_engine() {
        let palette = TerminalThemePalette {
            palette: vec![(1, 2, 3); 16],
            foreground: (4, 5, 6),
            background: (7, 8, 9),
        };
        let mut pane = TerminalPaneState::from_snapshot((20, 4), b"");
        pane.apply_theme(&palette);
        assert_eq!(pane.terminal.engine.palette_color(1), (1, 2, 3));
        // SGR 31 (red) resolves through the injected palette slot 1.
        pane.push_output(b"\x1b[31mR");
        let frame = render_terminal_pane(&pane, 20, 4);
        assert_eq!(frame.cell(0, 0).ch, 'R');
        assert_eq!(frame.cell(0, 0).fg, CellColor::Rgb(1, 2, 3));
        // OSC 11 painted the default background of a blank cell.
        assert_eq!(frame.cell(2, 10).bg, CellColor::Rgb(7, 8, 9));
        assert_eq!(frame.cell(2, 10).fg, CellColor::Rgb(4, 5, 6));
    }
}

/// G-141 — the live palette snapshot (the loop refreshes it when the
/// theme name/mode changes; new panes apply it on construction and the
/// store fans it out to mounted panes).
/// Bounded deviation: the ref recomputes inside the pane's createEffect
/// on token changes (terminal-pane.tsx:157-160) — the port's attach
/// task has no store access, so the loop-stage snapshot carries it.
pub fn set_theme_palette(palette: TerminalThemePalette) {
    THEME_PALETTE.with_borrow_mut(|slot| *slot = Some(palette));
}

/// The staged palette (empty until the first theme resolves).
#[must_use]
pub fn theme_palette() -> Option<TerminalThemePalette> {
    THEME_PALETTE.with_borrow(Clone::clone)
}

thread_local! {
    static THEME_PALETTE: std::cell::RefCell<Option<TerminalThemePalette>> =
        const { std::cell::RefCell::new(None) };
}
