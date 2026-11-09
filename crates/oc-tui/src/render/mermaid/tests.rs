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

//! Ported upstream core cases: `flowchart/flowchart.test.ts`,
//! `sequence/diagram.test.ts`, `core/*` (packages/merman v2.0.24).

use crate::render::mermaid::core::canvas::TextOptions;
use crate::render::mermaid::flowchart::types::FlowchartRenderOptions;
use crate::render::mermaid::flowchart::{
    draw_flowchart_diagram_grid, parse_mermaid_flowchart_diagram,
};
use crate::render::mermaid::sequence::{
    parse_mermaid_sequence_diagram, render_sequence_diagram, types::SequenceRenderOptions,
};

fn draw_flowchart(content: &str) -> String {
    let diagram = parse_mermaid_flowchart_diagram(content).expect("flowchart parses");
    draw_flowchart_diagram_grid(&diagram, &FlowchartRenderOptions::default())
        .expect("flowchart fits")
        .to_text(TextOptions { trim_top: true, trim_bottom: true })
}

// -- flowchart.test.ts: parsing ----------------------------------------------

#[test]
fn parses_mermaid_flowchart_nodes_and_standard_arrows() {
    let diagram = parse_mermaid_flowchart_diagram(
        "flowchart LR\n  A[Start] --> B{Check}\n  B -->|yes| C[(DB)]\n  B -->|no| D([Skip])\n  D --> E[[Sub]]",
    )
    .expect("parses");
    assert_eq!(diagram.nodes.len(), 5);
    let shapes: Vec<&str> = diagram
        .nodes
        .iter()
        .map(|node| match node.shape {
            crate::render::mermaid::flowchart::types::FlowchartNodeShape::Box => "box",
            crate::render::mermaid::flowchart::types::FlowchartNodeShape::Rounded => "rounded",
            crate::render::mermaid::flowchart::types::FlowchartNodeShape::Database => "database",
            crate::render::mermaid::flowchart::types::FlowchartNodeShape::Decision => "decision",
            crate::render::mermaid::flowchart::types::FlowchartNodeShape::Subroutine => {
                "subroutine"
            }
        })
        .collect();
    assert_eq!(shapes, vec!["box", "decision", "database", "rounded", "subroutine"]);
    assert_eq!(diagram.edges.len(), 4);
    assert_eq!(diagram.edges[1].label, "yes");
}

#[test]
fn decodes_html_entities_in_node_and_edge_labels() {
    let diagram = parse_mermaid_flowchart_diagram(
        "flowchart TD\n  A[Worker &amp; signer] -->|ack &lt;3s| B[&#8805; 1]",
    )
    .expect("parses");
    assert_eq!(diagram.nodes[0].label, "Worker & signer");
    assert_eq!(diagram.edges[0].label, "ack <3s");
    assert_eq!(diagram.nodes[1].label, "≥ 1");
}

#[test]
fn preserves_quoted_multiline_edge_labels() {
    let diagram =
        parse_mermaid_flowchart_diagram("flowchart LR\n  A -- \"line one<br/>line two\" --> B")
            .expect("parses");
    assert_eq!(diagram.edges[0].label, "line one<br/>line two");
}

#[test]
fn expands_ampersand_node_groups_into_fan_in_and_fan_out() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart TD\n  A --> B & C\n  B & C --> D")
        .expect("parses");
    assert_eq!(diagram.edges.len(), 4);
    assert!(diagram.edges.iter().any(|edge| edge.from == "A" && edge.to == "B"));
    assert!(diagram.edges.iter().any(|edge| edge.from == "A" && edge.to == "C"));
    assert!(diagram.edges.iter().any(|edge| edge.from == "B" && edge.to == "D"));
    assert!(diagram.edges.iter().any(|edge| edge.from == "C" && edge.to == "D"));
}

#[test]
fn keeps_ampersand_inside_quoted_labels_as_label_text() {
    let diagram =
        parse_mermaid_flowchart_diagram("flowchart TD\n  A[\"one & two\"] --> B").expect("parses");
    assert_eq!(diagram.nodes[0].label, "one & two");
    assert_eq!(diagram.nodes.len(), 2);
}

#[test]
fn rejects_empty_ampersand_group_members() {
    assert!(parse_mermaid_flowchart_diagram("flowchart TD\n  A & --> B").is_err());
}

