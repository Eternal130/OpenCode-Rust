//! Lane B3b — the GridEmitter ↔ opentui byte-differential gate.
//!
//! The core gate of the emitter port: the SAME (curated + pseudo-random)
//! grid sequences fed through (1) oc-tui's [`GridEmitter`] and (2)
//! opentui-core's `bench_emit_frame_sgr` oracle must emit byte-identical
//! SGR streams (the `?2026` envelope is the emitter's own addition and
//! is excluded — `render_sgr` returns the bare stream).
//!
//! Oracle driving: the opentui `CliRenderer`'s buffers and cursor state
//! are public (`next_render_buffer` / `current_render_buffer` /
//! `cursor_state`), and `bench_emit_frame_sgr` drives the internal
//! writer directly. One `render()` on the fresh renderer bumps
//! `frame_id` past 0 (the forced-full-repaint first frame — the
//! emitter's `first` flag mirrors it; both sides prime with a blank
//! full frame that the harness discards) and swaps the blank painted
//! buffer into `current`, which is cell-for-cell the emitter's blank
//! prev grid (`OptimizedBuffer::new`/`clear`: space, attrs 0, fg
//! literal white, bg transparent black). Each subsequent frame is
//! poked cell-by-cell into `next_render_buffer` (the packed
//! representation is same-source: ECMA attrs low byte, the color
//! intents one-to-one with [`CellColor`]), emitted through the bench
//! seam, and the buffers swapped — the `render()` swap minus the
//! rasterization that would overwrite the poked cells.
//!
//! Covered: pure text, 256-color, RGB, attribute combinations, wide
//! characters/CJK, continuation cells, empty frames, all-change frames,
//! row-end truncation (a wide cluster cut at the last column), resize
//! full repaint, and a pseudo-random sequence with cursor dynamics.

use crate::grid_emitter::{CursorStyle, GridEmitter};
use crate::render::{CONTINUATION, Cell, CellColor, Frame};
use opentui_core::buffer::CHAR_FLAG_CONTINUATION;
use opentui_core::color::Rgba;
use opentui_core::renderer::{CliRenderer, CliRendererConfig, create_cli_renderer};

// -- the oc cell ↔ opentui packed representation (same source both ways)

fn fg_rgba(color: CellColor) -> Rgba {
    match color {
        // default intent → `39` (ansi.rs emit_color :114-120)
        CellColor::Default => Rgba::default_color(255, 255, 255, 255),
        // indexed intent → `38;5;n` (:130-133)
        CellColor::Indexed(index) => Rgba::from_index(index),
        // literal RGB → `38;2;r;g;b` (:135-143)
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
    }
}

fn bg_rgba(color: CellColor) -> Rgba {
    match color {
        // alpha-0 background → `49` (ansi.rs emit_color :122-127)
        CellColor::Default => Rgba::from_ints(0, 0, 0, 0),
        CellColor::Indexed(index) => Rgba::from_index(index),
        CellColor::Rgb(r, g, b) => Rgba::rgb(r, g, b),
    }
}

/// The oracle renderer primed past its forced first frame (`frame_id`
/// >= 1, `current` = the blank painted buffer — cell-for-cell the
/// emitter's blank prev grid).
fn oracle(width: u32, height: u32) -> CliRenderer {
    let mut renderer =
        create_cli_renderer(CliRendererConfig { width, height, ..CliRendererConfig::default() });
    // Absorb the forced full blank frame into sgr_history (discarded —
    // the harness compares per-frame bench outputs only); this bumps
    // frame_id and swaps the blank painted buffer into current.
    renderer.render();
    renderer
}

/// The emitter primed the same way: the forced first frame consumed by
/// a blank full render (discarded), leaving the same blank diff base.
fn emitter(width: usize, height: usize) -> GridEmitter {
    let mut emitter = GridEmitter::new(width, height);
    let _ = emitter.render_sgr(&blank_grid(width, height));
    emitter
}

