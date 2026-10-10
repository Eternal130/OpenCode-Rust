//! Message-flow golden acceptance (7.2 wave 1 lane C).
//!
//! * **`markdown-frame`** (tier C1) — the `TextPart` markdown mount at
//!   80/40: text-grid **byte-exact** (headings/bold/inline-code/
//!   emphasis/lists/fence through the one wrap seam, the bold run
//!   crossing the wrap boundary at BOTH widths), canonical ANSI
//!   byte-exact everywhere except the tree-sitter code fence (C2 owns
//!   its colors).
//! * **`spinner-frozen`** (tier C1) — the running `InlineToolRow` frozen
//!   at clock 0 (braille `⠋`): text grid AND canonical ANSI
//!   byte-exact at 80/40.
//! * **`message-groups`** (tier A) — the full-app session frame: the
//!   production transcript (rows 0–22: user messages, `→ Explored` /
//!   `+ Thought` group headers, text parts, `Build · model · duration`
//!   footers) plus a **wave-2 composer stub** for the bottom chrome
//!   (rows 23–29) — the composer is wave 2's lane; its bytes here are
//!   a labeled fixture (`composer_bottom_chrome`), with the
//!   production-only canonical-ANSI proof over the transcript rows.

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use crate::components::message::list::{
    message_groups_fixture, render_transcript, FollowState, TranscriptOptions,
};
use crate::components::message::markdown::{
    markdown_frame_fixture, render_markdown, MarkdownStyles,
};
use crate::components::message::rows::spinner_frozen_frame;
use crate::components::tool_rows::fill_style;
use crate::layout::{Placed, PlacedLine};
use crate::render::{paint_placed, serialize_canonical, Frame};
use crate::theme::ThemeState;
use parity::tui_frame as pf;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn theme() -> ThemeState {
    ThemeState::v2_dark()
}

fn assert_text_grid_match(id: &str, width: usize, frame: &Frame) {
    let golden = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)\nfirst divergence row:\n  golden: {:?}\n  actual: {:?}",
        diff.first_divergence,
        diff.match_pct(),
        golden
            .rows
            .get(diff.first_divergence.as_ref().map_or(0, |cell| cell.row))
            .map(String::as_str),
        rendered
            .rows
            .get(diff.first_divergence.as_ref().map_or(0, |cell| cell.row))
            .map(String::as_str),
    );
}

/// The `TextPart` mount: markdown inside `box paddingLeft=3` at 100%.
fn markdown_frame(
    width: u16,
    height: u16,
    styles: &MarkdownStyles,
    styles_flat: &crate::theme::ThemeStyles,
) -> Frame {
    let body = usize::from(width) - 3;
    let lines = render_markdown(&markdown_frame_fixture(), body, styles);
    let placed = lines
        .iter()
        .enumerate()
        .map(|(index, line)| Placed {
            x: 3,
            y: index,
            width: body,
            lines: vec![PlacedLine { runs: line.0.clone() }],
            tag: None,
        })
        .collect::<Vec<_>>();
    paint_placed(&placed, (width, height), fill_style(styles_flat))
}

#[test]
fn markdown_frame_matches_text_grid_at_80() {
    let theme = theme();
    let frame =
        markdown_frame(80, 30, &MarkdownStyles::from_theme(&theme.styles()), &theme.styles());
    assert_text_grid_match("markdown-frame", 80, &frame);
}

#[test]
fn markdown_frame_matches_text_grid_at_40() {
    let theme = theme();
    let frame =
        markdown_frame(40, 30, &MarkdownStyles::from_theme(&theme.styles()), &theme.styles());
    assert_text_grid_match("markdown-frame", 40, &frame);
}