#[test]
fn parses_each_edge_in_a_chained_flowchart_statement() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart LR\n  A --> B --> C").expect("parses");
    assert_eq!(diagram.edges.len(), 2);
    assert_eq!(diagram.edges[0].to, "B");
    assert_eq!(diagram.edges[1].from, "B");
}

#[test]
fn parses_labeled_undirected_dashed_and_bidirectional_edges() {
    let diagram = parse_mermaid_flowchart_diagram(
        "flowchart LR\n  DB[(Durable Object SQLite)]\n  API[Slack API]\n  DB -. no shared transaction .- API\n  API <--> DB",
    )
    .expect("parses");
    assert_eq!(diagram.edges.len(), 2);
    assert_eq!(diagram.edges[0].label, "no shared transaction");
    assert!(!diagram.edges[0].arrowhead);
    assert!(matches!(
        diagram.edges[0].style,
        Some(crate::render::mermaid::flowchart::types::EdgeStyle::Dashed)
    ));
    assert!(diagram.edges[1].source_arrowhead);
}

#[test]
fn parses_mermaid_subgraph_groups() {
    let diagram = parse_mermaid_flowchart_diagram(
        "flowchart TB\n  subgraph One\n    A --> B\n  end\n  subgraph Two [Two Label]\n    C\n  end\n  B --> C",
    )
    .expect("parses");
    assert_eq!(diagram.subgraphs.len(), 2);
    assert_eq!(diagram.subgraphs[0].id, "One");
    assert_eq!(diagram.subgraphs[1].label, "Two Label");
    assert!(diagram.subgraphs[0].node_ids.contains(&"A".to_string()));
    assert!(diagram.subgraphs[0].node_ids.contains(&"B".to_string()));
    assert_eq!(diagram.edges.len(), 2);
}

#[test]
fn parses_and_renders_inline_dashed_edge_labels() {
    let diagram =
        parse_mermaid_flowchart_diagram("flowchart LR\n  A -. text .-> B").expect("parses");
    assert_eq!(diagram.edges[0].label, "text");
    assert!(diagram.edges[0].arrowhead);
    let text = draw_flowchart("flowchart LR\n  A -. text .-> B");
    assert!(text.contains("text"), "{text}");
    assert!(text.contains("╌") || text.contains("─") || text.contains("┄"), "{text}");
}

#[test]
fn uses_invisible_subgraph_links_for_ordering_without_rendering_them() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart TD\n  A ~~~ B").expect("parses");
    assert!(diagram.edges[0].order_only);
}

#[test]
fn rejects_statements_it_cannot_parse() {
    for source in [
        "flowchart TD\n  A ==> & B",
        "flowchart TD\n  end",
        "flowchart TD\n  subgraph X\n    A",
        "flowchart TD\n  A --> B; C --> D",
    ] {
        assert!(parse_mermaid_flowchart_diagram(source).is_err(), "{source}");
    }
}

// -- flowchart.test.ts: rendering --------------------------------------------

#[test]
fn keeps_every_branch_connected_at_a_shared_source_junction() {
    let source = "flowchart TB\n  A --> B\n  A --> C";
    let diagram = parse_mermaid_flowchart_diagram(source).expect("parses");
    let layout = crate::render::mermaid::flowchart::layout::layout_flowchart_diagram(
        &diagram,
        &crate::render::mermaid::flowchart::types::FlowchartRenderOptions {
            compact: Some(true),
            ..Default::default()
        },
    );
    let source_point = layout.routes[0].points[0];
    assert_eq!(layout.routes[1].points[0], source_point);

    let grid =
        draw_flowchart_diagram_grid(&diagram, &FlowchartRenderOptions::default()).expect("grid");
    assert_eq!(
        grid.get_cell(source_point.x, source_point.y).map(super::core::canvas::Cell::text),
        Some("┴")
    );
}

#[test]
fn renders_a_simple_vertical_flowchart_with_nodes_and_edges() {
    let text = draw_flowchart("flowchart TD\n  Start --> Stop");
    assert!(text.contains("Start"), "{text}");
    assert!(text.contains("Stop"), "{text}");
    assert!(text.contains('▼'), "{text}");
    // Node frames draw rounded boxes.
    assert!(text.contains('╭') && text.contains('╰'), "{text}");
}

#[test]
fn renders_labeled_edges_between_nodes() {
    let text = draw_flowchart("flowchart TD\n  A -->|label text| B");
    assert!(text.contains("label text"), "{text}");
}

