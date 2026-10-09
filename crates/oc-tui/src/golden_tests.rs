//! Foundation acceptance: text-grid frame comparisons against the
//! re-pinned golden corpus (`tools/parity/goldens/tui-frames`,
//! `tui-frame-golden/2` under v2-dark), wired through
//! `tools/parity::tui_frame` with the manifest's per-scenario
//! comparison depth (`compare_ansi_frames_at` / `compare_ansi_bytes_at`).
//!
//! 7.1 coverage:
//! * `session-tabs-horizontal` / `session-tabs-vertical` — text grid
//!   exact at 80/160 (per-cell glow gradients + per-tab background
//!   tints are the 7.2 ANSI polish).
//! * `tool-wrap-rows` — text grid **and** canonical ANSI byte-exact at
//!   the manifest depth (v2-dark tokens painted through
//!   [`crate::theme`]).

#![cfg(test)]
// Frame dimensions come from the manifest (bounded by terminal sizes),
// so the usize→u16 casts cannot truncate real inputs.
#![allow(clippy::cast_possible_truncation)]

use parity::tui_frame as pf;

use crate::components::session_tabs::{paint_bar, paint_rail, TabStatus, TabView, ADD_TAB_WIDTH};
use crate::components::tool_rows::render_tool_wrap;
use crate::store::{SessionTab, Unread};
use crate::theme::ThemeState;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

/// The golden scenario fixture (`tui-dump/scenarios/session_tabs.tsx`):
/// two tabs, first unread-activity, second active, detail "project",
/// the add affordance enabled.
fn golden_tab_views() -> Vec<TabView> {
    vec![
        TabView {
            tab: SessionTab { session_id: "first".into(), title: Some("First".into()) },
            status: TabStatus { unread: Some(Unread::Activity), ..TabStatus::EMPTY },
            detail: Some("project".into()),
        },
        TabView {
            tab: SessionTab { session_id: "second".into(), title: Some("Second".into()) },
            status: TabStatus::EMPTY,
            detail: Some("project".into()),
        },
    ]
}

fn manifest() -> pf::Manifest {
    pf::load_manifest(&goldens()).expect("golden manifest")
}

fn scenario(id: &str) -> pf::ManifestScenario {
    manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("scenario {id} in manifest"))
}

fn assert_text_grid_match(id: &str, width: usize, frame: &AnsiFrameRender) {
    let golden = pf::load_text_grid(&goldens(), id, width).expect("golden text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid.clone(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}% matching)",
        diff.first_divergence,
        diff.match_pct()
    );
}

/// Adapter so tests can talk about one rendered thing.
pub struct AnsiFrameRender {
    pub text_grid: Vec<String>,
    pub height: usize,
    pub width: usize,
    pub ansi: String,
}

impl AnsiFrameRender {
    fn from_frame(frame: &crate::render::Frame) -> Self {
        let (rows, height, width) = (frame.text_grid(), frame.height, frame.width);
        Self { text_grid: rows, height, width, ansi: crate::render::serialize_canonical(frame) }
    }
}

#[test]
fn session_tabs_horizontal_matches_golden_at_80() {
    let theme = ThemeState::v2_dark();
    let frame = paint_bar(&golden_tab_views(), Some("second"), true, (80, 10), &theme.styles());
    assert_text_grid_match("session-tabs-horizontal", 80, &AnsiFrameRender::from_frame(&frame));
}

#[test]
fn session_tabs_horizontal_matches_golden_at_160() {
    let theme = ThemeState::v2_dark();
    let frame = paint_bar(&golden_tab_views(), Some("second"), true, (160, 10), &theme.styles());
    assert_text_grid_match("session-tabs-horizontal", 160, &AnsiFrameRender::from_frame(&frame));
}

#[test]
fn session_tabs_vertical_matches_golden_at_80() {
    let theme = ThemeState::v2_dark();
    let frame = paint_rail(&golden_tab_views(), Some("second"), true, (80, 16), &theme.styles());
    assert_text_grid_match("session-tabs-vertical", 80, &AnsiFrameRender::from_frame(&frame));
}

