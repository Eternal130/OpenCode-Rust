//! The per-tool detail cells (`ToolPart`'s dispatch,
//! routes/session/index.tsx:2373-2446): the surfaces an EXPANDED tool
//! group (or an ungrouped tool part) renders — the shell block
//! (`$ cmd` with output preview), the read/glob/grep/webfetch/websearch
//! inline rows, the write/edit blocks, the execute card, and the
//! failed-tool error row.
//!
//! Geometry notes (read off the ref):
//! * `InlineToolRow`: `paddingLeft 3`, a 2-wide icon slot, the label
//!   at col 5 (`message-parts.tsx:198-275`).
//! * `BlockTool` (index.tsx:2694-2790): `┃` left rail over
//!   `background.raised.base`, `paddingTop 1 paddingBottom 1
//!   paddingLeft 2`, `gap 1` between children.
//! * The shell output preview collapses through
//!   `collapseShellOutput(input, output, 10, 10·max(20, w−6))`
//!   (index.tsx:2892-2895, util/collapse-tool-output.ts) — the port
//!   below is line-for-line.

#![allow(clippy::cast_possible_truncation)]
// The per-tool dispatch mirrors the ref's one-component-per-arm shape
// (index.tsx:2373-2446) — the cell builders are line-for-line ports.
#![allow(clippy::too_many_lines)]

use ratatui::style::Style;

use crate::components::diff_viewer::{parse_patch, PatchRow};
use crate::components::message::model::{ToolState, ToolStatus};
use crate::components::message::rows::{
    completed_tool_row, locale_number, running_tool_row, spinner_frame, RowRender, ICON_WIDTH,
    ROW_PADDING_LEFT,
};
use crate::layout::{Placed, PlacedLine};
use crate::render::Fill;
use crate::theme::ThemeStyles;

/// `collapseToolOutput` (util/collapse-tool-output.ts:3-21).
#[must_use]
pub fn collapse_tool_output(output: &str, max_lines: usize, max_chars: usize) -> (String, bool) {
    let lines: Vec<&str> = output.split('\n').collect();
    if lines.len() <= max_lines && output.chars().count() <= max_chars {
        return (output.to_owned(), false);
    }
    let mut visible: Vec<String> =
        lines.iter().take(max_lines).map(|line| (*line).to_owned()).collect();
    if lines.len() > max_lines && !visible.is_empty() {
        let last = visible.len() - 1;
        visible[last].push('…');
    }
    let preview = visible.join("\n");
    if preview.chars().count() > max_chars {
        let cut = preview.chars().take(max_chars.saturating_sub(1)).collect::<String>();
        return (format!("{cut}…"), true);
    }
    (preview, true)
}

/// `collapseTail` (util/collapse-tool-output.ts:75-87): the TAIL
/// preview with an `(N earlier lines)` label.
fn collapse_tail(output: &str, max_lines: usize, max_chars: usize) -> String {
    let lines: Vec<&str> = output.split('\n').collect();
    if lines.len() <= max_lines && output.chars().count() <= max_chars {
        return output.to_owned();
    }
    let count = lines.len().saturating_sub(max_lines.saturating_sub(1)).max(1);
    let label = format!("({count} earlier {})", if count == 1 { "line" } else { "lines" });
    if max_lines <= 1 {
        return label;
    }
    let preview: String =
        lines.iter().skip(lines.len() - (max_lines - 1)).copied().collect::<Vec<_>>().join("\n");
    let available = max_chars.saturating_sub(label.chars().count() + 1);
    if available == 0 {
        return label;
    }
    let tail: String =
        preview.chars().rev().take(available).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{label}\n{tail}")
}

/// `collapseShellCommand` (util/collapse-tool-output.ts:60-74): the
/// command wraps to at most `max_lines` rows of `line_width`, `…` on
/// overflow.
fn collapse_shell_command(
    input: &str,
    max_lines: usize,
    line_width: usize,
) -> (String, bool, usize) {
    let mut visible = String::new();
    let mut lines = 1usize;
    let mut width = 0usize;
    let mut overflow = false;
    for segment in input.chars() {
        if segment == '\n' {
            if lines >= max_lines {
                overflow = true;
                break;
            }
            visible.push(segment);
            lines += 1;
            width = 0;
            continue;
        }
        let next = crate::wrap::display_width(&segment.to_string());
        if width + next > line_width {
            if lines >= max_lines {
                overflow = true;
                break;
            }
            lines += 1;
            width = 0;
        }
        visible.push(segment);
        width += next;
    }
    if !overflow {
        return (input.to_owned(), false, lines);
    }
    if width >= line_width {
        visible.pop();
    }
    (format!("{visible}…"), true, lines)
}

