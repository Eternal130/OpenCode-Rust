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

//! Ported upstream `timeline/diagram.test.ts`.

use std::collections::HashSet;

use super::{
    draw_timeline_diagram_grid, parse_mermaid_timeline_diagram, render_timeline_diagram,
    render_timeline_grid_text, resolve_timeline_style_colors, TimelineCellStyle,
    TimelineColorInput, TimelineDirection, TimelinePeriod, TimelineRenderOptions,
};

fn parse(content: &str) -> super::TimelineDiagram {
    parse_mermaid_timeline_diagram(content).expect("parses")
}

fn render(content: &str) -> String {
    render_timeline_diagram(content).expect("renders")
}

#[test]
fn detects_and_parses_titles_sections_periods_events_and_continuations() {
    let diagram = parse(
        "\n%% product history\ntimeline LR\n  title Product &amp;<br/>Platform\n\n  section Foundation\n  2024 : Prototype : First release\n       : Public beta\n  section Growth\n  2025 : \"Scale: &#x2265; 10k\"\n",
    );

    assert_eq!(diagram.direction, TimelineDirection::Lr);
    assert_eq!(diagram.title.as_deref(), Some("Product &<br/>Platform"));
    assert_eq!(diagram.sections, vec!["Foundation", "Growth"]);
    assert_eq!(
        diagram.periods,
        vec![
            TimelinePeriod {
                period: "2024".to_string(),
                events: vec![
                    "Prototype".to_string(),
                    "First release".to_string(),
                    "Public beta".to_string()
                ],
            },
            TimelinePeriod {
                period: "2025".to_string(), events: vec!["Scale: ≥ 10k".to_string()]
            },
        ]
    );
    assert_eq!(diagram.entries.len(), 4);
    assert!(matches!(diagram.entries[0], super::TimelineEntry::Section { .. }));
    assert!(matches!(diagram.entries[1], super::TimelineEntry::Period(_)));
}

#[test]
fn renders_a_vertical_spine_with_title_sections_periods_and_events() {
    let output = render(
        "timeline\n  title Product &amp;<br/>Platform\n  section Foundation<br/>phase\n  2024 : Prototype<br/>ready : First release\n       : Scale &#x2265; 10k",
    );
    assert_eq!(
        output,
        [
            "           Product &",
            "           Platform",
            "",
            "Foundation ───┐",
            "     phase    │",
            "              │",
            "      2024 ───●  Prototype",
            "              │  ready",
            "              │  First release",
            "              │  Scale ≥ 10k",
            "              │",
        ]
        .join("\n")
    );
}

#[test]
fn uses_the_vertical_terminal_layout_for_every_header() {
    for header in ["timeline", "timeline TD", "timeline LR"] {
        let output = render(&format!("{header}\n  2024 : One\n  2025 : Two"));
        let lines: Vec<&str> = output.split('\n').collect();
        let first = lines.iter().position(|line| line.contains("2024")).unwrap();
        let second = lines.iter().position(|line| line.contains("2025")).unwrap();
        assert!(first < second, "{header}");
        assert!(output.contains('│'), "{header}");
        assert!(output.contains('●'), "{header}");
    }
}

#[test]
fn preserves_mermaid_direction_semantics() {
    assert_eq!(parse("timeline\n  2024 : One").direction, TimelineDirection::Lr);
    assert_eq!(parse("timeline TD\n  2024 : One").direction, TimelineDirection::Td);
}

#[test]
fn keeps_ordinary_colons_in_event_text() {
    let diagram = parse("timeline\n  2024 : https://example.com : event:detail : next event");
    assert_eq!(
        diagram.periods[0].events,
        vec!["https://example.com", "event:detail", "next event"]
    );
}

#[test]
fn does_not_treat_apostrophes_in_event_prose_as_quotes() {
    let diagram = parse("timeline\n  2024 : Kit's launch : Public beta");
    assert_eq!(diagram.periods[0].events, vec!["Kit's launch", "Public beta"]);
}

#[test]
fn supports_standalone_periods_followed_by_continuation_events() {
    let diagram = parse("timeline\n  2024\n  : First release\n  : Public beta");
    assert_eq!(
        diagram.periods,
        vec![TimelinePeriod {
            period: "2024".to_string(),
            events: vec!["First release".to_string(), "Public beta".to_string()],
        }]
    );
}