fn blank_grid(width: usize, height: usize) -> Frame {
    Frame {
        cells: vec![
            vec![
                Cell {
                    ch: ' ',
                    attrs: 0,
                    fg: CellColor::Rgb(255, 255, 255),
                    bg: CellColor::Default
                };
                width
            ];
            height
        ],
        width,
        height,
        clusters: None,
    }
}

/// Poke one oc frame's cells into the oracle's next buffer — the
/// packed representation is same-source (ECMA attrs low byte, the
/// color intents one-to-one, CONTINUATION ↔ the buffer's packed
/// continuation word).
fn poke(renderer: &mut CliRenderer, frame: &Frame) {
    for (y, row) in frame.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let (x, y) = (u32::try_from(x).unwrap(), u32::try_from(y).unwrap());
            let char_word = if cell.ch == CONTINUATION {
                CHAR_FLAG_CONTINUATION
            } else {
                u32::from(cell.ch as u32)
            };
            renderer.next_render_buffer.set_cell_word(
                x,
                y,
                char_word,
                fg_rgba(cell.fg),
                bg_rgba(cell.bg),
                u32::from(cell.attrs & 0xFF),
            );
        }
    }
}

/// One oracle frame: poke → cursor → bench emit → buffer swap.
fn oracle_frame(
    renderer: &mut CliRenderer,
    frame: &Frame,
    cursor: Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>,
) -> String {
    poke(renderer, frame);
    if let Some(((x, y), (style, blinking, color))) = cursor {
        let style = match style {
            CursorStyle::Block => opentui_core::renderables::textarea::CursorStyle::Block,
            CursorStyle::Line => opentui_core::renderables::textarea::CursorStyle::Line,
            CursorStyle::Underline => opentui_core::renderables::textarea::CursorStyle::Underline,
        };
        renderer.cursor_state.set_style(style, blinking, Rgba::rgb(color.0, color.1, color.2));
        renderer.cursor_state.set_position(x, y, true);
    } else {
        renderer.cursor_state.set_position(1, 1, false);
    }
    let mut out = String::new();
    renderer.bench_emit_frame_sgr(&mut out);
    core::mem::swap(&mut renderer.current_render_buffer, &mut renderer.next_render_buffer);
    out
}

/// One emitter frame with the same cursor input.
fn emitter_frame(
    emitter: &mut GridEmitter,
    frame: &Frame,
    cursor: Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>,
) -> String {
    if let Some(((x, y), (style, blinking, color))) = cursor {
        emitter.cursor.set_style(style, blinking, color);
        emitter.cursor.set_position(x, y, true);
    } else {
        emitter.cursor.set_position(1, 1, false);
    }
    emitter.render_sgr(frame)
}

/// The differential driver: a same-sized sequence on both sides, every
/// frame asserted byte-equal (with the frame index on failure).
fn assert_sequence(
    width: usize,
    height: usize,
    frames: &[Frame],
    cursors: &[Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>],
) {
    let mut oracle = oracle(u32::try_from(width).unwrap(), u32::try_from(height).unwrap());
    let mut emitter = emitter(width, height);
    let default_cursor = None;
    for (index, frame) in frames.iter().enumerate() {
        let cursor = cursors.get(index).unwrap_or(&default_cursor);
        let expected = oracle_frame(&mut oracle, frame, *cursor);
        let actual = emitter_frame(&mut emitter, frame, *cursor);
        assert_eq!(
            actual, expected,
            "frame {index} diverged\n  oc:      {actual:?}\n  opentui: {expected:?}"
        );
    }
}

type CursorInput = Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>;

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

fn styled_grid(width: usize, height: usize, rows: &[Vec<Cell>]) -> Frame {
    let mut cells: Vec<Vec<Cell>> = rows.to_vec();
    while cells.len() < height {
        cells.push(vec![Cell::default(); width]);
    }
    Frame { cells, width, height, clusters: None }
}

// -- scenarios -------------------------------------------------------------

