//! Lane B3b — the SGR frame emitter (`GridEmitter`).
//!
//! A faithful, standalone-value port of opentui-core's SGR frame writer
//! (`renderer.rs` `emit_frame_sgr`, checkout 4ff646d :2248-2386) plus the
//! terminal-cursor tail (`CursorOutputState::emit`, :165-209). The input
//! is oc-tui's own [`crate::render::Frame`] grid (a prev-grid diff), the
//! output is written to the session terminal's `output()` writer wrapped
//! in the `?2026` synchronized-update envelope (`terminal/modes.rs`
//! `SYNC_BEGIN`/`SYNC_END` :114-116) with a trailing flush — the exact
//! shape of the terminal pump's `pump_output` (`terminal/pumps.rs`
//! :200-228). The envelope is NOT capability-gated, matching upstream
//! (`caps.sync` is recorded but never gates; pumps.rs :200).
//!
//! Byte parity with the upstream writer is pinned by the differential
//! tests below: the same grid sequences fed through this emitter and
//! through opentui's `bench_emit_frame_sgr` must emit identical bytes.
//!
//! The emitter keeps its own prev grid, so it is reusable as a plain
//! value (the B4 mini surface takes it over as-is).

use crate::render::{Cell, CellColor, Frame, CONTINUATION};
use std::io::Write as _;

// -- SGR constants (opentui-core ansi.rs :152 `RESET`; the color trees
//    below mirror `ansi::emit_color`, the `emitColor` decision tree at
//    renderer.zig:844-885 for a full-capability terminal) --------------

const RESET: &str = "\x1b[0m";

/// Synchronized-update envelope open — opentui-core
/// `terminal/modes.rs:114` (`SYNC_BEGIN`).
pub const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
/// Synchronized-update envelope close — opentui-core
/// `terminal/modes.rs:116` (`SYNC_END`).
pub const SYNC_END: &[u8] = b"\x1b[?2026l";

/// ECMA-48 attribute bits in the low byte of the cell's `attrs` word —
/// the exact packing of opentui-core `ansi.rs` `text_attributes`
/// (:14-26) and oc-tui's canonical cell (same bit order, ECMA param 1
/// = bit 0 … param 9 = bit 7).
pub mod text_attributes {
    pub const BOLD: u16 = 1 << 0;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const BLINK: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const HIDDEN: u16 = 1 << 6;
    pub const STRIKETHROUGH: u16 = 1 << 7;
}

// -- SGR color emission (ansi.rs `emit_color` :114-144, specialized to
//    the oc-tui color vocabulary: Default = the terminal-default intent
//    (39/49; an alpha-0 background collapses to 49 in upstream), Indexed
//    = `38;5;n`/`48;5;n`, Rgb = `38;2;r;g;b`/`48;2;r;g;b`) --------------

fn emit_fg(buf: &mut String, color: CellColor) {
    match color {
        CellColor::Default => buf.push_str("\x1b[39m"),
        CellColor::Indexed(index) => buf.push_str(&format!("\x1b[38;5;{index}m")),
        CellColor::Rgb(r, g, b) => buf.push_str(&format!("\x1b[38;2;{r};{g};{b}m")),
    }
}

fn emit_bg(buf: &mut String, color: CellColor) {
    match color {
        CellColor::Default => buf.push_str("\x1b[49m"),
        CellColor::Indexed(index) => buf.push_str(&format!("\x1b[48;5;{index}m")),
        CellColor::Rgb(r, g, b) => buf.push_str(&format!("\x1b[48;2;{r};{g};{b}m")),
    }
}

/// The attribute SGR burst — opentui-core `ansi.rs`
/// `TextAttributes::apply_attributes` (:76-103): fixed order, one SGR
/// per set bit.
fn apply_attributes(buf: &mut String, attrs: u16) {
    if attrs & text_attributes::BOLD != 0 {
        buf.push_str("\x1b[1m");
    }
    if attrs & text_attributes::DIM != 0 {
        buf.push_str("\x1b[2m");
    }
    if attrs & text_attributes::ITALIC != 0 {
        buf.push_str("\x1b[3m");
    }
    if attrs & text_attributes::UNDERLINE != 0 {
        buf.push_str("\x1b[4m");
    }
    if attrs & text_attributes::BLINK != 0 {
        buf.push_str("\x1b[5m");
    }
    if attrs & text_attributes::INVERSE != 0 {
        buf.push_str("\x1b[7m");
    }
    if attrs & text_attributes::HIDDEN != 0 {
        buf.push_str("\x1b[8m");
    }
    if attrs & text_attributes::STRIKETHROUGH != 0 {
        buf.push_str("\x1b[9m");
    }
}

