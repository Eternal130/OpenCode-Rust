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

//! `gitgraph/*` — the `gitGraph` commit/branch/merge ASCII renderer
//! (`packages/merman/src/gitgraph.ts` upstream).

use std::collections::HashMap;

use crate::render::mermaid::core::canvas::{DiagramCanvas, TextOptions};
use crate::render::mermaid::core::color::Rgba;
use crate::render::mermaid::core::mermaid::{
    first_meaningful_mermaid_line, meaningful_numbered_mermaid_lines, strip_mermaid_quotes,
};
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::diagnostics::{MermaidDiagramKind, MermaidSyntaxError};

/// `GitGraphDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GitGraphDirection {
    #[default]
    Lr,
    Tb,
    Bt,
}

/// `GitGraphCommitType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitGraphCommitType {
    Normal,
    Reverse,
    Highlight,
}

/// `GitGraphBranch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitGraphBranch {
    pub name: String,
    pub order: Option<i64>,
    pub head: Option<String>,
}

/// `GitGraphCommit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitGraphCommit {
    pub id: String,
    pub message: Option<String>,
    pub tags: Vec<String>,
    pub kind: GitGraphCommitType,
    pub branch: String,
    pub parents: Vec<String>,
}

/// `GitGraphDiagram`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitGraphDiagram {
    pub direction: GitGraphDirection,
    pub branches: Vec<GitGraphBranch>,
    pub commits: Vec<GitGraphCommit>,
}

/// `GitGraphDiagramRenderOptions` — parsed for Mermaid compatibility;
/// git graphs always use a vertical terminal layout.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitGraphRenderOptions {
    pub direction: Option<GitGraphDirection>,
}

/// `GitGraphCellStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GitGraphCellStyle {
    Branch0,
    Branch1,
    Branch2,
    Branch3,
    Branch4,
    Branch5,
    Branch6,
    Branch7,
    Commit,
    Merge,
    Highlight,
    Reverse,
    Label,
}

/// `GitGraphGrid`.
pub type GitGraphGrid = DiagramCanvas<GitGraphCellStyle>;

fn syntax_error(line_number: usize, source_line: &str, reason: &str) -> MermaidSyntaxError {
    MermaidSyntaxError::new(MermaidDiagramKind::GitGraph, line_number, source_line, reason)
}

/// `HEADER_RE = ^gitGraph(?:\s+(LR|TB|BT))?\s*:?$/i`.
fn parse_header(line: &str) -> Option<Option<GitGraphDirection>> {
    if !line.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("gitGraph")) {
        return None;
    }
    let tail = &line[8..];
    let trimmed = tail.trim_start();
    if trimmed.len() < tail.len() && !trimmed.is_empty() && !trimmed.starts_with(':') {
        // `\s+ DIR` — the direction token ends at whitespace or the colon.
        let token_end =
            trimmed.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(trimmed.len());
        let token = &trimmed[..token_end];
        let after = &trimmed[token_end..];
        let direction = match token.to_uppercase().as_str() {
            "LR" => GitGraphDirection::Lr,
            "TB" => GitGraphDirection::Tb,
            "BT" => GitGraphDirection::Bt,
            _ => return None,
        };
        let after = after.trim();
        if after.is_empty() || after == ":" {
            Some(Some(direction))
        } else {
            None
        }
    } else {
        let tail = tail.trim();
        if tail.is_empty() || tail == ":" {
            Some(None)
        } else {
            None
        }
    }
}

/// `isMermaidGitGraphDiagram`.
#[must_use]
pub fn is_mermaid_git_graph_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content).and_then(parse_header).is_some()
}

/// `stripComment` — `%%` outside quotes truncates the line.
fn strip_comment(value: &str) -> &str {
    let mut quote: Option<char> = None;
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    for &(index, char) in &chars {
        if (char == '"' || char == '\'') && (quote.is_none() || quote == Some(char)) {
            quote = if quote == Some(char) { None } else { Some(char) };
            continue;
        }
        if quote.is_none() && char == '%' {
            let next = chars.iter().find(|&&(position, _)| position > index).map(|&(_, c)| c);
            if next == Some('%') {
                return value[..index].trim();
            }
        }
    }
    value.trim()
}

