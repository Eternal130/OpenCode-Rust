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

//! `flowchart/parser.ts` — nodes, shapes, subgraphs, `&` groups, and
//! the edge-operator scanner (the upstream `EDGE_OPERATOR_RE` —
//! hand-rolled here because the JS pattern uses lookahead).

use std::collections::HashMap;

use crate::render::mermaid::core::mermaid::{
    first_meaningful_mermaid_line, meaningful_numbered_mermaid_lines, strip_mermaid_quotes,
};
use crate::render::mermaid::diagnostics::{MermaidSyntaxError, MERMAID_FLOWCHART};
use crate::render::mermaid::flowchart::types::{
    EdgeStyle, FlowchartDiagram, FlowchartDirection, FlowchartEdge, FlowchartNode,
    FlowchartNodeShape, FlowchartSubgraph,
};

pub const DEFAULT_DIRECTION: FlowchartDirection = FlowchartDirection::Td;
const MAX_FLOWCHART_LINE_LENGTH: usize = 10_000;

/// `FLOWCHART_HEADER_RE = /^(flowchart|graph)(?:\s+(TB|TD|BT|LR|RL))?$/i`.
fn parse_header(line: &str) -> Option<Option<FlowchartDirection>> {
    let (keyword, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    if !keyword.eq_ignore_ascii_case("flowchart") && !keyword.eq_ignore_ascii_case("graph") {
        return None;
    }
    if rest.is_empty() {
        return Some(None);
    }
    // exactly one whitespace run between keyword and direction, nothing else
    let trimmed = rest.trim();
    if trimmed.len() != rest.len() || trimmed.is_empty() {
        return None;
    }
    FlowchartDirection::parse(trimmed).map(Some)
}

/// `[A-Za-z_][A-Za-z0-9_.-]*` — the greedy ID scan (returns the length).
fn id_len(token: &str) -> usize {
    let mut chars = token.chars();
    let Some(first) = chars.next() else { return 0 };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return 0;
    }
    let mut length = first.len_utf8();
    for char in chars {
        if char.is_ascii_alphanumeric() || char == '_' || char == '.' || char == '-' {
            length += char.len_utf8();
        } else {
            break;
        }
    }
    length
}

fn is_id_only(token: &str) -> bool {
    let length = id_len(token);
    length == token.len() && length > 0
}

/// `^(ID)DELIM(.+)DELIM$` — the greedy `.+$` content between a fixed
/// prefix/suffix (at least one char).
fn match_id_delimited<'a>(token: &'a str, prefix: &str, suffix: &str) -> Option<(String, &'a str)> {
    let id_length = id_len(token);
    if id_length == 0 {
        return None;
    }
    let rest = &token[id_length..];
    let content = rest.strip_prefix(prefix)?;
    let content = content.strip_suffix(suffix)?;
    if content.is_empty() {
        return None;
    }
    Some((token[..id_length].to_string(), content))
}

/// `parseNodeToken` shape probes, in the upstream's RE order.
fn parse_node_token(token: &str) -> FlowchartNode {
    let trimmed = token.trim().trim_end_matches(';');
    if let Some((id, label)) = match_id_delimited(trimmed, "[(", ")]") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Database,
        };
    }
    if let Some((id, label)) = match_id_delimited(trimmed, "[[", "]]") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Subroutine,
        };
    }
    if let Some((id, label)) = match_id_delimited(trimmed, "([", "])") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Rounded,
        };
    }
    if let Some((id, label)) = match_id_delimited(trimmed, "(", ")") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Rounded,
        };
    }
    if let Some((id, label)) = match_id_delimited(trimmed, "{", "}") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Decision,
        };
    }
    if let Some((id, label)) = match_id_delimited(trimmed, "[", "]") {
        return FlowchartNode {
            id,
            label: strip_mermaid_quotes(label),
            shape: FlowchartNodeShape::Box,
        };
    }
    FlowchartNode {
        id: trimmed.to_string(),
        label: trimmed.to_string(),
        shape: FlowchartNodeShape::Box,
    }
}

/// `CIRCLE_NODE_RE` — `^(ID)\(\(.+\)\)$` (rejected shape).
fn is_circle_node(token: &str) -> bool {
    match_id_delimited(token, "((", "))").is_some()
}

