//! R-07 fake-terminal display tests: the transcript's user-message
//! image strip end-to-end against captured output — the frame's cell
//! grid (the placeholder boxes) plus the graphics-protocol escapes a
//! real run loop would write after drawing. No terminal is attached:
//! `render_transcript` produces the placements, `image::emit_*`
//! serializes them into a `Vec<u8>` "wire".
//!
//! Chain under test (the ports):
//! * `SessionImages` (session/index.tsx:2456) →
//!   [`rows::image_strip_row`] / `user_message_row`
//! * opentui `<image protocol="auto">` → [`crate::image`]'s kitty
//!   APC / sixel passthrough emission
//! * the "No preview" error fallback (dialog-image-preview.tsx:62)
//!   → the bordered placeholder with the centered label

use crate::components::message::list::{render_transcript, TranscriptOptions};
use crate::components::message::model::Message;
use crate::components::message::rows::ImageStripOptions;
use crate::image::{self, GraphicsProtocol, ImageData};
use crate::render::CellColor;
use crate::theme::ThemeState;

const TINY_PNG: &[u8] = include_bytes!("../tests/fixtures/r07-tiny-4x2.png");

fn theme() -> ThemeState {
    ThemeState::v2_dark()
}

fn user_message_with_png() -> Message {
    Message::User {
        id: "msg_img_1".into(),
        text: "describe this screenshot".into(),
        created: 0,
        files: vec![crate::components::message::model::UserFile {
            mime: "image/png".into(),
            data: crate::clipboard::base64_encode::encode(TINY_PNG),
            name: Some("shot.png".into()),
        }],
    }
}

fn options(protocol: GraphicsProtocol) -> TranscriptOptions {
    TranscriptOptions {
        image_strip: ImageStripOptions { enabled: true, protocol, terminal_rows: 24 },
        ..TranscriptOptions::default()
    }
}

#[test]
fn kitty_terminal_receives_cup_then_the_apc_over_the_placeholder_rect() {
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(GraphicsProtocol::Kitty),
        &theme().styles(),
        80,
    );
    // One placement: the committed fixture PNG at the thumb rect,
    // offset by the row's marginTop-1 and the container inset (2).
    assert_eq!(transcript.images.len(), 1);
    let placement = transcript.images[0].clone();
    assert_eq!(placement.data, ImageData::Png(TINY_PNG.to_vec()));
    assert_eq!(placement.cols, 12, "thumb width = height(6) * 2");
    assert_eq!(placement.rows, 6);
    // Strip-internal (paddingTop 1, paddingLeft 2) + the transcript's
    // marginTop 1 + container inset 2.
    assert_eq!(placement.y, 2);
    assert_eq!(placement.x, 5);

    // The wire: CUP to the rect's 1-based cell, then the kitty
    // transmit-and-display APC with the fixture's base64 payload.
    let mut wire = Vec::new();
    let drawn = image::emit_frame_images(
        &mut wire,
        GraphicsProtocol::Kitty,
        std::slice::from_ref(&placement),
    )
    .expect("emit");
    assert_eq!(drawn, vec![placement.id]);
    let mut expected = image::cursor_move_escape(placement.x, placement.y).as_slice().to_vec();
    expected.extend_from_slice(&image::kitty_display_escape(&placement));
    assert_eq!(wire, expected);
    // The APC head is spec-exact (a=T, PNG, cell-sized, quiet).
    let text = String::from_utf8(wire.clone()).unwrap();
    assert!(text.contains("\x1b_Ga=T,f=100,c=12,r=6,"), "apc head: {text:?}");
    assert!(text.ends_with("\x1b\\"));
}

#[test]
fn sixel_terminal_transcodes_png_to_sixel() {
    // RB-07: the fixture is PNG and sixel CAN now carry it — the
    // transcoder (image::sixel) turns it into a DCS q sequence, so
    // a placement escapes and no block-glyph cells render.
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(GraphicsProtocol::Sixel),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images.len(), 1, "one sixel placement escapes");
    let body = crate::image::sixel_display_escape(&transcript.images[0]);
    assert!(body.starts_with(b"\x1b[") && body.ends_with(b"\x1b\\"), "CUP + DCS…ST");
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    for row in &transcript.rows {
        placed.extend(row.placed.clone());
        fills.extend(row.fills.clone());
    }
    let frame = crate::render::paint_placed_with_fills(
        &placed,
        &fills,
        (80, 12),
        crate::components::tool_rows::fill_style(&theme().styles()),
    );
    let text = frame.text_grid().join("\n");
    assert!(!text.contains('▀'), "no block glyphs over the sixel path: {text}");
    // The bordered placeholder rect stays beneath the escape, the
    // same semantics as the kitty path (escapes draw over the box).
    assert!(text.contains('┌'), "placeholder box beneath the sixel escape");
}

