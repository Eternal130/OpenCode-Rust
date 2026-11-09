//! Lane D4.2 — the emission-flip regression pins.
//!
//! The flip's construct proof ran as a LIVE differential while both
//! emitters existed (commit 41783d2a: every golden replay-corpus
//! frame and the full emitter vocabulary through the retired
//! `GridEmitter` AND the new `OpentuiEmitter`, byte-identical
//! including the `?2026` envelope and the two oc-tui cursor
//! policies). The retired emitter is deleted; these pins keep the
//! new emitter's bytes FROZEN at the values the differential proved:
//! stable digests over the corpus + vocabulary sequences (FNV-1a of
//! the emitted bytes, pinned below) plus per-frame cursor-policy
//! assertions and the cluster pixel contract.

use crate::render::opentui_emit::OpentuiEmitter;
use crate::render::{Cell, CellColor, Frame, CONTINUATION};
use opentui_core::renderables::textarea::CursorStyle;

type CursorInput = Option<((u32, u32), (CursorStyle, bool, (u8, u8, u8)))>;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn seed_cursor(new: &mut OpentuiEmitter, cursor: CursorInput) {
    if let Some(((x, y), (style, blinking, color))) = cursor {
        let state = new.cursor();
        state.set_style(style, blinking, opentui_core::Rgba::rgb(color.0, color.1, color.2));
        state.set_position(x, y, true);
    } else {
        new.cursor().set_position(1, 1, false);
    }
}

fn emit_bytes(new: &mut OpentuiEmitter, frame: &Frame, cursor: CursorInput) -> Vec<u8> {
    seed_cursor(new, cursor);
    let mut sink = Vec::new();
    new.emit(&mut sink, frame).expect("emit");
    sink
}

/// The policy tail: every emitted frame with a visible caret ends
/// with CUP + `?25h` inside the envelope; every frame with a hidden
/// caret carries `?25l`; a fully invisible frame writes nothing.
fn assert_cursor_policy(bytes: &[u8], cursor: CursorInput, label: &str) {
    let text = String::from_utf8_lossy(bytes);
    if cursor.is_some() {
        assert!(
            text == "\x1b[?2026h\x1b[?25l\x1b[?2026l" || text.ends_with("\x1b[?25h\x1b[?2026l"),
            "{label}: visible policy: {text:?}"
        );
    } else {
        assert!(
            text.is_empty() || text.ends_with("\x1b[?25l\x1b[?2026l"),
            "{label}: hidden policy: {text:?}"
        );
    }
}

// -- the golden replay corpus ----------------------------------------------

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

/// Drive every replay script through the unified pump, feeding each
/// step's live frame (route + blockers + dialog overlay) through the
/// emitter with the pump's own caret policy. The emission digest is
/// PINNED at the value the differential gate proved byte-identical to
/// the retired `GridEmitter`; the corpus frames carry no cluster
/// sidecars (a precondition — cluster frames print through the
/// deferred pass with different framing bytes, by design).
#[test]
fn replay_corpus_emission_is_pinned() {
    let dir = goldens();
    let scripts = std::fs::read_dir(dir.join("replay"))
        .expect("replay corpus")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().is_some_and(|e| e == "json")).then_some(path)
        })
        .collect::<Vec<_>>();
    assert!(!scripts.is_empty(), "the replay corpus must be present");
    let mut emitter = OpentuiEmitter::new(100, 30);
    let mut digest: u64 = 0;
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
        for (index, step) in script.steps.iter().enumerate() {
            crate::replay::arm_scenario(&script.id, &mut pump, index);
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
            // the pump's caret policy (app.rs)
            let cursor: CursorInput = if pump.store.top_dialog().is_none()
                && !pump.permission_live()
                && !pump.form_live()
            {
                pump.store.caret_cell().filter(|(x, y)| *x < 100 && *y < 30).map(|(x, y)| {
                    (
                        (u32::from(x) + 1, u32::from(y) + 1),
                        (CursorStyle::Line, true, (224, 224, 224)),
                    )
                })
            } else {
                None
            };
            let bytes = emit_bytes(&mut emitter, &frame, cursor);
            assert_cursor_policy(&bytes, cursor, &format!("{} step {index}", script.id));
            digest = digest.wrapping_add(fnv1a(&bytes));
            compared += 1;
        }
    }
    assert!(compared > 10, "the corpus must exercise real frames ({compared})");
    // PINNED (lane D4.2, commit 41783d2a's differential proof).
    assert_eq!(digest, 0x3bdb_d28c_1166_a117, "the corpus emission bytes drifted");
}

// -- the emitter vocabulary -------------------------------------------------

