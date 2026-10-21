// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! ASCII diagram rendering for fenced `mermaid` blocks — the port of
//! opencode v2 `packages/merman` (upstream v2.0.24).
//!
//! Upstream registers Merman unconditionally as the `mermaid`
//! code-block renderer (`packages/tui/src/plugin/builtins.ts`); the
//! supported subset in this lane is flowchart + sequence + state
//! (gitgraph/gantt/timeline stay on the default code-block render —
//! see the lane report).

pub mod core;
pub mod diagnostics;
pub mod flowchart;
pub mod gantt;
pub mod gitgraph;
pub mod palette;
pub mod sequence;
pub mod state;

#[cfg(test)]
mod tests;

use crate::render::mermaid::core::canvas::{CanvasSizeError, DiagramCanvas, TextOptions};
use crate::render::mermaid::core::color::Rgba;
use crate::render::mermaid::diagnostics::MermaidDiagramKind;
use crate::render::mermaid::flowchart::style::{DiagramSpan, FlowchartColorInput, FlowchartStyle};
use crate::render::mermaid::palette::OpenCodeDiagramPalette;

/// `detectMermaidDiagram` — the supported subset.
#[must_use]
pub fn detect_mermaid_diagram(content: &str) -> Option<MermaidDiagramKind> {
    if flowchart::is_mermaid_flowchart_diagram(content) {
        return Some(MermaidDiagramKind::Flowchart);
    }
    if sequence::is_mermaid_sequence_diagram(content) {
        return Some(MermaidDiagramKind::Sequence);
    }
    if state::is_mermaid_state_diagram(content) {
        return Some(MermaidDiagramKind::State);
    }
    if gitgraph::is_mermaid_git_graph_diagram(content) {
        return Some(MermaidDiagramKind::GitGraph);
    }
    if gantt::is_mermaid_gantt_diagram(content) {
        return Some(MermaidDiagramKind::Gantt);
    }
    // timeline stays on the default code-block render.
    None
}

/// `MermaidMarkdownRendererOptions` — the knobs the markdown seam
/// passes (upstream defaults: compact=true, layoutMaxWidth=120).
#[derive(Debug, Clone, Copy)]
pub struct MermaidRenderOptions {
    pub compact: bool,
    pub layout_max_width: Option<i64>,
    /// The available render width (min'd with the configured max,
    /// like upstream's `Math.min(configured, ctx.width)`).
    pub context_width: Option<i64>,
}

impl Default for MermaidRenderOptions {
    fn default() -> Self {
        Self { compact: true, layout_max_width: Some(120), context_width: None }
    }
}

/// The rendered diagram: trimmed rows of styled spans (one row per
/// terminal line).
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedDiagram {
    pub kind: MermaidDiagramKind,
    pub rows: Vec<Vec<DiagramSpan>>,
}

/// `createMermaidCodeBlockRenderer`'s happy path: detect → prepare →
/// styled rows. `None` = unsupported/unknown diagram (the caller falls
/// back to the default code render, upstream's `return undefined`).
#[must_use]
pub fn render_mermaid_block(
    content: &str,
    palette: &OpenCodeDiagramPalette,
    options: &MermaidRenderOptions,
) -> Option<RenderedDiagram> {
    let kind = detect_mermaid_diagram(content)?;
    let configured_max = options.layout_max_width.map(|width| 1.max(width)).unwrap_or(120);
    let layout_max_width = match options.context_width {
        Some(context_width) => configured_max.min(1.max(context_width)),
        None => configured_max,
    };
    prepare_diagram(kind, content, palette, options, layout_max_width).ok()
}