/// Pure text: first full frame, partial edits, and the no-op repeat.
#[test]
fn pure_text_edits() {
    let w = 10;
    let h = 3;
    let f0 = grid(w, h, &["hello worl", "second row", "third row!"]);
    let mut f1 = f0.clone();
    f1.cells[0][0].ch = 'H';
    f1.cells[2][9].ch = '?';
    let f2 = f1.clone(); // no-op frame (identical grid)
    assert_sequence(w, h, &[f0.clone(), f1, f2, f0], &[]);
}

/// 256-color palettes on both planes.
#[test]
fn indexed_256_colors() {
    let w = 8;
    let h = 2;
    let f0 = styled_grid(
        w,
        h,
        &[
            (0..w)
                .map(|i| Cell {
                    ch: char::from(b'a' + i as u8),
                    fg: CellColor::Indexed(u8::try_from(i * 7).unwrap()),
                    bg: CellColor::Default,
                    attrs: 0,
                })
                .collect(),
            (0..w)
                .map(|i| Cell {
                    ch: char::from(b'A' + i as u8),
                    fg: CellColor::Default,
                    bg: CellColor::Indexed(u8::try_from(255 - i * 3).unwrap()),
                    attrs: 0,
                })
                .collect(),
        ],
    );
    let mut f1 = f0.clone();
    for row in &mut f1.cells {
        for cell in row.iter_mut() {
            if let CellColor::Indexed(i) = cell.fg {
                cell.fg = CellColor::Indexed(i.wrapping_add(1));
            }
        }
    }
    assert_sequence(w, h, &[f0, f1], &[]);
}

/// Truecolor RGB on both planes.
#[test]
fn rgb_colors() {
    let w = 6;
    let h = 2;
    let f0 = styled_grid(
        w,
        h,
        &[
            (0..w)
                .map(|i| Cell {
                    ch: char::from(b'x' + i as u8),
                    fg: CellColor::Rgb(u8::try_from(i).unwrap(), 40, 255),
                    bg: CellColor::Rgb(0, u8::try_from(200 - i * 10).unwrap(), 30),
                    attrs: 0,
                })
                .collect(),
            (0..w)
                .map(|i| Cell {
                    ch: ' ',
                    fg: CellColor::Default,
                    bg: CellColor::Rgb((i * 40) as u8, (i * 40) as u8, (i * 40) as u8),
                    attrs: 0,
                })
                .collect(),
        ],
    );
    let f1 = blank_grid(w, h); // everything cleared back to blank
    assert_sequence(w, h, &[f0.clone(), f1, f0], &[]);
}

/// Every attribute bit, alone and combined.
#[test]
fn attribute_combinations() {
    let w = 8;
    let h = 2;
    let f0 = styled_grid(
        w,
        h,
        &[
            (0..w)
                .map(|i| Cell {
                    ch: char::from(b'0' + i as u8),
                    attrs: 1 << i,
                    fg: CellColor::Default,
                    bg: CellColor::Default,
                })
                .collect(),
            vec![
                Cell { ch: 'a', attrs: 0b1010_1010, ..Cell::default() },
                Cell { ch: 'b', attrs: 0b0101_0101, ..Cell::default() },
                Cell { ch: 'c', attrs: 0b1111_1111, ..Cell::default() },
                Cell { ch: 'd', attrs: 0, ..Cell::default() },
                Cell { ch: 'e', attrs: 1, ..Cell::default() },
                Cell { ch: 'f', attrs: 1 << 7, ..Cell::default() },
                Cell { ch: 'g', attrs: 0b0011_0000, ..Cell::default() },
                Cell { ch: 'h', attrs: 0b0000_1111, ..Cell::default() },
            ],
        ],
    );
    let mut f1 = f0.clone();
    for cell in f1.cells[0].iter_mut() {
        cell.attrs = cell.attrs.rotate_left(1) & 0xFF;
    }
    assert_sequence(w, h, &[f0, f1], &[]);
}