/// `collapseShellOutput` (util/collapse-tool-output.ts:23-48).
fn collapse_shell_output(
    input: &str,
    output: &str,
    max_lines: usize,
    max_chars: usize,
) -> (String, String, bool) {
    if input.is_empty() {
        let (_preview, overflow) = collapse_tool_output(output, max_lines, max_chars);
        return (
            input.to_owned(),
            if overflow { collapse_tail(output, max_lines, max_chars) } else { output.to_owned() },
            overflow,
        );
    }
    let command_lines = 2.min(max_lines);
    let line_chars = (max_chars / max_lines.max(1)).max(1);
    let (command, command_overflow, command_line_count) =
        collapse_shell_command(input, command_lines, line_chars);
    if output.is_empty() {
        return (command, output.to_owned(), command_overflow);
    }
    let lines = (max_lines.saturating_sub(command_line_count + 1)).max(1);
    let chars = (max_chars.saturating_sub(command.chars().count() + 2)).max(1);
    let (_preview, overflow) = collapse_tool_output(output, lines, chars);
    (
        command,
        if overflow { collapse_tail(output, lines, chars) } else { output.to_owned() },
        command_overflow || overflow,
    )
}

fn text_at(x: usize, y: usize, text: impl Into<String>, style: Style) -> Placed {
    let text: String = text.into();
    let width = crate::wrap::display_width(&text);
    Placed { x, y, width, lines: vec![PlacedLine::plain(text, style)], tag: None }
}