/// `ACCESSIBILITY_RE = ^acc(?:Title|Descr)(?::|\s|$)/i`.
fn is_accessibility_line(line: &str) -> bool {
    for word in ["accTitle", "accDescr"] {
        if let Some(rest) = line
            .get(..word.len())
            .filter(|head| head.eq_ignore_ascii_case(word))
            .map(|_| &line[word.len()..])
        {
            return rest.is_empty()
                || rest.starts_with(':')
                || rest.starts_with(|c: char| c.is_whitespace());
        }
    }
    false
}

/// `^title(?:\s|$)/i`.
fn is_title_line(line: &str) -> bool {
    line.get(..5).is_some_and(|head| head.eq_ignore_ascii_case("title"))
        && line[5..].chars().next().is_none_or(char::is_whitespace)
}

/// `tokenize` — whitespace-separated with quote-spanning tokens.
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut quote: Option<char> = None;
    for char in line.chars() {
        if (char == '"' || char == '\'') && (quote.is_none() || quote == Some(char)) {
            quote = if quote == Some(char) { None } else { Some(char) };
            token.push(char);
            continue;
        }
        if char.is_whitespace() && quote.is_none() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            continue;
        }
        token.push(char);
    }
    if quote.is_some() {
        return vec![line.to_string()];
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

/// `parseAttributes` — `key: value` pairs with unknown-key errors.
fn parse_attributes(
    tokens: &[String],
    line_number: usize,
    line: &str,
    allowed: &[&str],
) -> Result<HashMap<String, Vec<String>>, MermaidSyntaxError> {
    let mut result: HashMap<String, Vec<String>> = HashMap::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let key_token = &tokens[index];
        let (key, inline) = match key_token.find(':') {
            Some(separator) => (key_token[..separator].to_lowercase(), &key_token[separator + 1..]),
            None => (key_token.to_lowercase(), ""),
        };
        if !allowed.contains(&key.as_str()) {
            return Err(syntax_error(
                line_number,
                line,
                &format!("Unsupported GitGraph attribute \"{key}\""),
            ));
        }
        index += 1;
        let value_token: Option<&str> =
            if inline.is_empty() { tokens.get(index).map(String::as_str) } else { Some(inline) };
        if inline.is_empty() {
            index += 1;
        }
        let Some(value_token) = value_token else {
            return Err(syntax_error(
                line_number,
                line,
                &format!("GitGraph attribute \"{key}\" requires a value"),
            ));
        };
        result.entry(key).or_default().push(strip_mermaid_quotes(value_token));
    }
    Ok(result)
}

/// `single`.
fn single(
    attributes: &HashMap<String, Vec<String>>,
    key: &str,
    line_number: usize,
    line: &str,
) -> Result<Option<String>, MermaidSyntaxError> {
    match attributes.get(key) {
        None => Ok(None),
        Some(values) if values.len() > 1 => Err(syntax_error(
            line_number,
            line,
            &format!("GitGraph attribute \"{key}\" cannot repeat"),
        )),
        Some(values) => Ok(Some(values[0].clone())),
    }
}

/// `parseCommitType`.
fn parse_commit_type(
    value: Option<&str>,
    line_number: usize,
    line: &str,
) -> Result<GitGraphCommitType, MermaidSyntaxError> {
    let Some(value) = value else { return Ok(GitGraphCommitType::Normal) };
    match value.to_uppercase().as_str() {
        "NORMAL" => Ok(GitGraphCommitType::Normal),
        "REVERSE" => Ok(GitGraphCommitType::Reverse),
        "HIGHLIGHT" => Ok(GitGraphCommitType::Highlight),
        _ => Err(syntax_error(
            line_number,
            line,
            &format!("Unknown GitGraph commit type \"{value}\""),
        )),
    }
}

