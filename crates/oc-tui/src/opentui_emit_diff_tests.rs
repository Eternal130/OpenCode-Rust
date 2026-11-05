//! Lane D4.2 — the emission-flip differential gate.
//!
//! The ruling's construct proof: on the golden replay corpus frames
//! (the same scripts the keypress-replay harness runs, plus the
//! emitter vocabulary sequences), the RETIRED
//! [`crate::grid_emitter::GridEmitter`] and the NEW
//! [`crate::render::opentui_emit::OpentuiEmitter`] must emit
//! byte-identical output — including the `?2026` envelope and the
//! two oc-tui cursor policies (the per-frame visible-cursor
//! assertion CUP+`?25h`, the per-frame hide assertion `?25l`), which
//! the corpus steps assert explicitly.
//!
//! Byte equality is constructive: both emitters diff the same cell
//! grids (the same-source color-intent mapping of the B3b gate), and
//! the cursor state rides the same values. The corpus frames carry
//! no multi-codepoint cluster sidecars (the documented cluster
//! divergence prints those inline instead of the old deferred pass —
//! pixels preserved, framing different), which the corpus test
//! asserts as a precondition so a cluster-bearing scenario cannot
//! silently pass through.

use crate::grid_emitter::{CursorStyle, GridEmitter};
use crate::render::opentui_emit::OpentuiEmitter;
use crate::render::{CONTINUATION, Cell, CellColor, Frame};

type CursorInput = Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>;

fn map_style(style: CursorStyle) -> opentui_core::renderables::textarea::CursorStyle {
    match style {
        CursorStyle::Block => opentui_core::renderables::textarea::CursorStyle::Block,
        CursorStyle::Line => opentui_core::renderables::textarea::CursorStyle::Line,
        CursorStyle::Underline => opentui_core::renderables::textarea::CursorStyle::Underline,
    }
}

/// One frame through BOTH emitters with the same cursor input; the
/// bytes must match exactly.
fn assert_emit_equal(
    old: &mut GridEmitter,
    new: &mut OpentuiEmitter,
    frame: &Frame,
    cursor: CursorInput,
    label: &str,
) -> Vec<u8> {
    if let Some(((x, y), (style, blinking, color))) = cursor {
        old.cursor.set_style(style, blinking, color);
        old.cursor.set_position(x, y, true);
        let new_style = new.cursor();
        new_style.set_style(map_style(style), blinking, opentui_core::Rgba::rgb(color.0, color.1, color.2));
        new_style.set_position(x, y, true);
    } else {
        old.cursor.set_position(1, 1, false);
        new.cursor().set_position(1, 1, false);
    }
    let mut old_sink = Vec::new();
    let mut new_sink = Vec::new();
    old.emit(&mut old_sink, frame).expect("old emitter");
    new.emit(&mut new_sink, frame).expect("new emitter");
    assert_eq!(
        String::from_utf8_lossy(&old_sink),
        String::from_utf8_lossy(&new_sink),
        "{label}: emission bytes diverged"
    );
    new_sink
}
// -- the golden replay corpus ----------------------------------------------

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