/// Wide characters/CJK + continuation cells.
#[test]
fn wide_cjk_and_continuation() {
    let w = 8;
    let h = 2;
    let f0 = styled_grid(
        w,
        h,
        &[
            vec![
                Cell { ch: '漢', ..Cell::default() },
                Cell { ch: CONTINUATION, ..Cell::default() },
                Cell { ch: '字', ..Cell::default() },
                Cell { ch: CONTINUATION, ..Cell::default() },
                Cell { ch: 'a', ..Cell::default() },
                Cell { ch: 'あ', ..Cell::default() },
                Cell { ch: CONTINUATION, ..Cell::default() },
                Cell { ch: 'b', ..Cell::default() },
            ],
            vec![
                Cell { ch: 'Ｗ', attrs: 1, fg: CellColor::Indexed(202), ..Cell::default() },
                Cell { ch: CONTINUATION, attrs: 1, fg: CellColor::Indexed(202), ..Cell::default() },
                Cell { ch: '科', ..Cell::default() },
                Cell { ch: CONTINUATION, ..Cell::default() },
                Cell::default(),
                Cell::default(),
                Cell::default(),
                Cell::default(),
            ],
        ],
    );
    let mut f1 = f0.clone();
    f1.cells[0][4] = Cell { ch: '字', fg: CellColor::Rgb(1, 2, 3), ..Cell::default() };
    f1.cells[0][5] = Cell { ch: CONTINUATION, fg: CellColor::Rgb(1, 2, 3), ..Cell::default() };
    assert_sequence(w, h, &[f0.clone(), f1, f0.clone()], &[]);
}

/// Row-end truncation: a wide cluster cut at the last column (the
/// continuation cell falls outside the grid).
#[test]
fn row_end_truncation() {
    let w = 5;
    let f0 = styled_grid(
        w,
        1,
        &[vec![
            Cell { ch: 'a', ..Cell::default() },
            Cell { ch: 'b', ..Cell::default() },
            Cell { ch: 'c', ..Cell::default() },
            Cell { ch: 'd', ..Cell::default() },
            Cell { ch: '漢', ..Cell::default() },
        ]],
    );
    let f1 = styled_grid(
        w,
        1,
        &[vec![
            Cell { ch: '漢', ..Cell::default() },
            Cell { ch: CONTINUATION, ..Cell::default() },
            Cell { ch: 'x', ..Cell::default() },
            Cell { ch: 'y', ..Cell::default() },
            Cell { ch: '漢', ..Cell::default() },
        ]],
    );
    assert_sequence(w, 1, &[f0, f1], &[]);
}

/// Empty frames: the identical grid emits nothing (cursor steady).
#[test]
fn empty_frame_emits_nothing() {
    let w = 4;
    let f0 = grid(w, 1, &["abcd"]);
    let f1 = f0.clone();
    let mut oracle = oracle(u32::try_from(w).unwrap(), 1);
    let mut emitter = emitter(w, 1);
    let expected = oracle_frame(&mut oracle, &f0, None);
    let actual = emitter_frame(&mut emitter, &f0, None);
    assert_eq!(actual, expected);
    assert!(!actual.is_empty(), "the first frame emits a full repaint");
    let expected = oracle_frame(&mut oracle, &f1, None);
    let actual = emitter_frame(&mut emitter, &f1, None);
    assert_eq!(actual, expected);
    assert!(actual.is_empty(), "the identical frame is a no-op (no envelope)");
}

/// All-change frame after a settled one.
#[test]
fn all_change_frame() {
    let w = 6;
    let h = 2;
    let f0 = grid(w, h, &["aaaaaa", "bbbbbb"]);
    let mut f1 = blank_grid(w, h);
    for row in &mut f1.cells {
        for cell in row.iter_mut() {
            *cell = Cell {
                ch: 'z',
                attrs: 1 | 1 << 3,
                fg: CellColor::Indexed(99),
                bg: CellColor::Rgb(9, 9, 9),
            };
        }
    }
    assert_sequence(w, h, &[f0.clone(), f1, f0.clone(), f0.clone()], &[]);
}