/// `orderBranches` — unordered branches first (declaration order), then
/// ordered ones by (order, declaration index).
fn order_branches(branches: Vec<GitGraphBranch>) -> Vec<GitGraphBranch> {
    let mut main = branches;
    let rest: Vec<GitGraphBranch> = main.split_off(1);
    let mut unordered: Vec<GitGraphBranch> = Vec::new();
    let mut ordered: Vec<(usize, GitGraphBranch)> = Vec::new();
    for (index, branch) in rest.into_iter().enumerate() {
        match branch.order {
            None => unordered.push(branch),
            Some(_) => ordered.push((index, branch)),
        }
    }
    ordered.sort_by(|left, right| {
        left.1.order.unwrap_or(0).cmp(&right.1.order.unwrap_or(0)).then(left.0.cmp(&right.0))
    });
    main.extend(unordered);
    main.extend(ordered.into_iter().map(|(_, branch)| branch));
    main
}

/// `parseMermaidGitGraphDiagram`.
pub fn parse_mermaid_git_graph_diagram(
    content: &str,
) -> Result<GitGraphDiagram, MermaidSyntaxError> {
    let first_line = first_meaningful_mermaid_line(content).unwrap_or("");
    if parse_header(first_line).is_none() {
        return Err(syntax_error(1, first_line, "GitGraph header is required"));
    }
    let mut branches: Vec<GitGraphBranch> =
        vec![GitGraphBranch { name: "main".to_string(), order: Some(0), head: None }];
    let mut commits: Vec<GitGraphCommit> = Vec::new();
    let mut heads: HashMap<String, Option<String>> = HashMap::from([("main".to_string(), None)]);
    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut direction = GitGraphDirection::Lr;
    let mut current_branch = "main".to_string();
    let mut generated_id = 1usize;
    let mut in_accessibility_description = false;
    let mut header_seen = false;

    for source in meaningful_numbered_mermaid_lines(content) {
        let line = strip_comment(source.text).to_string();
        if in_accessibility_description {
            if line == "}" {
                in_accessibility_description = false;
            }
            continue;
        }
        // `^accDescr\s*\{$/i`
        if line.len() >= 8
            && line.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("accDescr"))
            && line[8..].trim() == "{"
        {
            in_accessibility_description = true;
            continue;
        }
        if line.is_empty() || is_accessibility_line(&line) || is_title_line(&line) {
            continue;
        }

        if let Some(header_direction) = parse_header(&line) {
            if header_seen {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "GitGraph header can only appear once",
                ));
            }
            header_seen = true;
            direction = header_direction.unwrap_or(GitGraphDirection::Lr);
            continue;
        }

        let mut tokens = tokenize(&line);
        let command = tokens.first().cloned().unwrap_or_default().to_lowercase();
        let mut rest: Vec<String> = tokens.split_off(1);

        match command.as_str() {
            "commit" => {
                let shorthand_message = rest
                    .first()
                    .filter(|token| {
                        (token.starts_with('"') && token.ends_with('"') && token.len() > 1)
                            || (token.starts_with('\'') && token.ends_with('\'') && token.len() > 1)
                    })
                    .map(|token| strip_mermaid_quotes(token));
                if shorthand_message.is_some() {
                    rest.remove(0);
                }
                let attributes = parse_attributes(
                    &rest,
                    source.line_number,
                    &line,
                    &["id", "msg", "tag", "type"],
                )?;
                let id =
                    single(&attributes, "id", source.line_number, &line)?.unwrap_or_else(|| {
                        format!("commit-{}", {
                            let id = generated_id;
                            generated_id += 1;
                            id
                        })
                    });
                if id.is_empty() {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        "GitGraph commit id cannot be empty",
                    ));
                }
                if ids.contains(&id) {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Duplicate commit id \"{id}\""),
                    ));
                }
                let kind = parse_commit_type(
                    single(&attributes, "type", source.line_number, &line)?.as_deref(),
                    source.line_number,
                    &line,
                )?;
                let parent = heads.get(&current_branch).cloned().flatten();
                let message =
                    single(&attributes, "msg", source.line_number, &line)?.or(shorthand_message);
                commits.push(GitGraphCommit {
                    id: id.clone(),
                    message,
                    tags: attributes.get("tag").cloned().unwrap_or_default(),
                    kind,
                    branch: current_branch.clone(),
                    parents: parent.map(|parent| vec![parent]).unwrap_or_default(),
                });
                ids.insert(id.clone());
                heads.insert(current_branch.clone(), Some(id));
            }
            "branch" => {
                if rest.is_empty() {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        "GitGraph branch name cannot be empty",
                    ));
                }
                let name = strip_mermaid_quotes(&rest[0]);
                if name.is_empty() {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        "GitGraph branch name cannot be empty",
                    ));
                }
                if heads.contains_key(&name) {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Duplicate branch \"{name}\""),
                    ));
                }
                let attributes =
                    parse_attributes(&rest[1..], source.line_number, &line, &["order"])?;
                let order = single(&attributes, "order", source.line_number, &line)?;
                let order = match order {
                    None => None,
                    Some(value) => match value.parse::<i64>() {
                        Ok(parsed) if parsed >= 0 => Some(parsed),
                        _ => {
                            return Err(syntax_error(
                                source.line_number,
                                &line,
                                "GitGraph branch order must be a non-negative integer",
                            ))
                        }
                    },
                };
                let inherited_head = heads.get(&current_branch).cloned().flatten();
                branches.push(GitGraphBranch { name: name.clone(), order, head: None });
                heads.insert(name.clone(), inherited_head);
                current_branch = name;
            }
            "checkout" | "switch" => {
                if rest.len() != 1 {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("GitGraph {command} requires one branch"),
                    ));
                }
                let name = strip_mermaid_quotes(&rest[0]);
                if !heads.contains_key(&name) {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Unknown branch \"{name}\""),
                    ));
                }
                current_branch = name;
            }
            "merge" => {
                if rest.is_empty() {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        "GitGraph merge requires a branch",
                    ));
                }
                let branch = strip_mermaid_quotes(&rest[0]);
                if !heads.contains_key(&branch) {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Unknown branch \"{branch}\""),
                    ));
                }
                if branch == current_branch {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        "GitGraph cannot merge a branch into itself",
                    ));
                }
                let current_head = heads.get(&current_branch).cloned().flatten();
                let merged_head = heads.get(&branch).cloned().flatten();
                let Some(current_head) = current_head else {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Branch \"{current_branch}\" has no commits"),
                    ));
                };
                let Some(merged_head) = merged_head else {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Branch \"{branch}\" has no commits"),
                    ));
                };
                if current_head == merged_head {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Branches already share head \"{merged_head}\""),
                    ));
                }
                let attributes = parse_attributes(
                    &rest[1..],
                    source.line_number,
                    &line,
                    &["id", "tag", "type"],
                )?;
                let id =
                    single(&attributes, "id", source.line_number, &line)?.unwrap_or_else(|| {
                        format!("commit-{}", {
                            let id = generated_id;
                            generated_id += 1;
                            id
                        })
                    });
                if ids.contains(&id) {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("Duplicate commit id \"{id}\""),
                    ));
                }
                let kind = parse_commit_type(
                    single(&attributes, "type", source.line_number, &line)?.as_deref(),
                    source.line_number,
                    &line,
                )?;
                commits.push(GitGraphCommit {
                    id: id.clone(),
                    message: None,
                    tags: attributes.get("tag").cloned().unwrap_or_default(),
                    kind,
                    branch: current_branch.clone(),
                    parents: vec![current_head, merged_head],
                });
                ids.insert(id.clone());
                heads.insert(current_branch.clone(), Some(id));
            }
            "cherry-pick" => {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Cherry-pick is not supported",
                ));
            }
            _ => return Err(syntax_error(source.line_number, &line, "")),
        }
    }

    let resolved_branches: Vec<GitGraphBranch> = branches
        .into_iter()
        .map(|mut branch| {
            branch.head = heads.get(&branch.name).cloned().flatten();
            branch
        })
        .collect();
    Ok(GitGraphDiagram { direction, branches: order_branches(resolved_branches), commits })
}