#[test]
fn ignores_timeline_comments_and_accessibility_directives() {
    let diagram = parse(
        "timeline\n  # product history\n  accTitle: Product timeline\n  accDescr Product release history\n  2024 : Prototype %% internal note",
    );
    assert_eq!(
        diagram.periods,
        vec![TimelinePeriod { period: "2024".to_string(), events: vec!["Prototype".to_string()] }]
    );
}

#[test]
fn ignores_multiline_accessibility_descriptions() {
    let diagram = parse(
        "timeline\n  accDescr {\n    Product milestones by year.\n    Includes launch and growth.\n  }\n  2024 : Ship",
    );
    assert_eq!(
        diagram.periods,
        vec![TimelinePeriod { period: "2024".to_string(), events: vec!["Ship".to_string()] }]
    );
}

#[test]
fn rejects_a_continuation_without_a_period_with_source_diagnostics() {
    let error = parse_mermaid_timeline_diagram("timeline\n  : orphan event").unwrap_err();
    assert_eq!(error.line_number, 2);
    assert!(
        error
            .message
            .contains("Timeline continuation requires a preceding period in timeline diagram"),
        "{}",
        error.message
    );
}

#[test]
fn rejects_unsupported_and_empty_syntax() {
    let error = parse_mermaid_timeline_diagram("timeline\n  section").unwrap_err();
    assert!(error.message.contains("Timeline section cannot be empty"));
    let error = parse_mermaid_timeline_diagram("timeline\n  2024 :").unwrap_err();
    assert!(error.message.contains("Timeline event cannot be empty"));
    let error = parse_mermaid_timeline_diagram("timeline\n  : unsupported").unwrap_err();
    assert!(error.message.contains("requires a preceding period"));
}

#[test]
fn draws_semantic_styles_for_every_timeline_role() {
    let diagram = parse("timeline\n title Roadmap\n section Now\n 2026 : Ship");
    let grid = draw_timeline_diagram_grid(&diagram, &TimelineRenderOptions::default());
    let mut styles: HashSet<String> = HashSet::new();
    for y in 0..grid.height() as i64 {
        for x in 0..grid.width() as i64 {
            if let Some(style) = grid.get_cell(x, y).and_then(|cell| cell.style) {
                styles.insert(format!("{style:?}"));
            }
        }
    }
    let expected: HashSet<String> = [
        "Title",
        "Section",
        "SectionFade1",
        "SectionFade2",
        "SectionFade3",
        "Spine",
        "Period",
        "PeriodFade1",
        "PeriodFade2",
        "PeriodFade3",
        "Event",
    ]
    .iter()
    .map(|style| (*style).to_string())
    .collect();
    assert_eq!(styles, expected);

    let colors = resolve_timeline_style_colors(TimelineColorInput::default());
    let resolved = colors.color(Some(TimelineCellStyle::Event));
    assert!(resolved.is_some());
    assert_eq!(
        render_timeline_grid_text(&grid),
        render("timeline\n title Roadmap\n section Now\n 2026 : Ship")
    );
}

#[test]
fn uses_section_starts_and_joins_with_ordered_color_ramps() {
    let diagram =
        parse("timeline\n section Morning\n 09:00 : Start\n section Midday\n 12:00 : Continue");
    let grid = draw_timeline_diagram_grid(&diagram, &TimelineRenderOptions::default());
    let text = render_timeline_grid_text(&grid);

    assert!(text.contains("Morning ───┐"));
    assert!(text.contains("Midday ───┤"));
    let mut row_styles: Vec<String> = Vec::new();
    for x in 0..grid.width() as i64 {
        if let Some(style) = grid.get_cell(x, 0).and_then(|cell| cell.style) {
            row_styles.push(format!("{style:?}"));
        }
    }
    assert_eq!(
        row_styles,
        vec![
            "Section",
            "Section",
            "Section",
            "Section",
            "Section",
            "Section",
            "Section",
            "SectionFade1",
            "SectionFade2",
            "SectionFade3",
            "Spine",
        ]
    );
}