#[test]
fn plain_terminal_renders_blocks_cells_deterministically() {
    // The v2 fallback (resolveImageRenderProtocol → "blocks"):
    // the 4×2 fixture decodes and quantizes into the thumb rect —
    // 12×3 block cells (fit at cell aspect 2) centered in the
    // 12×6 thumb, every glyph's colors exact pixel values.
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(GraphicsProtocol::None),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images, Vec::new(), "no protocol ⇒ no overlays");
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    for row in &transcript.rows {
        placed.extend(row.placed.clone());
        fills.extend(row.fills.clone());
    }
    let frame = crate::render::paint_placed_with_fills(
        &placed,
        &fills,
        (80, 12),
        crate::components::tool_rows::fill_style(&theme().styles()),
    );
    let grid = frame.text_grid();
    let text = grid.join("\n");
    assert!(!text.contains('┌'), "the thumb is the image, not a box");
    let text_row = grid.iter().position(|line| line.contains("describe this")).expect("text");
    // The block band: three rows (fit 12×3) centered in the
    // 6-row thumb above the message text. Band row 0 samples
    // pixel row 0 (uniform █), row 1 splits top/bottom pixels
    // (▀ where the top pixel is darker, ▄ where the bottom is),
    // row 2 samples pixel row 1 (uniform █).
    let band_top = text_row - 7;
    let band: Vec<&String> = grid[band_top..band_top + 3].iter().collect();
    assert!(band[0].contains("████████"), "uniform row: {}", band[0]);
    assert!(
        band[1].contains("▀▀▀") && band[1].contains("▄▄▄"),
        "split row orients fg to the darker half: {}",
        band[1]
    );
    assert!(band[2].contains("████████"), "uniform row: {}", band[2]);
    // Cell-exact colors: the fixture's pixel row 0 is
    // red/green/blue/yellow, row 1 white/black/gray/teal.

    let red = CellColor::Rgb(255, 0, 0);
    let white = CellColor::Rgb(255, 255, 255);
    let black = CellColor::Rgb(0, 0, 0);
    let green = CellColor::Rgb(0, 255, 0);
    let solid = frame.cell(band_top, 5);
    assert_eq!((solid.ch, solid.fg, solid.bg), ('█', red, red));
    let split = frame.cell(band_top + 1, 5);
    assert_eq!((split.ch, split.fg, split.bg), ('▀', red, white));
    // The green/black pair orients fg to black (▄).
    let flipped = frame.cell(band_top + 1, 8);
    assert_eq!((flipped.ch, flipped.fg, flipped.bg), ('▄', black, green));
    // Nothing emits on the wire.
    let mut wire = Vec::new();
    let drawn =
        image::emit_frame_images(&mut wire, GraphicsProtocol::None, &transcript.images).unwrap();
    assert_eq!(drawn, Vec::<u32>::new());
    assert_eq!(wire, Vec::<u8>::new());
}

#[test]
fn undecodable_payload_keeps_the_no_preview_placeholder() {
    // v2's onError fallback: a payload that cannot decode renders
    // the bordered box with the centered label, not blocks.
    let mut message = user_message_with_png();
    if let crate::components::message::model::Message::User { files, .. } = &mut message {
        files[0].data = crate::clipboard::base64_encode::encode(b"not a png");
    }
    let transcript =
        render_transcript(&[message], &options(GraphicsProtocol::None), &theme().styles(), 80);
    assert_eq!(transcript.images, Vec::new());
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    for row in &transcript.rows {
        placed.extend(row.placed.clone());
        fills.extend(row.fills.clone());
    }
    let frame = crate::render::paint_placed_with_fills(
        &placed,
        &fills,
        (80, 12),
        crate::components::tool_rows::fill_style(&theme().styles()),
    );
    let grid = frame.text_grid();
    let text = grid.join("\n");
    assert!(text.contains('┌'), "the placeholder box: {text}");
    assert!(text.contains("shot.png"), "the label: {text}");
    assert!(!text.contains('▀') && !text.contains('█'), "no blocks for a dead payload");
}

#[test]
fn the_strip_stays_hidden_until_session_image_preview_enables_it() {
    // Default options (the gate off — v2's falsy config check): the
    // message renders text-only even with image files present.
    let transcript = render_transcript(
        &[user_message_with_png()],
        &TranscriptOptions::default(),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images, Vec::new());
    let row = &transcript.rows[0];
    assert_eq!(row.height, 3, "1 + text + 1: no strip rows");
    assert!(!row.placed.iter().any(|item| item
        .lines
        .iter()
        .any(|line| line.runs.iter().any(|run| run.text.contains('┌')))));
}