// The four directional flags mirror the upstream `Connections`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default)]
struct Connections {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    style: Option<GitGraphCellStyle>,
}

const LANE_WIDTH: i64 = 2;
const LABEL_GAP: i64 = 2;

/// `drawGitGraphDiagramGrid`.
pub fn draw_git_graph_diagram_grid(
    diagram: &GitGraphDiagram,
    _options: &GitGraphRenderOptions,
) -> GitGraphGrid {
    if diagram.commits.is_empty() {
        return DiagramCanvas::new(0, 0, None)
            .unwrap_or_else(|_| DiagramCanvas::new(1, 1, None).unwrap());
    }
    let lane_by_branch: HashMap<&str, i64> = diagram
        .branches
        .iter()
        .enumerate()
        .map(|(index, branch)| (branch.name.as_str(), index as i64))
        .collect();
    let commit_by_id: HashMap<&str, &GitGraphCommit> =
        diagram.commits.iter().map(|commit| (commit.id.as_str(), commit)).collect();
    let spans = branch_spans(diagram, &commit_by_id);
    let heads = branch_heads(diagram);
    let graph_width = (diagram.branches.len() as i64 - 1) * LANE_WIDTH + 1;
    let mut label_width = 0i64;
    for commit in &diagram.commits {
        label_width = label_width.max(diagram_text_width(&commit_label(commit, &heads)) as i64);
    }
    let forks: Vec<bool> = diagram
        .commits
        .iter()
        .map(|commit| is_fork(commit, &lane_by_branch, &commit_by_id))
        .collect();
    let height = diagram.commits.len() as i64 + forks.iter().filter(|fork| **fork).count() as i64;
    let mut grid: GitGraphGrid =
        DiagramCanvas::new(graph_width + LABEL_GAP + label_width, height, None).unwrap();

    let mut row = 0i64;
    for (index, commit) in diagram.commits.iter().enumerate() {
        if forks[index] {
            draw_transition_row(
                &mut grid,
                &spans,
                &lane_by_branch,
                &commit_by_id,
                commit,
                index,
                row,
            );
            row += 1;
        }
        draw_commit_row(
            &mut grid,
            diagram,
            &spans,
            &lane_by_branch,
            &commit_by_id,
            commit,
            index,
            row,
        );
        grid.set_text(
            graph_width + LABEL_GAP,
            row,
            &commit_label(commit, &heads),
            Some(GitGraphCellStyle::Label),
        );
        row += 1;
    }
    grid
}