fn is_supported_node_token(token: &str) -> bool {
    let trimmed = strip_node_token(token);
    if is_circle_node(trimmed) {
        return false;
    }
    is_id_only(trimmed)
        || match_id_delimited(trimmed, "[(", ")]").is_some()
        || match_id_delimited(trimmed, "[[", "]]").is_some()
        || match_id_delimited(trimmed, "([", "])").is_some()
        || match_id_delimited(trimmed, "(", ")").is_some()
        || match_id_delimited(trimmed, "{", "}").is_some()
        || match_id_delimited(trimmed, "[", "]").is_some()
}

/// `EXPLICIT_NODE_SHAPE_RE = ^ID(?:\[|\(|\{)` — any bracketed shape.
fn has_explicit_node_shape(token: &str) -> bool {
    let trimmed = token.trim();
    let id_length = id_len(trimmed);
    if id_length == 0 {
        return false;
    }
    trimmed[id_length..].starts_with('[')
        || trimmed[id_length..].starts_with('(')
        || trimmed[id_length..].starts_with('{')
}

fn strip_node_token(token: &str) -> &str {
    // .replace(/\s*:::.*$/, "").replace(/;$/, "").trim()
    let mut trimmed = token.trim();
    if let Some(position) = trimmed.find(":::") {
        trimmed = trimmed[..position].trim_end();
    }
    if trimmed.ends_with(';') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }
    trimmed.trim()
}

/// `splitNodeGroup` — splits on `&` outside brackets/quotes.
fn split_node_group(token: &str) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0usize;
    let closes = [('(', ')'), ('[', ']'), ('{', '}')];

    for (index, character) in token.char_indices() {
        if let Some(active) = quote {
            if character == active && !token[..index].ends_with('\\') {
                quote = None;
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
            continue;
        }
        if let Some((_, close)) = closes.iter().find(|(open, _)| *open == character) {
            stack.push(*close);
            continue;
        }
        if !stack.is_empty() && stack.last() == Some(&character) {
            stack.pop();
            continue;
        }
        if stack.is_empty() && character == '&' {
            groups.push(token[start..index].to_string());
            start = index + 1;
        }
    }
    groups.push(token[start..].to_string());
    groups
}

/// `maskNodeLabelOperators` — replaces `<>=.-` inside brackets/quotes
/// with spaces so the operator scan sees only real edge syntax.
fn mask_node_label_operators(line: &str) -> Vec<char> {
    let mut characters: Vec<char> = line.chars().collect();
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let closes = [('(', ')'), ('[', ']'), ('{', '}')];

    for index in 0..characters.len() {
        let character = characters[index];
        if let Some(active) = quote {
            if character == active && index > 0 && characters[index - 1] != '\\' {
                quote = None;
            } else if matches!(character, '<' | '>' | '=' | '.' | '-') {
                characters[index] = ' ';
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
            continue;
        }
        if let Some((_, close)) = closes.iter().find(|(open, _)| *open == character) {
            stack.push(*close);
            continue;
        }
        if !stack.is_empty() && stack.last() == Some(&character) {
            stack.pop();
            continue;
        }
        if !stack.is_empty() && matches!(character, '<' | '>' | '=' | '.' | '-') {
            characters[index] = ' ';
        }
    }
    characters
}

/// `hasInternalStatementSeparator` — `;` outside brackets/quotes/edge
/// labels before the last non-space char.
fn has_internal_statement_separator(line: &str) -> bool {
    let mut stack: Vec<char> = Vec::new();
    let mut quote: Option<char> = None;
    let mut edge_label = false;
    let closes = [('(', ')'), ('[', ']'), ('{', '}')];
    let final_index = line.trim_end().chars().count().saturating_sub(1);

    for (char_index, (byte_index, character)) in line.char_indices().enumerate() {
        let index = char_index;
        if let Some(active) = quote {
            if character == active && index > 0 && !line[..byte_index].ends_with('\\') {
                quote = None;
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
            continue;
        }
        if let Some((_, close)) = closes.iter().find(|(open, _)| *open == character) {
            stack.push(*close);
            continue;
        }
        if !stack.is_empty() && stack.last() == Some(&character) {
            stack.pop();
            continue;
        }
        if stack.is_empty() && character == '|' {
            edge_label = !edge_label;
            continue;
        }
        if character == ';' && index < final_index && stack.is_empty() && !edge_label {
            return true;
        }
    }
    false
}

/// `ParsedEdgeOperator` — one matched operator occurrence.
#[derive(Debug, Clone)]
struct EdgeOperator {
    index: usize,
    end: usize,
    /// Byte range in the ORIGINAL line.
    label: Option<(usize, usize)>,
    style: Option<EdgeStyle>,
    arrowhead: bool,
    source_arrowhead: bool,
    order_only: bool,
}

fn edge_style_from_arrows(start_arrow: &str, end_arrow: &str) -> Option<EdgeStyle> {
    if start_arrow.contains("==") || end_arrow.contains("==") {
        return Some(EdgeStyle::Thick);
    }
    if start_arrow.contains('.') || end_arrow.contains('.') {
        return Some(EdgeStyle::Dashed);
    }
    None
}

fn matches_arrow_at(chars: &[char], at: usize, arrow: &str) -> bool {
    let arrow: Vec<char> = arrow.chars().collect();
    if at + arrow.len() > chars.len() {
        return false;
    }
    chars[at..at + arrow.len()] == arrow[..]
}

/// The alt2 operator text at `index` and the alt2 terminator ending at
/// `end` (both static strings).
fn alt2_arrow_pair<'a>(index: usize, _p: usize, end: usize, masked: &[char]) -> (&'a str, &'a str) {
    for arrow in ["--", "=="] {
        if matches_arrow_at(masked, index, arrow) {
            let terminator: String = masked[end.saturating_sub(4)..end].iter().collect();
            let _ = terminator;
            return (arrow, alt2_end_arrow_at(end, masked));
        }
    }
    let terminator = alt2_end_arrow_at(end, masked);
    ("-.", terminator)
}

fn alt2_end_arrow_at(end: usize, masked: &[char]) -> &'static str {
    for arrow in ALT2_END_ARROWS {
        let length = arrow.chars().count();
        if end >= length && masked[end - length..end].iter().collect::<String>() == arrow {
            return arrow;
        }
    }
    "-->"
}