/// Canonical ANSI parity for every row OUTSIDE the tree-sitter code
/// fence: the styled spans (bold/strong/em/code/heading) reproduce the
/// reference's per-cell styles exactly — the wrap boundary inside the
/// bold run keeps the strong style on both fragments.
#[test]
fn markdown_frame_ansi_matches_outside_the_code_fence() {
    let theme = theme();
    for width in [80usize, 40] {
        let frame = markdown_frame(
            width as u16,
            30,
            &MarkdownStyles::from_theme(&theme.styles()),
            &theme.styles(),
        );
        let golden =
            std::fs::read_to_string(goldens().join(format!("markdown-frame.w{width}.ansi")))
                .expect("golden ansi");
        let golden_grid =
            std::fs::read_to_string(goldens().join(format!("markdown-frame.w{width}.txt")))
                .expect("golden txt");
        // Locate the fence (its first code line) and blank it out of the
        // comparison on BOTH sides: rows [fence_start, fence_start + 2).
        let fence_line = golden_grid
            .lines()
            .enumerate()
            .find(|(_, line)| line.trim_end() == "   const x = parse(input)");
        let Some((fence_start, _)) = fence_line else {
            panic!("{width}: fence line not found in golden");
        };
        let drop_rows = [fence_start, fence_start + 1];
        let keep = |rows: Vec<Vec<pf::StyledCell>>| -> Vec<Vec<pf::StyledCell>> {
            rows.into_iter()
                .enumerate()
                .filter(|(index, _)| !drop_rows.contains(index))
                .map(|(_, row)| row)
                .collect()
        };
        let expected = pf::AnsiFrame {
            rows: keep(pf::parse_ansi(&golden, Some((30, width))).expect("golden parses").rows),
            height: 28,
            width,
        };
        let actual = pf::AnsiFrame {
            rows: keep(
                pf::parse_ansi(&serialize_canonical(&frame), Some((30, width)))
                    .expect("render parses")
                    .rows,
            ),
            height: 28,
            width,
        };
        let expected_bytes = pf::render_ansi(&expected);
        let actual_bytes = pf::render_ansi(&actual);
        let diff = pf::compare_ansi_bytes(&expected_bytes, &actual_bytes);
        assert!(
            diff.equal,
            "markdown-frame@{width} ansi differs outside the fence: first divergence {:?}",
            diff.first_divergent_offset
        );
    }
}

#[test]
fn spinner_frozen_matches_text_grid_and_ansi_at_80() {
    let theme = theme();
    let frame = spinner_frozen_frame(&theme.styles(), 80, 6);
    assert_text_grid_match("spinner-frozen", 80, &frame);
    assert_ansi_byte_match("spinner-frozen", 80, &frame);
}

#[test]
fn spinner_frozen_matches_text_grid_and_ansi_at_40() {
    let theme = theme();
    let frame = spinner_frozen_frame(&theme.styles(), 40, 6);
    assert_text_grid_match("spinner-frozen", 40, &frame);
    assert_ansi_byte_match("spinner-frozen", 40, &frame);
}

fn assert_ansi_byte_match(id: &str, width: usize, frame: &Frame) {
    let golden = std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi")))
        .expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(
        &golden,
        &serialize_canonical(frame),
        pf::ComparisonDepth::TruecolorCanonical,
    )
    .expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} canonical ansi differs: first divergence {:?}",
        diff.first_divergent_offset
    );
}

// -- message-groups ------------------------------------------------------------

// -- group-view-instructions -----------------------------------------------------

/// The `group-view-instructions` fixture (`tui-dump/scenarios/
/// session_ui.tsx` `mountGroupView`): two synthetic instruction loads
/// (one file each), collapsed — the summary row at paddingTop 2.
fn instructions_fixture(
) -> (Vec<crate::components::message::model::Message>, crate::components::message::model::Row) {
    use crate::components::message::model::{Entry, GroupKind, Message, Row};
    let messages = vec![
        Message::Synthetic {
            id: "AGENTS.md".into(),
            instruction_paths: vec!["AGENTS.md".into()],
            created: 1,
        },
        Message::Synthetic {
            id: "packages/tui/AGENTS.md".into(),
            instruction_paths: vec!["packages/tui/AGENTS.md".into()],
            created: 1,
        },
    ];
    let row = Row::Group {
        kind: GroupKind::Instructions,
        children: messages
            .iter()
            .map(|message| Row::Entry(Entry::Message { message_id: message.id().to_owned() }))
            .collect(),
        size: messages.len(),
        completed: true,
    };
    (messages, row)
}

fn group_view_instructions_frame(width: u16) -> Frame {
    let theme = theme();
    let (messages, row) = instructions_fixture();
    crate::components::message::list::group_view_frame(
        &row,
        &messages,
        &TranscriptOptions::default(),
        &theme.styles(),
        width,
        24,
        2,
    )
}