fn draw_transition_row(
    grid: &mut GitGraphGrid,
    spans: &HashMap<String, BranchSpan>,
    lane_by_branch: &HashMap<&str, i64>,
    commit_by_id: &HashMap<&str, &GitGraphCommit>,
    commit: &GitGraphCommit,
    index: usize,
    y: i64,
) {
    let mut cells: HashMap<i64, Connections> = HashMap::new();
    for (branch, span) in spans {
        if span.first >= index || span.last < index {
            continue;
        }
        let lane = lane_by_branch[branch.as_str()];
        connect(
            &mut cells,
            lane * LANE_WIDTH,
            Connections { up: true, down: true, ..Default::default() },
            branch_style(lane),
        );
    }

    let lane = lane_by_branch[commit.branch.as_str()];
    let first_parent = commit.parents.first().and_then(|parent| commit_by_id.get(parent.as_str()));
    if let Some(first_parent) = first_parent {
        if first_parent.branch != commit.branch {
            let parent_lane = lane_by_branch[first_parent.branch.as_str()];
            connect_horizontal(
                &mut cells,
                parent_lane,
                lane,
                SourceTargetVertical {
                    source_up: true,
                    source_down: true,
                    target_down: true,
                    target_up: false,
                },
                branch_style(lane),
            );
        }
    }
    paint_connections(grid, &cells, y);
}

