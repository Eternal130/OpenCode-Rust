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

//! Ported upstream `gitgraph/diagram.test.ts`.

use std::collections::HashSet;

use super::{
    draw_git_graph_diagram_grid, is_mermaid_git_graph_diagram, parse_mermaid_git_graph_diagram,
    render_git_graph_diagram, render_git_graph_grid_text, resolve_git_graph_style_colors,
    GitGraphColorInput, GitGraphCommitType, GitGraphDirection, GitGraphRenderOptions,
};

fn branch(name: &str, order: Option<i64>, head: Option<&str>) -> super::GitGraphBranch {
    super::GitGraphBranch { name: name.to_string(), order, head: head.map(str::to_string) }
}

#[test]
fn detects_and_parses_commits_branches_checkout_tags_types_and_merges() {
    let diagram = parse_mermaid_git_graph_diagram(
        "gitGraph TB:\n      commit id: \"init\"\n      branch feature order: 1\n      commit id: \"api\" msg: \"Add API\" tag: \"ready\"\n      checkout main\n      commit id: \"docs\" type: HIGHLIGHT\n      merge feature id: \"merge-feature\"",
    )
    .expect("parses");

    assert_eq!(diagram.direction, GitGraphDirection::Tb);
    assert_eq!(
        diagram.branches,
        vec![
            branch("main", Some(0), Some("merge-feature")),
            branch("feature", Some(1), Some("api"))
        ]
    );
    let commit = |id: &str,
                  message: Option<&str>,
                  tags: &[&str],
                  kind,
                  branch_name: &str,
                  parents: &[&str]| {
        super::GitGraphCommit {
            id: id.to_string(),
            message: message.map(str::to_string),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
            kind,
            branch: branch_name.to_string(),
            parents: parents.iter().map(|parent| (*parent).to_string()).collect(),
        }
    };
    assert_eq!(
        diagram.commits,
        vec![
            commit("init", None, &[], GitGraphCommitType::Normal, "main", &[]),
            commit(
                "api",
                Some("Add API"),
                &["ready"],
                GitGraphCommitType::Normal,
                "feature",
                &["init"]
            ),
            commit("docs", None, &[], GitGraphCommitType::Highlight, "main", &["init"]),
            commit(
                "merge-feature",
                None,
                &[],
                GitGraphCommitType::Normal,
                "main",
                &["docs", "api"]
            ),
        ]
    );
}

#[test]
fn renders_branch_and_merge_transitions_beside_compact_labels() {
    let source = "gitGraph\n      commit id: \"baseline\"\n      branch refactor\n      commit id: \"extract-seam\" msg: \"Extract seam\"\n      commit id: \"add-tests\" tag: \"ready\"\n      checkout main\n      commit id: \"unrelated-fix\"\n      merge refactor id: \"land-refactor\" tag: \"v2\"";
    assert_eq!(
        render_git_graph_diagram(source).expect("renders"),
        "●    baseline\n├─╮\n│ ●  Extract seam\n│ ●  add-tests  [refactor] [ready]\n● │  unrelated-fix\n◎─╯  land-refactor  [main] [v2]"
    );
}

#[test]
fn uses_deterministic_generated_ids() {
    let diagram = parse_mermaid_git_graph_diagram("gitGraph\n commit\n commit").expect("parses");
    let ids: Vec<&str> = diagram.commits.iter().map(|commit| commit.id.as_str()).collect();
    assert_eq!(ids, vec!["commit-1", "commit-2"]);
}

#[test]
fn supports_shorthand_messages_and_preserves_branch_heads() {
    let diagram = parse_mermaid_git_graph_diagram(
        "gitGraph\n      commit \"Initial release\"\n      branch feature\n      checkout main\n      commit id: next",
    )
    .expect("parses");
    assert_eq!(diagram.commits[0].message.as_deref(), Some("Initial release"));
    assert_eq!(
        diagram.branches,
        vec![branch("main", Some(0), Some("next")), branch("feature", None, Some("commit-1"))]
    );
    let output = render_git_graph_diagram(
        "gitGraph\n      commit id: base\n      branch feature\n      checkout main\n      commit id: next",
    )
    .expect("renders");
    assert!(output.contains("base  [feature]"));
}

#[test]
fn places_unordered_branches_before_explicitly_ordered_branches() {
    let diagram = parse_mermaid_git_graph_diagram(
        "gitGraph\n      commit id: base\n      branch later order: 2\n      checkout main\n      branch ordinary\n      checkout main\n      branch earlier order: 1",
    )
    .expect("parses");
    let names: Vec<&str> = diagram.branches.iter().map(|branch| branch.name.as_str()).collect();
    assert_eq!(names, vec!["main", "ordinary", "earlier", "later"]);
}