/// The third alternative's arrow list, in RE alternation order.
const ALT3_ARROWS: [&str; 6] = ["<-->", "-->", "==>", "-.->", "---", "~~~"];
/// The end-arrow list of alternative 2.
const ALT2_END_ARROWS: [&str; 5] = ["-->", "==>", ".->", "-.->", ".-"];

/// `parseEdgeOperators` — the three-alternative leftmost scan over the
/// masked line (labels index the ORIGINAL line).
fn parse_edge_operators(line: &str) -> Vec<EdgeOperator> {
    let chars: Vec<char> = line.chars().collect();
    let masked = mask_node_label_operators(line);
    let mut operators = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        // alt1: -\.(?!->)(.+?)\.(?:->|-)
        if masked[index] == '-' && masked.get(index + 1) == Some(&'.') {
            let label_start = index + 2;
            let blocked = matches_arrow_at(&masked, label_start, "->");
            if !blocked {
                let mut found = None;
                let mut content_end = label_start + 1; // (.+?) — at least one char
                while content_end + 1 < masked.len() {
                    if masked[content_end] == '.' {
                        if matches_arrow_at(&masked, content_end + 1, "->") {
                            found = Some((content_end, content_end + 3));
                            break;
                        }
                        if masked.get(content_end + 1) == Some(&'-') {
                            found = Some((content_end, content_end + 2));
                            break;
                        }
                    }
                    content_end += 1;
                }
                if let Some((dot, end)) = found {
                    // endArrow for this alternative is the whole match — it
                    // always contains '.', and ends with '>' only for "->".
                    let terminator: String = masked[dot..end].iter().collect();
                    operators.push(EdgeOperator {
                        index,
                        end,
                        label: Some((label_start, dot)),
                        style: Some(EdgeStyle::Dashed),
                        arrowhead: terminator.ends_with('>'),
                        source_arrowhead: false,
                        order_only: false,
                    });
                    index = end;
                    continue;
                }
            }
        }

        // alt2: (--|==|-\.)(.+?)\s+(.+?)\s+(-->|==>|\.->|-\.->|\.-)
        // modeled as: op, maximal \s+, lazy label (≥1 char), \s+ (≥1), arrow
        let alt2_start = if matches_arrow_at(&masked, index, "--") {
            Some((index + 2, "--"))
        } else if matches_arrow_at(&masked, index, "==") {
            Some((index + 2, "=="))
        } else if masked[index] == '-' && masked.get(index + 1) == Some(&'.') {
            Some((index + 2, "-."))
        } else {
            None
        };
        if let Some((after_op, _op)) = alt2_start {
            // maximal \s+ right after the operator (greedy, no backtrack)
            let mut p = after_op;
            while p < masked.len() && masked[p].is_whitespace() {
                p += 1;
            }
            if p > after_op {
                // lazy label from p+1: find earliest j ≥ p+1 where a \s+ run
                // (≥1) starting at j is followed by an alt2 arrow
                let mut j = p + 1;
                let mut matched = None;
                while j < masked.len() {
                    let mut k = j;
                    while k < masked.len() && masked[k].is_whitespace() {
                        k += 1;
                    }
                    if k > j {
                        for arrow in ALT2_END_ARROWS {
                            if matches_arrow_at(&masked, k, arrow) {
                                matched = Some((j, k + arrow.chars().count()));
                                break;
                            }
                        }
                        if matched.is_some() {
                            break;
                        }
                    }
                    j += 1;
                }
                if let Some((label_end, end)) = matched {
                    let (start_op, end_arrow) = alt2_arrow_pair(index, p, end, &masked);
                    operators.push(EdgeOperator {
                        index,
                        end,
                        label: Some((p, label_end)),
                        style: edge_style_from_arrows(start_op, end_arrow),
                        arrowhead: end_arrow == "~~~" || end_arrow.ends_with('>'),
                        source_arrowhead: start_op.starts_with('<'),
                        order_only: false,
                    });
                    index = end;
                    continue;
                }
            }
        }

        // alt3: (<-->|-->|==>|-\.->|---|~~~)\s*(?:\|([^|]*)\|\s*)?
        let mut alt3 = None;
        for arrow in ALT3_ARROWS {
            if matches_arrow_at(&masked, index, arrow) {
                alt3 = Some(arrow);
                break;
            }
        }
        if let Some(arrow) = alt3 {
            let arrow_len = arrow.chars().count();
            let mut p = index + arrow_len;
            while p < masked.len() && masked[p].is_whitespace() {
                p += 1;
            }
            let mut label = None;
            let mut end = p;
            if masked.get(p) == Some(&'|') {
                let mut q = p + 1;
                while q < masked.len() && masked[q] != '|' {
                    q += 1;
                }
                if q < masked.len() {
                    label = Some((p + 1, q));
                    end = q + 1;
                    while end < masked.len() && masked[end].is_whitespace() {
                        end += 1;
                    }
                }
            }
            let end_arrow = arrow;
            operators.push(EdgeOperator {
                index,
                end,
                label,
                style: edge_style_from_arrows(end_arrow, end_arrow),
                arrowhead: end_arrow == "~~~" || end_arrow.ends_with('>'),
                source_arrowhead: end_arrow.starts_with('<'),
                order_only: end_arrow == "~~~",
            });
            index = end;
            continue;
        }

        index += 1;
    }

    operators
}

