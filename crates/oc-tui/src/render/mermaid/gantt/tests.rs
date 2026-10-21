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

//! Ported upstream `gantt/diagram.test.ts`.

use super::{
    draw_gantt_diagram_grid, is_mermaid_gantt_diagram, parse_mermaid_gantt_diagram,
    render_gantt_diagram, GanttCellStyle, GanttEndpoints, GanttLabelLayout, GanttLineStyle,
    GanttRenderOptions, GanttRenderStyle, GanttTaskState, GanttTrack, GanttTrackTone,
};

const SECONDS_DIAGRAM: &str = "gantt\n  dateFormat s\n  axisFormat %Ss\n  section OLD (blocking)\n  provider.create (Modal sandbox) :crit, 0, 15\n  model streams first token :15, 17\n  section NEW (eager kick)\n  reserve (DB insert) :0, 1\n  model streams first token :0, 2\n  provisioning in background :active, 0, 15\n  model calls bash → spawn runs :15, 16\n  section NEW (pure chat thread)\n  reserve (DB insert) :0, 1\n  model answers, never calls bash :0, 4";

fn render(content: &str, options: &GanttRenderOptions) -> String {
    render_gantt_diagram(content, options).expect("renders")
}

#[test]
fn detects_and_parses_second_based_task_ranges_and_states() {
    let diagram = parse_mermaid_gantt_diagram(SECONDS_DIAGRAM).expect("parses");
    assert_eq!(diagram.date_format, "s");
    assert_eq!(diagram.axis_format, "%Ss");
    assert_eq!(diagram.tasks.len(), 8);
    assert_eq!(diagram.tasks[0].label, "provider.create (Modal sandbox)");
    assert_eq!(diagram.tasks[0].start, 0);
    assert_eq!(diagram.tasks[0].end, 15_000);
    assert_eq!(diagram.tasks[0].state, GanttTaskState::Critical);
    assert_eq!(diagram.tasks[4].label, "provisioning in background");
    assert_eq!(diagram.tasks[4].start, 0);
    assert_eq!(diagram.tasks[4].end, 15_000);
    assert_eq!(diagram.tasks[4].state, GanttTaskState::Active);
}

#[test]
fn renders_sections_a_formatted_axis_and_aligned_task_bars() {
    let output = render(
        SECONDS_DIAGRAM,
        &GanttRenderOptions { layout_max_width: Some(100), ..Default::default() },
    );
    let mut offset = 0usize;
    for needle in [
        "00s",
        "15s",
        "OLD (blocking)",
        "provider.create (Modal sandbox)",
        "model streams first token",
        "NEW (eager kick)",
        "provisioning in background",
        "NEW (pure chat thread)",
        "model answers, never calls bash",
    ] {
        let found = output[offset..].find(needle).unwrap_or_else(|| {
            panic!("missing {needle:?} after {offset} in:\n{output}");
        });
        offset += found + needle.len();
    }
    let default = render(SECONDS_DIAGRAM, &GanttRenderOptions::default());
    assert!(default.contains("\n\nNEW (eager kick)"));
    assert!(!default.contains('·'));
}

#[test]
fn renders_alternate_terminal_bar_styles() {
    let options = |mut base: GanttRenderOptions| {
        base.layout_max_width = Some(100);
        base
    };
    let base = GanttRenderOptions::default();
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions { style: Some(GanttRenderStyle::Block), ..base })
    )
    .contains('█'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions { style: Some(GanttRenderStyle::Capsule), ..base })
    )
    .contains('╶'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions { style: Some(GanttRenderStyle::Points), ..base })
    )
    .contains('●'));
    let dots = render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            track: Some(GanttTrack::Dots),
            ..base
        }),
    );
    assert!(dots.contains('·'));
    let track_line = render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            track: Some(GanttTrack::Line),
            ..base
        }),
    );
    assert!(!track_line.contains('·'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            endpoints: Some(GanttEndpoints::Points),
            ..base
        })
    )
    .contains('●'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            line: Some(GanttLineStyle::Thin),
            ..base
        })
    )
    .contains('─'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            line: Some(GanttLineStyle::Double),
            ..base
        })
    )
    .contains('═'));
    assert!(render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            line: Some(GanttLineStyle::Dashed),
            ..base
        })
    )
    .contains('╌'));
    let tree = render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions { labels: Some(GanttLabelLayout::Tree), ..base }),
    );
    assert!(tree.contains("├─ provider.create"));
    assert!(tree.contains("└─ model streams first token"));
    let spaced = render(
        SECONDS_DIAGRAM,
        &options(GanttRenderOptions { sections_spaced: Some(true), ..base }),
    );
    assert!(spaced.contains("\n\nNEW (eager kick)"));

    let diagram = parse_mermaid_gantt_diagram(SECONDS_DIAGRAM).expect("parses");
    let grid = draw_gantt_diagram_grid(
        &diagram,
        &GanttRenderOptions {
            style: Some(GanttRenderStyle::Track),
            endpoints: Some(GanttEndpoints::Points),
            track_tone: Some(GanttTrackTone::Faint),
            ..base
        },
    );
    for y in 0..grid.height() as i64 {
        for x in 0..grid.width() as i64 {
            if let Some(cell) = grid.get_cell(x, y) {
                if cell.text() == "●" {
                    assert_eq!(cell.style, Some(GanttCellStyle::TrackFaint));
                }
            }
        }
    }
}

#[test]
fn resolves_task_ids_after_dependencies_durations_and_milestones() {
    let diagram = parse_mermaid_gantt_diagram(
        "gantt\n      dateFormat YYYY-MM-DD\n      task one :done, first, 2026-08-01, 2d\n      deploy :milestone, after first, 0d",
    )
    .expect("parses");
    assert_eq!(diagram.tasks[1].state, GanttTaskState::Milestone);
    assert_eq!(diagram.tasks[1].start, 1_785_715_200_000); // Date.UTC(2026, 7, 3)
    assert_eq!(diagram.tasks[1].end, 1_785_715_200_000);
    let output = render(
        "gantt\n      dateFormat YYYY-MM-DD\n      task one :first, 2026-08-01, 2d\n      deploy :milestone, after first, 0d",
        &GanttRenderOptions::default(),
    );
    assert!(output.contains('◆'));
}

#[test]
fn rejects_unsupported_or_ambiguous_syntax_with_source_diagnostics() {
    let error = parse_mermaid_gantt_diagram("gantt\n task :not-a-date, 2d").unwrap_err();
    assert_eq!(error.line_number, 2);
    assert!(
        error.message.contains("Unsupported date \"not-a-date\" for dateFormat YYYY-MM-DD"),
        "{}",
        error.message
    );
    let error = parse_mermaid_gantt_diagram("gantt\n task :after missing, 2d").unwrap_err();
    assert!(error.message.contains("Unknown Gantt task id \"missing\""));
    let error = parse_mermaid_gantt_diagram("gantt\n excludes weekends").unwrap_err();
    assert!(error.message.contains("excludes is not supported"));
}

#[test]
fn recognizes_only_gantt_headers() {
    assert!(is_mermaid_gantt_diagram("%% comment\ngantt\n task :0, 1"));
    assert!(!is_mermaid_gantt_diagram("timeline\n 2026 : ship"));
}

#[test]
fn renders_partial_diagrams_containing_only_sections() {
    assert_eq!(render("gantt\n section Planning", &GanttRenderOptions::default()), "Planning");
}