/// CUP — opentui-core `ansi.rs` `move_to` (:145-148), 1-based.
fn move_to(buf: &mut String, x: usize, y: usize) {
    buf.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
}

// -- the terminal cursor tail (renderer.rs CursorOutputState :105-209) --

/// Cursor shape — opentui-core `renderables::textarea::CursorStyle`
/// (the DECSCUSR selector family).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Line,
    Underline,
}

/// The terminal cursor state + its emission diff cache — a faithful
/// port of opentui-core `renderer.rs` `CursorOutputState` (:105-209):
/// position (1-based), visibility, style/blink/color, and the
/// `last_*` diff caches (`None` = never emitted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorOutputState {
    pub x: u32,
    pub y: u32,
    pub visible: bool,
    pub style: CursorStyle,
    pub blinking: bool,
    pub color: (u8, u8, u8),
    last_x: Option<u32>,
    last_y: Option<u32>,
    last_visible: Option<bool>,
    last_style_tag: Option<u8>,
    last_blinking: Option<bool>,
    last_color: Option<[u8; 3]>,
}

impl Default for CursorOutputState {
    fn default() -> Self {
        CursorOutputState {
            x: 1,
            y: 1,
            visible: false,
            style: CursorStyle::Block,
            blinking: true,
            color: (255, 255, 255),
            last_x: None,
            last_y: None,
            last_visible: None,
            last_style_tag: None,
            last_blinking: None,
            last_color: None,
        }
    }
}

impl CursorOutputState {
    /// `Terminal.setCursorPosition` (renderer.rs :143-147): 1-based,
    /// clamped to >= 1.
    pub fn set_position(&mut self, x: u32, y: u32, visible: bool) {
        self.x = x.max(1);
        self.y = y.max(1);
        self.visible = visible;
    }

    /// `Terminal.setCursorStyle` + `setCursorColor` (renderer.rs
    /// :149-158).
    pub fn set_style(&mut self, style: CursorStyle, blinking: bool, color: (u8, u8, u8)) {
        self.style = style;
        self.blinking = blinking;
        self.color = color;
    }

    /// Mark the current position/show state as already emitted (the
    /// no-op-frame cursor pin writes CUP + ?25h directly; the diff
    /// caches must agree so the next real frame does not re-send a
    /// redundant style burst).
    fn pin(&mut self) {
        self.last_x = Some(self.x);
        self.last_y = Some(self.y);
        self.last_visible = Some(true);
    }

    /// The cursor emission tail (renderer.rs `emit` :165-209 —
    /// renderer.zig:2932-3016), appended after the frame's SGR stream.
    /// `frame_started` mirrors whether any cell bytes were written.
    fn emit(&mut self, out: &mut String, frame_started: bool) {
        let style_tag: u8 = match self.style {
            CursorStyle::Block => 1,
            CursorStyle::Line => 3,
            CursorStyle::Underline => 5,
        };
        if self.visible {
            let (cr, cg, cb) = self.color;
            let style_changed =
                self.last_style_tag != Some(style_tag) || self.last_blinking != Some(self.blinking);
            let color_changed = self.last_color != Some([cr, cg, cb]);
            let position_changed = self.last_x != Some(self.x) || self.last_y != Some(self.y);
            let visibility_changed = self.last_visible != Some(true);
            // If any visual output was produced this frame, cursor state
            // must be restored (renderer.zig:2975-2978)
            let needs_restore = frame_started
                || style_changed
                || color_changed
                || position_changed
                || visibility_changed;
            if needs_restore {
                if color_changed {
                    // OSC 12 with the BEL terminator (upstream form; the
                    // boot-time oc-tui constant keeps its ST terminator).
                    out.push_str(&format!("\x1b]12;#{cr:02x}{cg:02x}{cb:02x}\x07"));
                    self.last_color = Some([cr, cg, cb]);
                }
                if style_changed {
                    // DECSCUSR: 1/2 block, 3/4 line, 5/6 underline
                    // (blink/steady).
                    let code = style_tag + u8::from(!self.blinking);
                    out.push_str(&format!("\x1b[{code} q"));
                    self.last_style_tag = Some(style_tag);
                    self.last_blinking = Some(self.blinking);
                }
                out.push_str(&format!("\x1b[{};{}H", self.y, self.x));
                out.push_str("\x1b[?25h");
            }
            self.last_x = Some(self.x);
            self.last_y = Some(self.y);
            self.last_visible = Some(true);
        } else {
            if !frame_started && self.last_visible.is_none_or(|v| v) {
                out.push_str("\x1b[?25l");
            }
            self.last_style_tag = None;
            self.last_blinking = None;
            self.last_color = None;
            self.last_x = None;
            self.last_y = None;
            self.last_visible = Some(false);
        }
    }
}

