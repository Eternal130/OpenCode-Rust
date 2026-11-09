//! The image lane's consumer surface: placements + the opentui native
//! renderer (lane D3-4b).
//!
//! Before this lane oc-tui owned an `image/` family: its own protocol
//! detection (env + DA1), kitty `a=T` emitter, sixel transcoder and the
//! post-frame escape writer. All of that now lives in opentui-core
//! (`draw_image` + [`ImageFrameEmitter`]); this module is the thin
//! bridge:
//!
//! * [`ImagePlacement`] — the transcript/dialog overlay rect plus the
//!   PNG payload (what the consumers produce).
//! * [`resolve_protocol`] — `protocol="auto"` through opentui's
//!   [`resolve_image_render_protocol`] over the probed
//!   `TerminalCapabilities` (kitty-graphics probe + cell-px resolution).
//! * [`drawable`] — whether the resolved protocol can place this
//!   payload (kitty needs a decodable PNG header; sixel needs the PNG
//!   decoder because opentui's compressed decode is deferred).
//! * [`ImageFrame`] — the frame-tail emitter: rebuilds an
//!   `OptimizedBuffer`, records the placements through `draw_image`
//!   and lets `ImageFrameEmitter` own the kitty transmit/place/delete
//!   bookkeeping and the occlusion flatten ruling.
//! * [`blocks_cells`]/[`fit_cells`] — the `blocks` fallback cell grid,
//!   now rasterized by opentui (`draw_image(Blocks)` +
//!   `materialize_image_fallbacks`) instead of the local half-block
//!   quantizer.

use std::io::{self, Write};

use opentui_core::buffer::OptimizedBuffer;
use opentui_core::image::NativeImage;
use opentui_core::image_render::ImageFrameEmitter;
use opentui_core::renderables::image::{
    resolve_image_render_protocol, ImageRenderCtx, ImageRenderProtocol, ImageRenderableState,
};
use opentui_core::renderer::TerminalCapabilities;

use crate::image_codec::{decode_png, Rgba8Image};

/// One image placed over a cell rect of the frame (0-based cells).
/// The PNG payload is retained verbatim: kitty transmits it as `f=100`
/// passthrough, sixel decodes it for the raw-RGBA encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePlacement {
    /// Encoded PNG bytes (the attachment payload).
    pub data: Vec<u8>,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

/// One blocks-fallback cell: a block glyph plus its two colors (fg
/// paints the glyph's filled quadrants, bg the rest).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockCell {
    pub glyph: char,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

/// `protocol="auto"` against the terminal capability snapshot. A
/// missing snapshot resolves to `Blocks` (opentui's unknown-caps
/// ruling), which keeps the pre-probe frame on the cell-grid fallback.
#[must_use]
pub fn resolve_protocol(capabilities: Option<&TerminalCapabilities>) -> ImageRenderProtocol {
    resolve_image_render_protocol(
        ImageRenderProtocol::Auto,
        capabilities,
        capabilities.is_some_and(|caps| caps.pixel_resolution.is_some()),
    )
}

/// Whether the resolved protocol can actually place this payload.
#[must_use]
pub fn drawable(protocol: ImageRenderProtocol, png: &[u8]) -> bool {
    match protocol {
        ImageRenderProtocol::Kitty => NativeImage::decode(png).is_ok(),
        ImageRenderProtocol::Sixel => decode_png(png).is_some(),
        ImageRenderProtocol::Blocks | ImageRenderProtocol::Auto => false,
    }
}

#[allow(clippy::cast_possible_truncation)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
/// Aspect-fit `cols × rows` inside `max_cols × max_rows` (fit mode,
/// cell aspect 2 when the pixel resolution is unknown — opentui's
/// `getFittedSize`). Zero inputs return `(0, 0)`.
#[must_use]
pub fn fit_cells(src_w: usize, src_h: usize, max_cols: usize, max_rows: usize) -> (usize, usize) {
    let (cols, rows) = ImageRenderableState::default().get_fitted_size(
        max_cols as u32,
        max_rows as u32,
        2.0,
        src_w as u32,
        src_h as u32,
    );
    (cols as usize, rows as usize)
}

