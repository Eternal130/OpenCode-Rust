//! Chrome-family golden acceptance (the oracle gate's F1 followup):
//! text-grid **and** canonical-ANSI byte pins for the three R1
//! surfaces whose cell-level parity suites died with their retired
//! painters — the console overlay, the plugin route page (+ its
//! missing-route fallback) and the devtools bar. These are IN-HOUSE
//! regression pins (post-retirement byte coverage — the solid trees
//! are the only renderers; the fixtures pin the rendered bytes, not
//! a bun-side reference dump), through `tools/parity::tui_frame` at
//! the manifest's comparison depth like the footer corpus.
//!
//! Scenario → geometry pin:
//!
//! * `chrome-console-overlay` (6 widths ×12 rows) — the 40-line ring
//!   clamps at the 12-row budget and the 201-char line pins the
//!   per-width clip (`at + 1 >= width`).
//! * `chrome-plugin-page` (4 widths ×10 rows) — a title long enough
//!   to overrun the right-aligned `ctrl+c exit` hint, and body rows
//!   long enough to hit the `width − 1` bound.
//! * `chrome-plugin-route-missing` (2 widths) — the centered warning
//!   + the reverse-video `go home` button band.
//! * `chrome-devtools-bar-server` / `-theme` (2 widths ×1 row) — the
//!   full-row muted-on-raised chunk, two panels.

#![cfg(test)]
#![allow(clippy::cast_possible_truncation)]

use crate::components::console_solid::render_console_overlay_solid;
use crate::components::plugin_page_solid::{
    render_plugin_page_solid, render_plugin_route_missing_solid,
};
use crate::reactive::Runtime;
use crate::render::serialize_canonical;
use crate::render::Frame;
use crate::store::Store;
use crate::theme::ThemeState;
use parity::tui_frame as pf;

fn goldens() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
}

fn manifest() -> pf::Manifest {
    pf::load_manifest(&goldens()).expect("golden manifest")
}

fn variant(id: &str, width: usize) -> (usize, usize, pf::ComparisonDepth) {
    let scenario = manifest()
        .scenarios
        .into_iter()
        .find(|scenario| scenario.id == id)
        .unwrap_or_else(|| panic!("scenario {id}"));
    let depth = manifest().comparison_depth_for(&scenario);
    let variant = scenario
        .variants
        .iter()
        .find(|variant| variant.width == width)
        .unwrap_or_else(|| panic!("{id} variant @{width}"));
    (variant.rows, variant.cols, depth)
}

/// `PARITY_UPDATE=1` (re)writes the goldens instead of comparing —
/// the sanctioned refresh path (same contract as
/// `parity::golden::update_mode`).
fn update_mode() -> bool {
    std::env::var("PARITY_UPDATE").is_ok_and(|value| value == "1")
}

fn assert_golden(id: &str, width: usize, frame: &Frame) {
    let (rows, cols, depth) = variant(id, width);
    assert_eq!((frame.height, frame.width), (rows, cols), "{id}@{width} dims");
    if update_mode() {
        std::fs::write(
            goldens().join(format!("{id}.w{width}.txt")),
            frame.text_grid().join("\n") + "\n",
        )
        .expect("write golden txt");
        std::fs::write(goldens().join(format!("{id}.w{width}.ansi")), serialize_canonical(frame))
            .expect("write golden ansi");
        return;
    }
    let golden = pf::load_text_grid(&goldens(), id, width).expect("text grid");
    let rendered =
        pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
    let diff = pf::compare_text_grids(&golden, &rendered);
    assert!(
        diff.first_divergence.is_none(),
        "{id}@{width} text grid diverged: {:?} ({:.2}%)",
        diff.first_divergence,
        diff.match_pct()
    );
    let golden_ansi =
        std::fs::read_to_string(goldens().join(format!("{id}.w{width}.ansi"))).expect("ansi");
    let ansi = serialize_canonical(frame);
    let diff = pf::compare_ansi_bytes_at(&golden_ansi, &ansi, depth).expect("ansi parses");
    assert!(
        diff.equal,
        "{id}@{width} ansi differs at depth {depth}: first divergence {:?}",
        diff.first_divergent_offset
    );
}

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

/// The console ring: 40 diagnostics (past the 12-row clamp), the
/// NEWEST line 201 chars wide (past every clip bound in the matrix —
/// the tail always carries it).
fn console_lines() -> Vec<String> {
    let mut lines: Vec<String> =
        (0..40).map(|index| format!("line {index}: renderer diagnostic message")).collect();
    lines[39] = format!("clip{}", "y".repeat(200));
    lines
}

fn styles() -> crate::theme::ThemeStyles {
    ThemeState::v2_dark().styles()
}

// ---------------------------------------------------------------------------
// the gates
// ---------------------------------------------------------------------------

/// The console overlay: the 12-row clamp, the bordered box + title,
/// the tail order (newest at the bottom) and the per-width clip over
/// six widths.
#[test]
fn chrome_console_overlay_matches_the_golden() {
    let lines = console_lines();
    for width in [20_u16, 40, 60, 80, 120, 200] {
        let frame = render_console_overlay_solid(&lines, &styles(), width);
        assert_golden("chrome-console-overlay", usize::from(width), &frame);
    }
}

/// The plugin route page: the title row (a title long enough to
/// overrun the right-aligned hint), the body rows and their `width −
/// 1` clip, across four widths including the hint-suppression bound
/// (`hint_x > 4`).
#[test]
fn chrome_plugin_page_matches_the_golden() {
    let title = "a-very-long-plugin-route-title-that-overflows-the-header-row";
    let rows: Vec<String> = vec![
        "Usage statistics".to_owned(),
        format!("sessions: {}", "x".repeat(120)),
        "plugins: 3".to_owned(),
        "a fourth row the 10-row page must clip".to_owned(),
    ];
    for width in [26_u16, 40, 80, 120] {
        let frame = render_plugin_page_solid(title, &rows, &styles(), (width, 10));
        assert_golden("chrome-plugin-page", usize::from(width), &frame);
    }
}

/// The missing-route fallback: the centered warning line + the
/// reverse-video `go home` button band.
#[test]
fn chrome_plugin_route_missing_matches_the_golden() {
    for width in [40_u16, 80] {
        let frame = render_plugin_route_missing_solid("omo", "stats", &styles(), (width, 24));
        assert_golden("chrome-plugin-route-missing", usize::from(width), &frame);
    }
}

/// The devtools bar, two panels: the full-width muted-on-raised row
/// chunk reading live store state (server: sse/models/providers
/// counts of a fresh store; theme: the v2-dark accent/bg hex pair).
#[test]
fn chrome_devtools_bar_matches_the_golden() {
    for (id, panel_steps) in
        [("chrome-devtools-bar-server", 0_usize), ("chrome-devtools-bar-theme", 1)]
    {
        let store = Store::new(Runtime::new());
        store.set_devtools_enabled(true);
        for _ in 0..panel_steps {
            store.devtools_cycle_panel(1);
        }
        for width in [80_u16, 120] {
            let frame = crate::components::devtools_solid::devtools_bar_row_solid(
                &store,
                &styles(),
                (width, 24),
            );
            assert_golden(id, usize::from(width), &frame);
        }
    }
}
