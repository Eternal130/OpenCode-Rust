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

//! Ported upstream state cases: `state/diagram.test.ts` (goldens and
//! invariants), `state/visible-model.test.ts`, `state/style.test.ts`.

use std::collections::HashSet;

use crate::render::mermaid::core::spatial::spatial_path_spans;
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::state::drawing::create_state_diagram_drawing;
use crate::render::mermaid::state::layout::{
    create_state_diagram_layout, StateDiagramLayoutOptions,
};
use crate::render::mermaid::state::parser::{
    is_mermaid_state_diagram, parse_mermaid_state_diagram,
};
use crate::render::mermaid::state::render_state_diagram;
use crate::render::mermaid::state::search::StateSearchBudget;
use crate::render::mermaid::state::style::{resolve_state_style_colors, StateColorInput};
use crate::render::mermaid::state::types::{
    NotePosition, StateDiagramRenderOptions, StateDirection, StateKind,
};
use crate::render::mermaid::state::visible_model::{
    is_hidden_composite_marker, prepare_visible_state_diagram,
};

fn draw(content: &str) -> String {
    render_state_diagram(content, &StateDiagramRenderOptions::default()).expect("state renders")
}

fn draw_with(content: &str, options: &StateDiagramRenderOptions) -> String {
    render_state_diagram(content, options).expect("state renders")
}

fn line_width(line: &str) -> i64 {
    diagram_text_width(line) as i64
}

fn rendered_width(output: &str) -> i64 {
    output.split('\n').map(line_width).max().unwrap_or(0)
}

fn count_occurrences(output: &str, needle: &str) -> usize {
    output.split(needle).count() - 1
}

/// `responsiveStateChain`.
fn responsive_state_chain(direction: &str, profile: &str) -> String {
    let state_label = |id: &str| -> String {
        match profile {
            "long" => format!("{id} deliberate state with a long descriptive label"),
            "unicode" => format!("{id} 東京<br/>résumé 🚀"),
            _ => format!("{id} node"),
        }
    };
    let transition_label = |id: &str| -> String {
        match profile {
            "long" => format!("{id} transition carrying detailed context"),
            "unicode" => format!("{id} 東京<br/>✓ prêt"),
            _ => format!("{id} edge"),
        }
    };
    let ids = ["A", "B", "C", "D", "E"];
    let mut lines = vec!["stateDiagram-v2".to_string(), format!("direction {direction}")];
    for id in ids {
        lines.push(format!("state \"{}\" as {id}", state_label(id)));
    }
    lines.push("[*] --> A".to_string());
    for (index, id) in ids.iter().enumerate().take(ids.len() - 1) {
        lines.push(format!(
            "{id} --> {}: {}",
            ids[index + 1],
            transition_label(&format!("E0{}", index + 1))
        ));
    }
    lines.push("E --> [*]".to_string());
    lines.join("\n")
}

/// `expectCompleteStateDiagram` — labels present, state boxes pairwise
/// clear, note boxes and connectors clear of states and each other.
fn expect_complete_state_diagram(source: &str, output: &str) {
    let diagram =
        prepare_visible_state_diagram(&parse_mermaid_state_diagram(source).expect("parses"));
    let layout = create_state_diagram_layout(
        &diagram,
        &StateDiagramLayoutOptions { min_state_gap: 5, search_budget: None },
    )
    .expect("layout");
    let states: Vec<_> =
        diagram.states.iter().filter(|state| state.kind == StateKind::State).collect();
    let state_bounds: Vec<_> =
        states.iter().map(|state| layout.box_of(&state.id).expect("state bound")).collect();

    for text in states
        .iter()
        .map(|state| state.label.as_str())
        .chain(diagram.transitions.iter().map(|transition| transition.label.as_str()))
        .chain(diagram.notes.iter().flat_map(|note| note.lines.iter().map(String::as_str)))
        .filter(|text| !text.is_empty())
    {
        assert!(output.contains(text), "output missing {text:?}");
    }
    for (index, bound) in state_bounds.iter().enumerate() {
        for other in state_bounds.iter().skip(index + 1) {
            let overlaps = bound.left < other.left + other.width
                && bound.left + bound.width > other.left
                && bound.top < other.top + other.height
                && bound.top + bound.height > other.top;
            assert!(!overlaps, "{} overlaps {}", bound.id, other.id);
        }
    }
    let occupied_by_note: Vec<HashSet<(i64, i64)>> = layout
        .note_bounds
        .iter()
        .map(|note| {
            let connector = note.connector.as_ref().expect("placed note connector");
            let mut occupied: HashSet<(i64, i64)> = HashSet::new();
            for dy in 0..note.bound.height {
                for dx in 0..note.bound.width {
                    occupied.insert((note.bound.left + dx, note.bound.top + dy));
                }
            }
            for span in spatial_path_spans(&connector.points) {
                for dx in 0..=(span.to_x - span.from_x) {
                    occupied.insert((span.from_x + dx, span.y));
                }
            }
            occupied
        })
        .collect();
    for (index, occupied) in occupied_by_note.iter().enumerate() {
        for bound in &state_bounds {
            let overlaps = (0..bound.height).any(|dy| {
                (0..bound.width).any(|dx| occupied.contains(&(bound.left + dx, bound.top + dy)))
            });
            assert!(!overlaps, "{} overlaps {}", layout.note_bounds[index].bound.id, bound.id);
        }
        for other in occupied_by_note.iter().skip(index + 1) {
            assert!(occupied.is_disjoint(other), "note {index} overlaps another note");
        }
    }
}

#[test]
fn detects_and_parses_mermaid_state_diagrams() {
    let diagram = parse_mermaid_state_diagram(
        "\nstateDiagram-v2\n  %% request lifecycle\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  Loading --> Success: done\n  Success --> [*]\n",
    )
    .expect("parses");
    assert_eq!(diagram.direction, StateDirection::Lr);
    let state = |id: &str, label: &str, kind: StateKind| {
        crate::render::mermaid::state::types::StateDiagramState {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            parent_id: None,
        }
    };
    assert_eq!(
        diagram.states,
        vec![
            state("__start", "●", StateKind::Start),
            state("Idle", "Idle", StateKind::State),
            state("Loading", "Loading", StateKind::State),
            state("Success", "Success", StateKind::State),
            state("__end", "◎", StateKind::End),
        ]
    );
    let transition = |from: &str, to: &str, label: &str| {
        crate::render::mermaid::state::types::StateDiagramTransition {
            from: from.to_string(),
            to: to.to_string(),
            label: label.to_string(),
        }
    };
    assert_eq!(
        diagram.transitions,
        vec![
            transition("__start", "Idle", ""),
            transition("Idle", "Loading", "submit"),
            transition("Loading", "Success", "done"),
            transition("Success", "__end", ""),
        ]
    );
    assert!(is_mermaid_state_diagram("stateDiagram-v2\n[*] --> A"));
}

#[test]
fn parses_quoted_state_aliases() {
    let diagram = parse_mermaid_state_diagram(
        "\nstateDiagram-v2\n  state \"Waiting<br/>for Payment\" as WaitingPayment\n  [*] --> WaitingPayment\n",
    )
    .expect("parses");
    assert!(diagram.states.iter().any(|state| state.id == "WaitingPayment"
        && state.label == "Waiting<br/>for Payment"
        && state.kind == StateKind::State));
}

#[test]
fn decodes_html_entities_in_state_transition_and_note_labels() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\n  state \"Ready &amp; waiting\" as Ready\n  Ready --> Done: elapsed &lt;3s\n  note right of Done: result &#x2265; 1",
    )
    .expect("parses");
    assert_eq!(
        diagram.states.iter().find(|state| state.id == "Ready").unwrap().label,
        "Ready & waiting"
    );
    assert_eq!(diagram.transitions[0].label, "elapsed <3s");
    assert_eq!(diagram.notes[0].lines, vec!["result ≥ 1"]);
}