#[allow(clippy::cast_possible_truncation)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
/// Rasterize a decoded image into `cols × rows` block cells through
/// opentui's native blocks lane (`drawImage` reserves, then
/// `materializeImageFallbacks` folds the quadrant markers into glyph
/// cells).
#[must_use]
pub fn blocks_cells(image: &Rgba8Image, cols: usize, rows: usize) -> Vec<Vec<BlockCell>> {
    if cols == 0 || rows == 0 || image.width == 0 || image.height == 0 {
        return Vec::new();
    }
    let Ok(native) = NativeImage::from_rgba(&image.data, image.width as u32, image.height as u32)
    else {
        return Vec::new();
    };
    let mut buffer = OptimizedBuffer::new(cols as u32, rows as u32);
    let drawn = buffer.draw_image(
        &native,
        0,
        0,
        cols as u32,
        rows as u32,
        image.width as u32,
        image.height as u32,
        0,
        0,
        image.width as u32,
        image.height as u32,
        ImageRenderProtocol::Blocks,
    );
    if !matches!(drawn, Ok(true)) || buffer.materialize_image_fallbacks().is_err() {
        return Vec::new();
    }
    let mut grid = Vec::with_capacity(rows);
    for y in 0..rows {
        let mut line = Vec::with_capacity(cols);
        for x in 0..cols {
            let cell =
                buffer.get(x as u32, y as u32).unwrap_or_else(|| opentui_core::buffer::Cell {
                    char: u32::from(' '),
                    fg: opentui_core::color::Rgba::rgb(0, 0, 0),
                    bg: opentui_core::color::Rgba::rgb(0, 0, 0),
                    attributes: 0,
                });
            line.push(BlockCell {
                glyph: char::from_u32(cell.char).unwrap_or(' '),
                fg: [cell.fg.red(), cell.fg.green(), cell.fg.blue()],
                bg: [cell.bg.red(), cell.bg.green(), cell.bg.blue()],
            });
        }
        grid.push(line);
    }
    grid
}