/// Resize → full repaint: the emitter's prev grid invalidates; the
/// oracle mirrors upstream `resize` (fresh blank buffers — driven by
/// replacing the public buffer fields, avoiding the internal render's
/// rasterization over the harness's poked cells).
#[test]
fn resize_full_repaint() {
    let (w, h) = (4usize, 2usize);
    let f0 = grid(w, h, &["abcd", "efgh"]);
    let (w2, h2) = (6usize, 3usize);
    let f1 = grid(w2, h2, &["ijklmn", "opqrst", "uvwxyz"]);
    let mut oracle = oracle(u32::try_from(w).unwrap(), u32::try_from(h).unwrap());
    let mut emitter = emitter(w, h);
    let expected = oracle_frame(&mut oracle, &f0, None);
    let actual = emitter_frame(&mut emitter, &f0, None);
    assert_eq!(actual, expected);
    // resize both sides
    emitter.resize(w2, h2);
    oracle.config.width = u32::try_from(w2).unwrap();
    oracle.config.height = u32::try_from(h2).unwrap();
    oracle.next_render_buffer = opentui_core::buffer::OptimizedBuffer::new(
        u32::try_from(w2).unwrap(),
        u32::try_from(h2).unwrap(),
    );
    oracle.current_render_buffer = opentui_core::buffer::OptimizedBuffer::new(
        u32::try_from(w2).unwrap(),
        u32::try_from(h2).unwrap(),
    );
    let expected = oracle_frame(&mut oracle, &f1, None);
    let actual = emitter_frame(&mut emitter, &f1, None);
    assert_eq!(
        actual, expected,
        "post-resize frame diverged\n  oc:      {actual:?}\n  opentui: {expected:?}"
    );
    // and a follow-up diff frame at the new size
    let mut f2 = f1.clone();
    f2.cells[2][0].ch = 'X';
    let expected = oracle_frame(&mut oracle, &f2, None);
    let actual = emitter_frame(&mut emitter, &f2, None);
    assert_eq!(actual, expected);
}

/// Cursor dynamics: first show (the style/color burst), moves, hide,
/// re-show, style/blink/color changes.
#[test]
fn cursor_dynamics() {
    let (w, h) = (5usize, 2usize);
    let f0 = grid(w, h, &["hello", "world"]);
    let mut f1 = f0.clone();
    f1.cells[0][4].ch = '!';
    let cursor_show: CursorInput = Some(((5, 1), (CursorStyle::Block, true, (255, 255, 255))));
    let cursor_move: CursorInput = Some(((2, 2), (CursorStyle::Block, true, (255, 255, 255))));
    let cursor_hide: CursorInput = None;
    let cursor_restyle: CursorInput = Some(((2, 2), (CursorStyle::Underline, false, (1, 2, 3))));
    let cursor_line: CursorInput = Some(((1, 1), (CursorStyle::Line, true, (1, 2, 3))));
    assert_sequence(
        w,
        h,
        &[f0.clone(), f0.clone(), f1.clone(), f1.clone(), f0, f1],
        &[cursor_show, cursor_move, cursor_move, cursor_hide, cursor_restyle, cursor_line],
    );
}