#[test]
fn parses_choice_pseudo_states() {
    let diagram = parse_mermaid_state_diagram(
        "\nstateDiagram-v2\n  [*] --> Decision\n  state Decision <<choice>>\n  Decision --> Accepted: yes\n",
    )
    .expect("parses");
    assert!(diagram.states.iter().any(|state| state.id == "Decision"
        && state.label.is_empty()
        && state.kind == StateKind::Choice));
}

#[test]
fn parses_composite_states_and_notes() {
    let diagram = parse_mermaid_state_diagram(
        "\nstateDiagram-v2\n  state Authenticated {\n    [*] --> Idle\n    Idle --> Editing: open\n  }\n  note right of Editing\n    Draft changes\n  end note\n",
    )
    .expect("parses");
    assert!(diagram
        .composites
        .iter()
        .any(|composite| composite.id == "Authenticated" && composite.label == "Authenticated"));
    assert!(diagram.states.iter().any(|state| state.id == "Idle"
        && state.kind == StateKind::State
        && state.parent_id.as_deref() == Some("Authenticated")));
    assert!(diagram.states.iter().any(|state| state.id == "Authenticated.__start"
        && state.label == "●"
        && state.kind == StateKind::Start
        && state.parent_id.as_deref() == Some("Authenticated")));
    assert_eq!(
        diagram.notes,
        vec![crate::render::mermaid::state::types::StateDiagramNote {
            target: "Editing".to_string(),
            position: NotePosition::Right,
            lines: vec!["Draft changes".to_string()],
        }]
    );
}

#[test]
fn renders_a_horizontal_state_diagram() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  Loading --> Success: done\n  Success --> [*]\n");
    assert_eq!(
        output,
        "              ╭──────╮   submit    ╭─────────╮    done     ╭─────────╮\n●────────────▶│ Idle ├────────────▶│ Loading ├────────────▶│ Success ├────────────▶◎\n              ╰──────╯             ╰─────────╯             ╰─────────╯"
    );
}

#[test]
fn renders_reverse_horizontal_direction_from_right_to_left() {
    let output = draw("stateDiagram-v2\ndirection RL\nA --> B");
    let label_row = output
        .split('\n')
        .find(|line| line.contains(" A ") && line.contains(" B "))
        .expect("label row");
    assert!(label_row.find('B').unwrap() < label_row.find('A').unwrap());
    assert!(output.contains('◀'));
}

#[test]
fn renders_reverse_vertical_direction_from_bottom_to_top() {
    let source = "stateDiagram-v2\ndirection BT\nA --> B";
    let diagram = parse_mermaid_state_diagram(source).expect("parses");
    let drawing = create_state_diagram_drawing(&diagram, &StateDiagramRenderOptions::default())
        .expect("draws");
    let a = drawing.layout.box_of("A").unwrap();
    let b = drawing.layout.box_of("B").unwrap();
    assert!(a.top > b.top);
    assert!(drawing
        .grid
        .to_text(crate::render::mermaid::core::canvas::TextOptions {
            trim_top: true,
            trim_bottom: true
        })
        .contains('▲'));
}

#[test]
fn folds_responsive_chains_at_column_targets() {
    for direction in ["LR", "RL"] {
        for profile in ["short", "long", "unicode"] {
            for layout_max_width in [60, 80, 120] {
                let source = responsive_state_chain(direction, profile);
                let horizontal = draw(&source);
                let responsive = draw_with(
                    &source,
                    &StateDiagramRenderOptions {
                        layout_max_width: Some(layout_max_width),
                        ..Default::default()
                    },
                );
                let vertical = draw_with(
                    &source,
                    &StateDiagramRenderOptions {
                        direction: Some(if direction == "RL" {
                            StateDirection::Bt
                        } else {
                            StateDirection::Tb
                        }),
                        ..Default::default()
                    },
                );

                assert!(
                    rendered_width(&horizontal) > layout_max_width,
                    "{direction} {profile} {layout_max_width} horizontal should overflow"
                );
                assert_eq!(responsive, vertical, "{direction} {profile} {layout_max_width}");
                assert!(rendered_width(&responsive) < rendered_width(&horizontal));
                for content in ["A", "B", "C", "D", "E", "E01", "E02", "E03", "E04"] {
                    assert!(responsive.contains(content));
                }
            }
        }
    }
}

#[test]
fn keeps_the_narrower_orientation_for_broad_ranks() {
    for direction in ["LR", "RL"] {
        let ranks: Vec<String> = (0..8).map(|index| format!("  A --> B{index}")).collect();
        let source = format!("stateDiagram-v2\n  direction {direction}\n{}", ranks.join("\n"));
        let horizontal = draw(&source);
        let vertical = draw_with(
            &source,
            &StateDiagramRenderOptions {
                direction: Some(if direction == "RL" {
                    StateDirection::Bt
                } else {
                    StateDirection::Tb
                }),
                ..Default::default()
            },
        );
        let responsive = draw_with(
            &source,
            &StateDiagramRenderOptions { layout_max_width: Some(60), ..Default::default() },
        );

        assert!(rendered_width(&horizontal) < rendered_width(&vertical), "{direction}");
        assert_eq!(responsive, horizontal, "{direction}");
    }
}

#[test]
fn falls_back_before_allocating_an_oversized_horizontal_canvas() {
    let ids: Vec<String> = (0..301).map(|index| format!("S{index}")).collect();
    let label = "transition label carrying enough context to make the horizontal canvas too large";
    let transitions: Vec<String> = ids[..ids.len() - 1]
        .iter()
        .enumerate()
        .map(|(index, id)| format!("  {id} --> {}: {label}", ids[index + 1]))
        .collect();
    let source = format!("stateDiagram-v2\n  direction LR\n{}", transitions.join("\n"));
    let output = draw_with(
        &source,
        &StateDiagramRenderOptions { layout_max_width: Some(80), ..Default::default() },
    );

    assert!(output.contains("S0"));
    assert!(output.contains("S300"));
    assert!(rendered_width(&output) <= line_width(label) + 8);
}

#[test]
fn preserves_explicit_vertical_layouts_under_a_narrow_width_target() {
    for direction in [StateDirection::Tb, StateDirection::Td, StateDirection::Bt] {
        let source =
            format!("stateDiagram-v2\n  direction {}\n  A --> B: next", direction_str(direction));
        let narrow = draw_with(
            &source,
            &StateDiagramRenderOptions { layout_max_width: Some(1), ..Default::default() },
        );
        assert_eq!(narrow, draw(&source));
    }
}

fn direction_str(direction: StateDirection) -> &'static str {
    match direction {
        StateDirection::Tb => "TB",
        StateDirection::Td => "TD",
        StateDirection::Bt => "BT",
        StateDirection::Lr => "LR",
        StateDirection::Rl => "RL",
    }
}

#[test]
fn preserves_horizontal_layouts_that_fit_or_have_no_width_target() {
    let source = "stateDiagram-v2\ndirection LR\nA --> B";
    let output = draw(source);
    assert_eq!(
        draw_with(
            source,
            &StateDiagramRenderOptions { layout_max_width: Some(120), ..Default::default() }
        ),
        output
    );
    // `Number.POSITIVE_INFINITY` upstream → no finite target → no folding.
    assert_eq!(draw_with(source, &StateDiagramRenderOptions::default()), output);
}

#[test]
fn treats_a_single_irreducibly_wide_state_as_soft_overflow() {
    let label = "界".repeat(40);
    let source = format!("stateDiagram-v2\ndirection LR\nstate \"{label}\" as Wide");
    let output = draw_with(
        &source,
        &StateDiagramRenderOptions { layout_max_width: Some(60), ..Default::default() },
    );
    assert!(rendered_width(&output) > 60);
    assert!(output.contains(&label));
}

