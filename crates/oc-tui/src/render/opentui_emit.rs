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
//! harness's proven technique (the B3b grid-emitter differential
//! gate, green since lane B3b on the same mapping; the harness now
//! lives on as the pinned digests of `opentui_emit_diff_tests`).
//!
//! Byte parity with the retired `GridEmitter` (deleted this lane) is
//! pinned by the differential proof and the frozen digests of
//! `opentui_emit_diff_tests` on the golden replay corpus: the same
//! frame sequences through both
//! emitters emitted identical bytes INCLUDING the `?2026`
//! synchronized-update envelope and the oc-tui cursor policies (the
//! per-frame visible-cursor assertion and the per-frame hide
//! assertion), which this emitter layers on top of the upstream
//! stream exactly where the old `GridEmitter::emit` did.
//!
//! MF-1 #2 (closed): multi-codepoint cluster cells print INLINE
//! mid-run through upstream's own grapheme path — the pool-backed
//! grapheme word pokes below, the emitter's post-cluster CUP
//! reposition, and the frame diff's pool-bytes equality (all on the
//! opentui-rust side, gf2 rev 69466c4). The retired oc-tui deferred
//! cluster pass (G-452's spaces + per-cluster CUP/SGR burst) is
//! deleted: cluster-bearing frames now carry the REF's own byte
//! shape. The golden corpus frames carry no cluster sidecars
//! (paint.rs's "no golden or rig corpus carried wide clusters" note,
//! lane M4), so corpus parity stays constructive.

use opentui_core::buffer::{OptimizedBuffer, CHAR_FLAG_CONTINUATION};
use opentui_core::color::Rgba;
use opentui_core::renderer::{create_cli_renderer, CliRenderer, CliRendererConfig};

use super::{Cell, CellColor, Frame, CONTINUATION};
use std::fmt::Write as _; // format_push_string: write! into the existing String

/// G-287⑧ — one OSC-8 link rectangle of the frame being emitted:
/// `width` cells at (`x`, `y`) wrap `url`. Source: the transcript
/// walk's `OpenLink` hit rows (the link runs' own glyph extents in
/// screen space — the same rects the mouse release dispatches on),
/// filtered by the probe's hyperlink verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameLinkRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub url: String,
}

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
/// of the B3b grid-emitter differential gate (`fg_rgba`):
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
    /// G-457 — the last-emitted cursor assertion (visible, y, x). A
    /// frame whose DIFF is empty and whose cursor state is unchanged
    /// emits NOTHING (upstream's no-op-frame rule, renderer.zig:
    /// 3029-3032 — the cursor tail below used to force bytes out of
    /// every pass while the caret was visible, so back-to-back
    /// redundant passes produced sub-frame-interval feed gaps the
    /// E1 rhythm detector legitimately flagged).
    last_cursor: Option<(bool, u32, u32)>,
    /// G-457 — the last real emission's instant: the idle heartbeat
    /// (the cursor assertion on an otherwise-empty frame — the E1
    /// dialog-open-idle flush-cadence source) is rate-limited to one
    /// per [`IDLE_HEARTBEAT_MS`] so back-to-back redundant passes
    /// (the spinner pin's boundary + follow-up wake pairs, ~10 ms
    /// apart) stay byte-silent.
    last_emit_at: Option<std::time::Instant>,
}

