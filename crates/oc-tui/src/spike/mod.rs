//! Renderer-substrate spike (wave 7.0).
//!
//! Minimal **ratatui + taffy** pipeline rendering ONE exported golden
//! scenario — `tool-wrap-rows`, the simplest text-heavy component (a column
//! of `InlineToolRow`s; layout + glyph content, single paint style):
//!
//! 1. [`layout`] mirrors the opentui box tree of the reference fixture as a
//!    taffy flex tree with measured text leaves;
//! 2. [`paint`] paints the computed layout into a ratatui `Buffer`;
//! 3. [`paint::serialize_canonical`] serializes the buffer through the same
//!    canonical format the golden exporter uses (text grid + ANSI stream);
//! 4. tests compare against `tools/parity/goldens/tui-frames` via the
//!    `parity` crate (dev-dependency) and report fidelity.
//!
//! **Findings** (see `.slim/deepwork/rust-port/w7export-lane.md`): for this
//! component class — pure flex layout + word-wrapped narrow-width text +
//! flat cell styles — the spike reproduces the golden **byte-for-byte**:
//! 100% text-grid match and byte-equal ANSI streams at both 80 and 160
//! columns (see `golden_tests` below). What the substrate does not cover
//! (and Phase 7 must scope out or defer): opentui's full line-break
//! algorithm (the golden corpus only exercises break-after-space/-dot),
//! sub-cell glyph positioning, RGBA alpha blending, half-block/image rows
//! (kitty/sixel), spinner/time-driven frames, and textarea internals.

pub mod layout;
pub mod paint;
pub mod wrap;

pub use layout::{layout_tool_wrap, tool_wrap_fixture, LaidOutFixture, LaidOutRow, ToolRowFixture};
pub use paint::{paint, serialize_canonical, to_spike_frame, SpikeFrame};

use ratatui::buffer::Buffer;

/// Render the `tool-wrap-rows` fixture at a terminal size and return the
/// serialized canonical ANSI stream.
#[must_use]
pub fn render_tool_wrap_ansi(width: u16, height: u16) -> String {
    serialize_canonical(&render_tool_wrap(width, height))
}

/// Render the `tool-wrap-rows` fixture at a terminal size.
#[must_use]
pub fn render_tool_wrap(width: u16, height: u16) -> SpikeFrame {
    let fixture = layout_tool_wrap(&tool_wrap_fixture());
    let buffer: Buffer = paint(&fixture, width, height);
    to_spike_frame(&buffer)
}

#[cfg(test)]
mod golden_tests {
    use std::path::Path;

    use parity::tui_frame as pf;

    use super::*;

    fn goldens() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
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
        let ansi_diff = pf::compare_ansi_bytes(&golden_ansi, &serialize_canonical(&frame));
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
        assert!(ansi_diff.equal, "ansi stream must be byte-equal");
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
        assert!(ansi_diff.equal, "ansi stream must be byte-equal");
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
}