/// The pseudo-random battering: 40 frames of mixed vocabulary and
/// cursor churn on a 12x5 grid (deterministic seed).
#[test]
fn pseudo_random_sequence() {
    let (w, h) = (12usize, 5usize);
    // xorshift64* — a stable in-test PRNG (no external dep).
    let mut state: u64 = 0x2026_B3B5_0FF6_46D1;
    let mut next = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    let chars: Vec<char> = "ab漢字Z9 Ｗ x".chars().collect();
    let mut frames = Vec::new();
    for _ in 0..40 {
        let cells = (0..h)
            .map(|_| {
                (0..w)
                    .map(|_| {
                        let pick = next() % 100;
                        let ch = if pick < 8 {
                            CONTINUATION
                        } else {
                            chars[usize::try_from(next() % u64::try_from(chars.len()).unwrap())
                                .unwrap()]
                        };
                        let color = match next() % 3 {
                            0 => CellColor::Default,
                            1 => CellColor::Indexed((next() % 256) as u8),
                            _ => CellColor::Rgb(
                                (next() % 256) as u8,
                                (next() % 256) as u8,
                                (next() % 256) as u8,
                            ),
                        };
                        let bg = match next() % 4 {
                            0 => CellColor::Default,
                            1 => CellColor::Indexed((next() % 256) as u8),
                            _ => CellColor::Rgb((next() % 256) as u8, 3, 200),
                        };
                        Cell { ch, attrs: (next() % 256) as u16, fg: color, bg }
                    })
                    .collect()
            })
            .collect();
        frames.push(Frame { cells, width: w, height: h, clusters: None });
    }
    let mut cursors: Vec<CursorInput> = Vec::new();
    for i in 0..40 {
        let pick = next() % 4;
        let input = if pick == 0 {
            None
        } else {
            let style = match (next() % 3, pick) {
                (0, _) => CursorStyle::Block,
                (1, _) => CursorStyle::Line,
                _ => CursorStyle::Underline,
            };
            Some((
                (1 + (next() % w as u64) as u32, 1 + (next() % h as u64) as u32),
                (style, next() % 2 == 0, ((next() % 256) as u8, 0, (next() % 256) as u8)),
            ))
        };
        cursors.push(input);
        let _ = i;
    }
    assert_sequence(w, h, &frames, &cursors);
}

/// The upstream `frame_started` quirk, pinned: a run closed by a
/// trailing unchanged cell zeroes `run_length`, so a frame WITH cell
/// bytes can carry NO cursor tail (the oracle agrees byte-for-byte —
/// the port is faithful). `emit` re-asserts the visible cursor on top
/// (the retired writer's per-frame contract).
#[test]
fn steady_visible_cursor_over_changing_cells() {
    let (w, h) = (5usize, 2usize);
    let mut oracle = oracle(u32::try_from(w).unwrap(), u32::try_from(h).unwrap());
    let mut emitter = emitter(w, h);
    let mut frame = blank_grid(w, h);
    frame.cells[0][0].ch = 'a';
    let cursor: CursorInput = Some(((1, 1), (CursorStyle::Block, true, (255, 255, 255))));
    for step in 0..4u8 {
        frame.cells[0][0].ch = char::from(b'a' + step);
        let out = emitter_frame(&mut emitter, &frame, cursor);
        let reference = oracle_frame(&mut oracle, &frame, cursor);
        assert_eq!(out, reference, "frame {step} diverged");
        if step > 0 {
            // the quirk: cells emitted, no cursor tail (the run closed
            // on the unchanged cells behind the glyph)
            assert!(!out.ends_with("\x1b[?25h"), "frame {step}: {out:?}");
        }
        // emit() layers the cursor pin over the same sgr
        let mut emitter2 = GridEmitter::new(w, h);
        let _ = emitter2.render_sgr(&blank_grid(w, h));
        let mut sink = Vec::new();
        for warm in 0..=step {
            frame.cells[0][0].ch = char::from(b'a' + warm);
            sink.clear();
            emitter2.cursor.set_position(1, 1, true);
            emitter2.emit(&mut sink, &frame).expect("emit");
        }
        let text = String::from_utf8(sink).expect("utf8");
        assert!(text.ends_with("\x1b[?2026l"), "envelope closed: {text:?}");
        let body = text
            .strip_prefix("\x1b[?2026h")
            .and_then(|t| t.strip_suffix("\x1b[?2026l"))
            .unwrap_or_default();
        assert!(body.ends_with("\x1b[1;1H\x1b[?25h"), "cursor pin: {body:?}");
    }
}

// -- the cluster sidecar (lane B3b follow-up) ----------------------------
//
// `Cell::ch` carries only a cluster's first codepoint; the Frame's
// `clusters` sidecar carries the full multi-codepoint symbol. These
// tests pin the emitter's three observable behaviors: full-symbol
// printing, cluster-aware diffing, and the per-frame hidden-cursor
// assertion (the retired writer's observable contract).

fn cluster_frame(symbol: &str) -> Frame {
    let base = grid(4, 1, &["    "]);
    let mut frame = base;
    frame.cells[0][0].ch = symbol.chars().next().expect("nonempty");
    frame.cells[0][1].ch = CONTINUATION;
    let mut clusters = std::collections::BTreeMap::new();
    clusters.insert((0, 0), symbol.to_owned());
    frame.clusters = Some(Box::new(clusters));
    frame
}