/// The frame-tail image emitter. One per run loop: it owns the
/// `ImageFrameEmitter`'s committed id set and the previous placement
/// signature (the emit-on-change delta, matching the old
/// `frame_images != last_frame_images` gate).
#[derive(Debug, Default)]
pub struct ImageFrame {
    emitter: ImageFrameEmitter,
    signature: Vec<PlacementSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlacementSignature {
    hash: u64,
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
}

impl ImageFrame {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit one frame's placements to `writer` (after the cell-grid
    /// frame). A no-op frame writes nothing; content/rect changes
    /// retransmit through the opentui emitter, vanished ids delete.
    pub fn render<W: Write + ?Sized>(
        &mut self,
        writer: &mut W,
        placements: &[ImagePlacement],
        capabilities: Option<&TerminalCapabilities>,
        width: u16,
        height: u16,
    ) -> io::Result<()> {
        let signature: Vec<PlacementSignature> = placements
            .iter()
            .map(|placement| PlacementSignature {
                hash: fnv1a(&placement.data),
                x: placement.x,
                y: placement.y,
                cols: placement.cols,
                rows: placement.rows,
            })
            .collect();
        if signature == self.signature {
            return Ok(());
        }
        let protocol = resolve_protocol(capabilities);
        let ctx = ImageRenderCtx {
            capabilities,
            terminal_width: Some(u32::from(width)),
            terminal_height: Some(u32::from(height)),
            resolution: capabilities.and_then(|caps| caps.pixel_resolution),
        };
        // No placeable images (blocks terminal, or the placements left
        // the frame): clear the committed kitty images through the
        // emitter's empty-frame arm (image_deletes), then stop.
        if placements.is_empty()
            || protocol == ImageRenderProtocol::Blocks
            || occluded(placements, width, height)
        {
            let mut buffer = OptimizedBuffer::new(u32::from(width), u32::from(height));
            let _ = self.emitter.render_frame(writer, &mut buffer, &ctx);
            self.emitter = ImageFrameEmitter::new();
            self.signature = signature;
            return Ok(());
        }
        // Placement identities are buffer-local (1..n), so a changed
        // frame reuses them. Free the previous images first, then let
        // the fresh emitter transmit the new set (opentui recomputes
        // placements every frame; only the transmit is id-gated).
        let committed = self.emitter.committed().to_vec();
        if !committed.is_empty() {
            opentui_core::emit_image_deletes(writer, &committed)?;
            self.emitter = ImageFrameEmitter::new();
        }
        let mut buffer = OptimizedBuffer::new(u32::from(width), u32::from(height));
        for placement in placements {
            let Some((image, pixel_width, pixel_height)) = materialize(placement, protocol) else {
                continue;
            };
            let _ = buffer.draw_image(
                &image,
                i32::from(placement.x),
                i32::from(placement.y),
                u32::from(placement.cols),
                u32::from(placement.rows),
                pixel_width,
                pixel_height,
                0,
                0,
                pixel_width,
                pixel_height,
                protocol,
            );
        }
        self.emitter
            .render_frame(writer, &mut buffer, &ctx)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.signature = signature;
        Ok(())
    }
}

/// The opentui emitter flattens overlapping / off-screen placements to
/// blocks (which needs raw pixels); detect that case up front so the
/// consumer's cell-grid fallback (already drawn) stays the visual.
fn occluded(placements: &[ImagePlacement], width: u16, height: u16) -> bool {
    for (index, placement) in placements.iter().enumerate() {
        let x1 = u32::from(placement.x) + u32::from(placement.cols);
        let y1 = u32::from(placement.y) + u32::from(placement.rows);
        if x1 > u32::from(width) || y1 > u32::from(height) {
            return true;
        }
        for previous in &placements[..index] {
            let px1 = u32::from(previous.x) + u32::from(previous.cols);
            let py1 = u32::from(previous.y) + u32::from(previous.rows);
            if u32::from(previous.x) < x1
                && px1 > u32::from(placement.x)
                && u32::from(previous.y) < y1
                && py1 > u32::from(placement.y)
            {
                return true;
            }
        }
    }
    false
}

#[allow(clippy::cast_possible_truncation)] // rendering geometry/size math: values are terminal-bounded; the ref computes in JS number space
/// Build the opentui image for the resolved protocol. Kitty keeps the
/// encoded PNG (`f=100` passthrough); sixel materializes the decoded
/// RGBA (`raw()` is the encoder's input).
fn materialize(
    placement: &ImagePlacement,
    protocol: ImageRenderProtocol,
) -> Option<(NativeImage, u32, u32)> {
    match protocol {
        ImageRenderProtocol::Kitty => {
            let image = NativeImage::decode(&placement.data).ok()?;
            let (width, height) = (image.width().ok()?, image.height().ok()?);
            Some((image, width, height))
        }
        ImageRenderProtocol::Sixel => {
            let decoded = decode_png(&placement.data)?;
            let image =
                NativeImage::from_rgba(&decoded.data, decoded.width as u32, decoded.height as u32)
                    .ok()?;
            Some((image, decoded.width as u32, decoded.height as u32))
        }
        ImageRenderProtocol::Blocks | ImageRenderProtocol::Auto => None,
    }
}

/// FNV-1a over the payload — the placement change signature.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_PNG: &[u8] = include_bytes!("../tests/fixtures/r07-tiny-4x2.png");

    fn caps(kitty: bool, sixel: bool, resolution: Option<(u32, u32)>) -> TerminalCapabilities {
        TerminalCapabilities {
            kitty_graphics: kitty,
            sixel,
            pixel_resolution: resolution,
            ..TerminalCapabilities::default()
        }
    }

    #[test]
    fn auto_resolves_like_opentui() {
        assert_eq!(resolve_protocol(None), ImageRenderProtocol::Blocks);
        assert_eq!(resolve_protocol(Some(&caps(true, false, None))), ImageRenderProtocol::Kitty);
        assert_eq!(
            resolve_protocol(Some(&caps(false, true, Some((800, 480))))),
            ImageRenderProtocol::Sixel
        );
        assert_eq!(
            resolve_protocol(Some(&caps(false, true, None))),
            ImageRenderProtocol::Blocks,
            "sixel without a pixel resolution stays blocks"
        );
    }