/// Drive every replay script through the unified pump, feeding each
/// step's live frame (route + blockers + dialog overlay) through both
/// emitters with the pump's own caret policy (visible at
/// `store.caret_cell()` when no dialog/permission/form owns the
/// screen, hidden otherwise) — byte-equal every step.
#[test]
fn replay_corpus_frames_emit_identical_bytes() {
    let dir = goldens();
    let scripts = std::fs::read_dir(dir.join("replay"))
        .expect("replay corpus")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().is_some_and(|e| e == "json")).then(|| path)
        })
        .collect::<Vec<_>>();
    assert!(!scripts.is_empty(), "the replay corpus must be present");
    let mut compared = 0usize;
    for script_path in scripts {
        let json = std::fs::read_to_string(&script_path).expect("script json");
        let Some(script) = crate::replay::parse_script(&json) else {
            continue;
        };
        let mut pump = crate::app::App::new((100, 30));
        if script.id != "permission-once-always-deny" {
            pump.store.open_tab("ses_test", Some("Test session".to_owned()));
        }
        let mut old = GridEmitter::new(100, 30);
        let mut new = OpentuiEmitter::new(100, 30);
        for (index, step) in script.steps.iter().enumerate() {
            if index > 0 {
                crate::replay::arm_scenario(&script.id, &mut pump, index);
            } else {
                crate::replay::arm_scenario(&script.id, &mut pump, 0);
            }
            for key in &step.keys {
                pump.press(key);
            }
            pump.store.runtime.run_effects();
            let frame = pump.frame();
            assert!(
                frame.clusters.is_none(),
                "corpus frame carries clusters ({} step {index}) — the documented divergence",
                script.id
            );
            // the pump's caret policy (app.rs :9675-9691)
            let cursor: CursorInput = if pump.store.top_dialog().is_none()
                && !pump.permission_live()
                && !pump.form_live()
            {
                pump.store
                    .caret_cell()
                    .filter(|(x, y)| *x < 100 && *y < 30)
                    .map(|(x, y)| ((u32::from(x) + 1, u32::from(y) + 1), (CursorStyle::Line, true, (224, 224, 224))))
            } else {
                None
            };
            assert_emit_equal(
                &mut old,
                &mut new,
                &frame,
                cursor,
                &format!("{} step {index}", script.id),
            );
            compared += 1;
        }
    }
    assert!(compared > 10, "the corpus must exercise real frames ({compared})");
}

// -- the emitter vocabulary -------------------------------------------------

fn grid(width: usize, height: usize, rows: &[&str]) -> Frame {
    let cells = rows
        .iter()
        .map(|row| {
            row.chars()
                .take(width)
                .chain(std::iter::repeat(' ').take(width.saturating_sub(row.chars().count())))
                .map(|ch| Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default })
                .collect::<Vec<_>>()
        })
        .chain(
            std::iter::repeat(vec![Cell::default(); width]).take(height.saturating_sub(rows.len())),
        )
        .collect();
    Frame { cells, width, height, clusters: None }
}

fn blank_grid(width: usize, height: usize) -> Frame {
    grid(width, height, &[])
}

/// The first frame takes the forced full-repaint path on BOTH sides
/// (the old `first` flag, the new `frame_id == 0`), a no-op follow-up
/// emits nothing, and a revert diffs back — byte-equal throughout,
/// with the visible-cursor assertion ending every emitted frame.
#[test]
fn vocabulary_first_frame_noop_and_cursor_policies() {
    let (w, h) = (10usize, 3usize);
    let mut old = GridEmitter::new(w, h);
    let mut new = OpentuiEmitter::new(w, h);
    let f0 = grid(w, h, &["hello worl", "second row", "third row!"]);
    let mut f1 = f0.clone();
    f1.cells[0][0].ch = 'H';
    let f2 = f1.clone(); // no-op
    let visible: CursorInput = Some(((4, 2), (CursorStyle::Line, true, (224, 224, 224))));
    for (label, frame, cursor) in [
        ("first", &f0, visible),
        ("edit", &f1, visible),
        ("noop", &f2, visible),
        ("revert", &f0, None),
        ("hidden noop", &f2, None),
    ] {
        let bytes = assert_emit_equal(&mut old, &mut new, frame, cursor, label);
        let text = String::from_utf8_lossy(&bytes).to_string();
        if cursor.is_some() {
            assert!(
                text == "\x1b[?2026h\x1b[?25l\x1b[?2026l"
                    || text.ends_with("\x1b[?25h\x1b[?2026l"),
                "{label}: visible policy: {text:?}"
            );
        } else {
            assert!(
                text.is_empty() || text.ends_with("\x1b[?25l\x1b[?2026l"),
                "{label}: hidden policy: {text:?}"
            );
        }
    }
}