fn prepare_diagram(
    kind: MermaidDiagramKind,
    source: &str,
    palette: &OpenCodeDiagramPalette,
    options: &MermaidRenderOptions,
    layout_max_width: i64,
) -> Result<RenderedDiagram, MermaidError> {
    match kind {
        MermaidDiagramKind::Flowchart => {
            let diagram = flowchart::parse_mermaid_flowchart_diagram(source)?;
            let grid = flowchart::draw_flowchart_diagram_grid(
                &diagram,
                &flowchart::types::FlowchartRenderOptions {
                    compact: Some(options.compact),
                    layout_max_width: Some(layout_max_width),
                    ..Default::default()
                },
            )?;
            let rows = styled_rows(&grid, &flowchart_colors(palette));
            Ok(RenderedDiagram { kind, rows })
        }
        MermaidDiagramKind::Sequence => {
            let diagram = sequence::parse_mermaid_sequence_diagram(source)?;
            let grid = sequence::draw_sequence_diagram_grid(
                &diagram,
                &sequence::types::SequenceRenderOptions {
                    compact: options.compact,
                    ..Default::default()
                },
            )?;
            let rows = styled_rows(&grid, &sequence_colors(palette));
            Ok(RenderedDiagram { kind, rows })
        }
        MermaidDiagramKind::State => {
            let diagram = state::parse_mermaid_state_diagram(source)?;
            let grid = state::draw_state_diagram_grid(
                &diagram,
                &state::StateDiagramRenderOptions {
                    layout_max_width: Some(layout_max_width),
                    ..Default::default()
                },
            )
            .map_err(|_| MermaidError::Unsupported)?;
            let colors = state_colors(palette);
            let rows =
                styled_rows_state(&grid, &|style| colors.color(style), palette.label_background);
            Ok(RenderedDiagram { kind, rows })
        }
        MermaidDiagramKind::GitGraph => {
            let diagram = gitgraph::parse_mermaid_git_graph_diagram(source)?;
            let grid = gitgraph::draw_git_graph_diagram_grid(
                &diagram,
                &gitgraph::GitGraphRenderOptions::default(),
            );
            let colors = gitgraph::resolve_git_graph_style_colors(gitgraph::GitGraphColorInput {
                primary: Some(palette.primary),
                secondary: Some(palette.secondary),
                muted: Some(palette.muted),
                warning: Some(palette.warning),
                text: Some(palette.text),
            });
            let rows = styled_rows_trim_bottom(&grid, &|style| colors.color(style));
            Ok(RenderedDiagram { kind, rows })
        }
        MermaidDiagramKind::Gantt => {
            let diagram = gantt::parse_mermaid_gantt_diagram(source)?;
            let grid = gantt::draw_gantt_diagram_grid(
                &diagram,
                &gantt::GanttRenderOptions {
                    layout_max_width: Some(layout_max_width),
                    ..Default::default()
                },
            );
            let colors = gantt::resolve_gantt_style_colors(gantt::GanttColorInput {
                title: Some(palette.text),
                axis: Some(palette.muted),
                background: Some(palette.background),
                section: Some(palette.secondary),
                task: Some(palette.primary),
                active: Some(palette.primary),
                critical: Some(palette.warning),
                done: Some(palette.muted),
                milestone: None,
            });
            let rows = styled_rows_trim_bottom(&grid, &|style| colors.color(style));
            Ok(RenderedDiagram { kind, rows })
        }
        MermaidDiagramKind::Timeline => Err(MermaidError::Unsupported),
    }
}

