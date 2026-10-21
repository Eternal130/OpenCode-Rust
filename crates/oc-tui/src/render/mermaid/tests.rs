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