#[test]
fn does_not_mutate_a_parsed_diagram_when_rendering_with_a_direction_override() {
    let diagram =
        parse_mermaid_state_diagram("stateDiagram-v2\ndirection LR\nA --> B").expect("parses");
    let _ = crate::render::mermaid::state::drawing::draw_state_diagram_grid(
        &diagram,
        &StateDiagramRenderOptions { direction: Some(StateDirection::Rl), ..Default::default() },
    );
    assert_eq!(diagram.direction, StateDirection::Lr);
}

#[test]
fn places_right_to_left_transition_labels_between_intact_frames() {
    let output = draw(
        "stateDiagram-v2\ndirection RL\nA --> B: reopen after a very detailed reviewer comment",
    );
    assert!(output.contains("╭───╮"));
    assert_eq!(count_occurrences(&output, "╭───╮"), 2);
    assert!(output.contains("reopen after a very detailed reviewer comment"));
}

#[test]
fn keeps_unicode_state_labels_inside_their_measured_frame() {
    let output = draw("stateDiagram-v2\ndirection LR\nstate \"界\" as Wide");
    let widths: HashSet<i64> = output.split('\n').map(line_width).collect();
    assert_eq!(widths.len(), 1);
    assert!(output.contains('界'));
}

#[test]
fn reserves_horizontal_room_for_long_transition_labels() {
    let label = "this transition label is much wider than the route";
    let output = draw(&format!("stateDiagram-v2\ndirection LR\nA --> B: {label}"));
    let label_row = output.split('\n').find(|line| line.contains(label)).expect("label row");
    assert_eq!(count_occurrences(label_row, "╭───╮"), 2);
}

#[test]
fn renders_every_line_of_multiline_transition_labels() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B: first<br/>second");
    assert!(output.contains("first"));
    assert!(output.contains("second"));
}

#[test]
fn keeps_reciprocal_multiline_transition_labels_clear_of_routes() {
    let output = draw(
        "stateDiagram-v2\n    [*] --> Running: create from base image\n    Running --> Dormant: 📸 suspend hook fires<br/>(WE must call it on idle)\n    Dormant --> Running: wake from snapshot image<br/>(apt installs restored!)\n    Running --> Lost: 💥 sandbox dies BEFORE hook fires<br/>(crash, our bug, race)\n    Lost --> Running: wake from LAST snapshot<br/>⚠ files since then GONE",
    );
    let label_lines = [
        "create from base image",
        "📸 suspend hook fires",
        "(WE must call it on idle)",
        "wake from snapshot image",
        "(apt installs restored!)",
        "💥 sandbox dies BEFORE hook fires",
        "(crash, our bug, race)",
        "wake from LAST snapshot",
        "⚠ files since then GONE",
    ];
    for line in label_lines {
        assert_eq!(count_occurrences(&output, line), 1, "{line}");
    }
    assert_eq!(
        output,
        "  create from base image ╭─────────╮\n●───────────────────────▶│ Running │\n                         ╰──┬──────╯\n                          ▲ │   ▲        💥 sandbox dies BEFORE hook fires\n                 ╭────────┼─┴───┼───────╮(crash, our bug, race)\n                 ▼   ╭────┼─────╯       ▼\n              ╭──────┴──╮ │           ╭──────╮\n              │ Dormant │ │           │ Lost │\n              ╰─────────╯ │           ╰───┬──╯\n                          │               │\n 📸 suspend hook fires    │               │ wake from LAST snapshot\n (WE must call it on idle)│               │ ⚠ files since then GONE\n                          ╰───────────────╯\n                      wake from snapshot image\n                      (apt installs restored!)"
    );
}

#[test]
fn renders_a_vertical_state_diagram() {
    let output = draw("\nstateDiagram-v2\n  direction TB\n  [*] --> Cart\n  Cart --> Payment: checkout\n  Payment --> Complete\n");
    assert_eq!(
        output,
        "      ●\n      │\n      │\n      │\n      ▼\n  ╭──────╮\n  │ Cart │\n  ╰───┬──╯\n      │\n      │ checkout\n      │\n      ▼\n ╭─────────╮\n │ Payment │\n ╰────┬────╯\n      │\n      │\n      │\n      ▼\n╭──────────╮\n│ Complete │\n╰──────────╯"
    );
}

#[test]
fn renders_branched_and_backward_transitions_visibly() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  Loading --> Success: 200 OK\n  Loading --> Error: timeout\n  Error --> Loading: retry\n  Success --> [*]\n");
    assert_eq!(
        output,
        "              ╭──────╮   submit    ╭─────────╮   200 OK    ╭─────────╮\n●────────────▶│ Idle ├────────────▶│ Loading ├────────────▶│ Success ├────────────▶◎\n              ╰──────╯             ╰──┬──────╯             ╰─────────╯\n                                      │   ▲\n                             timeout  │   │\n                                      ▼   │  retry\n                                    ╭─────┴─╮\n                                    │ Error │\n                                    ╰───────╯"
    );
}

#[test]
fn captures_converging_labeled_branches_with_long_state_names() {
    let output = draw("stateDiagram-v2\n  [*] --> Waiting\n  state \"Waiting for surface and workspace\" as Waiting\n  state \"Surface bound only\" as Surface\n  state \"Workspace bound only\" as Workspace\n  state \"Ready with queued input\" as Ready\n  state \"Agent activity requested\" as Active\n  Waiting --> Surface: InteractionSurfaceBound\n  Waiting --> Workspace: WorkspaceBound\n  Surface --> Ready: WorkspaceBound\n  Workspace --> Ready: InteractionSurfaceBound\n  Ready --> Active: AgentActivityRequested");
    assert_eq!(
        output,
        "              ╭───────────────────────────────────╮ InteractionSurfaceBound ╭────────────────────╮ WorkspaceBound ╭─────────────────────────╮ AgentActivityRequested ╭──────────────────────────╮\n●────────────▶│ Waiting for surface and workspace ├────────────────────────▶│ Surface bound only ├───────────────▶│ Ready with queued input ├───────────────────────▶│ Agent activity requested │\n              ╰───────────────┬───────────────────╯                         ╰────────────────────╯                ╰─────────────────────────╯                        ╰──────────────────────────╯\n                              │ WorkspaceBound                                                                                   ▲\n                              ╰──────────────────────────────────────────────────────╮                                           │\n                                                                                     │           InteractionSurfaceBound         │\n                                                                                     ▼   ╭───────────────────────────────────────╯\n                                                                           ╭─────────────┴────────╮\n                                                                           │ Workspace bound only │\n                                                                           ╰──────────────────────╯"
    );
}

#[test]
fn keeps_raised_note_connectors_off_outgoing_transitions() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  Loading --> Success: 200 OK\n  Loading --> Error: timeout\n  note right of Loading : waiting for response\n  Error --> Loading: retry\n  Success --> [*]\n");
    assert_eq!(
        output,
        "                                                  ╔══════════════════════╗\n                                              ╔═══╣ waiting for response ║\n                                              ║   ╚══════════════════════╝\n                                              ║\n                                              ║\n              ╭──────╮   submit    ╭─────────╮   200 OK    ╭─────────╮\n●────────────▶│ Idle ├────────────▶│ Loading ├────────────▶│ Success ├────────────▶◎\n              ╰──────╯             ╰──┬──────╯             ╰─────────╯\n                                      │   ▲\n                             timeout  │   │\n                                      ▼   │  retry\n                                    ╭─────┴─╮\n                                    │ Error │\n                                    ╰───────╯"
    );
}

