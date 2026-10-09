//! Renderer-substrate spike (wave 7.0) — **absorbed**. The bespoke
//! taffy/wrap/paint pipeline now lives in the production modules
//! ([`crate::layout`], [`crate::wrap`], [`crate::render`],
//! [`crate::components::tool_rows`]); this module keeps the spike's
//! public surface as aliases and its golden comparison as the
//! absorption proof, upgraded to the P1 corpus:
//!
//! * painted under the **v2-dark corpus theme** (frame fill =
//!   renderer-default foreground over `background.base`; icon+label =
//!   `text.muted`) — matching the re-pinned `tui-frame-golden/2`
//!   streams byte-for-byte;
//! * ANSI comparisons run at the manifest's per-scenario comparison
//!   depth (`parity::tui_frame::compare_ansi_bytes_at`).

pub use crate::components::tool_rows::{
    layout_tool_wrap, tool_wrap_fixture, ToolRowFixture, BOX_WIDTH, ICON_WIDTH,
};
pub use crate::render::{
    serialize_canonical, Cell as SpikeCell, CellColor as SpikeColor, Frame as SpikeFrame,
};
pub use crate::wrap::{display_width, wrap_label};

use crate::components::tool_rows::{fill_style, render_tool_wrap as render_themed};
use crate::render::Frame;
use crate::theme::ThemeState;

/// Render the `tool-wrap-rows` fixture at a terminal size (spike API:
/// v2-dark corpus theme).
#[must_use]
pub fn render_tool_wrap(width: u16, height: u16) -> Frame {
    let theme = ThemeState::v2_dark();
    render_themed(&theme.styles(), width, height)
}

/// Render the `tool-wrap-rows` fixture and serialize the canonical ANSI
/// stream (spike API).
#[must_use]
pub fn render_tool_wrap_ansi(width: u16, height: u16) -> String {
    serialize_canonical(&render_tool_wrap(width, height))
}

/// The spike's frame fill style, now derived from the corpus theme.
#[must_use]
pub fn base_style() -> ratatui::style::Style {
    fill_style(&ThemeState::v2_dark().styles())
}

#[cfg(test)]
mod golden_tests {
    use std::path::Path;

    use parity::tui_frame as pf;

    use super::*;

    fn goldens() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
    }

    fn manifest() -> pf::Manifest {
        pf::load_manifest(&goldens()).expect("golden manifest")
    }

    fn measure(width: usize, height: u16) -> (pf::TextGridDiff, pf::AnsiByteDiff) {
        let dir = goldens();
        let golden = pf::load_text_grid(&dir, "tool-wrap-rows", width).expect("golden text grid");
        let frame =
            render_tool_wrap(u16::try_from(width).expect("terminal width fits u16"), height);
        let spike_grid =
            pf::TextGrid { rows: frame.text_grid(), height: frame.height, width: frame.width };
        let text_diff = pf::compare_text_grids(&golden, &spike_grid);
        let golden_ansi =
            std::fs::read_to_string(dir.join(format!("tool-wrap-rows.w{width}.ansi")))
                .expect("golden ansi");
        // P1: the corpus is re-pinned under v2-dark; compare at the
        // manifest's per-scenario depth.
        let scenario = manifest()
            .scenarios
            .into_iter()
            .find(|scenario| scenario.id == "tool-wrap-rows")
            .expect("tool-wrap-rows scenario");
        let depth = manifest().comparison_depth_for(&scenario);
        let ansi_diff =
            pf::compare_ansi_bytes_at(&golden_ansi, &serialize_canonical(&frame), depth)
                .expect("ansi parses");
        (text_diff, ansi_diff)
    }

    #[test]
    fn spike_reproduces_tool_wrap_rows_at_80_columns() {
        let (text_diff, ansi_diff) = measure(80, 12);
        println!(
            "text: {:.2}% matching (first divergence {:?}), ansi: {} (first offset {:?})",
            text_diff.match_pct(),
            text_diff.first_divergence,
            if ansi_diff.equal { "byte-equal" } else { "differs" },
            ansi_diff.first_divergent_offset
        );
        assert_eq!(text_diff.first_divergence, None, "text grid must match exactly");
        assert!(ansi_diff.equal, "ansi stream must be byte-equal at the manifest depth");
    }

    #[test]
    fn spike_reproduces_tool_wrap_rows_at_160_columns() {
        let (text_diff, ansi_diff) = measure(160, 12);
        println!(
            "text: {:.2}% matching (first divergence {:?}), ansi: {} (first offset {:?})",
            text_diff.match_pct(),
            text_diff.first_divergence,
            if ansi_diff.equal { "byte-equal" } else { "differs" },
            ansi_diff.first_divergent_offset
        );
        assert_eq!(text_diff.first_divergence, None, "text grid must match exactly");
        assert!(ansi_diff.equal, "ansi stream must be byte-equal at the manifest depth");
    }

    /// The spike's serializer must produce bytes the parity parser
    /// understands (format agreement between the two implementations).
    #[test]
    fn spike_serializer_agrees_with_the_parity_canonical_format() {
        let frame = render_tool_wrap(80, 12);
        let serialized = serialize_canonical(&frame);
        let parsed =
            pf::parse_ansi(&serialized, Some((12, 80))).expect("parity parses spike bytes");
        assert_eq!(pf::render_ansi(&parsed), serialized, "roundtrip must be byte-stable");
        for (row, line) in frame.text_grid().iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                assert_eq!(parsed.cell(row, col).ch, ch);
            }
        }
    }

    /// The v2-dark corpus theme is what the goldens pin: the fill pairs
    /// the renderer-default foreground with the theme background, and
    /// the rows paint `text.muted`.
    #[test]
    fn corpus_theme_mapping_matches_the_golden_fill() {
        let theme = ThemeState::v2_dark();
        let styles = theme.styles();
        assert_eq!(
            styles.background,
            ratatui::style::Color::Rgb(
                theme.resolved.tokens.background.base.r,
                theme.resolved.tokens.background.base.g,
                theme.resolved.tokens.background.base.b
            )
        );
        let fill = base_style();
        assert_eq!(fill.fg, Some(crate::components::tool_rows::DEFAULT_FOREGROUND));
        assert_eq!(fill.bg, Some(styles.background));
    }
}
