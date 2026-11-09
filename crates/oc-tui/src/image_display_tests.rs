//! R-07 fake-terminal display tests: the transcript's user-message
//! image strip end-to-end against captured output — the frame's cell
//! grid (the placeholder boxes / blocks glyphs) plus the graphics
//! bytes a real run loop writes after drawing, now through opentui's
//! `ImageFrameEmitter` (lane D3-4b; the old local emit family retired).
//!
//! Chain under test (the ports):
//! * `SessionImages` (session/index.tsx:2456) →
//!   [`rows::image_strip_row`] / `user_message_row`
//! * opentui `<image protocol="auto">` → [`crate::image_view::ImageFrame`]
//!   (drawImage + `ImageFrameEmitter`: kitty APC / sixel DCS / delete)
//! * the "No preview" error fallback (dialog-image-preview.tsx:62)
//!   → the bordered placeholder with the centered label

use crate::components::message::list::{render_transcript, TranscriptOptions};
use crate::components::message::model::Message;
use crate::components::message::rows::ImageStripOptions;
use crate::image_view::{resolve_protocol, ImageFrame};
use crate::render::CellColor;
use crate::theme::ThemeState;
use opentui_core::renderer::TerminalCapabilities;
use opentui_core::ImageRenderProtocol;

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

fn caps(kitty: bool, sixel: bool, resolution: Option<(u32, u32)>) -> TerminalCapabilities {
    TerminalCapabilities {
        kitty_graphics: kitty,
        sixel,
        pixel_resolution: resolution,
        ..TerminalCapabilities::default()
    }
}

fn options(capabilities: Option<TerminalCapabilities>) -> TranscriptOptions {
    TranscriptOptions {
        image_strip: ImageStripOptions { enabled: true, capabilities, terminal_rows: 24 },
        ..TranscriptOptions::default()
    }
}

fn frame_cells(transcript: &crate::components::message::list::Transcript) -> crate::render::Frame {
    let mut placed = Vec::new();
    let mut fills = Vec::new();
    for row in &transcript.rows {
        placed.extend(row.placed.clone());
        fills.extend(row.fills.clone());
    }
    crate::render::paint_placed_with_fills(
        &placed,
        &fills,
        (80, 12),
        crate::components::tool_rows::fill_style(&theme().styles()),
    )
}

#[test]
fn kitty_terminal_receives_transmit_and_placement_over_the_placeholder_rect() {
    let cap = caps(true, false, None);
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(Some(cap.clone())),
        &theme().styles(),
        80,
    );
    // One placement: the committed fixture PNG at the thumb rect,
    // offset by the row's marginTop-1 and the container inset (2).
    assert_eq!(transcript.images.len(), 1);
    let placement = transcript.images[0].clone();
    assert_eq!(placement.data, TINY_PNG.to_vec());
    assert_eq!(placement.cols, 12, "thumb width = height(6) * 2");
    assert_eq!(placement.rows, 6);
    assert_eq!(placement.y, 2);
    assert_eq!(placement.x, 5);

    // The wire: opentui's kitty transmit (`a=t,f=100`) then the
    // placement APC (`a=p`) at the rect's 1-based cell.
    assert_eq!(resolve_protocol(Some(&cap)), ImageRenderProtocol::Kitty);
    let mut wire = Vec::new();
    let mut frame = ImageFrame::new();
    frame.render(&mut wire, std::slice::from_ref(&placement), Some(&cap), 80, 24).expect("emit");
    let text = String::from_utf8(wire.clone()).unwrap();
    assert!(text.contains("\x1b_Ga=t,f=100,i=1,"), "transmit head: {text:?}");
    assert!(text.contains("\x1b[3;6H\x1b_Ga=p,i=1,p=1,c=12,r=6,"), "placement: {text:?}");
    assert!(text.ends_with("\x1b\\"));
}

#[test]
fn sixel_terminal_transcodes_png_to_sixel() {
    // RB-07: the fixture is PNG and sixel materializes it into a DCS q
    // sequence through opentui's encoder.
    let cap = caps(false, true, Some((800, 480)));
    let transcript = render_transcript(
        &[user_message_with_png()],
        &options(Some(cap.clone())),
        &theme().styles(),
        80,
    );
    assert_eq!(transcript.images.len(), 1, "one sixel placement escapes");
    let frame = frame_cells(&transcript);
    let text = frame.text_grid().join("\n");
    assert!(!text.contains('▀'), "no block glyphs over the sixel path: {text}");
    assert!(text.contains('┌'), "placeholder box beneath the sixel escape");

    let mut wire = Vec::new();
    ImageFrame::new().render(&mut wire, &transcript.images, Some(&cap), 80, 24).expect("emit");
    let text = String::from_utf8(wire).unwrap();
    assert!(text.starts_with("\x1b[3;6H\x1bP"), "CUP + DCS: {text:?}");
    assert!(text.ends_with("\x1b\\"), "ST terminator");
}

#[test]
fn plain_terminal_renders_blocks_cells_deterministically() {
    // The v2 fallback (resolveImageRenderProtocol → "blocks"):
    // the 4×2 fixture decodes and opentui quantizes it into the thumb
    // rect — 12×3 block cells (fit at cell aspect 2) centered in the
    // 12×6 thumb, every glyph's colors exact pixel values.
    let transcript =
        render_transcript(&[user_message_with_png()], &options(None), &theme().styles(), 80);
    assert_eq!(transcript.images, Vec::new(), "no protocol ⇒ no overlays");
    let frame = frame_cells(&transcript);
    let grid = frame.text_grid();
    let text = grid.join("\n");
    assert!(!text.contains('┌'), "the thumb is the image, not a box");
    let text_row = grid.iter().position(|line| line.contains("describe this")).expect("text");
    // The block band: three rows (fit 12×3) centered in the
    // 6-row thumb above the message text.
    let band_top = text_row - 7;
    let band: Vec<&String> = grid[band_top..band_top + 3].iter().collect();
    assert!(band[0].contains("████████"), "uniform row: {}", band[0]);
    assert!(
        band[1].contains("▀▀▀") && band[1].contains("▄▄▄"),
        "split row orients fg to the darker half: {}",
        band[1]
    );
    assert!(band[2].contains("████████"), "uniform row: {}", band[2]);

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
    ImageFrame::new().render(&mut wire, &transcript.images, None, 80, 24).expect("emit");
    assert_eq!(wire, [] as [u8; 0]);
}

#[test]
fn undecodable_payload_keeps_the_no_preview_placeholder() {
    // v2's onError fallback: a payload that cannot decode renders
    // the bordered box with the centered label, not blocks.
    let mut message = user_message_with_png();
    if let crate::components::message::model::Message::User { files, .. } = &mut message {
        files[0].data = crate::clipboard::base64_encode::encode(b"not a png");
    }
    let transcript = render_transcript(&[message], &options(None), &theme().styles(), 80);
    assert_eq!(transcript.images, Vec::new());
    let frame = frame_cells(&transcript);
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