/// `SUBGRAPH_WITH_LABEL_RE` — `^(ID)\s*\[(.+)\]$`.
fn parse_subgraph_token(token: &str, index: usize) -> (String, String) {
    // .trim() → strip `\s*:::.*$` → strip trailing `;` (no re-trim)
    let mut trimmed = token.trim();
    if let Some(position) = trimmed.find(":::") {
        trimmed = trimmed[..position].trim_end();
    }
    if trimmed.ends_with(';') {
        trimmed = &trimmed[..trimmed.len() - 1];
    }

    let id_length = id_len(trimmed);
    if id_length > 0 {
        let after_id = trimmed[id_length..].trim_start();
        if after_id.starts_with('[') && after_id.ends_with(']') && after_id.len() > 2 {
            let label = &after_id[1..after_id.len() - 1];
            if !label.is_empty() {
                return (trimmed[..id_length].to_string(), strip_mermaid_quotes(label));
            }
        }
    }
    let label = strip_mermaid_quotes(trimmed);
    let id =
        if is_id_only(trimmed) { trimmed.to_string() } else { format!("subgraph_{}", index + 1) };
    (id, label)
}

/// `SUBGRAPH_RE = ^subgraph\s+(.+)$` — returns the token after `subgraph`.
fn parse_subgraph_header(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("subgraph")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let token = &line["subgraph".len()..];
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// `SUBGRAPH_DIRECTION_RE`.
fn parse_subgraph_direction(line: &str) -> Option<FlowchartDirection> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("direction")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let direction = rest.trim();
    FlowchartDirection::parse(direction)
}