fn draw_commit_row(
    grid: &mut GitGraphGrid,
    diagram: &GitGraphDiagram,
    spans: &HashMap<String, BranchSpan>,
    lane_by_branch: &HashMap<&str, i64>,
    commit_by_id: &HashMap<&str, &GitGraphCommit>,
    commit: &GitGraphCommit,
    index: usize,
    y: i64,
) {
    let mut cells: HashMap<i64, Connections> = HashMap::new();
    for branch in &diagram.branches {
        let Some(span) = spans.get(branch.name.as_str()) else { continue };
        if span.first > index || (span.last <= index && branch.name != commit.branch) {
            continue;
        }
        let lane = lane_by_branch[branch.name.as_str()];
        connect(
            &mut cells,
            lane * LANE_WIDTH,
            Connections { up: index > 0, down: span.last > index, ..Default::default() },
            branch_style(lane),
        );
    }

    let lane = lane_by_branch[commit.branch.as_str()];
    let second_parent = commit.parents.get(1).and_then(|parent| commit_by_id.get(parent.as_str()));
    if let Some(second_parent) = second_parent {
        let parent_lane = lane_by_branch[second_parent.branch.as_str()];
        connect_horizontal(
            &mut cells,
            lane,
            parent_lane,
            SourceTargetVertical {
                source_up: true,
                target_up: true,
                source_down: false,
                target_down: false,
            },
            branch_style(parent_lane),
        );
    }
    paint_connections(grid, &cells, y);
    grid.set_cell(lane * LANE_WIDTH, y, commit_glyph(commit), Some(commit_style(commit)));
}

// The source/target vertical flags mirror the upstream helper.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
struct SourceTargetVertical {
    source_up: bool,
    source_down: bool,
    target_up: bool,
    target_down: bool,
}

/// `connectHorizontal`.
fn connect_horizontal(
    cells: &mut HashMap<i64, Connections>,
    source_lane: i64,
    target_lane: i64,
    vertical: SourceTargetVertical,
    style: GitGraphCellStyle,
) {
    if source_lane == target_lane {
        return;
    }
    let source = source_lane * LANE_WIDTH;
    let target = target_lane * LANE_WIDTH;
    let direction = (target - source).signum();
    connect(
        cells,
        source,
        Connections {
            up: vertical.source_up,
            down: vertical.source_down,
            right: direction > 0,
            left: direction < 0,
            style: None,
        },
        style,
    );
    let mut x = source + direction;
    while x != target {
        connect(cells, x, Connections { left: true, right: true, ..Default::default() }, style);
        x += direction;
    }
    connect(
        cells,
        target,
        Connections {
            up: vertical.target_up,
            down: vertical.target_down,
            left: direction > 0,
            right: direction < 0,
            style: None,
        },
        style,
    );
}

/// `connect` — merges connection flags; the first style wins.
fn connect(
    cells: &mut HashMap<i64, Connections>,
    x: i64,
    additions: Connections,
    style: GitGraphCellStyle,
) {
    let current = cells.get(&x).copied().unwrap_or_default();
    let merged = Connections {
        up: current.up || additions.up,
        down: current.down || additions.down,
        left: current.left || additions.left,
        right: current.right || additions.right,
        style: current.style.or(Some(style)),
    };
    cells.insert(x, merged);
}

/// `paintConnections`.
fn paint_connections(grid: &mut GitGraphGrid, cells: &HashMap<i64, Connections>, y: i64) {
    for (&x, connections) in cells {
        let style = connections.style.unwrap_or(GitGraphCellStyle::Branch0);
        grid.set_cell(x, y, connection_glyph(*connections), Some(style));
    }
}