#[test]
fn places_a_composite_note_above_an_occupied_right_side() {
    let output = draw("stateDiagram-v2\n  [*] --> Processing\n\n  state Processing {\n    [*] --> Validate\n    Validate --> Decision\n    state Decision <<choice>>\n    Decision --> Accepted: valid\n    Decision --> Rejected: invalid\n    Accepted --> [*]\n    Rejected --> [*]\n  }\n\n  note right of Processing\n    Validation and routing happen\n    inside this composite state.\n  end note\n\n  Processing --> Complete\n  Complete --> [*]");
    let lines: Vec<&str> = output.split('\n').collect();
    let note_bottom = lines
        .iter()
        .position(|line| line.contains("╚═══════════════════════════════╝"))
        .expect("note bottom");
    let complete_row = lines.iter().position(|line| line.contains(" Complete ")).expect("complete");

    assert!(note_bottom < complete_row);
    assert!(lines[note_bottom + 1..complete_row].iter().all(|line| !line.contains("Complete")));
    assert_eq!(lines.len(), 20);
}

#[test]
fn renders_configurable_line_arrowheads() {
    let output = draw_with(
        "\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n",
        &StateDiagramRenderOptions {
            arrow_head_style: Some(crate::render::mermaid::core::drawing::ArrowHeadStyle::Line),
            ..Default::default()
        },
    );
    assert!(output.contains('→'));
    assert!(!output.contains('▶'));
}

#[test]
fn renders_self_transitions_and_choice_branches() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  state Decision <<choice>>\n  [*] --> Editing\n  Editing --> Editing: type\n  Editing --> Decision: submit\n  Decision --> Saved: ok\n  Decision --> Error: fail\n  Error --> Editing: retry\n");
    assert_eq!(
        output,
        "              ╭─────────╮   submit          ok      ╭───────╮\n●────────────▶│ Editing ├────────────▶◆────────────▶│ Saved │\n              ╰──┬──────╯             │             ╰───────╯\n               ▲ │    ▲ type          │ fail\n               │ ╰────╯               │\n               │                      ▼\n               │                  ╭───────╮\n               │                  │ Error │\n               │                  ╰───┬───╯\n               │                      │\n               │                      │\n               │        retry         │\n               ╰──────────────────────╯"
    );
}

#[test]
fn connects_lower_routed_branches_into_choice_junctions() {
    let output = draw("stateDiagram-v2\ndirection LR\nstate Decision <<choice>>\n[*] --> Fork\nFork --> Upper\nFork --> Lower\nUpper --> Decision\nLower --> Decision\nDecision --> Done\nDone --> [*]");
    assert!(output.contains("Upper ├────────────▶◆────────────▶│ Done"));
}

#[test]
fn renders_self_transitions_as_loops_in_vertical_diagrams() {
    let output = draw("stateDiagram-v2\ndirection TB\nWorking --> Working: retry");
    assert_eq!(output, "╭─────────╮\n│ Working │\n╰──┬──────╯\n   │    ▲ retry\n   ╰────╯");
}

#[test]
fn renders_self_transitions_from_choice_pseudo_states() {
    let output = draw("stateDiagram-v2\ndirection LR\nstate Decision <<choice>>\nDecision --> Decision: reconsider");
    assert!(output.contains('◆'));
    assert!(output.contains("reconsider"));
    assert_eq!(output.split('\n').filter(|line| !line.trim().is_empty()).count(), 3);
}

#[test]
fn trims_leading_rows_from_standalone_choices() {
    for direction in ["LR", "RL"] {
        let output =
            draw(&format!("stateDiagram-v2\ndirection {direction}\nstate Decision <<choice>>"));
        assert_eq!(output, "◆", "{direction}");
    }
}

#[test]
fn renders_parallel_transitions_without_losing_labels() {
    let horizontal = draw("stateDiagram-v2\ndirection LR\nA --> B: first\nA --> B: second");
    let vertical = draw("stateDiagram-v2\ndirection TB\nA --> B: first\nA --> B: second");
    for output in [&horizontal, &vertical] {
        assert!(output.contains("first"));
        assert!(output.contains("second"));
    }
}

fn cyclic_chain(
    prefix: &str,
    node_count: usize,
    edges: &[(&str, &str, &str)],
    notes: &[(&str, &str, &str)],
) -> String {
    let mut lines = vec!["stateDiagram-v2".to_string(), "  direction TD".to_string()];
    for index in 0..node_count {
        lines.push(format!("  state \"Node {prefix}_{index}\" as {prefix}_{index}"));
    }
    for (from, to, label) in edges {
        lines.push(format!("  {prefix}_{from} --> {prefix}_{to}: {label}"));
    }
    for (side, target, note) in notes {
        let position = if *side == "left" { "left" } else { "right" };
        lines.push(format!("  note {position} of {prefix}_{target}: {note}"));
    }
    lines.join("\n")
}

#[test]
fn renders_cyclic_same_rank_vertical_parallels_with_a_note() {
    let output = draw(&cyclic_chain(
        "S13",
        5,
        &[
            ("0", "1", "state_edge_13_0"),
            ("1", "2", "state_edge_13_1"),
            ("2", "3", "state_edge_13_2"),
            ("3", "4", "state_edge_13_3"),
            ("0", "2", "state_edge_13_4"),
            ("4", "1", "state_edge_13_5"),
            ("2", "0", "state_edge_13_6"),
            ("1", "2", "state_edge_13_7"),
            ("3", "2", "state_edge_13_8"),
            ("3", "0", "state_edge_13_9"),
            ("0", "0", "state_edge_13_10"),
        ],
        &[("left", "0", "state_note_13_0")],
    ));
    for index in 0..5 {
        assert!(output.contains(&format!("Node S13_{index}")));
    }
    for index in 0..=10 {
        assert!(output.contains(&format!("state_edge_13_{index}")));
    }
    assert!(output.contains("state_note_13_0"));
}

#[test]
fn preserves_every_state_in_a_routed_multi_note_vertical_graph() {
    let source = cyclic_chain(
        "S21",
        6,
        &[
            ("0", "1", "state_edge_21_0"),
            ("1", "2", "state_edge_21_1"),
            ("2", "3", "state_edge_21_2"),
            ("3", "4", "state_edge_21_3"),
            ("4", "5", "state_edge_21_4"),
            ("4", "5", "state_edge_21_5"),
            ("2", "4", "state_edge_21_6"),
            ("3", "1", "state_edge_21_7"),
        ],
        &[
            ("right", "1", "state_note_21_0"),
            ("left", "0", "state_note_21_1"),
            ("left", "4", "state_note_21_2"),
        ],
    );
    let output = draw(&source);
    for index in 0..6 {
        assert!(output.contains(&format!("Node S21_{index}")));
    }
    for index in 0..=7 {
        assert!(output.contains(&format!("state_edge_21_{index}")));
    }
    for index in 0..=2 {
        assert!(output.contains(&format!("state_note_21_{index}")));
    }
    expect_complete_state_diagram(&source, &output);
}

