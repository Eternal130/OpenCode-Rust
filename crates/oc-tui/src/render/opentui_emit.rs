//! Lane D4.2 — the opentui frame emitter (`OpentuiEmitter`).
//!
//! The emission flip: the main TUI pump's frame bytes now
//! come from opentui-core's own SGR frame writer
//! (`CliRenderer::emit_frame_sgr`, checkout e52adf6 renderer/frame.rs
//! :527 — the D1a/b live path) — oc-tui no longer owns a byte writer.
//!
//! Feeding mechanism (ruling (a) refined, variant A — buffer-direct):
//! the pump's composed pixels (the shared-tree composite result, still
//! assembled as a [`Frame`] grid by `render_app`) are written CELL FOR
//! CELL into a dedicated opentui `CliRenderer`'s
//! `next_render_buffer` (the packed representation is same-source:
//! ECMA attrs low byte, the `CellColor` color intents one-to-one with
//! opentui's `ColorIntent`), the cursor state rides the renderer's own
//! `CursorOutputState`, and `bench_emit_frame_sgr` (the doc-hidden
//! public seam over the internal writer) diffs current vs next and
//! emits. The buffers then swap exactly like `render()`'s tail — the
//! harness's proven technique (grid_emitter_tests.rs, lane B3b, green
//! since B3b on the same mapping).
//!
//! Byte parity with the retired [`crate::grid_emitter::GridEmitter`]
//! is pinned by the differential harness (`opentui_emit_diff_tests`)
//! on the golden replay corpus: the same frame sequences through both
//! emitters must emit identical bytes INCLUDING the `?2026`
//! synchronized-update envelope and the oc-tui cursor policies (the
//! per-frame visible-cursor assertion and the per-frame hide
//! assertion), which this emitter layers on top of the upstream
//! stream exactly where the old `GridEmitter::emit` did.
//!
//! Known bounded divergence (documented): multi-codepoint cluster
//! cells print INLINE mid-run (upstream's grapheme path) instead of
//! the old emitter's deferred cluster pass. The golden corpus frames
//! carry no cluster sidecars (paint.rs's "no golden or rig corpus
//! carried wide clusters" note, lane M4), so corpus parity is
//! constructive; cluster-bearing frames keep their pixels (the full
//! symbol still prints) with different framing bytes.

use opentui_core::buffer::{OptimizedBuffer, CHAR_FLAG_CONTINUATION};
use opentui_core::color::Rgba;
use opentui_core::renderer::{CliRenderer, CliRendererConfig, create_cli_renderer};

use super::{Cell, CellColor, ClusterMap, Frame, CONTINUATION};

/// Synchronized-update envelope open — opentui-core
/// `terminal/modes.rs:114` (`SYNC_BEGIN`).
pub const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
/// Synchronized-update envelope close — opentui-core
/// `terminal/modes.rs:116` (`SYNC_END`).
pub const SYNC_END: &[u8] = b"\x1b[?2026l";

/// The blank background vocabulary — the exact clear argument of a
/// fresh `OptimizedBuffer` (`buffer.rs` `new`: transparent black,
/// mirroring the renderer's frame-clear default) and the value the
/// old emitter's `blank_cell` mapped to (`CellColor::Default` bg).
fn transparent_black() -> Rgba {
    Rgba::from_ints(0, 0, 0, 0)
}

/// A [`CellColor`] fg as an opentui color — the same-source mapping
/// of the B3b differential gate (grid_emitter_tests `fg_rgba`):
/// Default → the default intent (`39`), Indexed → the indexed intent
/// (`38;5;n`), Rgb → literal (`38;2;r;g;b`).
fn fg_rgba(color: CellColor) -> Rgba {
    match color {
        CellColor::Default => Rgba::default_color(255, 255, 255, 255),
        CellColor::Indexed(index) => Rgba::from_index(index),
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
    }
}

/// A [`CellColor`] bg as an opentui color — Default collapses to the
/// alpha-0 transparent black (`49` at emission, and equal to the
/// buffer's blank vocabulary so the diff base matches the old
/// emitter's blank prev grid cell-for-cell).
fn bg_rgba(color: CellColor) -> Rgba {
    match color {
        CellColor::Default => transparent_black(),
        CellColor::Indexed(index) => Rgba::from_index(index),
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
    }
}