#[test]
fn renders_subgraph_frames_with_labels() {
    let text = draw_flowchart("flowchart LR\n  subgraph Group\n    A[Node]\n  end");
    assert!(text.contains("Group"), "{text}");
    assert!(text.contains("Node"), "{text}");
}

#[test]
fn renders_horizontal_directions() {
    let text = draw_flowchart("flowchart LR\n  A --> B");
    assert!(text.contains('▶'), "{text}");
}

// -- sequence/diagram.test.ts -------------------------------------------------

#[test]
fn parses_mermaid_sequence_participants_and_messages() {
    let diagram = parse_mermaid_sequence_diagram(
        "\nsequenceDiagram\n  participant B as Browser\n  participant S as Server\n  B->>S: GET /\n  S-->>B: 401 WWW-Auth\n",
    )
    .expect("parses");
    assert_eq!(
        diagram.participants,
        vec![
            crate::render::mermaid::sequence::types::SequenceParticipant {
                id: "B".into(),
                label: "Browser".into(),
            },
            crate::render::mermaid::sequence::types::SequenceParticipant {
                id: "S".into(),
                label: "Server".into(),
            },
        ]
    );
    assert_eq!(diagram.messages.len(), 2);
    assert_eq!(diagram.messages[0].label, "GET /");
    assert!(!diagram.messages[0].dashed);
    assert!(diagram.messages[1].dashed);
}

#[test]
fn decodes_html_entities_in_participant_message_and_note_labels() {
    let diagram = parse_mermaid_sequence_diagram(
        "sequenceDiagram\n  participant A as Worker &amp; signer\n  participant B\n  A->>B: ack &lt;3s\n  Note over A,B: result &#8805; 1",
    )
    .expect("parses");
    assert_eq!(diagram.participants[0].label, "Worker & signer");
    assert_eq!(diagram.messages[0].label, "ack <3s");
    assert_eq!(diagram.messages[0].label, "ack <3s");
    let note = diagram
        .steps
        .iter()
        .find_map(|step| match step {
            crate::render::mermaid::sequence::types::SequenceStep::Note(note) => Some(note),
            _ => None,
        })
        .expect("note");
    assert_eq!(note.label, "result ≥ 1");
}

#[test]
fn renders_a_terminal_sequence_diagram() {
    let output = render_sequence_diagram(
        "\nsequenceDiagram\n  participant B as Browser\n  participant S as Server\n  B->>S: GET /\n  S-->>B: 401 WWW-Auth\n",
        &SequenceRenderOptions::default(),
    )
    .expect("renders");
    let expected = [
        "Browser           Server",
        "───┬───           ───┬──",
        "   │                 │",
        "   │ GET /           │",
        "   ├─────────────────►",
        "   │                 │",
        "   │ 401 WWW-Auth    │",
        "   ◄─────────────────┤",
        "   │                 │",
    ]
    .join("\n");
    assert_eq!(output, expected);
}

#[test]
fn renders_a_compact_sequence_diagram_without_participant_boxes() {
    let output = render_sequence_diagram(
        "\nsequenceDiagram\n  participant Leaf as leaf tool\n  participant Location as LocationMutation\n  participant File as FileMutation\n  Leaf->>Location: resolve(path)\n  Location-->>Leaf: Plan(target, authority anchor)\n  Leaf->>File: commit(plan)\n  File->>Location: revalidate(plan)\n  Location-->>File: same target or reject\n",
        &SequenceRenderOptions { compact: true, ..Default::default() },
    )
    .expect("renders");
    let expected = [
        "leaf tool                       LocationMutation             FileMutation",
        "    │                                   │                          │",
        "    ├───────── resolve(path) ───────────►                          │",
        "    │                                   │                          │",
        "    ◄─ Plan(target, authority anchor) ──┤                          │",
        "    │                                   │                          │",
        "    ├─────────────────────── commit(plan) ─────────────────────────►",
        "    │                                   │                          │",
        "    │                                   ◄─── revalidate(plan) ─────┤",
        "    │                                   │                          │",
        "    │                                   ├─ same target or reject ──►",
        "    │                                   │                          │",
    ]
    .join("\n");
    assert_eq!(output, expected);
}