#[test]
fn renders_every_note_in_a_cyclic_vertical_graph() {
    let source = format!(
        "stateDiagram-v2\n  direction TB\n{}\n{}\n{}",
        (0..6)
            .map(|i| format!("  state \"Node S27_{i}\" as S27_{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        [
            ("S27_0", "S27_1", "state_edge_27_0"),
            ("S27_1", "S27_2", "state_edge_27_1"),
            ("S27_2", "S27_3", "state_edge_27_2"),
            ("S27_3", "S27_4", "state_edge_27_3"),
            ("S27_4", "S27_5", "state_edge_27_4"),
            ("S27_1", "S27_0", "state_edge_27_5"),
            ("S27_5", "S27_1", "state_edge_27_6"),
            ("S27_2", "S27_1", "state_edge_27_7"),
            ("S27_1", "S27_5", "state_edge_27_8"),
        ]
        .iter()
        .map(|(from, to, label)| format!("  {from} --> {to}: {label}"))
        .collect::<Vec<_>>()
        .join("\n"),
        [
            ("left", "S27_5", "state_note_27_0"),
            ("right", "S27_0", "state_note_27_1"),
            ("left", "S27_3", "state_note_27_2"),
        ]
        .iter()
        .map(|(side, target, note)| format!("  note {side} of {target}: {note}"))
        .collect::<Vec<_>>()
        .join("\n")
    );
    let output = draw(&source);
    for index in 0..6 {
        assert!(output.contains(&format!("Node S27_{index}")));
    }
    for index in 0..=8 {
        assert!(output.contains(&format!("state_edge_27_{index}")));
    }
    for index in 0..=2 {
        assert!(output.contains(&format!("state_note_27_{index}")));
    }
    expect_complete_state_diagram(&source, &output);
}

#[test]
fn places_exhausted_notes_on_deterministic_exterior_lanes() {
    let source = cyclic_chain(
        "S33",
        7,
        &[
            ("0", "1", "state_edge_33_0"),
            ("1", "2", "state_edge_33_1"),
            ("2", "3", "state_edge_33_2"),
            ("3", "4", "state_edge_33_3"),
            ("4", "5", "state_edge_33_4"),
            ("5", "6", "state_edge_33_5"),
            ("6", "4", "state_edge_33_6"),
            ("2", "4", "state_edge_33_7"),
            ("3", "3", "state_edge_33_8"),
        ],
        &[("left", "2", "state_note_33_0"), ("left", "6", "state_note_33_1")],
    );
    let output = draw(&source);
    let diagram =
        prepare_visible_state_diagram(&parse_mermaid_state_diagram(&source).expect("parses"));
    let layout = create_state_diagram_layout(
        &diagram,
        &StateDiagramLayoutOptions {
            min_state_gap: 5,
            search_budget: Some(StateSearchBudget { remaining: 0 }),
        },
    )
    .expect("layout");

    for index in 0..7 {
        assert!(output.contains(&format!("Node S33_{index}")));
    }
    for index in 0..=8 {
        assert!(output.contains(&format!("state_edge_33_{index}")));
    }
    for index in 0..=1 {
        assert!(output.contains(&format!("state_note_33_{index}")));
    }
    assert_eq!(layout.note_bounds.len(), 2);
    expect_complete_state_diagram(&source, &output);
}

#[test]
fn separates_reciprocal_rl_branches_sharing_a_parallel_lane() {
    let source = format!(
        "stateDiagram-v2\n  direction RL\n{}\n{}\n{}",
        (0..6)
            .map(|i| format!("  state \"Node S237_{i}\" as S237_{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
        [
            ("S237_0", "S237_1", "state_edge_237_0"),
            ("S237_1", "S237_2", "state_edge_237_1"),
            ("S237_2", "S237_3", "state_edge_237_2"),
            ("S237_3", "S237_4", "state_edge_237_3"),
            ("S237_4", "S237_5", "state_edge_237_4"),
            ("S237_1", "S237_5", "state_edge_237_5"),
            ("S237_4", "S237_5", "state_edge_237_6"),
            ("S237_4", "S237_0", "state_edge_237_7"),
            ("S237_0", "S237_4", "state_edge_237_8"),
        ]
        .iter()
        .map(|(from, to, label)| format!("  {from} --> {to}: {label}"))
        .collect::<Vec<_>>()
        .join("\n"),
        [
            ("right", "S237_1", "state_note_237_0"),
            ("left", "S237_5", "state_note_237_1"),
            ("right", "S237_2", "state_note_237_2"),
        ]
        .iter()
        .map(|(side, target, note)| format!("  note {side} of {target}: {note}"))
        .collect::<Vec<_>>()
        .join("\n")
    );
    let output = draw(&source);
    for index in 0..6 {
        assert!(output.contains(&format!("Node S237_{index}")));
    }
    for index in 0..=8 {
        assert!(output.contains(&format!("state_edge_237_{index}")));
    }
    for index in 0..=2 {
        assert!(output.contains(&format!("state_note_237_{index}")));
    }
    expect_complete_state_diagram(&source, &output);
}

#[test]
fn separates_labels_on_four_parallel_vertical_transitions() {
    let output = draw(
        "stateDiagram-v2\ndirection TB\nA --> B: one\nA --> B: two\nA --> B: three\nA --> B: four",
    );
    assert!(!output.contains("twothree"));
    for label in ["one", "two", "three", "four"] {
        assert_eq!(count_occurrences(&output, label), 1, "{label}");
    }
}

#[test]
fn grows_parallel_vertical_diagrams_by_the_maximum_label_width() {
    let render = |labels: &[&str]| -> String {
        let transitions: Vec<String> =
            labels.iter().map(|label| format!("  A --> B: {label}")).collect();
        draw(&format!("stateDiagram-v2\n  direction TB\n{}", transitions.join("\n")))
    };
    let short_labels = ["one", "two", "three"];
    let long_labels = [
        "alpha route label that is deliberately long",
        "beta route label that is deliberately long",
        "gamma route label that is deliberately long",
    ];
    let label_growth = long_labels.iter().map(|l| line_width(l)).max().unwrap()
        - short_labels.iter().map(|l| line_width(l)).max().unwrap();
    let growth = rendered_width(&render(&long_labels)) - rendered_width(&render(&short_labels));
    assert!(growth <= label_growth + 2);
}

#[test]
fn keeps_audited_parallel_labels_clear_of_frames_and_rails() {
    let output = draw("stateDiagram-v2\ndirection TB\nA --> B: alpha route label that is deliberately long\nA --> B: beta route label that is deliberately long\nA --> B: gamma route label that is deliberately long");
    assert_eq!(
        output,
        "╭───╮ alpha route label that is deliberately long\n│ A ├───┬──╮\n╰─┬─╯   │  │ gamma route label that is deliberately long\n  │     │  │\n  │     │  │ beta route label that is deliberately long\n  │     │  │\n  ▼     │  │\n╭───╮   │  │\n│ B │◀──┴──╯\n╰───╯"
    );
}

#[test]
fn keeps_audited_repeated_self_transition_lanes_distinct_and_readable() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> A: one\nA --> A: two\nA --> A: three");
    assert_eq!(
        output,
        "╭───╮\n│ A │\n╰─┬─╯\n  │  ▲   ▲   ▲\n  ├──╯   │   │\n  │      │   │\n  │ one  │   │\n  ├──────╯   │\n  │          │\n  │     two  │ three\n  ╰──────────╯"
    );
}

#[test]
fn expands_composites_around_self_transition_labels() {
    let source = "stateDiagram-v2\ndirection LR\nstate Outer {\n  A --> A: loop-0\n  A --> A: loop-1\n  A --> A: loop-2\n}\nA --> C";
    let diagram = parse_mermaid_state_diagram(source).expect("parses");
    let drawing = create_state_diagram_drawing(&diagram, &StateDiagramRenderOptions::default())
        .expect("draws");
    let outer_slot = *drawing.layout.composite_bounds.get("Outer").unwrap();
    let outer = &drawing.layout.boxes[outer_slot];
    let external = drawing.layout.box_of("C").unwrap();
    let output = drawing.grid.to_text(crate::render::mermaid::core::canvas::TextOptions {
        trim_top: true,
        trim_bottom: true,
    });

    let overlaps = external.left < outer.left + outer.width
        && external.left + external.width > outer.left
        && external.top < outer.top + outer.height
        && external.top + external.height > outer.top;
    assert!(!overlaps);
    for plan in &drawing.transition_plans {
        let transition = &drawing.diagram.transitions[plan.route.transition_index];
        if transition.from != transition.to {
            continue;
        }
        let label = plan.label.as_ref().expect("self label");
        assert!(label.x > outer.left);
        assert!(label.y > outer.top);
        let width = label.lines.iter().map(|line| line_width(line)).max().unwrap();
        assert!(label.x + width < outer.left + outer.width);
        assert!(label.y + (label.lines.len() as i64) < outer.top + outer.height);
    }
    for label in ["loop-0", "loop-1", "loop-2"] {
        assert_eq!(count_occurrences(&output, label), 1);
    }
}

#[test]
fn keeps_audited_nested_note_connectors_direct_and_inside_every_frame() {
    let output = draw("stateDiagram-v2\ndirection TB\nstate Outer {\n  state Inner {\n    A --> B: down\n    B --> A: up\n    note right of B: note\n  }\n}");
    assert_eq!(
        output,
        "╭─ Outer ───────────────╮\n│                       │\n│ ╭─ Inner ───────────╮ │\n│ │                   │ │\n│ │ ╭───╮             │ │\n│ │▶│ A │             │ │\n│ ││╰─┬─╯             │ │\n│ ││  │               │ │\n│ ││  │ down          │ │\n│ ││  │               │ │\n│ ││  ▼               │ │\n│ ││╭───╮    ╔══════╗ │ │\n│ │╰┤ B │════╣ note ║ │ │\n│ │ ╰───╯    ╚══════╝ │ │\n│ │up                 │ │\n│ ╰───────────────────╯ │\n│                       │\n╰───────────────────────╯"
    );
}

#[test]
fn leaves_frame_gaps_where_external_transitions_cross_nested_boundaries() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\ndirection LR\nstate Session {\n  [*] --> Open\n  state Open {\n    [*] --> Clean\n  }\n}\n[*] --> Session: hydrate",
    )
    .expect("parses");
    let drawing = create_state_diagram_drawing(&diagram, &StateDiagramRenderOptions::default())
        .expect("draws");
    let hydrate = drawing
        .transition_plans
        .iter()
        .find(|plan| drawing.diagram.transitions[plan.route.transition_index].label == "hydrate")
        .expect("hydrate plan");
    let crossings: Vec<_> = hydrate
        .cells
        .iter()
        .filter(|cell| {
            drawing.layout.composite_bounds.values().any(|&slot| {
                let bounds = &drawing.layout.boxes[slot];
                let right = bounds.left + bounds.width - 1;
                let bottom = bounds.top + bounds.height - 1;
                ((cell.x == bounds.left || cell.x == right)
                    && cell.y >= bounds.top
                    && cell.y <= bottom)
                    || ((cell.y == bounds.top || cell.y == bottom)
                        && cell.x >= bounds.left
                        && cell.x <= right)
            })
        })
        .collect();

    assert_eq!(crossings.len(), 2);
    for cell in crossings {
        assert_eq!(drawing.grid.get_cell(cell.x, cell.y).unwrap().text(), "─");
    }
}