/// The opentui-backed SGR frame emitter — the `GridEmitter`'s
/// replacement (lane D4.2): a dedicated `CliRenderer` whose buffers
/// carry consecutive composed frames (next = this frame, current =
/// the previous one after the post-emission swap) and whose
/// `bench_emit_frame_sgr` produces the bytes. The renderer carries
/// NO renderables — its tree is empty, `render()` is never called on
/// the live path (it would rasterize blank over the poked cells);
/// only the buffer fields, the cursor state, and the frame counter
/// are exercised.
pub struct OpentuiEmitter {
    renderer: CliRenderer,
    width: usize,
    height: usize,
}

impl OpentuiEmitter {
    /// A fresh emitter at `width`×`height`. The renderer starts at
    /// `frame_id == 0`, so the FIRST emission takes the forced
    /// full-repaint path — the retired emitter's `first` flag
    /// semantics, by construction.
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        let renderer = create_cli_renderer(CliRendererConfig {
            width: u32::try_from(width).unwrap_or(u32::MAX),
            height: u32::try_from(height).unwrap_or(u32::MAX),
            ..CliRendererConfig::default()
        });
        OpentuiEmitter { renderer, width, height }
    }

    /// The renderer's terminal-cursor state + emission diff cache —
    /// the same channel the retired `GridEmitter::cursor` exposed.
    /// Position/show-hide rides `set_position`; the boot seam seeds
    /// shape/blink/color once through `set_style`.
    pub fn cursor(&mut self) -> &mut opentui_core::renderer::CursorOutputState {
        &mut self.renderer.cursor_state
    }

    /// A size change invalidates the diff base — fresh blank buffers
    /// at the new size (upstream `resize`'s full-repaint-on-diff
    /// semantics). The forced-first flag is NOT re-armed (upstream
    /// keeps `frame_id` across resizes; the retired emitter matched).
    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.renderer.config.width = u32::try_from(width).unwrap_or(u32::MAX);
        self.renderer.config.height = u32::try_from(height).unwrap_or(u32::MAX);
        self.renderer.next_render_buffer = OptimizedBuffer::new(
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        );
        self.renderer.current_render_buffer = OptimizedBuffer::new(
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        );
    }

    /// The current grid size.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Force the next frame down the full-repaint path (the
    /// post-editor / post-suspend repaint — the retired writer's
    /// `clear()` equivalent): the frame counter drops to 0 so the
    /// next emission takes the `frame_id == 0` forced path.
    pub fn invalidate_full(&mut self) {
        self.renderer.bench_force_full_frame();
    }

    /// Write one composed frame's cells into the renderer's next
    /// buffer (the same-source packed representation), emit the SGR
    /// stream through opentui's writer, then swap the buffers —
    /// `render()`'s emission tail + swap without the rasterization.
    fn render_sgr(&mut self, frame: &Frame) -> String {
        if frame.width != self.width || frame.height != self.height {
            let (width, height) = (frame.width, frame.height);
            self.resize(width, height);
        }
        poke(&mut self.renderer, frame);
        let mut out = String::new();
        self.renderer.bench_emit_frame_sgr(&mut out);
        // The `render()` tail (renderer/frame.rs :122-127): the
        // just-emitted frame becomes the diff base.
        core::mem::swap(&mut self.renderer.current_render_buffer, &mut self.renderer.next_render_buffer);
        // The forced path (frame_id == 0) does not self-advance
        // through the bench seam — run one EMPTY-tree frame to bump
        // the counter past the forced path, restoring the diff base
        // and the cursor cache around it (render() would blank
        // `next` — fine, every frame is poked in full — and hide the
        // cursor — restored here).
        if self.renderer.frame_id() == 0 {
            let saved_current = std::mem::replace(
                &mut self.renderer.current_render_buffer,
                OptimizedBuffer::new(self.renderer.config.width, self.renderer.config.height),
            );
            let saved_cursor = self.renderer.cursor_state;
            self.renderer.render();
            self.renderer.current_render_buffer = saved_current;
            self.renderer.cursor_state = saved_cursor;
        }
        out
    }

    /// Emit one frame to the writer: the opentui SGR stream with the
    /// two oc-tui cursor policies layered on top (identical to the
    /// retired `GridEmitter::emit`), wrapped in the `?2026`
    /// synchronized-update envelope with a trailing flush — the
    /// exact `pump_output` shape. The envelope is not
    /// capability-gated (upstream never gates it).
    ///
    /// * while the caret is visible, EVERY emitted frame ends with
    ///   the cursor assertion (CUP + `?25h`) — the pty cursor gates.
    /// * while the caret is hidden, every frame carries `?25l` — the
    ///   E1 dialog-open-idle flush-cadence detector.
    /// * a fully invisible frame writes nothing (no envelope).
    pub fn emit<W: std::io::Write>(&mut self, out: &mut W, frame: &Frame) -> std::io::Result<()> {
        let mut sgr = self.render_sgr(frame);
        let cursor = &self.renderer.cursor_state;
        if cursor.visible {
            if !sgr.ends_with("\x1b[?25h") {
                sgr.push_str(&format!("\x1b[{};{}H\x1b[?25h", cursor.y, cursor.x));
            }
        } else if !sgr.ends_with("\x1b[?25l") {
            sgr.push_str("\x1b[?25l");
        }
        if sgr.is_empty() {
            return Ok(()); // no-op invisible frame — no envelope
        }
        out.write_all(SYNC_BEGIN)?;
        out.write_all(sgr.as_bytes())?;
        out.write_all(SYNC_END)?;
        out.flush()
    }
}