/// `renderGitGraphGridStyledText` — `toString({ trimBottom: true })`
/// row split (git graphs trim only the bottom).
fn styled_rows_trim_bottom<S: Copy + PartialEq>(
    grid: &DiagramCanvas<S>,
    colors: &dyn Fn(Option<S>) -> Option<Rgba>,
) -> Vec<Vec<DiagramSpan>> {
    let mut rows: Vec<Vec<DiagramSpan>> = Vec::new();
    let mut current: Vec<DiagramSpan> = Vec::new();
    grid.for_each_run(TextOptions { trim_top: false, trim_bottom: true }, &mut |run| match run {
        Some(run) => current.push(DiagramSpan {
            text: run.text,
            fg: colors(run.style),
            bg: None,
            attributes: run.attributes.unwrap_or(0),
        }),
        None => rows.push(std::mem::take(&mut current)),
    });
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

/// The renderer's failure modes (upstream catches
/// `MermaidSyntaxError` + `DiagramCanvasSizeError` and returns `undefined`).
enum MermaidError {
    Unsupported,
}

impl From<MermaidSyntaxError> for MermaidError {
    fn from(_: MermaidSyntaxError) -> Self {
        Self::Unsupported
    }
}

impl From<CanvasSizeError> for MermaidError {
    fn from(_: CanvasSizeError) -> Self {
        Self::Unsupported
    }
}

use crate::render::mermaid::diagnostics::MermaidSyntaxError;

/// `renderDiagramGridStyledTextByStyle` — run-split, trim, and color.
fn styled_rows<S: Copy + PartialEq>(
    grid: &DiagramCanvas<S>,
    colors: &dyn Fn(Option<S>) -> Option<Rgba>,
) -> Vec<Vec<DiagramSpan>> {
    let mut rows: Vec<Vec<DiagramSpan>> = Vec::new();
    let mut current: Vec<DiagramSpan> = Vec::new();
    grid.for_each_run(TextOptions { trim_top: true, trim_bottom: true }, &mut |run| match run {
        Some(run) => current.push(DiagramSpan {
            text: run.text,
            fg: colors(run.style),
            bg: None,
            attributes: run.attributes.unwrap_or(0),
        }),
        None => rows.push(std::mem::take(&mut current)),
    });
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}

fn flowchart_colors(
    palette: &OpenCodeDiagramPalette,
) -> impl Fn(Option<FlowchartStyle>) -> Option<Rgba> {
    let colors =
        crate::render::mermaid::flowchart::style::FlowchartColors::resolve(FlowchartColorInput {
            node: Some(palette.box_text),
            node_border: Some(palette.box_border),
            database: Some(palette.box_text),
            database_border: Some(palette.box_border),
            edge: Some(palette.line),
            label: Some(palette.text),
            group: Some(palette.group),
            group_label: Some(palette.group_text),
        });
    move |style| colors.color(style)
}

fn sequence_colors(
    palette: &OpenCodeDiagramPalette,
) -> impl Fn(Option<sequence::types::SequenceStyle>) -> Option<Rgba> {
    let colors = sequence::style::SequenceColors::resolve(sequence::style::SequenceColorInput {
        participant: Some(palette.primary),
        lifeline: Some(palette.muted),
        group: Some(palette.secondary),
        request: Some(palette.request),
        response: Some(palette.response),
        fragment: Some(palette.secondary),
        fragment_label_bg: Some(palette.background),
        note: Some(palette.note),
        note_bg: Some(palette.note_background),
    });
    move |style| colors.color(style)
}

/// The markdown seam's state color resolution (upstream
/// `resolveStateStyleColors` inputs) plus the `label` background.
fn state_colors(palette: &OpenCodeDiagramPalette) -> state::style::StateStyleColors {
    state::style::resolve_state_style_colors(state::style::StateColorInput {
        state: Some(palette.box_text),
        state_border: Some(palette.box_border),
        composite: Some(palette.group),
        composite_label: Some(palette.group_text),
        transition: Some(palette.line),
        label: Some(palette.text),
        note_border: Some(palette.note_border),
        note_text: Some(palette.note_text),
        note_connector: Some(palette.note_connector),
        start: Some(palette.marker),
        end: Some(palette.marker),
        choice: Some(palette.secondary),
    })
}

/// `renderStateGridStyledText` — like `styled_rows`, but the state grid
/// also paints the label-cell background (`{ label: labelBackground }`).
fn styled_rows_state(
    grid: &DiagramCanvas<state::types::StateCellStyle>,
    colors: &dyn Fn(Option<state::types::StateCellStyle>) -> Option<Rgba>,
    label_background: Rgba,
) -> Vec<Vec<DiagramSpan>> {
    let mut rows: Vec<Vec<DiagramSpan>> = Vec::new();
    let mut current: Vec<DiagramSpan> = Vec::new();
    grid.for_each_run(TextOptions { trim_top: true, trim_bottom: true }, &mut |run| match run {
        Some(run) => current.push(DiagramSpan {
            text: run.text,
            fg: colors(run.style),
            bg: match run.style {
                Some(state::types::StateCellStyle::Label) => Some(label_background),
                _ => None,
            },
            attributes: run.attributes.unwrap_or(0),
        }),
        None => rows.push(std::mem::take(&mut current)),
    });
    if !current.is_empty() || rows.is_empty() {
        rows.push(current);
    }
    rows
}