#[test]
fn keeps_explicit_choices_visible_in_choice_only_cycles() {
    let output = draw("stateDiagram-v2\ndirection TB\nstate One <<choice>>\nstate Two <<choice>>\nstate Three <<choice>>\nOne --> Two: clockwise\nTwo --> Three: clockwise\nThree --> One: clockwise");
    assert_eq!(count_occurrences(&output, "◆"), 3);
}

#[test]
fn routes_dense_horizontal_transitions_around_unrelated_states() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B: ab\nA --> C: ac\nA --> D: ad\nB --> A: ba\nB --> C: bc\nB --> D: bd\nC --> A: ca\nC --> B: cb\nC --> D: cd\nD --> A: da\nD --> B: db\nD --> C: dc");
    for state in ["A", "B", "C", "D"] {
        assert_eq!(count_occurrences(&output, state), 1, "{state}");
    }
}

#[test]
fn keeps_dense_vertical_fan_routes_out_of_sibling_states() {
    let output = draw("stateDiagram-v2\ndirection TB\nstate \"Alpha\" as A\nstate \"Beta\" as B\nstate \"Gamma store\" as C\nstate \"Delta notifier\" as D\nA --> B\nA --> C\nA --> D\nB --> A: back\nB --> D: across");
    for text in ["Alpha", "Beta", "Gamma store", "Delta notifier", "back", "across"] {
        assert!(output.contains(text));
    }
}

#[test]
fn routes_parallel_transitions_around_vertically_offset_states() {
    let output = draw("stateDiagram-v2\nA --> B: first<br/>line two\nA --> B: second<br/>another line\nB --> A: return<br/>with details");
    assert!(output.contains(" A "));
    assert!(output.contains("│ B │"));
    assert!(output.contains("first"));
    assert!(output.contains("second"));
    assert!(output.contains("return"));
}

#[test]
fn keeps_independent_overlapping_feedback_labels_and_paths_distinct() {
    let content = |direction: &str| {
        format!("stateDiagram-v2\n  direction {direction}\n  A --> B: advance\n  B --> C: continue\n  C --> D: finish\n  C --> A: reset A\n  D --> B: reset B")
    };
    for direction in ["LR", "RL"] {
        let output = draw(&content(direction));
        assert!(output.contains("reset A"), "{direction}");
        assert!(output.contains("reset B"), "{direction}");
        assert!(!output.contains("res│t"), "{direction}");
    }
}

#[test]
fn keeps_independent_internal_feedback_paths_inside_their_composite_frame() {
    let output = draw("stateDiagram-v2\ndirection LR\nstate Runtime {\n  A --> B: advance\n  B --> C: continue\n  C --> D: finish\n  C --> A: reset A\n  D --> B: reset B\n}");
    let lines: Vec<&str> = output.split('\n').collect();
    let frame_top = lines.iter().position(|line| line.contains("Runtime")).unwrap();
    let upper_feedback = lines.iter().position(|line| line.contains("reset B")).unwrap();
    assert!(upper_feedback > frame_top);
    assert!(output.contains("reset A"));
    assert!(!output.contains("res│t"));
}

#[test]
fn places_notes_away_from_independent_feedback_corridors() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B: advance\nB --> C: continue\nC --> D: finish\nC --> A: reset A\nD --> B: reset B\nnote right of B : note beside B");
    assert!(output.contains("note beside B"));
    assert!(output.contains("reset B"));
    assert!(!output.contains("╭─║"));
    assert!(!output.contains("║──"));
}

#[test]
fn places_notes_away_from_transition_routes_and_labels() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B: proceed only after validation\nnote left of B: blocking note");
    assert!(output.contains("proceed only after validation"));
    assert!(output.contains("blocking note"));
    assert_eq!(count_occurrences(&output, " A ") + count_occurrences(&output, " B "), 2);
    assert!(output.contains('╠') || output.contains('╣'));
}

#[test]
fn keeps_adjacent_note_connectors_out_of_other_notes() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B: advance\nB --> C: continue\nnote right of B: first note\nnote right of B: second note\nnote left of C: left note");
    for text in ["advance", "continue", "first note", "second note", "left note"] {
        assert!(output.contains(text));
    }
    assert!(!output.contains("╝═════╗"));
}

#[test]
fn keeps_note_connectors_out_of_vertical_loop_and_transition_labels() {
    let output = draw("stateDiagram-v2\ndirection TB\nstate \"Processing Ω<br/>phase two\" as Processing\nstate \"Result ≥ threshold\" as Result\nReady --> Processing: first\nReady --> Processing: duplicate\nProcessing --> Ready: restore\nProcessing --> Processing: heartbeat\nProcessing --> Result: result ≥ 1\nResult --> Ready: reopen\nnote right of Processing: Unicode note 界");
    for text in ["heartbeat", "result ≥ 1", "Unicode note 界"] {
        assert!(output.contains(text));
    }
    assert!(!output.contains("hea║tbeat"));
    assert!(!output.contains("result║≥ 1"));
}

#[test]
fn keeps_nested_state_labels_clear_of_note_connectors() {
    let output = draw("stateDiagram-v2\ndirection LR\nstate \"Open document\" as Open {\n  [*] --> Clean: load\n  Clean --> Dirty: edit\n  Dirty --> Clean: save\n}\nnote right of Dirty: unsaved changes");
    for text in ["Open document", "Clean", "Dirty", "load", "edit", "save", "unsaved changes"] {
        assert!(output.contains(text));
    }
}