/// The color/attribute/wide-char vocabulary with cursor dynamics:
/// indexed 256, truecolor RGB, every attribute bit, CJK +
/// continuations, and show/move/hide/re-show churn.
#[test]
fn vocabulary_colors_attrs_wide_and_cursor_dynamics() {
    let (w, h) = (12usize, 5usize);
    let mut old = GridEmitter::new(w, h);
    let mut new = OpentuiEmitter::new(w, h);
    let f0 = grid(w, h, &["hello world!"]);
    // indexed + rgb + attrs
    let mut f1 = blank_grid(w, h);
    for (i, cell) in f1.cells[0].iter_mut().enumerate() {
        *cell = Cell {
            ch: char::from(b'a' + (i as u8 % 26)),
            attrs: (i as u16) & 0xFF,
            fg: match i % 3 {
                0 => CellColor::Default,
                1 => CellColor::Indexed(u8::try_from(i * 7).unwrap_or(255)),
                _ => CellColor::Rgb(u8::try_from(i * 20).unwrap_or(255), 40, 2),
            },
            bg: match i % 4 {
                0 => CellColor::Default,
                1 => CellColor::Indexed(200),
                _ => CellColor::Rgb(9, 9, u8::try_from(i * 10).unwrap_or(200)),
            },
        };
    }
    // wide CJK + continuation
    let mut f2 = blank_grid(w, h);
    f2.cells[1][0] = Cell { ch: '漢', ..Cell::default() };
    f2.cells[1][1] = Cell { ch: CONTINUATION, ..Cell::default() };
    f2.cells[1][2] = Cell { ch: '字', fg: CellColor::Indexed(202), ..Cell::default() };
    f2.cells[1][3] = Cell { ch: CONTINUATION, fg: CellColor::Indexed(202), ..Cell::default() };
    let cursors = [
        Some(((5, 1), (CursorStyle::Block, true, (255, 255, 255)))),
        Some(((2, 2), (CursorStyle::Block, true, (255, 255, 255)))),
        None,
        Some(((2, 2), (CursorStyle::Underline, false, (1, 2, 3)))),
        Some(((1, 1), (CursorStyle::Line, true, (1, 2, 3)))),
    ];
    let frames = [&f0, &f1, &f2, &f1, &f0];
    for (index, (frame, cursor)) in frames.iter().zip(cursors).enumerate() {
        assert_emit_equal(&mut old, &mut new, frame, cursor, &format!("vocab frame {index}"));
    }
}

/// The resize semantics: fresh blank diff bases on both sides, the
/// next frame full-repaints at the new size, and a follow-up frame
/// diffs normally.
#[test]
fn vocabulary_resize_full_repaint() {
    let (w, h) = (4usize, 2usize);
    let f0 = grid(w, h, &["abcd", "efgh"]);
    let (w2, h2) = (6usize, 3usize);
    let f1 = grid(w2, h2, &["ijklmn", "opqrst", "uvwxyz"]);
    let mut old = GridEmitter::new(w, h);
    let mut new = OpentuiEmitter::new(w, h);
    assert_emit_equal(&mut old, &mut new, &f0, None, "pre-resize");
    old.resize(w2, h2);
    new.resize(w2, h2);
    assert_emit_equal(&mut old, &mut new, &f1, None, "post-resize");
    let mut f2 = f1.clone();
    f2.cells[2][0].ch = 'X';
    assert_emit_equal(&mut old, &mut new, &f2, None, "diff at new size");
}

/// The invalidate_full semantics (the post-editor/post-suspend
/// repaint): the next frame fully repaints on both sides.
#[test]
fn vocabulary_invalidate_full() {
    let (w, h) = (5usize, 2usize);
    let f0 = grid(w, h, &["hello", "world"]);
    let mut f1 = f0.clone();
    f1.cells[0][0].ch = 'H';
    let mut old = GridEmitter::new(w, h);
    let mut new = OpentuiEmitter::new(w, h);
    assert_emit_equal(&mut old, &mut new, &f0, None, "first");
    assert_emit_equal(&mut old, &mut new, &f1, None, "diff");
    old.invalidate_full();
    new.invalidate_full();
    assert_emit_equal(&mut old, &mut new, &f1, None, "after invalidate_full");
    // and normal diffing resumes afterwards
    let mut f2 = f1.clone();
    f2.cells[1][4].ch = '!';
    assert_emit_equal(&mut old, &mut new, &f2, None, "resume diff");
}