// -- the emitter -----------------------------------------------------------

/// The prev-grid "blank" cell — the exact vocabulary of a fresh
/// opentui `OptimizedBuffer` (`buffer.rs` `new`/`clear` :235-266: space,
/// attrs 0, fg literal white `Rgba::rgb(255,255,255)`, bg the
/// transparent-black clear argument). A [`CellColor::Default`
/// background maps to the same alpha-0 transparent black, so the
/// diff base is byte-identical on both sides.
fn blank_cell() -> Cell {
    Cell { ch: ' ', attrs: 0, fg: CellColor::Rgb(255, 255, 255), bg: CellColor::Default }
}

fn blank_frame(width: usize, height: usize) -> Frame {
    Frame {
        cells: vec![vec![blank_cell(); width]; height],
        width,
        height,
    }
}

/// The SGR frame emitter — a port of opentui-core's
/// `CliRenderer::emit_frame_sgr` (renderer.rs :2248-2386) operating on
/// oc-tui's own [`Frame`] grids. Owns the previous frame (the diff
/// base) and the cursor diff cache; a standalone value reusable by any
/// frame writer (the B4 mini surface).
#[derive(Debug, Clone)]
pub struct GridEmitter {
    width: usize,
    height: usize,
    prev: Frame,
    /// The `frame_id == 0` analog (renderer.rs :2253): the first
    /// emission takes the forced full-repaint path.
    first: bool,
    pub cursor: CursorOutputState,
}