#[test]
fn keeps_duplicate_feedback_labels_away_from_an_independent_return_path() {
    let output = draw("stateDiagram-v2\ndirection LR\nA --> B\nB --> C\nC --> D\nC --> A: ca\nD --> B: db1\nD --> B: db2");
    assert!(output.contains("ca"));
    assert!(output.contains("db1"));
    assert!(output.contains("db2"));
    assert!(!output.contains("c│"));
}

#[test]
fn renders_composite_state_containers() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  state Authenticated {\n    [*] --> Idle\n    Idle --> Editing: open\n    Editing --> [*]: save\n  }\n");
    assert_eq!(
        output,
        "╭─ Authenticated ──────────────────────────────────────────────╮\n│                                                              │\n│               ╭──────╮    open     ╭─────────╮    save       │\n│ ─────────────▶│ Idle ├────────────▶│ Editing ├────────────── │\n│               ╰──────╯             ╰─────────╯               │\n│                                                              │\n╰──────────────────────────────────────────────────────────────╯"
    );
}

#[test]
fn routes_transitions_through_scoped_markers() {
    let content = "\nstateDiagram-v2\n  direction LR\n  [*] --> Authenticated: login\n  state Authenticated {\n    [*] --> Idle\n    Idle --> Editing: open\n    Editing --> [*]: save\n  }\n  Authenticated --> [*]: logout\n";
    let diagram = parse_mermaid_state_diagram(content).expect("parses");
    let output = draw(content);
    let has = |from: &str, to: &str, label: &str| {
        diagram.transitions.iter().any(|transition| {
            transition.from == from && transition.to == to && transition.label == label
        })
    };
    assert!(has("__start", "Authenticated.__start", "login"));
    assert!(has("Authenticated.__end", "__end", "logout"));
    assert_eq!(
        output,
        "            ╭─ Authenticated ──────────────────╮\n            │                                  │ save\n    login   │ ╭──────╮    open     ╭─────────╮ │ logout\n●────────────▶│ Idle ├────────────▶│ Editing ├────────────▶◎\n            │ ╰──────╯             ╰─────────╯ │\n            │                                  │\n            ╰──────────────────────────────────╯"
    );
}

#[test]
fn keeps_nested_composite_entry_and_exit_routes_within_the_outer_frame_height() {
    let output = draw("stateDiagram-v2\nstate Session {\n  [*] --> Open\n  state Open {\n    [*] --> Clean\n    Clean --> Dirty: edit\n    Dirty --> Clean: save\n  }\n  note right of Open: document lifecycle\n  Open --> [*]: close\n}\n[*] --> Session\n  Session --> [*]");
    let lines: Vec<&str> = output.split('\n').collect();
    let outer_frame_top = lines.iter().find(|line| line.contains("Session")).unwrap();
    let frame_left = outer_frame_top.chars().position(|c| c == '╭').unwrap();
    let frame_right =
        outer_frame_top.chars().collect::<Vec<char>>().iter().rposition(|&c| c == '╮').unwrap();
    let outer_frame_bottom = lines
        .iter()
        .position(|line| {
            let chars: Vec<char> = line.chars().collect();
            chars.len() > frame_right && chars[frame_left] == '╰' && chars[frame_right] == '╯'
        })
        .unwrap_or(0);
    let start_column = lines
        .iter()
        .find(|line| line.contains('●'))
        .unwrap()
        .chars()
        .position(|c| c == '●')
        .unwrap();
    let end_column = lines
        .iter()
        .find(|line| line.contains('◎'))
        .unwrap()
        .chars()
        .position(|c| c == '◎')
        .unwrap();

    assert!(outer_frame_bottom > 0);
    assert!(start_column < frame_left);
    assert!(end_column > frame_right);
    assert!(lines[outer_frame_bottom + 1..].iter().all(|line| line.trim().is_empty()));
    assert!(output.contains("Open"));
    assert!(output.contains("document lifecycle"));
    assert!(output.contains("close"));
}

#[test]
fn renders_notes_attached_to_states() {
    let output = draw("\nstateDiagram-v2\n  direction LR\n  [*] --> Idle\n  Idle --> Loading: submit\n  note right of Loading : waits for response\n");
    assert_eq!(
        output,
        "              ╭──────╮   submit    ╭─────────╮    ╔════════════════════╗\n●────────────▶│ Idle ├────────────▶│ Loading │════╣ waits for response ║\n              ╰──────╯             ╰─────────╯    ╚════════════════════╝"
    );
}

#[test]
fn keeps_composite_to_choice_elbows_connected() {
    let output = draw("stateDiagram-v2\n  [*] --> Authenticated\n  state Authenticated {\n    [*] --> Editing\n    Editing --> Ready\n  }\n  note right of Editing\n    Draft changes\n  end note\n  Authenticated --> Decision\n  state Decision <<choice>>\n  Decision --> [*]");
    assert!(output.contains("╰─────────────▼"));
    assert!(output.contains("◆────────────▶◎"));
}

#[test]
fn keeps_vertical_branch_labels_from_overwriting_state_labels() {
    let output = draw("stateDiagram-v2\ndirection TB\nstate \"Branch root\" as Root\nstate \"Upper branch\" as Upper\nstate \"Lower branch\" as Lower\nstate \"Merged branch\" as Merge\nRoot --> Upper: branch-up\nRoot --> Lower: branch-down\nUpper --> Merge: merge-up\nLower --> Merge: merge-down\nMerge --> Root: branch-feedback");
    for text in [
        "Branch root",
        "Upper branch",
        "Lower branch",
        "Merged branch",
        "branch-up",
        "branch-down",
        "merge-up",
        "merge-down",
        "branch-feedback",
    ] {
        assert!(output.contains(text));
    }
}

#[test]
fn routes_vertical_branch_feedback_around_sibling_state_bodies() {
    let output = draw("stateDiagram-v2\ndirection TB\n[*] --> Root\nRoot --> A\nRoot --> B\nA --> Merge\nB --> Merge\nMerge --> A: retry");
    for state in ["Root", "A", "B", "Merge"] {
        assert_eq!(count_occurrences(&output, state), 1, "{state}");
    }
    assert!(output.contains("│ B │"));
    assert!(output.contains("retry"));
}

#[test]
fn keeps_lifecycle_states_intact_around_branches_and_feedback() {
    let output = draw("stateDiagram-v2\n[*] --> Idle\nIdle --> MailboxPending: enqueue + setAlarm\nMailboxPending --> PromptSubmitted: drain mailbox\nPromptSubmitted --> Polling: prompt admitted\nPolling --> Polling: execution still active\nPolling --> Completed: terminal log event\nPolling --> Polling: retry after transient failure\nCompleted --> Idle: final Slack projection\nIdle --> Expired: 30 days inactive\nExpired --> [*]: delete SQLite state");
    for state in ["Idle", "MailboxPending", "PromptSubmitted", "Polling", "Completed", "Expired"] {
        assert_eq!(count_occurrences(&output, state), 1, "{state}");
    }
}

