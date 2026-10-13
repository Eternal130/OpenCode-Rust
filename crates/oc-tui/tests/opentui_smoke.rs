//! Lane M0 integration smoke — proves the opentui crates (pinned git
//! dep, optionally patched to the local checkout; see root Cargo.toml)
//! resolve, link, and RUN inside this workspace's dependency graph.
//! No production code touches them yet: this file is the only consumer
//! until the oc-tui migration lanes start.
//!
//! The core check mirrors opentui-rust's own headless harness: mount a
//! Text renderable, paint it into an offscreen `OptimizedBuffer`, and
//! assert the rendered cells (char + fg) — the raster seam the oc-tui
//! migration builds on. The keymap/solid checks touch each crate's
//! cheapest pure seam so all three actually link and execute here.

use opentui_core::buffer::DEFAULT_SPACE_CHAR;
use opentui_core::{OptimizedBuffer, Rgba, TextOptions, TextRenderableState};
use opentui_keymap::{stringify_key_sequence, StringifyOptions};
use opentui_solid::{Reactive, Signal};

#[test]
fn opentui_core_text_paints_into_headless_buffer() {
    let fg = Rgba::rgb(255, 80, 0);
    let opts = TextOptions { fg: Some(fg), ..TextOptions::with_str_content("hi") };
    let mut text = TextRenderableState::from_options(&opts);
    text.update_layout(8, 1);
    let mut frame = OptimizedBuffer::new(8, 1);
    text.paint(&mut frame, 0, 0);

    // Rendered span: both cells carry the renderable's fg.
    let h = frame.get(0, 0).expect("cell (0,0)");
    assert_eq!(h.char, u32::from(b'h'));
    assert_eq!(h.fg, fg);
    let i = frame.get(1, 0).expect("cell (1,0)");
    assert_eq!(i.char, u32::from(b'i'));
    assert_eq!(i.fg, fg);

    // Untouched cell: the frame-clear space with the default white fg —
    // proves the paint clipped to the span instead of flooding the row.
    let blank = frame.get(2, 0).expect("cell (2,0)");
    assert_eq!(blank.char, DEFAULT_SPACE_CHAR);
    assert_ne!(blank.fg, fg);
}

#[test]
fn opentui_keymap_and_solid_link_and_run() {
    // keymap: the stringify seam (stringifyKeySequence) with the
    // upstream-default options.
    let opts = StringifyOptions { prefer_display: false, separator: None };
    assert_eq!(stringify_key_sequence(&[], &opts), "");

    // solid: Signal (createSignal) — the reactive primitive the
    // reconciler binding drives re-renders through.
    let signal = Signal::new(7_u32);
    signal.set(9);
    assert_eq!(signal.get(), 9);
}