#[test]
fn parses_activation_shorthand_and_control_blocks() {
    let diagram = parse_mermaid_sequence_diagram(
        "sequenceDiagram\n  Browser->>+Server: request\n  alt accepted\n    Server-->>-Browser: response\n  else rejected\n    activate Server\n    Server-->>Browser: error\n    deactivate Server\n  end\n  loop retry\n    A->>B: again\n  end",
    )
    .expect("parses");
    assert_eq!(diagram.messages[0].activate.as_deref(), Some("Server"));
    assert_eq!(diagram.messages[1].deactivate.as_deref(), Some("Server"));
    let fragments: Vec<&crate::render::mermaid::sequence::types::SequenceFragment> = diagram
        .steps
        .iter()
        .filter_map(|step| match step {
            crate::render::mermaid::sequence::types::SequenceStep::Fragment(fragment) => {
                Some(fragment)
            }
            _ => None,
        })
        .collect();
    assert_eq!(fragments.len(), 5); // alt, else, end, loop, end
}

#[test]
fn parses_and_renders_autonumbered_messages() {
    let output = render_sequence_diagram(
        "sequenceDiagram\n  autonumber\n  A->>B: first\n  B-->>A: second",
        &SequenceRenderOptions::default(),
    )
    .expect("renders");
    assert!(output.contains("1. first"), "{output}");
    assert!(output.contains("2. second"), "{output}");
}

#[test]
fn supports_autonumber_start_and_increment() {
    let diagram = parse_mermaid_sequence_diagram(
        "sequenceDiagram\n  autonumber 10 5\n  A->>B: one\n  B-->>A: two",
    )
    .expect("parses");
    assert_eq!(diagram.messages[0].number, Some(10));
    assert_eq!(diagram.messages[1].number, Some(15));
}

#[test]
fn parses_mermaid_arrow_head_variants() {
    let diagram = parse_mermaid_sequence_diagram(
        "sequenceDiagram\n  A->>B: solid\n  A--xB: cross\n  A--)B: async\n  A->B: open",
    )
    .expect("parses");
    let heads: Vec<Option<&str>> = diagram
        .messages
        .iter()
        .map(|message| {
            message.head.map(|head| match head {
                crate::render::mermaid::sequence::types::SequenceArrowHead::Open => "open",
                crate::render::mermaid::sequence::types::SequenceArrowHead::Cross => "cross",
                crate::render::mermaid::sequence::types::SequenceArrowHead::Async => "async",
            })
        })
        .collect();
    assert_eq!(heads, vec![None, Some("cross"), Some("async"), Some("open")]);
}

#[test]
fn parses_mermaid_box_participant_groups() {
    let diagram = parse_mermaid_sequence_diagram(
        "sequenceDiagram\n  box Frontend\n    participant A\n    participant B\n  end\n  A->>B: hi",
    )
    .expect("parses");
    assert_eq!(diagram.groups.len(), 1);
    assert_eq!(diagram.groups[0].label, "Frontend");
    assert_eq!(diagram.groups[0].participant_ids, vec!["A", "B"]);
}

#[test]
fn renders_self_messages_as_loopback_arrows() {
    let output = render_sequence_diagram(
        "sequenceDiagram\n  A->>A: self",
        &SequenceRenderOptions::default(),
    )
    .expect("renders");
    assert!(output.contains("self"), "{output}");
    assert!(output.contains('◄'), "{output}");
}

#[test]
fn renders_notes_in_their_reserved_rows() {
    let output = render_sequence_diagram(
        "sequenceDiagram\n  A->>B: hi\n  Note over A,B: combined note",
        &SequenceRenderOptions::default(),
    )
    .expect("renders");
    assert!(output.contains("combined note"), "{output}");
}

#[test]
fn rejects_unsupported_bidirectional_messages_and_bad_lines() {
    assert!(parse_mermaid_sequence_diagram("sequenceDiagram\n  A<<->>B: nope").is_err());
    assert!(parse_mermaid_sequence_diagram("sequenceDiagram\n  totally unknown line").is_err());
    assert!(parse_mermaid_sequence_diagram("sequenceDiagram\n  else lonely").is_err());
    assert!(parse_mermaid_sequence_diagram("sequenceDiagram\n  loop forever\n  A->>B: x").is_err());
}

// -- G-176: fan bus grouping keys (edge from/to, not edge_index) --------------

fn route_for(
    layout: &crate::render::mermaid::flowchart::layout::FlowchartLayout,
    edge_index: usize,
) -> &crate::render::mermaid::flowchart::types::EdgeRoute {
    layout
        .routes
        .iter()
        .find(|route| route.edge_index == edge_index)
        .unwrap_or_else(|| panic!("no route for edge {edge_index}"))
}