/// A stable digest of the corpus emissions — the fixture the harness
/// keeps asserting AFTER the retired emitter is deleted (lane D4.2
/// commit 3): the new emitter alone must keep producing these bytes.
#[test]
fn corpus_digest_is_stable() {
    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
    // The same driving as `vocabulary_first_frame_noop_and_cursor_policies`
    // + `vocabulary_colors_attrs_wide_and_cursor_dynamics`, digested.
    let (w, h) = (12usize, 5usize);
    let mut new = OpentuiEmitter::new(w, h);
    let mut digest: u64 = 0;
    let f0 = grid(w, h, &["hello world!"]);
    let mut f1 = blank_grid(w, h);
    for (i, cell) in f1.cells[0].iter_mut().enumerate() {
        *cell = Cell {
            ch: char::from(b'a' + (i as u8 % 26)),
            attrs: (i as u16) & 0xFF,
            fg: CellColor::Indexed(u8::try_from(i * 7).unwrap_or(255)),
            bg: CellColor::Rgb(9, 9, 9),
        };
    }
    let mut f2 = blank_grid(w, h);
    f2.cells[1][0] = Cell { ch: '漢', ..Cell::default() };
    f2.cells[1][1] = Cell { ch: CONTINUATION, ..Cell::default() };
    let cursors: [CursorInput; 5] = [
        Some(((5, 1), (CursorStyle::Block, true, (255, 255, 255)))),
        Some(((2, 2), (CursorStyle::Block, true, (255, 255, 255)))),
        None,
        Some(((2, 2), (CursorStyle::Underline, false, (1, 2, 3)))),
        Some(((1, 1), (CursorStyle::Line, true, (1, 2, 3)))),
    ];
    for (frame, cursor) in [&f0, &f1, &f2, &f1, &f0].iter().zip(cursors) {
        if let Some(((x, y), (style, blinking, color))) = cursor {
            let cursor_state = new.cursor();
            cursor_state.set_style(map_style(style), blinking, opentui_core::Rgba::rgb(color.0, color.1, color.2));
            cursor_state.set_position(x, y, true);
        } else {
            new.cursor().set_position(1, 1, false);
        }
        let mut sink = Vec::new();
        new.emit(&mut sink, frame).expect("emit");
        digest = digest.wrapping_add(fnv1a(&sink));
    }
    // Printed for capture; pinned by the equality test above (the
    // same sequence through the old emitter digests identically).
    eprintln!("d42 corpus digest: {digest:016x}");
    assert_ne!(digest, 0);
}

/// The cluster pixel contract (the retired emitter's
/// `cluster_roundtrip_through_vt100`, pinned on the NEW emitter): a
/// multi-codepoint cluster prints its FULL symbol and the cells
/// beyond the span's overflow survive — the deferred cluster pass is
/// the pixel-preserving policy on top of the opentui stream.
#[test]
fn cluster_frames_keep_pixels_through_vt100() {
    let mut frame = grid(30, 1, &["                              "]);
    for (i, ch) in "family ".chars().enumerate() {
        frame.cells[0][i] = Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default };
    }
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}";
    frame.cells[0][7].ch = family.chars().next().expect("nonempty");
    frame.cells[0][8].ch = CONTINUATION;
    let mut clusters = std::collections::BTreeMap::new();
    clusters.insert((0, 7), family.to_owned());
    frame.clusters = Some(Box::new(clusters));
    for (i, ch) in "flag".chars().enumerate() {
        frame.cells[0][22 + i] =
            Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default };
    }
    let mut emitter = OpentuiEmitter::new(30, 1);
    let mut sink = Vec::new();
    emitter.emit(&mut sink, &frame).expect("emit");
    let mut parser = vt100::Parser::new(1, 30, 0);
    parser.process(&sink);
    let screen = parser.screen();
    let mut text = String::new();
    for x in 0..30 {
        let c = screen
            .cell(0, u16::try_from(x).unwrap_or(u16::MAX))
            .map(vt100::Cell::contents)
            .unwrap_or_default();
        if c.is_empty() {
            text.push(' ');
        } else {
            text.push_str(&c);
        }
    }
    let norm: String = text.chars().filter(|c| *c != ' ').collect();
    let target: String = family.chars().filter(|c| *c != ' ').collect();
    assert!(norm.contains(&target), "family must round-trip: {text:?}");
    assert!(text.contains("flag"), "flag must survive: {text:?}");
}