/// The idle heartbeat's eligibility window — 90 ms threads BOTH
/// constraints: under the app tick (~100 ms — an anchored-tick pass
/// must always be past the window, or heartbeat gaps jitter toward
/// 2 ticks and the E1 detector reads them as stalls) and above the
/// busy cadence's intra-frame follow-up wakes (≤80 ms after the
/// spinner boundary frame — those must stay suppressed or sub-70 ms
/// feed gaps merge bursts).
const IDLE_HEARTBEAT_MS: u64 = 90;

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
        OpentuiEmitter {
            renderer,
            width,
            height,
            last_cursor: None,
            last_emit_at: None,
        }
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
    ///
    /// Multi-codepoint cluster cells ride the DEFERRED CLUSTER PASS
    /// (the retired emitter's pixel-preserving policy, kept on top of
    /// the opentui stream): their cells poke as continuation words
    /// (no inline bytes — upstream's inline grapheme print drifts the
    /// run under the vt width model, the a3 wide-char corpus red),
    /// and after the opentui stream every CHANGED cluster prints with
    /// its own CUP + SGR burst + full symbol + RESET. Cluster-bearing
    /// frames therefore keep their PIXELS with a bounded byte
    /// divergence from the retired emitter (the cursor tail order);
    /// the golden corpus carries no clusters, so corpus parity stays
    /// byte-exact.
    fn render_sgr(&mut self, frame: &Frame, links: &[FrameLinkRect]) -> String {
        if frame.width != self.width || frame.height != self.height {
            let (width, height) = (frame.width, frame.height);
            self.resize(width, height);
        }
        // (MF-1 #2: the retired deferred cluster pass carried the
        // `force` flag; the inline path needs no cluster-side force —
        // frame_id == 0 is the buffer emitter's own full-repaint.)
        // G-287⑧ — resolve the frame's link rects into per-cell link
        // ids over the renderer's own link pool (`allocLink`/
        // link.zig — interned, so the same URL keeps its id across
        // frames): the poke packs the id into cell attributes 8-31
        // and `emit_frame_sgr`'s OSC 8 branch resolves it back to
        // `\x1b]8;id=…;URL\x1b\\` opens (renderer/frame.rs:582-614).
        // No links (the headless/no-probe profile) → an empty map and
        // byte-identical emission.
        let mut link_ids: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        let mut link_cells: std::collections::HashMap<(usize, usize), u32> =
            std::collections::HashMap::new();
        for link in links {
            let id = *link_ids
                .entry(link.url.clone())
                .or_insert_with(|| self.renderer.alloc_link(&link.url));
            for x in link.x..link.x.saturating_add(link.width) {
                link_cells.insert((x, link.y), id);
            }
        }
        poke(&mut self.renderer, frame, &link_cells);
        // MF-1 #2 — clusters print inline in this stream (the poked
        // grapheme words + upstream's post-cluster CUP); the retired
        // deferred cluster pass and its sidecar diff are gone.
        let mut out = String::new();
        self.renderer.bench_emit_frame_sgr(&mut out);
        // The `render()` tail (renderer/frame.rs :122-127): the
        // just-emitted frame becomes the diff base.
        core::mem::swap(
            &mut self.renderer.current_render_buffer,
            &mut self.renderer.next_render_buffer,
        );
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
    pub fn emit<W: std::io::Write>(
        &mut self,
        out: &mut W,
        frame: &Frame,
    ) -> std::io::Result<usize> {
        self.emit_with_links(out, frame, &[])
    }

    /// G-287⑧ — [`Self::emit`] with the frame's OSC-8 link rects: the
    /// poked cells of each rect carry the interned link id and the
    /// opentui SGR stream wraps them in `\x1b]8;id=…;URL\x1b\\` …
    /// `\x1b]8;;\x1b\\` (the terminal's native click-open). An empty
    /// `links` slice is byte-identical to [`Self::emit`] — the
    /// headless/no-probe profile emits no OSC 8.
    pub fn emit_with_links<W: std::io::Write>(
        &mut self,
        out: &mut W,
        frame: &Frame,
        links: &[FrameLinkRect],
    ) -> std::io::Result<usize> {
        self.emit_opts(out, frame, links, true)
    }

    /// [`Self::emit_with_links`] with the idle-heartbeat gate: while
    /// an animation owns the flush cadence (the spinner pin / the
    /// marquee — content frames already flow at their own interval)
    /// the cursor-assertion heartbeat stays OFF, or its extra empty
    /// frames land between the animation's and merge the VT bursts.
    pub fn emit_opts<W: std::io::Write>(
        &mut self,
        out: &mut W,
        frame: &Frame,
        links: &[FrameLinkRect],
        idle_heartbeat: bool,
    ) -> std::io::Result<usize> {
        let mut sgr = self.render_sgr(frame, links);
        let cursor = &self.renderer.cursor_state;
        let cursor_now: (bool, u32, u32) = (cursor.visible, cursor.y, cursor.x);
        // ms truncation is the intended granularity (a >584 My elapsed
        // is impossible in-process).
        #[allow(clippy::cast_possible_truncation)]
        let since_last = self.last_emit_at.map_or(u64::MAX, |at| at.elapsed().as_millis() as u64);
        if sgr.is_empty()
            && self.last_cursor == Some(cursor_now)
            && (!idle_heartbeat || since_last < IDLE_HEARTBEAT_MS)
        {
            // G-457 — a redundant no-op pass inside the heartbeat
            // window: empty diff, unchanged cursor, nothing new to
            // assert. Byte-silent (upstream's no-op-frame rule,
            // renderer.zig:3029-3032); past the window the cursor
            // assertion goes out as the idle heartbeat.
            return Ok(0);
        }
        if cursor.visible {
            if !sgr.ends_with("\x1b[?25h") {
                let _ = write!(sgr, "\x1b[{};{}H\x1b[?25h", cursor.y, cursor.x);
            }
        } else if !sgr.ends_with("\x1b[?25l") {
            sgr.push_str("\x1b[?25l");
        }
        self.last_cursor = Some(cursor_now);
        if sgr.is_empty() {
            return Ok(0); // no-op invisible frame — no envelope
        }
        self.last_emit_at = Some(std::time::Instant::now());
        let bytes = SYNC_BEGIN.len() + sgr.len() + SYNC_END.len();
        out.write_all(SYNC_BEGIN)?;
        out.write_all(sgr.as_bytes())?;
        out.write_all(SYNC_END)?;
        out.flush()?;
        Ok(bytes)
    }
}