#[test]
fn horizontal_fan_out_edges_share_a_bus_lane() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart LR\n  A --> B\n  A --> C")
        .expect("parses");
    let layout = crate::render::mermaid::flowchart::layout::layout_flowchart_diagram(
        &diagram,
        &FlowchartRenderOptions::default(),
    );
    let lanes = |index: usize| -> std::collections::HashSet<i64> {
        route_for(&layout, index)
            .points
            .iter()
            .map(|point| point.x)
            .collect::<std::collections::HashSet<_>>()
    };
    assert!(
        !lanes(0).is_disjoint(&lanes(1)),
        "fan-out routes must share a bus x lane: {:?} vs {:?}",
        layout.routes[0].points,
        layout.routes[1].points
    );
}

#[test]
fn horizontal_fan_in_edges_share_a_bus_lane() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart LR\n  B --> A\n  C --> A")
        .expect("parses");
    let layout = crate::render::mermaid::flowchart::layout::layout_flowchart_diagram(
        &diagram,
        &FlowchartRenderOptions::default(),
    );
    let lanes = |index: usize| -> std::collections::HashSet<i64> {
        let points = &route_for(&layout, index).points;
        points[1..points.len().saturating_sub(1)]
            .iter()
            .map(|point| point.x)
            .collect::<std::collections::HashSet<_>>()
    };
    assert!(
        !lanes(0).is_disjoint(&lanes(1)),
        "fan-in routes must share a bus x lane: {:?} vs {:?}",
        layout.routes[0].points,
        layout.routes[1].points
    );
}

#[test]
fn vertical_fan_out_edges_share_a_bus_lane() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart TD\n  A --> B\n  A --> C")
        .expect("parses");
    let layout = crate::render::mermaid::flowchart::layout::layout_flowchart_diagram(
        &diagram,
        &FlowchartRenderOptions::default(),
    );
    let lanes = |index: usize| -> std::collections::HashSet<i64> {
        route_for(&layout, index)
            .points
            .iter()
            .map(|point| point.y)
            .collect::<std::collections::HashSet<_>>()
    };
    assert!(
        !lanes(0).is_disjoint(&lanes(1)),
        "fan-out routes must share a bus y lane: {:?} vs {:?}",
        layout.routes[0].points,
        layout.routes[1].points
    );
}

#[test]
fn vertical_fan_in_edges_share_a_bus_lane() {
    let diagram = parse_mermaid_flowchart_diagram("flowchart TD\n  B --> A\n  C --> A")
        .expect("parses");
    let layout = crate::render::mermaid::flowchart::layout::layout_flowchart_diagram(
        &diagram,
        &FlowchartRenderOptions::default(),
    );
    let lanes = |index: usize| -> std::collections::HashSet<i64> {
        let points = &route_for(&layout, index).points;
        points[1..points.len().saturating_sub(1)]
            .iter()
            .map(|point| point.y)
            .collect::<std::collections::HashSet<_>>()
    };
    assert!(
        !lanes(0).is_disjoint(&lanes(1)),
        "fan-in routes must share a bus y lane: {:?} vs {:?}",
        layout.routes[0].points,
        layout.routes[1].points
    );
}

// -- G-178: open edges (---) render a line end, not an arrowhead -------------

#[test]
fn open_edge_renders_a_line_end_without_an_arrowhead() {
    let text = draw_flowchart("flowchart LR\n  A --- B");
    for arrow in ['▶', '◀', '▲', '▼'] {
        assert!(!text.contains(arrow), "open edge must not draw {arrow}:\n{text}");
    }
    // The edge still terminates on the target border with a line glyph.
    assert!(text.contains('─'), "open edge should draw a horizontal line:\n{text}");
}

#[test]
fn arrow_edge_still_renders_an_arrowhead() {
    let text = draw_flowchart("flowchart LR\n  A --> B");
    assert!(text.contains('▶'), "arrow edge must keep its arrowhead:\n{text}");
}

#[test]
fn merged_edge_cell_keeps_ref_arrowhead_precedence() {
    use crate::render::mermaid::core::canvas::Cell;
    use crate::render::mermaid::flowchart::style::{merge_flowchart_cell, FlowchartStyle};
    let edge = || Some(FlowchartStyle::Edge);
    // An already-drawn arrowhead survives an incoming line glyph.
    let existing_arrow = Cell::build("▶", edge(), None);
    let incoming_line = Cell::build("─", edge(), None);
    assert_eq!(merge_flowchart_cell(&existing_arrow, &incoming_line).text(), "▶");
    // An incoming arrowhead replaces an already-drawn line glyph.
    let existing_line = Cell::build("─", edge(), None);
    let incoming_arrow = Cell::build("▶", edge(), None);
    assert_eq!(merge_flowchart_cell(&existing_line, &incoming_arrow).text(), "▶");
}