#[test]
fn session_tabs_vertical_matches_golden_at_160() {
    let theme = ThemeState::v2_dark();
    let frame = paint_rail(&golden_tab_views(), Some("second"), true, (160, 16), &theme.styles());
    assert_text_grid_match("session-tabs-vertical", 160, &AnsiFrameRender::from_frame(&frame));
}

#[test]
fn tool_wrap_rows_matches_text_grid_and_ansi_at_80() {
    let scenario = scenario("tool-wrap-rows");
    let variant = scenario.variants.iter().find(|v| v.width == 80).expect("w80 variant");
    let theme = ThemeState::v2_dark();
    let frame = render_tool_wrap(&theme.styles(), 80, variant.height as u16);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-rows", 80, &render);
    // ANSI at the manifest's comparison depth (P1 re-pin: v2-dark).
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi =
        std::fs::read_to_string(goldens().join("tool-wrap-rows.w80.ansi")).expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-rows@80 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

#[test]
fn tool_wrap_rows_matches_text_grid_and_ansi_at_160() {
    let scenario = scenario("tool-wrap-rows");
    let variant = scenario.variants.iter().find(|v| v.width == 160).expect("w160 variant");
    let theme = ThemeState::v2_dark();
    let frame = render_tool_wrap(&theme.styles(), 160, variant.height as u16);
    let render = AnsiFrameRender::from_frame(&frame);
    assert_text_grid_match("tool-wrap-rows", 160, &render);
    let depth = manifest().comparison_depth_for(&scenario);
    let golden_ansi =
        std::fs::read_to_string(goldens().join("tool-wrap-rows.w160.ansi")).expect("golden ansi");
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &render.ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "tool-wrap-rows@160 ansi differs at manifest depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

/// Fidelity-classified comparison through the full parity pipeline
/// (`compare_ansi_frames_at` at the manifest depth) for the
/// session-tabs scenarios — text grids are exact today; style fidelity
/// (glow gradients) is the tracked 7.2 gap, asserted here as glyph-only
/// so the instrument is wired and green.
#[test]
fn session_tabs_ansi_glyph_fidelity_at_manifest_depth() {
    for id in ["session-tabs-horizontal", "session-tabs-vertical"] {
        let scenario = scenario(id);
        let depth = manifest().comparison_depth_for(&scenario);
        for variant in &scenario.variants {
            let golden = pf::load_ansi_frame(&goldens(), id, variant.width).expect("ansi golden");
            let theme = ThemeState::v2_dark();
            let frame = if id == "session-tabs-horizontal" {
                paint_bar(
                    &golden_tab_views(),
                    Some("second"),
                    true,
                    (variant.width as u16, variant.height as u16),
                    &theme.styles(),
                )
            } else {
                paint_rail(
                    &golden_tab_views(),
                    Some("second"),
                    true,
                    (variant.width as u16, variant.height as u16),
                    &theme.styles(),
                )
            };
            let render = pf::parse_ansi(
                &crate::render::serialize_canonical(&frame),
                Some((variant.height, variant.width)),
            )
            .expect("render parses");
            let fidelity = pf::compare_ansi_frames_at(&golden, &render, depth);
            // Text-grid parity is this wave's gate: glyphs must match
            // everywhere; the remaining divergences may only be style
            // (per-tab glow gradients) — the tracked 7.2 ANSI gap.
            assert_eq!(
                fidelity.glyph_match_pct(),
                100.0,
                "{id}@{} glyphs must match exactly",
                variant.width
            );
            if let Some((row, col, class)) = fidelity.first_divergence() {
                assert_eq!(
                    class,
                    pf::DivergenceClass::Style,
                    "{id}@{} first divergence at ({row},{col}) must be style-only",
                    variant.width
                );
            }
        }
    }
}

/// The strip reserves room for the " + " affordance exactly like the
/// reference (`ADD_TAB_WIDTH`).
#[test]
fn bar_plus_affordance_alignment() {
    let theme = ThemeState::v2_dark();
    let frame = paint_bar(&golden_tab_views(), Some("second"), true, (80, 1), &theme.styles());
    let row = &frame.text_grid()[0];
    // Tabs occupy [0, 64); the plus glyph sits at 65 inside " + ".
    assert_eq!(row.chars().nth(65), Some('+'), "row: {row:?}");
    assert_eq!(ADD_TAB_WIDTH, 3);
}