/// Poke one composed frame's cells into the renderer's next buffer:
/// clear to the blank vocabulary first (every cell is rewritten; a
/// stale pool from the swapped buffer must not leak grapheme ids),
/// then write every cell through the buffer's own `set`. Cluster
/// cells (start + their continuation span) poke as SPACES — G-452:
/// the deferred cluster pass prints the real bytes after the opentui
/// stream, and the base pass must RESERVE the span's columns (a
/// continuation word emits no bytes, so sequential base text would
/// drift left under the cluster and the cluster pass would overwrite
/// it — the observed `w13-g-emoji` misalignment). Spaces reserve the
/// frame columns; last-writer-wins cluster bytes then land exactly on
/// them (a wide cluster's vt write blanks its covered cells, and a
/// narrower-than-width emission shape like `❤️` keeps the reserved
/// blank — the ref's padded extent, see wrap's `ref_emission_shape`).
fn poke(
    renderer: &mut CliRenderer,
    frame: &Frame,
    link_cells: &std::collections::HashMap<(usize, usize), u32>,
) {
    let buffer = &mut renderer.next_render_buffer;
    buffer.clear(transparent_black());
    for (y, row) in frame.cells.iter().enumerate() {
        let mut x = 0usize;
        while x < row.len() {
            let cell = &row[x];
            // G-452 — the vt-advance accounting of the base SGR pass
            // (a sequential byte stream a real terminal advances by
            // MF-1 #2 — a CLUSTER span rides the buffer's own grapheme
            // pool and prints INLINE in the base SGR run (the ref's
            // per-grapheme-cluster emission, upstream renderer.zig:
            // 2788-2812): the start cell packs the pool id + extent,
            // the span's remaining cells are continuation words (no
            // bytes), and the emitter's post-cluster CUP reposition
            // lands the run exactly after the reserved span — the
            // retired deferred cluster pass's pixel guarantee, now
            // with the ref's own byte shape (the old G-452 spaces +
            // deferred pass is gone).
            if let Some(symbol) = frame.cluster_at(y, x) {
                let attrs = match link_cells.get(&(x, y)) {
                    Some(id) => {
                        opentui_core::ansi::set_link_id(u32::from(cell.attrs & 0xFF), *id)
                    }
                    None => u32::from(cell.attrs & 0xFF),
                };
                let span = crate::wrap::display_width(symbol).max(1);
                let span32 = u32::try_from(span).unwrap_or(u32::MAX);
                let gid = buffer.alloc_grapheme_pub(symbol);
                let start = opentui_core::buffer::pack_grapheme_start(gid, span32);
                set_word(buffer, x, y, start, cell, attrs);
                for dx in 1..span {
                    if let Some(span_cell) = row.get(x + dx) {
                        let span_attrs = match link_cells.get(&(x + dx, y)) {
                            Some(id) => opentui_core::ansi::set_link_id(
                                u32::from(span_cell.attrs & 0xFF),
                                *id,
                            ),
                            None => u32::from(span_cell.attrs & 0xFF),
                        };
                        let cont = opentui_core::buffer::pack_continuation(
                            u32::try_from(dx).unwrap_or(u32::MAX),
                            span32 - 1 - u32::try_from(dx).unwrap_or(u32::MAX),
                            gid,
                        );
                        set_word(buffer, x + dx, y, cont, span_cell, span_attrs);
                    }
                }
                x += span;
                continue;
            }
            let word =
                if cell.ch == CONTINUATION { CHAR_FLAG_CONTINUATION } else { u32::from(cell.ch) };
            // G-287⑧ — a linked cell packs its link id into
            // attributes 8-31 (ansi.ts's packed order); the OSC 8
            // emission branch reads it back at emit time.
            let attrs = match link_cells.get(&(x, y)) {
                Some(id) => opentui_core::ansi::set_link_id(u32::from(cell.attrs & 0xFF), *id),
                None => u32::from(cell.attrs & 0xFF),
            };
            set_word(buffer, x, y, word, cell, attrs);
            x += 1;
        }
    }
}

fn set_word(buffer: &mut OptimizedBuffer, x: usize, y: usize, word: u32, cell: &Cell, attrs: u32) {
    buffer.set_cell_word(
        u32::try_from(x).unwrap_or(u32::MAX),
        u32::try_from(y).unwrap_or(u32::MAX),
        word,
        fg_rgba(cell.fg),
        bg_rgba(cell.bg),
        attrs,
    );
}