/// `IGNORED_PRESENTATION_RE = ^(?:classDef|class|style|linkStyle)\b`.
fn is_ignored_presentation(line: &str) -> bool {
    for keyword in ["classDef", "class", "style", "linkStyle"] {
        if line.len() >= keyword.len()
            && line.as_bytes()[..keyword.len()].eq_ignore_ascii_case(keyword.as_bytes())
        {
            let next = line.as_bytes().get(keyword.len()).copied();
            let boundary = match next {
                None => true,
                Some(byte) => {
                    let char = byte as char;
                    !(char.is_ascii_alphanumeric() || char == '_')
                }
            };
            if boundary {
                return true;
            }
        }
    }
    false
}

/// `isMermaidFlowchartDiagram`.
#[must_use]
pub fn is_mermaid_flowchart_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content).is_some_and(|line| parse_header(line).is_some())
}

/// `parseMermaidFlowchartDiagram`.
pub fn parse_mermaid_flowchart_diagram(
    content: &str,
) -> Result<FlowchartDiagram, MermaidSyntaxError> {
    let mut nodes: Vec<FlowchartNode> = Vec::new();
    let mut node_index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<FlowchartEdge> = Vec::new();
    let mut subgraphs: Vec<FlowchartSubgraph> = Vec::new();
    let mut subgraph_stack: Vec<(usize, usize, String)> = Vec::new(); // (subgraph idx, line, source)
    let mut direction = DEFAULT_DIRECTION;

    // `ensureNode`: insertion-ordered nodes map.
    macro_rules! ensure_node {
        ($token:expr) => {{
            let node = parse_node_token($token);
            let index = *node_index.entry(node.id.clone()).or_insert_with(|| {
                nodes.push(node.clone());
                nodes.len() - 1
            });
            if has_explicit_node_shape($token) {
                nodes[index].label = node.label.clone();
                nodes[index].shape = node.shape;
            }
            nodes[index].id.clone()
        }};
    }

    for source in meaningful_numbered_mermaid_lines(content) {
        let line = source.text;
        if line.chars().count() > MAX_FLOWCHART_LINE_LENGTH {
            return Err(MermaidSyntaxError::new(
                MERMAID_FLOWCHART,
                source.line_number,
                line,
                "Flowchart statement is too long",
            ));
        }
        if has_internal_statement_separator(line) {
            return Err(MermaidSyntaxError::new(
                MERMAID_FLOWCHART,
                source.line_number,
                line,
                "Unsupported syntax",
            ));
        }
        if let Some(header_direction) = parse_header(line) {
            direction = header_direction.unwrap_or(DEFAULT_DIRECTION);
            continue;
        }
        // Mermaid CSS styling does not apply to terminal theme rendering.
        if is_ignored_presentation(line) {
            continue;
        }
        if let Some(token) = parse_subgraph_header(line) {
            let (id, label) = parse_subgraph_token(&token, subgraphs.len());
            let parent_id = subgraph_stack.last().map(|(index, _, _)| subgraphs[*index].id.clone());
            subgraphs.push(FlowchartSubgraph {
                id: id.clone(),
                label,
                node_ids: Vec::new(),
                parent_id,
                direction: None,
            });
            let index = subgraphs.len() - 1;
            subgraph_stack.push((index, source.line_number, line.to_string()));
            continue;
        }
        if line.eq_ignore_ascii_case("end") {
            if subgraph_stack.pop().is_none() {
                return Err(MermaidSyntaxError::new(
                    MERMAID_FLOWCHART,
                    source.line_number,
                    line,
                    "Unexpected \"end\" without an open subgraph",
                ));
            }
            continue;
        }

        let current_subgraph = subgraph_stack.last().map(|(index, _, _)| *index);

        if let Some(subgraph_direction) = parse_subgraph_direction(line) {
            let Some(current) = current_subgraph else {
                return Err(MermaidSyntaxError::new(
                    MERMAID_FLOWCHART,
                    source.line_number,
                    line,
                    "A \"direction\" statement requires an open subgraph",
                ));
            };
            subgraphs[current].direction = Some(subgraph_direction);
            continue;
        }

        let edge_operators = parse_edge_operators(line);
        if !edge_operators.is_empty() {
            // Each chain position may be an `&` group.
            let mut node_groups: Vec<Vec<String>> = Vec::new();
            let mut ok = true;
            let mut cursor = 0usize;
            for operator in &edge_operators {
                let group = char_range(line, cursor, operator.index);
                let stripped: Vec<String> = split_node_group(&group)
                    .into_iter()
                    .map(|token| strip_node_token(&token).to_string())
                    .collect();
                if stripped.iter().any(String::is_empty) {
                    ok = false;
                    break;
                }
                node_groups.push(stripped);
                cursor = operator.end;
            }
            if ok {
                let tail_text = char_range(line, cursor, line.chars().count());
                let tail: Vec<String> = split_node_group(&tail_text)
                    .into_iter()
                    .map(|token| strip_node_token(&token).to_string())
                    .collect();
                if tail.iter().any(String::is_empty) {
                    ok = false;
                } else {
                    node_groups.push(tail);
                }
            }

            if ok && node_groups.len() == edge_operators.len() + 1 {
                let order_only_endpoint = |group_index: usize| -> bool {
                    edge_operators
                        .get(group_index.wrapping_sub(1))
                        .is_some_and(|operator| operator.order_only)
                        || edge_operators
                            .get(group_index)
                            .is_some_and(|operator| operator.order_only)
                };
                let unsupported = node_groups.iter().enumerate().any(|(group_index, group)| {
                    group.iter().any(|stripped| {
                        let order_only_subgraph = order_only_endpoint(group_index)
                            && subgraphs.iter().any(|subgraph| subgraph.id == *stripped);
                        !order_only_subgraph && !is_supported_node_token(stripped)
                    })
                });
                if unsupported {
                    return Err(MermaidSyntaxError::new(
                        MERMAID_FLOWCHART,
                        source.line_number,
                        line,
                        "Unsupported syntax",
                    ));
                }
                let chain_node_ids: Vec<Vec<String>> = node_groups
                    .iter()
                    .enumerate()
                    .map(|(group_index, group)| {
                        let order_only_subgraph = order_only_endpoint(group_index);
                        group
                            .iter()
                            .map(|stripped| {
                                if order_only_subgraph
                                    && subgraphs.iter().any(|subgraph| subgraph.id == *stripped)
                                {
                                    stripped.clone()
                                } else {
                                    ensure_node!(stripped.as_str())
                                }
                            })
                            .collect()
                    })
                    .collect();
                for node_ids in &chain_node_ids {
                    for node_id in node_ids {
                        if node_index.contains_key(node_id) {
                            if let Some(current) = current_subgraph {
                                let subgraph = &mut subgraphs[current];
                                if !subgraph.node_ids.contains(node_id) {
                                    subgraph.node_ids.push(node_id.clone());
                                }
                            }
                        }
                    }
                }
                for (operator_index, operator) in edge_operators.iter().enumerate() {
                    for from in &chain_node_ids[operator_index] {
                        for to in &chain_node_ids[operator_index + 1] {
                            let label = operator
                                .label
                                .map(|(start, end)| char_range(line, start, end))
                                .unwrap_or_default();
                            let mut edge = FlowchartEdge::new(
                                from.clone(),
                                to.clone(),
                                strip_mermaid_quotes(&label),
                                operator.style,
                                operator.arrowhead,
                                operator.source_arrowhead,
                            );
                            edge.order_only = operator.order_only;
                            edges.push(edge);
                        }
                    }
                }
                continue;
            }
        }

        let node_group: Vec<String> = split_node_group(line);
        if node_group.iter().all(|token| is_supported_node_token(token)) {
            for token in &node_group {
                let id = ensure_node!(strip_node_token(token));
                if let Some(current) = current_subgraph {
                    let subgraph = &mut subgraphs[current];
                    if !subgraph.node_ids.contains(&id) {
                        subgraph.node_ids.push(id);
                    }
                }
            }
            continue;
        }

        return Err(MermaidSyntaxError::new(
            MERMAID_FLOWCHART,
            source.line_number,
            line,
            "Unsupported syntax",
        ));
    }

    if let Some((_, line_number, source_line)) = subgraph_stack.last() {
        return Err(MermaidSyntaxError::new(
            MERMAID_FLOWCHART,
            *line_number,
            source_line,
            "Unclosed subgraph; expected \"end\"",
        ));
    }

    Ok(FlowchartDiagram { direction, nodes, edges, subgraphs })
}

/// Char-index → byte-safe slice for label ranges (indices are char
/// positions from the masked scan).
fn char_range(line: &str, start: usize, end: usize) -> String {
    line.chars().skip(start).take(end.saturating_sub(start)).collect()
}