/// `connectionGlyph`.
fn connection_glyph(connections: Connections) -> &'static str {
    let mask = (
        u8::from(connections.up),
        u8::from(connections.down),
        u8::from(connections.left),
        u8::from(connections.right),
    );
    match mask {
        (1, 1 | 0, 0, 0) | (1 | 0, 1, 0, 0) if false => unreachable!(),
        (_, 1 | 0, 0, 0) if true => "│",
        (0, 0, 1, _) | (0, 0, 0, 1) => "─",
        (0, 1, 0, 1) => "╭",
        (0, 1, 1, 0) => "╮",
        (1, 0, 0, 1) => "╰",
        (1, 0, 1, 0) => "╯",
        (1, 1, 0, 1) => "├",
        (1, 1, 1, 0) => "┤",
        (0, 1, 1, 1) => "┬",
        (1, 0, 1, 1) => "┴",
        (1, 1, 1, 1) => "┼",
        _ => " ",
    }
}

#[derive(Debug, Clone, Copy)]
struct BranchSpan {
    first: usize,
    last: usize,
}

/// `branchSpans`.
fn branch_spans(
    diagram: &GitGraphDiagram,
    commit_by_id: &HashMap<&str, &GitGraphCommit>,
) -> HashMap<String, BranchSpan> {
    let mut spans: HashMap<String, BranchSpan> = HashMap::new();
    for (index, commit) in diagram.commits.iter().enumerate() {
        match spans.get_mut(&commit.branch) {
            Some(span) => span.last = index,
            None => {
                spans.insert(commit.branch.clone(), BranchSpan { first: index, last: index });
            }
        }
        for parent_id in &commit.parents {
            let Some(parent) = commit_by_id.get(parent_id.as_str()) else { continue };
            if parent.branch == commit.branch {
                continue;
            }
            if let Some(parent_span) = spans.get_mut(&parent.branch) {
                parent_span.last = parent_span.last.max(index);
            }
        }
    }
    spans
}

/// `branchHeads`.
fn branch_heads(diagram: &GitGraphDiagram) -> HashMap<String, Vec<String>> {
    let mut heads: HashMap<String, Vec<String>> = HashMap::new();
    for branch in &diagram.branches {
        let Some(head) = &branch.head else { continue };
        heads.entry(head.clone()).or_default().push(branch.name.clone());
    }
    heads
}

/// `isFork`.
fn is_fork(
    commit: &GitGraphCommit,
    lane_by_branch: &HashMap<&str, i64>,
    commit_by_id: &HashMap<&str, &GitGraphCommit>,
) -> bool {
    let Some(parent) = commit.parents.first().and_then(|parent| commit_by_id.get(parent.as_str()))
    else {
        return false;
    };
    lane_by_branch.get(parent.branch.as_str()) != lane_by_branch.get(commit.branch.as_str())
}

/// `commitGlyph`.
fn commit_glyph(commit: &GitGraphCommit) -> &'static str {
    if commit.kind == GitGraphCommitType::Reverse {
        return "⊗";
    }
    if commit.kind == GitGraphCommitType::Highlight {
        return "◆";
    }
    if commit.parents.len() > 1 {
        "◎"
    } else {
        "●"
    }
}

/// `commitStyle`.
fn commit_style(commit: &GitGraphCommit) -> GitGraphCellStyle {
    if commit.kind == GitGraphCommitType::Reverse {
        return GitGraphCellStyle::Reverse;
    }
    if commit.kind == GitGraphCommitType::Highlight {
        return GitGraphCellStyle::Highlight;
    }
    if commit.parents.len() > 1 {
        GitGraphCellStyle::Merge
    } else {
        GitGraphCellStyle::Commit
    }
}

/// `commitLabel`.
fn commit_label(commit: &GitGraphCommit, heads: &HashMap<String, Vec<String>>) -> String {
    let subject = commit.message.as_deref().unwrap_or(&commit.id);
    let mut decorations: Vec<String> = Vec::new();
    if let Some(names) = heads.get(&commit.id) {
        decorations.extend(names.iter().map(|value| format!("[{value}]")));
    }
    decorations.extend(commit.tags.iter().map(|value| format!("[{value}]")));
    if decorations.is_empty() {
        subject.to_string()
    } else {
        format!("{subject}  {}", decorations.join(" "))
    }
}