#[test]
fn cluster_sidecar_prints_the_full_symbol() {
    let mut emitter = GridEmitter::new(4, 1);
    let out = emitter.render_sgr(&cluster_frame("👨‍👩‍👧‍👦"));
    assert!(
        out.contains("👨\u{200d}👩\u{200d}👧\u{200d}👦"),
        "the full ZWJ family must be printed: {out:?}"
    );
}

#[test]
fn cluster_sidecar_diffs_on_cluster_change_alone() {
    let mut emitter = emitter(4, 1);
    let _ = emitter.render_sgr(&cluster_frame("👨‍👩‍👧‍👦"));
    // same first char + same cells, a DIFFERENT cluster: must repaint
    let repainted = emitter.render_sgr(&cluster_frame("👨‍👩‍👧"));
    assert!(repainted.contains("👨\u{200d}👩\u{200d}👧"), "cluster change repaints: {repainted:?}");
    assert!(!repainted.contains("👦"), "the old cluster tail is gone: {repainted:?}");
    // identical cluster: cell-level no-op (cursor hidden ⇒ empty sgr)
    emitter.cursor.set_position(1, 1, false);
    let noop = emitter.render_sgr(&cluster_frame("👨‍👩‍👧"));
    assert!(noop.is_empty(), "identical cluster frame is a no-op: {noop:?}");
}

#[test]
fn emit_asserts_hidden_cursor_every_frame() {
    let mut emitter = emitter(4, 1);
    emitter.cursor.set_position(1, 1, false);
    let mut sink = Vec::new();
    // a fully no-op frame with a hidden caret still carries ?25l (the
    // retired ratatui writer asserted hide on every draw)
    emitter.emit(&mut sink, &blank_grid(4, 1)).expect("emit");
    let text = String::from_utf8(sink.clone()).expect("utf8");
    assert_eq!(text, "\x1b[?2026h\x1b[?25l\x1b[?2026l", "per-frame hide assertion: {text:?}");
    // and the frame after it too (the E1 dialog-open-idle cadence)
    sink.clear();
    emitter.emit(&mut sink, &blank_grid(4, 1)).expect("emit");
    let text = String::from_utf8(sink).expect("utf8");
    assert_eq!(text, "\x1b[?2026h\x1b[?25l\x1b[?2026l", "the assertion repeats: {text:?}");
}

#[test]
fn cluster_roundtrip_through_vt100() {
    // frame: "family <family> flag" on row 0, family = 2 reserved cells
    let mut frame = grid(30, 1, &["                              "]);
    for (i, ch) in "family ".chars().enumerate() {
        frame.cells[0][i] = Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default };
    }
    let family = "👨‍👩‍👧‍👦";
    frame.cells[0][7].ch = '👨';
    frame.cells[0][8].ch = CONTINUATION;
    let mut clusters = std::collections::BTreeMap::new();
    clusters.insert((0, 7), family.to_owned());
    frame.clusters = Some(Box::new(clusters));
    // "flag" BEYOND the family's overflow span (its 2 frame columns
    // plus the 6-column per-codepoint overflow a non-joining width
    // model produces): cells inside the span are last-writer-loses
    // under such a model, cells outside must survive.
    for (i, ch) in "flag".chars().enumerate() {
        frame.cells[0][22 + i] =
            Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default };
    }
    let mut emitter = GridEmitter::new(30, 1);
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
    eprintln!("vt row: {text:?}");
    // (space-stripped containment — the wide-glyph continuation cells
    // read as spaces, exactly the a3 corpus assertion's semantics)
    let norm: String = text.chars().filter(|c| *c != ' ').collect();
    let target: String = family.chars().filter(|c| *c != ' ').collect();
    assert!(norm.contains(&target), "family must round-trip: {text:?}");
    assert!(text.contains("flag"), "flag must survive: {text:?}");
}
