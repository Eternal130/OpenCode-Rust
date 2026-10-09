//! 7.2 wave-2 diff-viewer golden (`diff-viewer-split` @80/160) —
//! **tier B**: text-grid byte-exact and canonical ANSI compared at
//! the manifest's `degraded-256` depth (the tier's style fidelity
//! bar).

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use parity::tui_frame as pf;

use crate::components::diff_viewer::{
    diff_viewer_fixture, render_diff_viewer, DiffViewerOptions, DiffViewerState,
};
use crate::render::serialize_canonical;
use crate::theme::ThemeState;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn assert_text_grid_match(id: &str, width: usize, frame: &crate::render::Frame) {
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

fn diff_frame(width: u16) -> crate::render::Frame {
    let theme = ThemeState::v2_dark();
    render_diff_viewer(
        &diff_viewer_fixture(),
        &DiffViewerState::default(),
        &DiffViewerOptions::default(),
        &theme.styles(),
        usize::from(width),
        24,
    )
}

#[test]
fn diff_viewer_split_matches_text_grid_and_ansi_at_80() {
    let frame = diff_frame(80);
    assert_text_grid_match("diff-viewer-split", 80, &frame);
    let golden =
        std::fs::read_to_string(goldens().join("diff-viewer-split.w80.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(
        &golden,
        &serialize_canonical(&frame),
        pf::ComparisonDepth::Degraded256,
    )
    .expect("ansi parses");
    assert!(
        diff.equal,
        "diff-viewer-split@80 ansi differs at degraded-256: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn diff_viewer_split_matches_text_grid_and_ansi_at_160() {
    let frame = diff_frame(160);
    assert_text_grid_match("diff-viewer-split", 160, &frame);
    let golden =
        std::fs::read_to_string(goldens().join("diff-viewer-split.w160.ansi")).expect("ansi");
    let diff = pf::compare_ansi_bytes_at(
        &golden,
        &serialize_canonical(&frame),
        pf::ComparisonDepth::Degraded256,
    )
    .expect("ansi parses");
    assert!(
        diff.equal,
        "diff-viewer-split@160 ansi differs at degraded-256: first divergence {:?}",
        diff.first_divergent_offset
    );
}