/// The `metadata.files` array of an edit/patch tool
/// (`parseApplyPatchFiles`) — `{relativePath, patch}` entries.
fn metadata_files(metadata: Option<&serde_json::Value>) -> Vec<(String, String)> {
    metadata
        .as_ref()
        .and_then(|value| value.get("files"))
        .and_then(|files| files.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    Some((
                        file.get("relativePath").or_else(|| file.get("path"))?.as_str()?.to_owned(),
                        file.get("patch")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The shell block (`ShellDisplay` + `BlockTool`, index.tsx:2790-2952):
/// `┃` rail over raised, `$ {command}` in `text.base` (spinner +
/// colored while running), the output preview muted below.
#[allow(clippy::too_many_lines)]
fn shell_block(
    state: &ToolState,
    expanded: bool,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let raised = theme.background_raised;
    let muted = theme.text_muted;
    let base = theme.text.fg.unwrap_or(theme.background);
    let command = state.input.get("command").cloned().unwrap_or_default();
    let workdir = state.input.get("workdir").cloned().unwrap_or_default();
    let prefix = if !workdir.is_empty() && workdir != "." {
        format!("cd {workdir} && ")
    } else {
        String::new()
    };
    let running = matches!(state.status, ToolStatus::Running | ToolStatus::Streaming);
    let full_input = if command.is_empty() {
        String::new()
    } else {
        format!("{}{}{}", if running { "" } else { "$ " }, prefix, command)
    };
    let output = state.output.as_deref().map(str::trim).unwrap_or_default().to_owned();
    // maxLines 10, maxChars 10·max(20, width−6) (index.tsx:2892).
    let max_lines = 10;
    let max_chars = 10 * 20.max(width.saturating_sub(6));
    let (limited_input, limited_output) = if expanded {
        (full_input.clone(), output.clone())
    } else {
        let (input, out, _overflow) =
            collapse_shell_output(&full_input, &output, max_lines, max_chars);
        (input, out)
    };
    let mut rows: Vec<Vec<Placed>> = Vec::new();
    if command.is_empty() {
        rows.push(vec![text_at(0, 0, "Writing command…", muted)]);
    } else {
        // The command wraps (`wrapMode char` collapsed / `word`
        // expanded, maxHeight 2 collapsed).
        for line in limited_input.split('\n') {
            rows.push(vec![text_at(
                0,
                0,
                crate::wrap::take_width(line, width.saturating_sub(3)),
                if running { muted } else { Style::new().fg(base).bg(raised) },
            )]);
        }
    }
    if !limited_output.is_empty() {
        rows.push(Vec::new()); // the box's gap 1
        for line in limited_output.split('\n') {
            rows.push(vec![text_at(
                0,
                0,
                crate::wrap::take_width(line, width.saturating_sub(3)),
                muted,
            )]);
        }
    }
    let height = rows.len() + 2; // + paddingTop/Bottom 1
    let mut placed = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        for item in row {
            let mut item = item.clone();
            item.y += 1 + index;
            item.x += 3;
            placed.push(item);
        }
    }
    for row in 0..height {
        placed.push(text_at(0, row, "┃", Style::new().fg(theme.background).bg(theme.background)));
    }
    if running && !command.is_empty() && !rows.is_empty() {
        // The running command's spinner glyph leads the first row
        // (Spinner + the colored text).
        placed.push(text_at(3, 1, spinner_frame(clock_ms), muted));
    }
    let fills = vec![Fill {
        x: 0,
        y: 0,
        width: width as u16,
        height: height as u16,
        style: Style::new().bg(raised),
    }];
    RowRender { placed, fills, height, images: Vec::new() }
}

/// The edit block (`Edit` + `BlockTool` + `PatchDiff`,
/// index.tsx:3240-3330): `← Edit {path}` header + the unified patch
/// rows (gutter numbers, `+`/`−` signs, row backgrounds).
fn edit_block(state: &ToolState, theme: &ThemeStyles, width: usize) -> RowRender {
    let raised = theme.background_raised;
    let muted = theme.text_muted;
    let base = theme.text.fg.unwrap_or(theme.background);
    let files = metadata_files(state.metadata_json.as_ref());
    let Some((file_path, patch)) = files.first().cloned() else {
        // Preparing edit — the title-only BlockTool.
        let pending = state.input.get("path").cloned().unwrap_or_default();
        let height = 3;
        let mut placed = vec![text_at(
            3,
            1,
            if pending.is_empty() {
                "# Preparing edit…".to_owned()
            } else {
                format!("← Edit {pending}")
            },
            muted,
        )];
        for row in 0..height {
            placed.push(text_at(
                0,
                row,
                "┃",
                Style::new().fg(theme.background).bg(theme.background),
            ));
        }
        return RowRender {
            placed,
            fills: vec![Fill {
                x: 0,
                y: 0,
                width: width as u16,
                height: height as u16,
                style: Style::new().bg(raised),
            }],
            height,
            images: Vec::new(),
        };
    };
    let hunks = parse_patch(&patch);
    let digits = hunks
        .iter()
        .flat_map(|hunk| hunk.rows.iter())
        .map(|row| match row {
            PatchRow::Context { old, .. } | PatchRow::Deleted { old, .. } => *old,
            PatchRow::Added { new, .. } => *new,
        })
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(1);
    let mut rows: Vec<Vec<Placed>> =
        vec![vec![text_at(0, 0, format!("← Edit {file_path}"), muted)]];
    for hunk in &hunks {
        rows.push(vec![text_at(
            0,
            0,
            hunk.header.clone(),
            Style::new().fg(theme.diff.hunk_header).bg(raised),
        )]);
        for row in &hunk.rows {
            match row {
                PatchRow::Context { old, new, text } => {
                    rows.push(vec![
                        text_at(
                            0,
                            0,
                            format!("{old:>digits$}"),
                            Style::new()
                                .fg(theme.diff.line_number)
                                .bg(theme.diff.background_context),
                        ),
                        text_at(
                            digits,
                            0,
                            format!(" {new:>digits$}"),
                            Style::new()
                                .fg(theme.diff.line_number)
                                .bg(theme.diff.background_context),
                        ),
                        text_at(
                            digits * 2 + 1,
                            0,
                            format!(" {text}"),
                            Style::new().fg(base).bg(theme.diff.background_context),
                        ),
                    ]);
                }
                PatchRow::Deleted { old, text } => {
                    rows.push(vec![
                        text_at(
                            0,
                            0,
                            format!("{old:>digits$}"),
                            Style::new()
                                .fg(theme.diff.line_number)
                                .bg(theme.diff.background_removed),
                        ),
                        text_at(
                            digits * 2 + 1,
                            0,
                            format!("-{text}"),
                            Style::new().fg(base).bg(theme.diff.background_removed),
                        ),
                    ]);
                }
                PatchRow::Added { new, text } => {
                    rows.push(vec![
                        text_at(
                            digits,
                            0,
                            format!(" {new:>digits$}"),
                            Style::new().fg(theme.diff.line_number).bg(theme.diff.background_added),
                        ),
                        text_at(
                            digits * 2 + 1,
                            0,
                            format!("+{text}"),
                            Style::new().fg(base).bg(theme.diff.background_added),
                        ),
                    ]);
                }
            }
        }
    }
    block_from_rows(&rows, theme, width)
}

/// The write block (`Write` + `BlockTool` + `line_number`,
/// index.tsx:2954-2988): `# Wrote {path}` + the content rows with a
/// 3-wide right-aligned gutter.
fn write_block(state: &ToolState, theme: &ThemeStyles, width: usize) -> RowRender {
    let muted = theme.text_muted;
    let base = theme.text.fg.unwrap_or(theme.background);
    let path = state.input.get("path").cloned().unwrap_or_default();
    let content = state.input.get("content").cloned().unwrap_or_default();
    let mut rows: Vec<Vec<Placed>> = vec![vec![text_at(0, 0, format!("# Wrote {path}"), muted)]];
    for (index, line) in content.split('\n').enumerate() {
        let number = format!("{:>3}", index + 1);
        rows.push(vec![
            text_at(0, 0, format!("{number} "), muted),
            text_at(
                4,
                0,
                crate::wrap::take_width(line, width.saturating_sub(7)),
                Style::new().fg(base).bg(theme.background_raised),
            ),
        ]);
    }
    block_from_rows(&rows, theme, width)
}

/// Assemble a `BlockTool` from pre-built rows: the gap-1 children,
/// padding 1/1/2, the `┃` rail and the raised fill.
fn block_from_rows(rows: &[Vec<Placed>], theme: &ThemeStyles, width: usize) -> RowRender {
    let raised = theme.background_raised;
    let height = rows.len() + 2;
    let mut placed = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        for item in row {
            let mut item = item.clone();
            item.y += 1 + index;
            item.x += 3;
            placed.push(item);
        }
    }
    for row in 0..height {
        placed.push(text_at(0, row, "┃", Style::new().fg(theme.background).bg(theme.background)));
    }
    RowRender {
        placed,
        fills: vec![Fill {
            x: 0,
            y: 0,
            width: width as u16,
            height: height as u16,
            style: Style::new().bg(raised),
        }],
        height,
        images: Vec::new(),
    }
}

/// `ExecuteCallView`'s summary row (`executeCallSummary`,
/// util/tool-display.ts:40-42): `tool [k=v, …]`.
fn execute_call_summary(tool: &str, input: &serde_json::Value) -> String {
    let entries: Vec<String> = input
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(_, value)| value.is_string() || value.is_number() || value.is_boolean())
                .map(|(key, value)| match value {
                    serde_json::Value::String(text) => format!("{key}={text}"),
                    other => format!("{key}={other}"),
                })
                .collect()
        })
        .unwrap_or_default();
    if entries.is_empty() {
        tool.to_owned()
    } else {
        format!("{tool} [{}]", entries.join(", "))
    }
}