#[test]
fn group_view_instructions_matches_text_grid_and_ansi_at_80() {
    let frame = group_view_instructions_frame(80);
    assert_text_grid_match("group-view-instructions", 80, &frame);
    assert_ansi_byte_match("group-view-instructions", 80, &frame);
}

#[test]
fn group_view_instructions_matches_text_grid_and_ansi_at_160() {
    let frame = group_view_instructions_frame(160);
    assert_text_grid_match("group-view-instructions", 160, &frame);
    assert_ansi_byte_match("group-view-instructions", 160, &frame);
}

/// The transcript rows (production): 23 rows at 80 — content (21),
/// `paddingBottom 1`, the jump-to-latest slot row.
fn transcript_frame(width: u16, transcript_height: u16, theme: &ThemeState) -> Frame {
    let messages = message_groups_fixture();
    let transcript = render_transcript(
        &messages,
        &TranscriptOptions::default(),
        &theme.styles(),
        usize::from(width),
    );
    transcript.paint(FollowState::Sticky, &theme.styles(), (width, transcript_height), None)
}

/// The bottom chrome rows the golden frame carries below the
/// transcript — the REAL composer mounted at the session route's shape
/// (the F4 swap: the placeholder stub is retired from the goldens),
/// pasted at the column's inset into a full-width frame.
fn composer_bottom_chrome(width: u16, theme: &ThemeState) -> Frame {
    let inset: u16 = if width < 44 { 1 } else { 2 };
    let composer = crate::components::session_screen::session_composer_frame(
        &crate::components::composer::PromptDraft::default(),
        width - inset * 2,
        theme,
        "/tmp/opencode/packages/tui:main",
    );
    let mut cells = crate::render::paint_placed(
        &[],
        (width, 7),
        crate::components::tool_rows::fill_style(&theme.styles()),
    )
    .cells;
    for (row, line) in composer.cells.iter().enumerate() {
        let slot = &mut cells[row];
        slot[inset as usize..inset as usize + line.len()].clone_from_slice(line);
    }
    Frame { cells, width: usize::from(width), height: 7 }
}

/// Stitch the production transcript rows with the composer stub into
/// the full golden frame. The transcript column paints its own 23-row
/// frame (22-row sticky viewport + the jump slot) so the bottom-sticky
/// clipping matches the reference scrollbox; the chrome rows then
/// follow.
fn message_groups_frame(width: u16, height: u16, theme: &ThemeState) -> Frame {
    let transcript_rows = height - 7;
    let transcript = transcript_frame(width, transcript_rows, theme);
    let chrome = composer_bottom_chrome(width, theme);
    let mut cells = transcript.cells;
    cells.extend(chrome.cells);
    Frame { cells, width: usize::from(width), height: usize::from(height) }
}

#[test]
fn message_groups_matches_text_grid_at_80() {
    let theme = theme();
    let frame = message_groups_frame(80, 30, &theme);
    assert_text_grid_match("message-groups", 80, &frame);
}

#[test]
fn message_groups_matches_text_grid_at_40() {
    let theme = theme();
    let frame = message_groups_frame(40, 30, &theme);
    assert_text_grid_match("message-groups", 40, &frame);
}

/// Production-only canonical ANSI over the transcript rows: the user
/// boxes (raised fill + agent border), group headers, markdown text
/// parts and footers match the golden stream byte-for-byte — the
/// composer stub participates in no way.
#[test]
fn message_groups_transcript_ansi_is_byte_exact_at_80_and_40() {
    for width in [80u16, 40] {
        let theme = theme();
        let transcript = transcript_frame(width, 23, &theme);
        let golden =
            std::fs::read_to_string(goldens().join(format!("message-groups.w{width}.ansi")))
                .expect("golden ansi");
        let golden_frame =
            pf::parse_ansi(&golden, Some((30, usize::from(width)))).expect("golden parses");
        let transcript_slice = pf::AnsiFrame {
            rows: golden_frame.rows.into_iter().take(23).collect(),
            height: 23,
            width: usize::from(width),
        };
        let diff = pf::compare_ansi_bytes_at(
            &pf::render_ansi(&transcript_slice),
            &serialize_canonical(&transcript),
            pf::ComparisonDepth::TruecolorCanonical,
        )
        .expect("ansi parses");
        assert!(
            diff.equal,
            "message-groups@{width} transcript ansi differs: first divergence {:?}",
            diff.first_divergent_offset
        );
    }
}