// -- G-179: seam width wiring + streaming truncation recovery -----------------

fn test_palette() -> super::palette::OpenCodeDiagramPalette {
    let rgba = super::core::color::Rgba::from_ints;
    super::palette::create_open_code_diagram_palette(&super::palette::OpenCodeDiagramPaletteInput {
        text: rgba(220, 220, 220, 255),
        subdued: rgba(140, 140, 140, 255),
        info: rgba(100, 200, 255, 255),
        success: rgba(120, 255, 140, 255),
        warning: rgba(255, 200, 100, 255),
        background: rgba(20, 20, 20, 255),
        accent_soft: rgba(150, 150, 255, 255),
        accent_clear: rgba(150, 150, 255, 255),
    })
}

#[test]
fn context_width_bounds_the_rendered_diagram() {
    let palette = test_palette();
    let source = "flowchart LR\n  Alpha[Long Start Node] --> Beta[Middle Node]\n  Beta --> Gamma[Long Tail Node]";
    let wide = super::render_mermaid_block(
        source,
        &palette,
        &super::MermaidRenderOptions { context_width: None, ..Default::default() },
    )
    .expect("unclamped render parses");
    let clamped = super::render_mermaid_block(
        source,
        &palette,
        &super::MermaidRenderOptions {
            context_width: Some(30),
            ..Default::default()
        },
    )
    .expect("clamped render parses");
    assert!(
        clamped.rows.iter().any(|row| row.iter().map(|span| span.text.chars().count()).sum::<usize>() > 0),
        "clamped render must produce rows"
    );
    let clamped_width =
        clamped.rows.iter().map(|row| row.iter().map(|span| span.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    let wide_width =
        wide.rows.iter().map(|row| row.iter().map(|span| span.text.chars().count()).sum::<usize>()).max().unwrap_or(0);
    assert!(clamped_width <= 30, "context width must bound the render: {clamped_width}");
    assert!(wide_width > 30 || clamped_width < wide_width, "clamping should actually change the layout: wide={wide_width} clamped={clamped_width}");
}

#[test]
fn truncated_partial_source_renders_instead_of_falling_back() {
    let palette = test_palette();
    let mut cache = super::MermaidLastGoodCache::default();
    // A streaming-partial source: the trailing line is mid-statement and
    // nothing follows it (error line > 2, blank remainder).
    let partial = "flowchart TD\n  A --> B\n  B --";
    let rendered = super::render_mermaid_block_with_recovery(
        partial,
        &palette,
        &super::MermaidRenderOptions::default(),
        Some("fence:0"),
        &mut cache,
    );
    let Some(diagram) = rendered else {
        panic!("truncated retry must render the parseable prefix");
    };
    let text = diagram.rows.iter().map(|row| row.iter().map(|span| span.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n");
    assert!(text.contains('A') && text.contains('B'), "prefix nodes must render: {text}");
}

#[test]
fn last_good_diagram_is_replayed_during_a_broken_stream_update() {
    let palette = test_palette();
    let mut cache = super::MermaidLastGoodCache::default();
    let good = super::render_mermaid_block_with_recovery(
        "flowchart TD\n  A --> B",
        &palette,
        &super::MermaidRenderOptions::default(),
        Some("fence:0"),
        &mut cache,
    )
    .expect("good source renders");
    // Mid-rewrite garbage: same kind, syntax error with a non-blank tail
    // (retry refused) — the lastGood diagram must be replayed.
    let broken = super::render_mermaid_block_with_recovery(
        "flowchart TD\n  A --!-> ~~~garbage~~~",
        &palette,
        &super::MermaidRenderOptions::default(),
        Some("fence:0"),
        &mut cache,
    )
    .expect("lastGood must replay");
    assert_eq!(good, broken);
}

#[test]
fn unparseable_source_without_a_good_prefix_still_falls_back() {
    let palette = test_palette();
    let mut cache = super::MermaidLastGoodCache::default();
    let rendered = super::render_mermaid_block_with_recovery(
        "flowchart TD\n  A --!-> ??? broken tail that keeps going",
        &palette,
        &super::MermaidRenderOptions::default(),
        Some("fence:9"),
        &mut cache,
    );
    assert!(rendered.is_none(), "unparseable source must fall back to the code block");
}