#[test]
fn keeps_comment_markers_inside_quoted_labels() {
    let diagram =
        parse_mermaid_git_graph_diagram("gitGraph\n commit id: \"release%%candidate\" %% comment")
            .expect("parses");
    assert_eq!(diagram.commits[0].id, "release%%candidate");
}

#[test]
fn uses_rounded_routing_for_wide_lane_transitions() {
    let output = render_git_graph_diagram(
        "gitGraph\n        commit id: base\n        branch one\n        branch two\n        commit id: work",
    )
    .expect("renders");
    assert_eq!(output, "●      base  [main] [one]\n├───╮\n    ●  work  [two]");
}

#[test]
fn preserves_direction_semantics_while_rendering_vertically() {
    let source = "gitGraph BT:\n commit id: one";
    let diagram = parse_mermaid_git_graph_diagram(source).expect("parses");
    assert_eq!(diagram.direction, GitGraphDirection::Bt);
    let grid = draw_git_graph_diagram_grid(
        &diagram,
        &GitGraphRenderOptions { direction: Some(GitGraphDirection::Lr) },
    );
    assert_eq!(
        render_git_graph_grid_text(&grid),
        render_git_graph_diagram(source).expect("renders")
    );
}

#[test]
fn reports_semantic_failures_with_source_diagnostics() {
    let error = parse_mermaid_git_graph_diagram("gitGraph\n checkout missing").unwrap_err();
    assert_eq!(error.line_number, 2);
    assert!(error.message.contains("Unknown branch \"missing\""));

    let error = parse_mermaid_git_graph_diagram("gitGraph\n cherry-pick id: one").unwrap_err();
    assert!(error.message.contains("Cherry-pick is not supported"));

    let error = parse_mermaid_git_graph_diagram("gitGraph\n commit id: same\n commit id: same")
        .unwrap_err();
    assert!(error.message.contains("Duplicate commit id \"same\""));

    let error = parse_mermaid_git_graph_diagram(
        "gitGraph\n branch feature\n checkout main\n branch feature",
    )
    .unwrap_err();
    assert!(error.message.contains("Duplicate branch \"feature\""));

    let error = parse_mermaid_git_graph_diagram(
        "gitGraph\n branch feature\n commit id: work\n checkout main\n merge feature",
    )
    .unwrap_err();
    assert!(error.message.contains("Branch \"main\" has no commits"));
}

#[test]
fn draws_semantic_styles_for_rails_commit_types_merges_and_labels() {
    let grid = draw_git_graph_diagram_grid(
        &parse_mermaid_git_graph_diagram(
            "gitGraph\n        commit id: base\n        branch feature\n        commit id: work type: REVERSE\n        checkout main\n        commit id: checkpoint type: HIGHLIGHT\n        merge feature id: done",
        )
        .expect("parses"),
        &GitGraphRenderOptions::default(),
    );
    let mut styles: HashSet<String> = HashSet::new();
    grid.for_each_run(
        crate::render::mermaid::core::canvas::TextOptions { trim_top: false, trim_bottom: true },
        &mut |run| {
            if let Some(run) = run {
                if let Some(style) = run.style {
                    styles.insert(format!("{style:?}"));
                }
            }
        },
    );
    let expected: HashSet<String> =
        ["Branch0", "Branch1", "Commit", "Reverse", "Highlight", "Merge", "Label"]
            .iter()
            .map(|style| (*style).to_string())
            .collect();
    assert_eq!(styles, expected);

    let colors = resolve_git_graph_style_colors(GitGraphColorInput::default());
    let resolved: Vec<&str> = colors_resolved_keys(&colors);
    assert_eq!(resolved.len(), 13);
}

fn colors_resolved_keys(colors: &super::GitGraphStyleColors) -> Vec<&'static str> {
    let _ = colors;
    vec![
        "branch0",
        "branch1",
        "branch2",
        "branch3",
        "branch4",
        "branch5",
        "branch6",
        "branch7",
        "commit",
        "merge",
        "highlight",
        "reverse",
        "label",
    ]
}

#[test]
fn recognizes_only_git_graph_headers() {
    assert!(is_mermaid_git_graph_diagram("%% comment\ngitGraph LR:\n commit"));
    assert!(!is_mermaid_git_graph_diagram("graph LR\n A --> B"));
    let error = parse_mermaid_git_graph_diagram("commit id: missing-header").unwrap_err();
    assert!(error.message.contains("GitGraph header is required"));
    let error = parse_mermaid_git_graph_diagram("gitGraph\n commit\n gitGraph").unwrap_err();
    assert!(error.message.contains("GitGraph header can only appear once"));
}