impl GridEmitter {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        GridEmitter {
            width,
            height,
            prev: blank_frame(width, height),
            first: true,
            cursor: CursorOutputState::default(),
        }
    }

    /// A size change invalidates the prev grid — new blank buffers, the
    /// full-repaint-on-diff semantics of upstream `resize` (renderer.rs
    /// :2141-2152; the next emission diffs against blank, repainting
    /// everything visible). The `first` force flag is NOT re-armed
    /// (upstream keeps `frame_id` across resizes).
    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.prev = blank_frame(width, height);
    }

    /// The current grid size.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Force the next frame down the full-repaint path: the prev grid
    /// drops to blank AND the first-frame force flag re-arms (the
    /// post-editor / post-suspend repaint — the retired writer's
    /// `clear()` equivalent, every cell incl. blanks rewritten).
    pub fn invalidate_full(&mut self) {
        self.prev = blank_frame(self.width, self.height);
        self.first = true;
    }

    /// Diff-emit one frame's SGR bytes (the frame's own byte stream,
    /// WITHOUT the `?2026` envelope — the test seam). Grid cells
    /// outside the emitter's size read as blank; a frame at a different
    /// size than the emitter first invalidates the prev grid (the
    /// resize semantics above).
    pub fn render_sgr(&mut self, frame: &Frame) -> String {
        if frame.width != self.width || frame.height != self.height {
            self.resize(frame.width, frame.height);
        }
        let width = self.width;
        let height = self.height;
        // `frame_id == 0` — the forced full-repaint first frame
        // (renderer.rs :2253).
        let force = self.first;
        self.first = false;
        let mut out = String::new();
        // the SGR state machine — renderer.rs :2254-2352
        let mut current_fg: Option<CellColor> = None;
        let mut current_bg: Option<CellColor> = None;
        let mut current_attrs: Option<u16> = None;
        let mut run_start: i64 = -1;
        let mut run_length: u32 = 0;
        // (No hyperlink branch: the oc-tui cell vocabulary carries no
        // link ids, so the OSC 8 arm collapses to upstream's `0` path —
        // no bytes; renderer.rs :2292-2316.)
        for y in 0..height {
            // row-equality fast path (renderer.rs :2256-2271)
            if !force {
                let prev_row = self.prev.cells.get(y);
                let next_row = frame.cells.get(y);
                let mut equal = true;
                for x in 0..width {
                    let cur = prev_row.and_then(|row| row.get(x)).copied().unwrap_or_else(blank_cell);
                    let next =
                        next_row.and_then(|row| row.get(x)).copied().unwrap_or_else(blank_cell);
                    if cur != next {
                        equal = false;
                        break;
                    }
                }
                if equal {
                    continue;
                }
            }
            for x in 0..width {
                let cur =
                    self.prev.cells.get(y).and_then(|row| row.get(x)).copied().unwrap_or_else(blank_cell);
                let next =
                    frame.cells.get(y).and_then(|row| row.get(x)).copied().unwrap_or_else(blank_cell);
                // unchanged cell: close any open run (renderer.rs
                // :2272-2282)
                if !force && cur == next {
                    if run_length > 0 {
                        out.push_str(RESET);
                        run_start = -1;
                        run_length = 0;
                    }
                    continue;
                }
                let fg_match = current_fg.is_some_and(|c| c == next.fg);
                let bg_match = current_bg.is_some_and(|c| c == next.bg);
                let same_attrs = fg_match && bg_match && current_attrs == Some(next.attrs);
                // run boundary: close the previous run, open a new one at
                // this cell with a fresh CUP + SGR burst (renderer.rs
                // :2325-2343)
                if !same_attrs || run_start == -1 {
                    if run_length > 0 {
                        out.push_str(RESET);
                    }
                    run_start = i64::try_from(x).unwrap_or(-1);
                    run_length = 0;
                    current_fg = Some(next.fg);
                    current_bg = Some(next.bg);
                    current_attrs = Some(next.attrs);
                    move_to(&mut out, x, y);
                    emit_fg(&mut out, next.fg);
                    emit_bg(&mut out, next.bg);
                    apply_attributes(&mut out, next.attrs);
                }
                // print the cell's text — renderer.rs :2345-2352. The
                // wide-cluster continuation cell emits NO bytes; the
                // cursor advances implicitly past it (the wide glyph
                // printed before it already advanced the terminal two
                // columns).
                if next.ch == CONTINUATION {
                    // no bytes; cursor advances implicitly
                } else {
                    out.push(next.ch);
                }
                run_length += 1;
            }
        }
        // run tail RESET — renderer.rs :2354-2356
        if run_length > 0 {
            out.push_str(RESET);
        }
        // (No frame-tail link close: no link ids in this vocabulary —
        // renderer.rs :2358-2364's `link_tail` is always false, so
        // `frame_started` is exactly `run_length > 0`.)
        let frame_started = run_length > 0;
        // the cursor emission tail — renderer.rs :2365-2372
        self.cursor.emit(&mut out, frame_started);
        self.prev = clone_sized(frame, width, height);
        out
    }

    /// Emit one frame to the writer: the SGR stream wrapped in the
    /// `?2026` synchronized-update envelope with a trailing flush —
    /// the exact `pump_output` shape (terminal/pumps.rs :200-228).
    /// The envelope is not capability-gated (upstream never gates it).
    ///
    /// One oc-tui policy on top of the upstream stream: while the
    /// caret is visible, EVERY emitted frame ends with the cursor
    /// assertion (CUP + `?25h`). Upstream's `CursorOutputState::emit`
    /// skips the assertion when `frame_started` is false — and a run
    /// closed by a trailing unchanged cell zeroes `run_length`, so a
    /// spinner-tick frame can carry cell bytes WITHOUT the cursor
    /// tail, stranding the hardware cursor on the last cell run. The
    /// retired writer asserted the cursor position every frame; this
    /// keeps that observable contract (the pty cursor-row gates) while
    /// `render_sgr` stays byte-faithful to the oracle (the
    /// differential gate pins the quirk itself).
    pub fn emit<W: std::io::Write>(&mut self, out: &mut W, frame: &Frame) -> std::io::Result<()> {
        let mut sgr = self.render_sgr(frame);
        if self.cursor.visible && !sgr.ends_with("\x1b[?25h") {
            sgr.push_str(&format!("\x1b[{};{}H\x1b[?25h", self.cursor.y, self.cursor.x));
            self.cursor.pin();
        }
        if sgr.is_empty() {
            return Ok(()); // no-op invisible frame — no envelope (pumps.rs :222-224)
        }
        out.write_all(SYNC_BEGIN)?;
        out.write_all(sgr.as_bytes())?;
        out.write_all(SYNC_END)?;
        out.flush()
    }
}

/// Copy a frame padded/cropped to the emitter's grid size (rows and
/// columns outside the frame read as blank cells, matching
/// [`Frame::cell`]'s out-of-bounds default with the blank vocabulary).
fn clone_sized(frame: &Frame, width: usize, height: usize) -> Frame {
    let mut cells = Vec::with_capacity(height);
    for y in 0..height {
        let row = frame.cells.get(y);
        let mut new_row = Vec::with_capacity(width);
        for x in 0..width {
            new_row.push(
                row.and_then(|r| r.get(x)).copied().unwrap_or_else(blank_cell),
            );
        }
        cells.push(new_row);
    }
    Frame { cells, width, height }
}
