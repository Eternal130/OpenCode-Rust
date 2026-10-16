//! Home-route golden acceptance (Phase 8, the registered export chore
//! closed): the full reference home frame — OPENCODE wordmark,
//! centered prompt composer (placeholder pinned to entry 0, the
//! exporter's `Math.random = () => 0` seed) and the `home.footer`
//! version row — text grid AND canonical ANSI byte-exact at the
//! manifest's `truecolor-canonical` depth (`home.w80` / `home.w160`).

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use crate::components::composer::PromptDraft;
use crate::components::home::{render_home_view, HomeView};
use crate::render::{serialize_canonical, Frame};
use crate::theme::ThemeState;
use parity::tui_frame as pf;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

/// The fixture the exporter mounts: placeholder entry 0, the fixture
/// location, app version "test".
fn view(draft: &PromptDraft) -> HomeView<'_> {
    HomeView {
        draft,
        placeholder: "Ask anything… \"Fix a TODO in the codebase\"".into(),
        location: "/tmp/opencode/packages/tui:main".into(),
        directory: "/tmp/opencode/packages/tui".into(),
        location_hovered: false,
        version: "test".into(),
        metadata: crate::components::composer::render::MetadataLine::default(),
        plugin_hints: Vec::new(),
        plugin_actions: Vec::new(),
        mcp: None,
        plugins_failed: None,
        autocomplete: None,
        rail: None,
        caret: None,
    }
}

fn assert_matches(id: &str, width: usize, frame: &Frame) {
    let golden = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)\nrendered:\n{}\ngolden:\n{}",
        diff.first_divergence,
        diff.match_pct(),
        rendered.rows.join("\n"),
        golden.rows.join("\n")
    );
    let ansi = serialize_canonical(frame);
    let golden_ansi =
        std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi"))).expect("ansi");
    let diff =
        pf::compare_ansi_bytes_at(&golden_ansi, &ansi, pf::ComparisonDepth::TruecolorCanonical)
            .expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} ansi differs: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn home_matches_text_grid_and_ansi_at_80() {
    let theme = ThemeState::v2_dark();
    let draft = PromptDraft::default();
    let (frame, _) = render_home_view(&view(&draft), &theme, (80, 30));
    assert_matches("home", 80, &frame);
}

#[test]
fn home_matches_text_grid_and_ansi_at_160() {
    let theme = ThemeState::v2_dark();
    let draft = PromptDraft::default();
    let (frame, _) = render_home_view(&view(&draft), &theme, (160, 30));
    assert_matches("home", 160, &frame);
}