/// One tool detail cell. `expanded` is the INLINE disclosure
/// (`ShellDisplay`'s toggle / `GenericTool`'s expand).
#[allow(clippy::too_many_lines)]
pub fn tool_detail_row(
    tool: &str,
    state: &ToolState,
    expanded: bool,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let muted = theme.text_muted;
    let input = |key: &str| state.input.get(key).cloned().unwrap_or_default();
    match tool {
        "shell" => shell_block(state, expanded, clock_ms, theme, width),
        "edit" | "patch" => edit_block(state, theme, width),
        "write" => write_block(state, theme, width),
        "read" => {
            let mut label = format!("Read {}", input("path"));
            if state.input.contains_key("offset") || state.input.contains_key("limit") {
                let offset: u64 =
                    state.input.get("offset").and_then(|v| v.parse().ok()).unwrap_or(1);
                let limit: Option<u64> = state.input.get("limit").and_then(|v| v.parse().ok());
                label = format!(
                    "{label}:{}-{}",
                    offset.max(1),
                    limit.map_or_else(String::new, |limit| (offset.max(1) + limit - 1).to_string())
                );
            }
            let running = matches!(state.status, ToolStatus::Running | ToolStatus::Streaming);
            let head = if running {
                running_tool_row(&label, clock_ms, theme, width)
            } else {
                completed_tool_row('→', &label, theme, width)
            };
            // The `↳ Loaded {path}` rows (`metadata.loaded`,
            // index.tsx:3022-3032).
            let loaded = state
                .metadata_json
                .as_ref()
                .and_then(|value| value.get("loaded"))
                .and_then(|value| value.as_array())
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(|path| path.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if loaded.is_empty() {
                return head;
            }
            let mut rows = Vec::new();
            for path in loaded {
                rows.push(vec![text_at(6, 0, format!("↳ Loaded {path}"), muted)]);
            }
            let body_height = rows.len();
            let mut placed = head.placed.clone();
            for (index, row) in rows.iter().enumerate() {
                for item in row {
                    let mut item = item.clone();
                    item.y = head.height + index;
                    placed.push(item);
                }
            }
            RowRender {
                placed,
                fills: head.fills.clone(),
                height: head.height + body_height,
                images: Vec::new(),
            }
        }
        "glob" | "grep" => {
            let count_key = if tool == "glob" { "count" } else { "matches" };
            let count = state.metadata.get(count_key).and_then(|v| v.parse::<u64>().ok());
            let mut label = format!("{tool} \"{}\"", input("pattern"));
            let scope = input("path");
            if !scope.is_empty() {
                label = format!("{label} in {scope} ");
            }
            if let Some(count) = count {
                label =
                    format!("{label}({count} {})", if count == 1 { "match" } else { "matches" });
            }
            let label = label.replace("glob", "Glob").replace("grep", "Grep");
            if matches!(state.status, ToolStatus::Running | ToolStatus::Streaming) {
                running_tool_row(&label, clock_ms, theme, width)
            } else {
                completed_tool_row('✱', label.trim(), theme, width)
            }
        }
        "webfetch" => {
            let label = format!("WebFetch {}", input("url"));
            detail_inline_row(&label, "Fetching from the web…", state, clock_ms, theme, width)
        }
        "websearch" => {
            let label = format!("Web Search \"{}\"", input("query"));
            detail_inline_row(&label, "Searching web…", state, clock_ms, theme, width)
        }
        "execute" => {
            // `Execute` (index.tsx:3197-3238): the `execute` InlineTool
            // + the ExecuteCallView rows + the error output preview
            // (4 lines, `collapseToolOutput`).
            let has_error = state.error.is_some() || matches!(state.status, ToolStatus::Error);
            let loading = matches!(state.status, ToolStatus::Running | ToolStatus::Streaming);
            let head = if loading {
                running_tool_row("execute", clock_ms, theme, width)
            } else {
                completed_tool_row(if has_error { '✗' } else { '✓' }, "execute", theme, width)
            };
            let mut rows: Vec<Vec<Placed>> = Vec::new();
            if let Some(calls) = state
                .metadata_json
                .as_ref()
                .and_then(|value| value.get("toolCalls"))
                .and_then(|value| value.as_array())
            {
                for call in calls {
                    let Some(tool) = call.get("tool").and_then(|v| v.as_str()) else { continue };
                    let status = call.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    let summary = execute_call_summary(
                        tool,
                        call.get("input").unwrap_or(&serde_json::Value::Null),
                    );
                    let icon = if status == "error" { "✗" } else { "›" };
                    let style = if status == "error" {
                        Style::new().fg(theme.error).bg(theme.background)
                    } else {
                        muted
                    };
                    rows.push(vec![text_at(ROW_PADDING_LEFT, 0, icon, style)]);
                    rows.last_mut().unwrap().push(text_at(
                        ROW_PADDING_LEFT + ICON_WIDTH,
                        0,
                        crate::wrap::take_width(&summary, width.saturating_sub(5)),
                        style,
                    ));
                    if expanded {
                        if let Some(fields) = call.get("input").and_then(|v| v.as_object()) {
                            for (key, value) in fields {
                                let text = match value {
                                    serde_json::Value::String(text) => text.clone(),
                                    other => other.to_string(),
                                };
                                rows.push(vec![
                                    text_at(ROW_PADDING_LEFT + 1, 0, format!("{key}: "), muted),
                                    text_at(
                                        ROW_PADDING_LEFT + 1 + key.len() + 2,
                                        0,
                                        crate::wrap::take_width(&text, width.saturating_sub(9)),
                                        Style::new()
                                            .fg(theme.text.fg.unwrap_or(theme.background))
                                            .bg(theme.background),
                                    ),
                                ]);
                            }
                        }
                    }
                }
            }
            if has_error {
                if let Some(output) =
                    state.output.as_deref().map(str::trim).filter(|output| !output.is_empty())
                {
                    let (preview, _) =
                        collapse_tool_output(output, 4, 4 * 20.max(width.saturating_sub(6)));
                    rows.push(Vec::new());
                    for (index, line) in preview.split('\n').enumerate() {
                        rows.push(vec![text_at(
                            6,
                            0,
                            format!("{}{line}", if index == 0 { "↳ " } else { "  " }),
                            Style::new().fg(theme.error).bg(theme.background),
                        )]);
                    }
                }
            }
            if rows.is_empty() {
                return head;
            }
            let mut placed = head.placed.clone();
            for (index, row) in rows.iter().enumerate() {
                for item in row {
                    let mut item = item.clone();
                    item.y = head.height + index;
                    placed.push(item);
                }
            }
            RowRender {
                placed,
                fills: head.fills.clone(),
                height: head.height + rows.len(),
                images: Vec::new(),
            }
        }
        // The generic tool (`GenericTool`, index.tsx:2532-2584): the
        // tool label + `[k=v, …]`; a failed tool renders the error
        // row below.
        _ => {
            let summary = if state.input.is_empty() {
                tool.to_owned()
            } else {
                let entries: Vec<String> =
                    state.input.iter().map(|(key, value)| format!("{key}={value}")).collect();
                format!("{tool} [{}]", entries.join(", "))
            };
            let head = if matches!(state.status, ToolStatus::Running | ToolStatus::Streaming) {
                running_tool_row(&summary, clock_ms, theme, width)
            } else {
                completed_tool_row(
                    if matches!(state.status, ToolStatus::Error) { '✗' } else { '✓' },
                    &summary,
                    theme,
                    width,
                )
            };
            if let Some(error) = state.error.as_deref().filter(|_| expanded) {
                let error_row =
                    crate::components::message::rows::assistant_error_row(error, theme, width);
                return crate::components::message::rows::stack_rows(head, error_row);
            }
            head
        }
    }
}

/// The completed/running inline row pair for webfetch/websearch.
fn detail_inline_row(
    label: &str,
    pending: &str,
    state: &ToolState,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    if matches!(state.status, ToolStatus::Running | ToolStatus::Streaming) {
        running_tool_row(pending, clock_ms, theme, width)
    } else {
        let icon = if matches!(state.status, ToolStatus::Error) { '✗' } else { '✱' };
        completed_tool_row(icon, label, theme, width)
    }
}

/// The `{n} tokens` label for the sidebar's context section
/// (`toLocaleString`, context.tsx:19).
#[must_use]
pub fn tokens_label(tokens: u64) -> String {
    format!("{} tokens", locale_number(tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::message::model::ToolState;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    fn state_with(
        status: ToolStatus,
        input: &[(&str, &str)],
        output: Option<&str>,
        metadata_json: Option<serde_json::Value>,
    ) -> ToolState {
        ToolState {
            status,
            input: input
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            metadata: std::collections::BTreeMap::new(),
            metadata_json,
            output: output.map(str::to_owned),
            error: None,
        }
    }

    fn frame_of(row: &RowRender) -> crate::render::Frame {
        crate::render::paint_placed_with_fills(
            &row.placed,
            &row.fills,
            (60, u16::try_from(row.height).unwrap_or(u16::MAX)),
            crate::components::tool_rows::fill_style(&theme()),
        )
    }

    #[test]
    fn collapse_tool_output_matches_the_ts_semantics() {
        // Within budget: unchanged.
        assert_eq!(collapse_tool_output("a\nb", 4, 40), ("a\nb".to_owned(), false));
        // Over the LINE budget: the last visible line grows `…`.
        let lines: Vec<String> = (0..6).map(|index| format!("l{index}")).collect();
        let (out, overflow) = collapse_tool_output(&lines.join("\n"), 4, 10_000);
        assert!(overflow);
        assert!(out.ends_with("l3…"), "tail ellipsis: {out}");
        // Over the CHAR budget: truncated to `max_chars - 1` + `…`.
        let (out, overflow) = collapse_tool_output("abcdefg", 1, 4);
        assert!(overflow);
        assert_eq!(out, "abc…");
    }

    #[test]
    fn collapse_shell_output_keeps_the_tail_with_the_omitted_label() {
        let output = (0..12).map(|index| format!("line{index}")).collect::<Vec<_>>().join("\n");
        let (input, out, overflow) = collapse_shell_output("$ run", &output, 10, 10_000);
        assert_eq!(input, "$ run");
        assert!(overflow);
        // collapseTail: `count = 12 − (10−1)` dropped lines, the last
        // `10−1` stay (line5..line11).
        assert!(out.starts_with("(5 earlier lines)"), "the collapseTail label: {out}");
        assert!(out.contains("line11"), "the TAIL stays: {out}");
        assert!(!out.contains("line4\n"), "the head dropped: {out}");
    }

    #[test]
    fn shell_block_paints_rail_command_and_preview() {
        let state = state_with(
            ToolStatus::Completed,
            &[("command", "echo hi"), ("workdir", ".")],
            Some("hi\nsecond line"),
            None,
        );
        let row = tool_detail_row("shell", &state, false, 0, &theme(), 60);
        // padTop + command + gap + 2 output rows + padBottom.
        assert_eq!(row.height, 6);
        let grid = frame_of(&row).text_grid();
        assert!(grid[1].contains("$ echo hi"), "{}", grid[1]);
        assert!(grid[3].contains("hi"), "{}", grid[3]);
        assert!(grid[4].contains("second line"), "{}", grid[4]);
        assert!(grid[1].starts_with('┃'), "the BlockTool rail: {}", grid[1]);
    }

    #[test]
    fn edit_block_renders_the_patch_hunks() {
        let state = state_with(
            ToolStatus::Completed,
            &[("path", "src/a.rs")],
            None,
            Some(serde_json::json!({
                "files": [{
                    "relativePath": "src/a.rs",
                    "patch": "@@ -1,2 +1,2 @@\n context\n-old\n+new"
                }]
            })),
        );
        let row = tool_detail_row("edit", &state, false, 0, &theme(), 60);
        let grid = frame_of(&row).text_grid();
        assert!(grid[1].trim_start().contains("← Edit src/a.rs"), "{}", grid[1]);
        assert!(grid[2].contains("@@ -1,2 +1,2 @@"), "hunk header: {}", grid[2]);
        assert!(grid[3].contains("context"), "{}", grid[3]);
        assert!(grid[4].contains("-old"), "{}", grid[4]);
        assert!(grid[5].contains("+new"), "{}", grid[5]);
    }

    #[test]
    fn execute_card_previews_the_error_output_four_lines() {
        let state = state_with(
            ToolStatus::Error,
            &[("description", "run tests")],
            Some("b1\nb2\nb3\nb4\nb5\nb6"),
            Some(serde_json::json!({
                "toolCalls": [
                    {"tool": "read", "status": "completed", "input": {"path": "x.rs"}}
                ]
            })),
        );
        let mut state = state;
        state.error = Some("exit 1".to_owned());
        let row = tool_detail_row("execute", &state, false, 0, &theme(), 60);
        let grid = frame_of(&row).text_grid();
        assert!(grid[0].contains("✗ execute"), "{}", grid[0]);
        assert!(grid[1].contains("› read"), "the call row: {}", grid[1]);
        // The preview: one spacer row (the ref's gap), then 4 lines
        // max with `↳ ` leading the first.
        assert_eq!(grid[2].trim(), "", "the gap row before the preview");
        assert!(grid[3].contains("↳ b1"), "{}", grid[3]);
        assert!(grid[6].contains("b4"), "{}", grid[6]);
        assert_eq!(row.height, 7, "no b5/b6 past the 4-line budget");
    }

    #[test]
    fn read_row_lists_loaded_paths() {
        let state = state_with(
            ToolStatus::Completed,
            &[("path", "src/main.rs")],
            None,
            Some(serde_json::json!({"loaded": ["src/lib.rs", "src/util.rs"]})),
        );
        let row = tool_detail_row("read", &state, false, 0, &theme(), 60);
        let grid = frame_of(&row).text_grid();
        assert!(grid[0].contains("Read src/main.rs"), "{}", grid[0]);
        assert!(grid[1].trim_start().starts_with("↳ Loaded src/lib.rs"), "{}", grid[1]);
        assert!(grid[2].trim_start().starts_with("↳ Loaded src/util.rs"), "{}", grid[2]);
    }
}