fn grid(width: usize, height: usize, rows: &[&str]) -> Frame {
    let cells = rows
        .iter()
        .map(|row| {
            row.chars()
                .take(width)
                .chain(std::iter::repeat_n(' ', width.saturating_sub(row.chars().count())))
                .map(|ch| Cell { ch, attrs: 0, fg: CellColor::Default, bg: CellColor::Default })
                .collect::<Vec<_>>()
        })
        .chain(
            std::iter::repeat_n(vec![Cell::default(); width], height.saturating_sub(rows.len())),
        )
        .collect();
    Frame { cells, width, height, clusters: None }
}

fn blank_grid(width: usize, height: usize) -> Frame {
    grid(width, height, &[])
}

/// The pinned vocabulary sequence: first-frame force, no-op, revert,
/// hidden idle, then the color/attribute/wide-char vocabulary with
/// cursor show/move/hide/re-style churn, resize full-repaint, and
/// `invalidate_full` — every frame's bytes digested and pinned at the
/// differential-proven value, the cursor policies asserted per frame.
#[test]
fn vocabulary_emission_is_pinned() {
    let (w, h) = (10usize, 3usize);
    let mut emitter = OpentuiEmitter::new(w, h);
    let mut digest: u64 = 0;
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
        let bytes = emit_bytes(&mut emitter, frame, cursor);
        assert_cursor_policy(&bytes, cursor, label);
        digest = digest.wrapping_add(fnv1a(&bytes));
    }

    let (w, h) = (12usize, 5usize);
    let mut emitter = OpentuiEmitter::new(w, h);
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
    for (index, (frame, cursor)) in [&f0, &f1, &f2, &f1, &f0].iter().zip(cursors).enumerate() {
        let bytes = emit_bytes(&mut emitter, frame, cursor);
        assert_cursor_policy(&bytes, cursor, &format!("vocab frame {index}"));
        digest = digest.wrapping_add(fnv1a(&bytes));
    }

    // resize full-repaint + follow-up diff
    let f0 = grid(4, 2, &["abcd", "efgh"]);
    let f1 = grid(6, 3, &["ijklmn", "opqrst", "uvwxyz"]);
    let mut emitter = OpentuiEmitter::new(4, 2);
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f0, None)));
    emitter.resize(6, 3);
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f1, None)));
    let mut f2 = f1.clone();
    f2.cells[2][0].ch = 'X';
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f2, None)));

    // invalidate_full (the post-editor/post-suspend repaint) + resumed diff
    let f0 = grid(5, 2, &["hello", "world"]);
    let mut f1 = f0.clone();
    f1.cells[0][0].ch = 'H';
    let mut emitter = OpentuiEmitter::new(5, 2);
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f0, None)));
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f1, None)));
    emitter.invalidate_full();
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f1, None)));
    let mut f2 = f1.clone();
    f2.cells[1][4].ch = '!';
    digest = digest.wrapping_add(fnv1a(&emit_bytes(&mut emitter, &f2, None)));

    // PINNED (lane D4.2, the differential proof of commit 41783d2a).
    assert_eq!(digest, 0xf748_ba52_0bf0_d923, "the vocabulary emission bytes drifted");
}

/// The cluster pixel contract (the retired emitter's
/// `cluster_roundtrip_through_vt100`, pinned on the new emitter): a
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

// -- G-287⑧: OSC-8 hyperlink emission --------------------------------------

use crate::render::FrameLinkRect;

fn link(x: usize, y: usize, width: usize, url: &str) -> FrameLinkRect {
    FrameLinkRect { x, y, width, url: url.to_owned() }
}

fn emit_linked(emitter: &mut OpentuiEmitter, frame: &Frame, links: &[FrameLinkRect]) -> String {
    emitter.cursor().set_position(1, 1, false);
    let mut sink = Vec::new();
    emitter.emit_with_links(&mut sink, frame, links).expect("emit");
    String::from_utf8_lossy(&sink).into_owned()
}

/// The probe-true face: the link rect's cells wrap in an OSC 8 open
/// (`ESC]8;id=1;URL ESC\`, the renderer's id-pool shape,
/// renderer/frame.rs:599-607) closed by the empty-params terminator
/// (`ESC]8;;ESC\`, renderer.zig:2754) — and the glyph content is
/// untouched (visible text zero-change; the wrapping is the ONLY
/// delta vs the same frame emitted without links).
#[test]
fn osc8_link_rect_wraps_exactly_the_linked_cells() {
    let frame = grid(30, 1, &["see the docs here             "]);
    // Two fresh emitters: each emission is a first-frame full
    // repaint, so the two streams cover the same glyph set and the
    // strip comparison is meaningful (a shared emitter would diff —
    // only the link-attributed cells would re-emit).
    let plain = emit_linked(&mut OpentuiEmitter::new(30, 1), &frame, &[]);
    let wrapped = emit_linked(
        &mut OpentuiEmitter::new(30, 1),
        &frame,
        &[link(8, 0, 4, "https://example.com")],
    );
    assert!(plain.contains("see the docs here"), "{plain:?}");
    assert!(wrapped.contains("\x1b]8;id=1;https://example.com\x1b\\"), "{wrapped:?}");
    assert!(wrapped.contains("\x1b]8;;\x1b\\"), "the close terminator: {wrapped:?}");
    // The visible text is unchanged — strip the OSC 8 sequences and
    // the SGR runs and the glyphs match the plain emission.
    let strip = |s: &str| -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(pos) = rest.find('\x1b') {
            out.push_str(&rest[..pos]);
            rest = &rest[pos..];
            if rest.starts_with("\x1b]8;") {
                let end = rest.find("\x1b\\").expect("OSC terminator");
                rest = &rest[end + 2..];
            } else {
                // any other escape: skip to the letter
                let letter = rest[1..].chars().position(char::is_alphabetic);
                let skip = letter.map_or(rest.len(), |i| i + 2);
                rest = &rest[skip..];
            }
        }
        out.push_str(rest);
        out
    };
    assert_eq!(strip(&plain), strip(&wrapped), "glyph content zero-change");
}