/// `branchStyle`.
fn branch_style(lane: i64) -> GitGraphCellStyle {
    match lane.rem_euclid(8) {
        0 => GitGraphCellStyle::Branch0,
        1 => GitGraphCellStyle::Branch1,
        2 => GitGraphCellStyle::Branch2,
        3 => GitGraphCellStyle::Branch3,
        4 => GitGraphCellStyle::Branch4,
        5 => GitGraphCellStyle::Branch5,
        6 => GitGraphCellStyle::Branch6,
        _ => GitGraphCellStyle::Branch7,
    }
}

/// `renderGitGraphGridText` — `toString({ trimBottom: true })`.
#[must_use]
pub fn render_git_graph_grid_text(grid: &GitGraphGrid) -> String {
    grid.to_text(TextOptions { trim_top: false, trim_bottom: true })
}

/// `renderGitGraphDiagram`.
pub fn render_git_graph_diagram(content: &str) -> Result<String, MermaidSyntaxError> {
    let diagram = parse_mermaid_git_graph_diagram(content)?;
    Ok(render_git_graph_grid_text(&draw_git_graph_diagram_grid(
        &diagram,
        &GitGraphRenderOptions::default(),
    )))
}

/// `GitGraphStyleColors` — `resolveGitGraphStyleColors`.
#[derive(Debug, Clone, Copy)]
pub struct GitGraphStyleColors {
    pub branch0: Rgba,
    pub branch1: Rgba,
    pub branch2: Rgba,
    pub branch3: Rgba,
    pub branch4: Rgba,
    pub branch5: Rgba,
    pub branch6: Rgba,
    pub branch7: Rgba,
    pub commit: Rgba,
    pub merge: Rgba,
    pub highlight: Rgba,
    pub reverse: Rgba,
    pub label: Rgba,
}

impl GitGraphStyleColors {
    #[must_use]
    pub fn color(&self, style: Option<GitGraphCellStyle>) -> Option<Rgba> {
        let style = style?;
        Some(match style {
            GitGraphCellStyle::Branch0 => self.branch0,
            GitGraphCellStyle::Branch1 => self.branch1,
            GitGraphCellStyle::Branch2 => self.branch2,
            GitGraphCellStyle::Branch3 => self.branch3,
            GitGraphCellStyle::Branch4 => self.branch4,
            GitGraphCellStyle::Branch5 => self.branch5,
            GitGraphCellStyle::Branch6 => self.branch6,
            GitGraphCellStyle::Branch7 => self.branch7,
            GitGraphCellStyle::Commit => self.commit,
            GitGraphCellStyle::Merge => self.merge,
            GitGraphCellStyle::Highlight => self.highlight,
            GitGraphCellStyle::Reverse => self.reverse,
            GitGraphCellStyle::Label => self.label,
        })
    }
}

/// `GitGraphColorInput`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitGraphColorInput {
    pub primary: Option<Rgba>,
    pub secondary: Option<Rgba>,
    pub muted: Option<Rgba>,
    pub warning: Option<Rgba>,
    pub text: Option<Rgba>,
}

/// `resolveGitGraphStyleColors`.
#[must_use]
pub fn resolve_git_graph_style_colors(input: GitGraphColorInput) -> GitGraphStyleColors {
    let rail = input.muted.unwrap_or(Rgba::from_ints(111, 138, 126, 255));
    GitGraphStyleColors {
        branch0: rail,
        branch1: rail,
        branch2: rail,
        branch3: rail,
        branch4: rail,
        branch5: rail,
        branch6: rail,
        branch7: rail,
        commit: input.primary.unwrap_or(Rgba::from_ints(134, 225, 200, 255)),
        merge: input.secondary.unwrap_or(Rgba::from_ints(154, 184, 169, 255)),
        highlight: input.warning.unwrap_or(Rgba::from_ints(230, 177, 126, 255)),
        reverse: input.warning.unwrap_or(Rgba::from_ints(225, 134, 166, 255)),
        label: input.text.unwrap_or(Rgba::from_ints(228, 239, 232, 255)),
    }
}

#[cfg(test)]
mod tests;