    #[test]
    fn drawable_checks_the_payload() {
        assert!(drawable(ImageRenderProtocol::Kitty, TINY_PNG));
        assert!(drawable(ImageRenderProtocol::Sixel, TINY_PNG));
        assert!(!drawable(ImageRenderProtocol::Kitty, b"\x89PNG-tiny"));
        assert!(!drawable(ImageRenderProtocol::Sixel, b"\x89PNG-tiny"));
        assert!(!drawable(ImageRenderProtocol::Blocks, TINY_PNG));
    }

    #[test]
    fn blocks_cells_match_the_fixture_bands() {
        let image = decode_png(TINY_PNG).expect("fixture");
        let grid = blocks_cells(&image, 12, 6);
        assert_eq!(grid.len(), 6);
        assert!(grid.iter().all(|row| row.len() == 12));
        assert!(grid.iter().flatten().all(|cell| cell.glyph == '█' && cell.fg == cell.bg));
        assert_eq!(grid[0][0].fg, [255, 0, 0], "red");
        assert_eq!(grid[0][6].fg, [0, 0, 255], "blue");
        assert_eq!(grid[3][0].fg, [255, 255, 255], "white");
        assert_eq!(grid[5][8].fg, [128, 128, 128], "gray");
    }

    #[test]
    fn fit_matches_the_cell_aspect() {
        // 4×2 at aspect 2 fits 12×6 as 12×3.
        assert_eq!(fit_cells(4, 2, 12, 6), (12, 3));
        assert_eq!(fit_cells(0, 2, 12, 6), (0, 0));
    }

    #[test]
    fn kitty_frame_transmits_places_and_deletes() {
        let cap = caps(true, false, None);
        let placement = ImagePlacement { data: TINY_PNG.to_vec(), x: 4, y: 2, cols: 12, rows: 6 };
        let mut frame = ImageFrame::new();
        let mut wire = Vec::new();
        frame
            .render(&mut wire, std::slice::from_ref(&placement), Some(&cap), 80, 24)
            .expect("emit");
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("\x1b_Ga=t,f=100,i=1,"), "kitty transmit: {text:?}");
        assert!(text.contains("\x1b[3;5H\x1b_Ga=p,i=1,p=1,c=12,r=6,"), "placement: {text:?}");
        // Unchanged frame = no bytes.
        let before = wire.len();
        frame.render(&mut wire, std::slice::from_ref(&placement), Some(&cap), 80, 24).unwrap();
        assert_eq!(wire.len(), before, "no-op frame writes nothing");
        // Empty frame deletes the committed image.
        frame.render(&mut wire, &[], Some(&cap), 80, 24).unwrap();
        let tail = String::from_utf8_lossy(&wire[before..]).to_string();
        assert!(tail.contains("\x1b_Ga=d,d=I,i=1"), "delete: {tail:?}");
    }

    #[test]
    fn sixel_frame_emits_cup_and_dcs() {
        let cap = caps(false, true, Some((800, 480)));
        let placement = ImagePlacement { data: TINY_PNG.to_vec(), x: 0, y: 0, cols: 12, rows: 6 };
        let mut frame = ImageFrame::new();
        let mut wire = Vec::new();
        frame.render(&mut wire, &[placement], Some(&cap), 80, 24).expect("emit");
        let text = String::from_utf8_lossy(&wire);
        assert!(text.starts_with("\x1b[1;1H\x1bP"), "CUP + DCS: {text:?}");
        assert!(text.ends_with("\x1b\\"), "ST terminator");
    }

    #[test]
    fn blocks_terminal_emits_nothing() {
        let mut frame = ImageFrame::new();
        let mut wire = Vec::new();
        frame
            .render(
                &mut wire,
                &[ImagePlacement { data: TINY_PNG.to_vec(), x: 0, y: 0, cols: 12, rows: 6 }],
                None,
                80,
                24,
            )
            .unwrap();
        assert_eq!(wire, [] as [u8; 0]);
    }
}