/// The fallback face (the headless/no-probe profile): an empty link
/// slice emits ZERO OSC 8 bytes — byte-identical to the plain emit
/// (the entire golden corpus rides this path).
#[test]
fn no_links_emits_no_osc8() {
    let frame = grid(30, 1, &["see the docs here             "]);
    let mut emitter = OpentuiEmitter::new(30, 1);
    let bytes = emit_linked(&mut emitter, &frame, &[]);
    assert!(!bytes.contains("]8;"), "{bytes:?}");
}

/// The link pool interns (link.zig: same URL → same id across
/// frames): re-emitting the same rect after a no-link frame reuses
/// `id=1`, and a second URL takes `id=2`. A no-change frame after
/// the link frame emits only the close (the diff base keeps the
/// linked cells).
#[test]
fn link_ids_intern_across_frames() {
    let frame = grid(30, 1, &["see the docs here             "]);
    let other = grid(30, 1, &["other row                     "]);
    let mut emitter = OpentuiEmitter::new(30, 1);
    let first = emit_linked(&mut emitter, &frame, &[link(8, 0, 4, "https://a.example")]);
    assert!(first.contains("id=1;https://a.example"), "{first:?}");
    // no-link frame: different content, no OSC 8
    let mid = emit_linked(&mut emitter, &other, &[]);
    assert!(!mid.contains("]8;"), "{mid:?}");
    // the same URL again: still id=1; a new URL takes id=2
    let again = emit_linked(&mut emitter, &frame, &[link(8, 0, 4, "https://a.example")]);
    assert!(again.contains("id=1;https://a.example"), "{again:?}");
    // a fresh rect on new cells takes id=2 (the id=1 cells are
    // unchanged vs the diff base, so they do not re-emit)
    let both = emit_linked(&mut emitter, &frame, &[link(0, 0, 3, "https://b.example")]);
    assert!(both.contains("id=2;https://b.example"), "{both:?}");
}

/// A live transcript frame end-to-end: the walk's `OpenLink` hit rows
/// (probe on) convert to rects and the label's cells carry the OSC 8
/// wrap; probe off, the same frame emits no OSC 8 (the fallback face
/// the golden corpus pins).
#[test]
fn transcript_openlink_rows_drive_osc8_under_the_probe_gate() {
    use crate::components::message::model::{Message, Part, PartTime};
    let app = crate::app::App::new((80, 24));
    app.store.set_messages(
        "s1",
        vec![Message::Assistant {
            id: "a1".into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![Part::Text {
                text: "see [the docs](https://example.com/router) for details".into(),
                time: PartTime::default(),
            }],
            finish: Some("success".into()),
            error: None,
            time: PartTime { created: Some(1), completed: Some(2) },
            tokens: None,
            usage_total: None,
            cache_read: None,
            retry: None,
        }],
    );
    app.store.open_tab("s1", Some("Links".to_owned()));
    let frame = app.frame();
    let rows = app.store.transcript_hit_rows();
    let links: Vec<FrameLinkRect> = rows
        .into_iter()
        .filter_map(|row| match row.action {
            crate::store::TranscriptHitAction::OpenLink { url } => {
                Some(FrameLinkRect { x: row.x, y: row.y, width: row.width, url })
            }
            _ => None,
        })
        .collect();
    assert!(!links.is_empty(), "the label armed a hit rect");
    let mut emitter = OpentuiEmitter::new(80, 24);
    let fallback = emit_linked(&mut emitter, &frame, &[]);
    assert!(!fallback.contains("]8;"), "probe off: plain text: {fallback:?}");
    let wrapped = emit_linked(&mut emitter, &frame, &links);
    assert!(
        wrapped.contains("\x1b]8;id=1;https://example.com/router\x1b\\"),
        "probe on: the label wraps: {wrapped:?}"
    );
}
