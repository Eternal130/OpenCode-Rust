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
fn sixel_terminal_without_raw_sixel_data_gets_the_placeholder_only() {
    // The fixture is PNG; sixel cannot carry it without a transcoder
    // (the R-08 pixel-decode seam) — the placement drops and the
    // placeholder box IS the strip.
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(GraphicsProtocol::Sixel),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images, Vec::new());
    let row = &transcript.rows[0];
    assert!(row.placed.iter().any(|item| item
        .lines
        .iter()
        .any(|line| line.runs.iter().any(|run| run.text.contains('┌')))));
}

#[test]
fn plain_terminal_renders_the_placeholder_boxes_deterministically() {
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(GraphicsProtocol::None),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images, Vec::new(), "no protocol ⇒ no overlays");
    // Paint the transcript's rows into a frame and read the grid: the
    // thumb box (bordered, "[Image 1]"-labeled — the v2 fallback
    // shape) sits above the message text inside the user box.
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
    let box_top = grid.iter().position(|line| line.contains('┌')).expect("thumb box top");
    assert!(grid[box_top].contains("┌──────────┐"), "12-wide thumb border: {}", grid[box_top]);
    let label_row = &grid[box_top + 3];
    assert!(label_row.contains("shot.png"), "centered file label: {label_row}");
    assert!(label_row.contains('│'), "box sides: {label_row}");
    let text_row = grid.iter().position(|line| line.contains("describe this")).expect("text");
    assert!(text_row > box_top, "text below the strip");
    // Nothing emits on the wire.
    let mut wire = Vec::new();
    let drawn =
        image::emit_frame_images(&mut wire, GraphicsProtocol::None, &transcript.images).unwrap();
    assert_eq!(drawn, Vec::<u32>::new());
    assert_eq!(wire, Vec::<u8>::new());
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