#[test]
fn keeps_composite_titles_intact_under_reciprocal_composite_routes() {
    let source = "stateDiagram-v2\n  direction LR\n  state FirstGroup {\n    [*] --> FirstInner\n    FirstInner --> [*]: first-out\n  }\n  state SecondGroup {\n    [*] --> SecondInner\n    SecondInner --> [*]: second-out\n  }\n  FirstGroup --> SecondGroup: group-next\n    SecondGroup --> FirstGroup: group-back";
    for direction in [StateDirection::Lr, StateDirection::Tb] {
        let diagram = parse_mermaid_state_diagram(source).expect("parses");
        let drawing = create_state_diagram_drawing(
            &diagram,
            &StateDiagramRenderOptions { direction: Some(direction), ..Default::default() },
        )
        .expect("draws");
        let output = drawing.grid.to_text(crate::render::mermaid::core::canvas::TextOptions {
            trim_top: true,
            trim_bottom: true,
        });
        for id in ["FirstGroup", "SecondGroup"] {
            let slot = *drawing.layout.composite_bounds.get(id).unwrap();
            let bounds = &drawing.layout.boxes[slot];
            let right = bounds.left + bounds.width - 1;
            let bottom = bounds.top + bounds.height - 1;
            let boundary_styles: Vec<_> = (0..bounds.height - 2)
                .flat_map(|offset| {
                    [
                        drawing.grid.get_cell(bounds.left, bounds.top + offset + 1).unwrap().style,
                        drawing.grid.get_cell(right, bounds.top + offset + 1).unwrap().style,
                    ]
                })
                .chain((0..bounds.width - 2).map(|offset| {
                    drawing.grid.get_cell(bounds.left + offset + 1, bottom).unwrap().style
                }))
                .collect::<Vec<_>>();

            assert_eq!(count_occurrences(&output, id), 1, "{id} {direction:?}");
            assert!(
                boundary_styles.iter().all(|style| matches!(
                    style,
                    Some(
                        crate::render::mermaid::state::types::StateCellStyle::Composite
                            | crate::render::mermaid::state::types::StateCellStyle::Transition
                            | crate::render::mermaid::state::types::StateCellStyle::StateDepartureRamp1
                            | crate::render::mermaid::state::types::StateCellStyle::StateDepartureRamp2
                            | crate::render::mermaid::state::types::StateCellStyle::StateDepartureRamp3
                    )
                )),
                "{id} {direction:?}"
            );
        }
    }
}

// ---- `visible-model.test.ts` ----

fn state(
    id: &str,
    kind: StateKind,
    parent_id: Option<&str>,
) -> crate::render::mermaid::state::types::StateDiagramState {
    crate::render::mermaid::state::types::StateDiagramState {
        id: id.to_string(),
        label: id.to_string(),
        kind,
        parent_id: parent_id.map(str::to_string),
    }
}

#[test]
fn collapses_composite_marker_transitions() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\n  [*] --> Authenticated: login\n  state Authenticated {\n    [*] --> Idle\n    Idle --> Editing: open\n    Editing --> [*]: save\n  }\n  Authenticated --> [*]: logout",
    )
    .expect("parses");

    let visible = prepare_visible_state_diagram(&diagram);
    let entry = visible.transitions.iter().find(|t| t.from == "__start").unwrap();
    let exit = visible.transitions.iter().find(|t| t.to == "__end").unwrap();

    assert!(!visible.states.iter().any(|state| state.id == "Authenticated.__start"));
    assert!(!visible.states.iter().any(|state| state.id == "Authenticated.__end"));
    assert!(entry.from == "__start" && entry.to == "Idle" && entry.label == "login");
    assert!(exit.from == "Editing" && exit.to == "__end" && exit.label == "save<br/>logout");
}

#[test]
fn collapses_nested_composite_entry_chains_without_retaining_scoped_markers() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\n  state Session {\n    [*] --> Open\n    state Open {\n      [*] --> Clean\n      Clean --> Dirty: edit\n      Dirty --> Clean: save\n    }\n    Open --> [*]: close\n  }\n  [*] --> Session\n  Session --> [*]",
    )
    .expect("parses");
    let visible = prepare_visible_state_diagram(&diagram);

    let ids: Vec<&str> = visible.states.iter().map(|state| state.id.as_str()).collect();
    assert_eq!(ids, vec!["Clean", "Dirty", "__start", "__end"]);
    assert!(visible
        .transitions
        .iter()
        .any(|t| t.from == "__start" && t.to == "Clean" && t.label.is_empty()));
    assert!(!visible.transitions.iter().any(|t| t.from.contains(".__start")));
    assert!(!visible.transitions.iter().any(|t| t.to.contains(".__start")));
}

#[test]
fn resolves_an_outgoing_composite_transition_through_its_concrete_exit_state() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\n  state Session {\n    [*] --> Open\n    state Open {\n      [*] --> Clean\n      Clean --> Dirty: edit\n      Dirty --> Clean: save\n      Dirty --> [*]\n    }\n    Open --> Closing: request close\n    Closing --> Open: cancel\n  }\n  [*] --> Session: hydrate",
    )
    .expect("parses");
    let visible = prepare_visible_state_diagram(&diagram);

    assert!(visible
        .transitions
        .iter()
        .any(|t| t.from == "Dirty" && t.to == "Closing" && t.label == "request close"));
    assert!(visible
        .transitions
        .iter()
        .any(|t| t.from == "Closing" && t.to == "Clean" && t.label == "cancel"));
    assert!(!visible.transitions.iter().any(|t| t.from == "Open"));
}

#[test]
fn preserves_labels_on_both_sides_of_collapsed_composite_markers() {
    let diagram = parse_mermaid_state_diagram(
        "stateDiagram-v2\n  [*] --> Session: open session\n  state Session {\n    [*] --> Ready: initialize\n    Ready --> [*]: finalize\n  }\n  Session --> [*]: close session",
    )
    .expect("parses");
    let visible = prepare_visible_state_diagram(&diagram);

    assert!(visible.transitions.iter().any(|t| t.from == "__start"
        && t.to == "Ready"
        && t.label == "open session<br/>initialize"));
    assert!(visible
        .transitions
        .iter()
        .any(|t| t.from == "Ready" && t.to == "__end" && t.label == "finalize<br/>close session"));
}

#[test]
fn hidden_composite_marker_requires_parent_and_start_or_end_kind() {
    let hidden = state("X.__start", StateKind::Start, Some("X"));
    assert!(is_hidden_composite_marker(Some(&hidden)));
    let plain = state("X.__start", StateKind::Start, None);
    assert!(!is_hidden_composite_marker(Some(&plain)));
    let body = state("X.__start", StateKind::State, Some("X"));
    assert!(!is_hidden_composite_marker(Some(&body)));
    assert!(!is_hidden_composite_marker(None));
}

// ---- `style.test.ts` ----

#[test]
fn resolves_state_style_colors_with_fallbacks_and_ramps() {
    let colors = resolve_state_style_colors(StateColorInput {
        state: Some(crate::render::mermaid::core::color::Rgba::from_ints(1, 2, 3, 255)),
        ..Default::default()
    });
    assert_eq!(
        colors.color(Some(crate::render::mermaid::state::types::StateCellStyle::State)),
        Some(crate::render::mermaid::core::color::Rgba::from_ints(1, 2, 3, 255))
    );
    // stateBorder falls back to state.
    assert_eq!(
        colors.color(Some(crate::render::mermaid::state::types::StateCellStyle::StateBorder)),
        colors.color(Some(crate::render::mermaid::state::types::StateCellStyle::State))
    );
    // Defaults apply.
    assert_eq!(
        colors.color(Some(crate::render::mermaid::state::types::StateCellStyle::Composite)),
        Some(crate::render::mermaid::core::color::Rgba::from_ints(111, 138, 126, 255))
    );
    // Ramp styles resolve between noteConnector and noteBorder.
    assert_eq!(
        colors
            .color(Some(crate::render::mermaid::state::types::StateCellStyle::NoteConnectorRamp2)),
        crate::render::mermaid::core::color::blend_color(
            Some(crate::render::mermaid::core::color::Rgba::from_ints(141, 169, 155, 255)),
            Some(crate::render::mermaid::core::color::Rgba::from_ints(141, 169, 155, 255)),
            0.5
        )
    );
}

#[test]
fn unsupported_syntax_falls_back_to_an_error() {
    let error = parse_mermaid_state_diagram("stateDiagram-v2\n  nonsense line").unwrap_err();
    assert_eq!(error.line_number, 2);
    assert!(error.message.contains("Unsupported syntax"));
}