/// Poke one composed frame's cells into the renderer's next buffer:
/// clear to the blank vocabulary first (every cell is rewritten; a
/// stale pool from the swapped buffer must not leak grapheme ids),
/// then write every cell through the buffer's own `set` (span
/// planting for cluster starts included).
fn poke(renderer: &mut CliRenderer, frame: &Frame) {
    let buffer = &mut renderer.next_render_buffer;
    buffer.clear(transparent_black());
    for (y, row) in frame.cells.iter().enumerate() {
        let mut x = 0usize;
        while x < row.len() {
            let cell = &row[x];
            if let Some(symbol) = frame.cluster_at(y, x) {
                // A multi-codepoint cluster: allocate the symbol in
                // the buffer's grapheme pool and plant the packed
                // start word (the buffer's own `set` plants the span's
                // continuation cells with the run's colors).
                let mut span = 1usize;
                while x + span < row.len() && row[x + span].ch == CONTINUATION {
                    span += 1;
                }
                let gid = buffer.alloc_grapheme_pub(symbol);
                let word = opentui_core::buffer::pack_grapheme_start(
                    gid,
                    u32::try_from(span).unwrap_or(1),
                );
                set_word(buffer, x, y, word, cell);
                x += span;
                continue;
            }
            let word = if cell.ch == CONTINUATION {
                CHAR_FLAG_CONTINUATION
            } else {
                u32::from(cell.ch)
            };
            set_word(buffer, x, y, word, cell);
            x += 1;
        }
    }
}

fn set_word(buffer: &mut OptimizedBuffer, x: usize, y: usize, word: u32, cell: &Cell) {
    buffer.set_cell_word(
        u32::try_from(x).unwrap_or(u32::MAX),
        u32::try_from(y).unwrap_or(u32::MAX),
        word,
        fg_rgba(cell.fg),
        bg_rgba(cell.bg),
        u32::from(cell.attrs & 0xFF),
    );
}

/// The frame's cluster sidecar, cropped to a size (the digest
/// harness's stable serialization input).
#[must_use]
pub fn cluster_sized(frame: &Frame, width: usize, height: usize) -> ClusterMap {
    frame.clusters.as_ref().map_or_else(ClusterMap::new, |m| {
        m.iter()
            .filter(|&((y, x), _)| *y < height && *x < width)
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    })
}
