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

use crate::style::Style;

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

/// G-182 — one `parseApplyPatchFiles` entry (session/index.tsx:3535-
/// 3559): `type` (or the `status` mapping added/deleted/modified →
/// add/delete/update), the path, the patch and the line counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPatchFile {
    /// `add` / `delete` / anything else (update).
    pub type_: String,
    pub relative_path: String,
    pub patch: String,
    pub additions: i64,
    pub deletions: i64,
}

/// G-182 — `parseApplyPatchFiles(metadata.files)` (index.tsx:3535):
/// every entry needs a type (or mappable status), a path, a patch
/// and BOTH counts — anything less drops (the ref's `flatMap`
/// return-[] guard).
#[must_use]
pub fn parse_apply_patch_files(metadata: Option<&serde_json::Value>) -> Vec<ApplyPatchFile> {
    let Some(files) = metadata
        .as_ref()
        .and_then(|value| value.get("files"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    files
        .iter()
        .filter_map(|file| {
            let number = |key: &str| file.get(key).and_then(serde_json::Value::as_i64).unwrap_or(0);
            let type_ =
                file.get("type").and_then(serde_json::Value::as_str).map(str::to_owned).or_else(
                    || match file.get("status").and_then(serde_json::Value::as_str)? {
                        "added" => Some("add".to_owned()),
                        "deleted" => Some("delete".to_owned()),
                        "modified" => Some("update".to_owned()),
                        _ => None,
                    },
                )?;
            let relative_path = file
                .get("file")
                .or_else(|| file.get("relativePath"))
                .and_then(serde_json::Value::as_str)?
                .to_owned();
            let patch = file.get("patch").and_then(serde_json::Value::as_str)?.to_owned();
            Some(ApplyPatchFile {
                type_,
                relative_path,
                patch,
                additions: number("additions"),
                deletions: number("deletions"),
            })
        })
        .collect()
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

/// G-182 — the per-file patch rows shared by the edit card and the
/// apply-patch cards: the hunk headers + gutter-numbered `+`/`−`
/// rows (the edit block's body, `PatchDiff`).
fn patch_hunk_rows(
    hunks: &[crate::components::diff_viewer::Hunk],
    theme: &ThemeStyles,
    raised: crate::style::Color,
    base: crate::style::Color,
) -> Vec<Vec<Placed>> {
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
    let mut rows: Vec<Vec<Placed>> = Vec::new();
    for hunk in hunks {
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
    rows
}

/// G-182 — the `patch` tool's detail: ONE `BlockTool` CARD PER FILE
/// (`ApplyPatch`, index.tsx:3333-3378 — `<For each={files()}>`): the
/// per-file status header (`# Created` / `# Deleted` / `← Patched`)
/// \+ path; deletions render the `-N lines` count instead of a diff;
/// everything else renders the file's hunks.
fn apply_patch_block(state: &ToolState, theme: &ThemeStyles, width: usize) -> RowRender {
    let files = parse_apply_patch_files(state.metadata_json.as_ref());
    let mut card = None::<RowRender>;
    for file in &files {
        let rows = match file.type_.as_str() {
            "delete" => vec![vec![text_at(
                0,
                0,
                format!("-{} line{}", file.deletions, if file.deletions == 1 { "" } else { "s" }),
                Style::new().fg(theme.diff.text_removed).bg(theme.background),
            )]],
            _ => patch_hunk_rows(
                &parse_patch(&file.patch),
                theme,
                theme.background_raised,
                theme.text.fg.unwrap_or(theme.background),
            ),
        };
        let label = match file.type_.as_str() {
            "add" => "# Created",
            "delete" => "# Deleted",
            _ => "← Patched",
        };
        let mut rows = rows;
        rows.insert(
            0,
            vec![text_at(0, 0, format!("{label} {}", file.relative_path), theme.text_muted)],
        );
        let card_render = block_from_rows(&rows, theme, width);
        card = Some(match card {
            Some(stack) => crate::components::message::rows::stack_rows(stack, card_render),
            None => card_render,
        });
    }
    card.unwrap_or_default()
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
    let mut rows: Vec<Vec<Placed>> =
        vec![vec![text_at(0, 0, format!("← Edit {file_path}"), muted)]];
    rows.extend(patch_hunk_rows(&hunks, theme, raised, base));
    block_from_rows(&rows, theme, width)
}

/// The write block (`Write` + `BlockTool` + `line_number`,
/// index.tsx:2954-2988): `# Wrote {path}` + the content rows with a
/// 3-wide right-aligned gutter.
/// G-140③ — `parseDiagnostics` (index.tsx:3577-3598): the metadata's
/// `diagnostics` map keyed by file path; severity 1 only, ≤ 3 rows,
/// each `{message}` with the range's 1-based start.
fn parse_diagnostics(state: &ToolState, file_path: &str) -> Vec<(usize, usize, String)> {
    let Some(diagnostics) = state
        .metadata_json
        .as_ref()
        .and_then(|metadata| metadata.get("diagnostics"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    let normalize = |path: &str| path.trim_start_matches("./").to_owned();
    let target = normalize(file_path);
    let rows = diagnostics
        .get(file_path)
        .or_else(|| {
            diagnostics.iter().find(|(key, _)| normalize(key) == target).map(|(_, value)| value)
        })
        .and_then(serde_json::Value::as_array);
    let Some(rows) = rows else { return Vec::new() };
    rows.iter()
        .filter_map(|item| {
            if item.get("severity").and_then(serde_json::Value::as_i64) != Some(1) {
                return None;
            }
            let start = item.pointer("/range/start")?;
            let line = start.get("line")?.as_i64()?;
            let character = start.get("character")?.as_i64()?;
            let message = item.get("message")?.as_str()?.to_owned();
            Some((usize::try_from(line).ok()?, usize::try_from(character).ok()?, message))
        })
        .take(3)
        .collect()
}

/// G-140③ — one `Error [line:col] message` row
/// (`text.feedback.error.base`, paddingLeft 3).
fn diagnostic_row(label: &str, theme: &ThemeStyles, width: usize) -> RowRender {
    let style = Style::new().fg(theme.error).bg(theme.background);
    let label_width = width.saturating_sub(ROW_PADDING_LEFT);
    let atoms = crate::wrap::flatten(label, style);
    let placed = crate::wrap::wrap_atoms(&atoms, label_width)
        .into_iter()
        .enumerate()
        .map(|(line, atoms)| Placed {
            x: ROW_PADDING_LEFT,
            y: line,
            width: label_width,
            lines: vec![PlacedLine { runs: crate::wrap::styled_runs(&atoms) }],
            tag: None,
        })
        .collect();
    RowRender { placed, fills: Vec::new(), height: 1, images: Vec::new() }
}

/// G-140③ — the write/edit block + its diagnostics rows
/// (`<Diagnostics …>` under the tool block, index.tsx:2978/3284).
fn with_diagnostics(
    block: RowRender,
    state: &ToolState,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let path = state.input.get("path").cloned().unwrap_or_default();
    let diagnostics = parse_diagnostics(state, &path);
    if diagnostics.is_empty() {
        return block;
    }
    let error = theme.error;
    let mut rows = Vec::new();
    for (line, character, message) in diagnostics {
        rows.push(diagnostic_row(
            &format!("Error [{}:{}] {message}", line + 1, character + 1),
            theme,
            width,
        ));
    }
    let _ = error;
    rows.into_iter().fold(block, crate::components::message::rows::stack_rows)
}

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

/// TUI-TD01 — the `shell` MESSAGE row (`ShellMessage` →
/// `ShellDisplay`, session/index.tsx:2166-2186): the same `┃` shell
/// block the bash tool paints, fed from the message's
/// command/status/exit/output, with the status error line appended
/// ("Command cancelled" / "Command timed out" /
/// "Command exited with code N" — `ShellMessage`'s `error` memo).
#[must_use]
pub fn shell_message_row(
    command: &str,
    status: &str,
    exit: Option<i64>,
    output: Option<&str>,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let mut state = ToolState::default();
    state.input.insert("command".to_owned(), command.to_owned());
    state.status = match status {
        "running" => ToolStatus::Running,
        _ => ToolStatus::Completed,
    };
    if let Some(output) = output.map(str::trim).filter(|text| !text.is_empty()) {
        state.output = Some(output.to_owned());
    }
    let error = match status {
        "killed" => Some("Command cancelled".to_owned()),
        "timeout" => Some("Command timed out".to_owned()),
        _ => exit.filter(|exit| *exit != 0).map(|exit| format!("Command exited with code {exit}")),
    };
    let mut render = shell_block(&state, false, clock_ms, theme, width);
    if let Some(error) = error {
        // `BlockTool`'s error row: the message in the error color
        // under the content (index.tsx:2694-2710).
        let muted = theme.text_muted;
        let height = render.height;
        let error_row =
            text_at(3, height, crate::wrap::take_width(&error, width.saturating_sub(3)), muted);
        render.placed.push(error_row);
        render.height += 1;
        if let Some(fill) = render.fills.first_mut() {
            fill.height = u16::try_from(render.height).unwrap_or(fill.height);
        }
    }
    render
}

/// G-183 — `parseQuestions` (index.tsx:3562-3568): the `question`
/// strings of `input.questions`.
fn parse_questions(input_json: Option<&serde_json::Value>) -> Vec<String> {
    let Some(items) =
        input_json.and_then(|value| value.get("questions")).and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            item.get("question").and_then(serde_json::Value::as_str).map(str::to_owned)
        })
        .collect()
}

/// G-183 — `parseQuestionAnswers` (index.tsx:3570-3574): the
/// metadata's `answers` — an array per question (non-array entries
/// coerce to `[]`); `None` when the array is absent (pending).
fn parse_question_answers(metadata: Option<&serde_json::Value>) -> Option<Vec<Vec<String>>> {
    metadata
        .as_ref()
        .and_then(|value| value.get("answers"))
        .and_then(serde_json::Value::as_array)
        .map(|answers| {
            answers
                .iter()
                .map(|answer| {
                    answer
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
}

/// G-183 — the `question` tool (`Question`, index.tsx:3421-3458):
/// answered → a `# Questions` `BlockTool` of Q/A pairs (question muted,
/// answer base, `(no answer)` fallback); pending → the
/// `Asking questions…` `InlineTool` / `Asked N question(s)`.
fn question_block(
    state: &ToolState,
    clock_ms: u64,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let muted = theme.text_muted;
    let base = theme.text.fg.unwrap_or(theme.background);
    let questions = parse_questions(state.input_json.as_ref());
    let answers = parse_question_answers(state.metadata_json.as_ref());
    let count = questions.len();
    if let Some(answers) = answers {
        let mut rows: Vec<Vec<Placed>> = vec![vec![text_at(0, 0, "# Questions", muted)]];
        for (index, question) in questions.iter().enumerate() {
            let answer = answers.get(index).map(|answer| {
                if answer.is_empty() {
                    "(no answer)".to_owned()
                } else {
                    answer.join(", ")
                }
            });
            rows.push(vec![text_at(0, 0, question.clone(), muted)]);
            rows.push(vec![text_at(
                0,
                0,
                answer.unwrap_or_else(|| "(no answer)".to_owned()),
                Style::new().fg(base).bg(theme.background),
            )]);
        }
        block_from_rows(&rows, theme, width)
    } else {
        let label = format!("Asked {count} question{}", if count == 1 { "" } else { "s" });
        if matches!(state.status, ToolStatus::Running | ToolStatus::Streaming) {
            running_tool_row("Asking questions…", clock_ms, theme, width)
        } else {
            completed_tool_row('→', &label, theme, width)
        }
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
        // G-182 — the `patch` tool renders ONE CARD PER FILE
        // (`ApplyPatch`, index.tsx:3333-3378); `edit` keeps the ref's
        // single first-file card (`Edit`, :3254 — `files[0]`).
        "patch" => apply_patch_block(state, theme, width),
        // G-140③ — Write/Edit LSP diagnostics (index.tsx:2978/3284 +
        // `Diagnostics`, :3469-3482): up to three severity-1 issues
        // for the tool's path render under the block in the error
        // color (`Error [line:col] message`, 1-based).
        "edit" => with_diagnostics(edit_block(state, theme, width), state, theme, width),
        "write" => with_diagnostics(write_block(state, theme, width), state, theme, width),
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
                return error_reveal(head, state, expanded, theme, width);
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
            error_reveal(
                RowRender {
                    placed,
                    fills: head.fills.clone(),
                    height: head.height + body_height,
                    images: Vec::new(),
                },
                state,
                expanded,
                theme,
                width,
            )
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
                error_reveal(
                    completed_tool_row('✱', label.trim(), theme, width),
                    state,
                    expanded,
                    theme,
                    width,
                )
            }
        }
        "webfetch" => {
            let label = format!("WebFetch {}", input("url"));
            detail_inline_row(
                &label,
                "Fetching from the web…",
                state,
                clock_ms,
                theme,
                width,
                expanded,
            )
        }
        "websearch" => {
            // G-370⑩ — `WebSearch` (index.tsx:3062-3084): the
            // settled label carries the provider name
            // (`webSearchProviderLabel`: "Web Search via {Name}",
            // "opencode" → "OpenCode", first-letter titlecase); no
            // provider metadata → plain "Web Search". The ref's
            // RetryProvider fade/morph between providers mid-retry
            // (retry-provider.tsx) stays bounded — the G-128 pump
            // drives only the rail spring, and no oracle frame
            // exercises a provider swap.
            let label = match state.metadata.get("provider").map(String::as_str) {
                Some(provider) if !provider.is_empty() => {
                    let name = if provider == "opencode" {
                        "OpenCode".to_owned()
                    } else {
                        let mut chars = provider.chars();
                        match chars.next() {
                            Some(first) => {
                                first.to_uppercase().collect::<String>() + chars.as_str()
                            }
                            None => String::new(),
                        }
                    };
                    format!("Web Search via {name} \"{}\"", input("query"))
                }
                _ => format!("Web Search \"{}\"", input("query")),
            };
            detail_inline_row(&label, "Searching web…", state, clock_ms, theme, width, expanded)
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
        "question" => question_block(state, clock_ms, theme, width),
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
    expanded: bool,
) -> RowRender {
    let head = if matches!(state.status, ToolStatus::Running | ToolStatus::Streaming) {
        running_tool_row(pending, clock_ms, theme, width)
    } else {
        let icon = if matches!(state.status, ToolStatus::Error) { '✗' } else { '✱' };
        completed_tool_row(icon, label, theme, width)
    };
    // Lane KM (auditA #9): the failed InlineTool's error reveal —
    // `errorExpanded` renders the error row under the head (the
    // failed mouseup branch, index.tsx:2656-2668).
    error_reveal(head, state, expanded, theme, width)
}

/// The failed-row error reveal shared by the `InlineTool` families:
/// `expanded` gates the error row stacked under the head.
fn error_reveal(
    head: RowRender,
    state: &ToolState,
    expanded: bool,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    let Some(error) = state.error.as_deref().filter(|_| expanded) else { return head };
    let error_row = crate::components::message::rows::assistant_error_row(error, theme, width);
    crate::components::message::rows::stack_rows(head, error_row)
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
            images: Vec::new(),
            input_json: None,
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

    /// G-182 — a multi-file patch renders ONE CARD PER FILE
    /// (`<For each={files()}>`, index.tsx:3333-3378).
    #[test]
    fn patch_block_renders_one_card_per_file() {
        let state = state_with(
            ToolStatus::Completed,
            &[],
            None,
            Some(serde_json::json!({
                "files": [
                    {
                        "type": "update", "relativePath": "src/a.rs",
                        "patch": "@@ -1,2 +1,2 @@\n ctx\n-old\n+new",
                        "additions": 1, "deletions": 1
                    },
                    {
                        "type": "add", "relativePath": "src/b.rs",
                        "patch": "@@ -0,0 +1,1 @@\n+fresh",
                        "additions": 1, "deletions": 0
                    },
                    {
                        "type": "delete", "relativePath": "src/c.rs",
                        "patch": "@@ -1,1 +0,0 @@\n-gone",
                        "additions": 0, "deletions": 3
                    }
                ]
            })),
        );
        let row = tool_detail_row("patch", &state, false, 0, &theme(), 60);
        let text = frame_of(&row).text_grid().join("\n");
        // Per-file status + path headers (one per card).
        assert!(text.contains("← Patched src/a.rs"), "{text}");
        assert!(text.contains("# Created src/b.rs"), "{text}");
        assert!(text.contains("# Deleted src/c.rs"), "{text}");
        // Each card keeps its own hunks/counts.
        assert!(text.contains("+new"), "card 1 hunks: {text}");
        assert!(text.contains("+fresh"), "card 2 hunks: {text}");
        assert!(text.contains("-3 lines"), "the delete count row: {text}");
        // Three separate BlockTool cards stack (one raised fill each).
        assert_eq!(row.fills.len(), 3, "three cards");
    }

    /// G-183 — the answered question tool renders the `# Questions`
    /// Q/A block; pending renders the asking row.
    #[test]
    fn question_tool_renders_answered_block_and_pending_row() {
        let answered = state_with(
            ToolStatus::Completed,
            &[],
            None,
            Some(serde_json::json!({"answers": [["Yes"], [], ["a", "b"]]})),
        );
        let answered = ToolState {
            input_json: Some(serde_json::json!({
                "questions": [
                    {"question": "Run tests?"},
                    {"question": "Verbose?"},
                    {"question": "Which scope?"}
                ]
            })),
            ..answered
        };
        let row = tool_detail_row("question", &answered, false, 0, &theme(), 60);
        let grid = frame_of(&row).text_grid();
        assert!(grid[1].contains("# Questions"), "{}", grid[1]);
        assert!(grid[2].contains("Run tests?"), "{}", grid[2]);
        assert!(grid[3].contains("Yes"), "{}", grid[3]);
        assert!(grid[5].contains("(no answer)"), "the empty-answer fallback: {}", grid[5]);
        assert!(grid[7].contains("a, b"), "answers join with ', ': {}", grid[7]);

        let pending = state_with(ToolStatus::Running, &[], None, None);
        let pending = ToolState {
            input_json: Some(serde_json::json!({
                "questions": [{"question": "Run tests?"}]
            })),
            ..pending
        };
        let row = tool_detail_row("question", &pending, false, 0, &theme(), 60);
        let grid = frame_of(&row).text_grid();
        assert!(grid[0].contains("Asking questions…"), "{}", grid[0]);
    }
}

// -- G-370⑨: the diagnostics rows under write/edit blocks ----------------------

#[cfg(test)]
mod g370_diag_tests {
    use super::*;

    fn state_with_diagnostics(path: &str, raw: &str) -> ToolState {
        ToolState {
            status: ToolStatus::Completed,
            input: [("path".to_owned(), path.to_owned())].into_iter().collect(),
            metadata_json: Some(serde_json::from_str(raw).expect("diagnostics json")),
            ..ToolState::default()
        }
    }

    #[test]
    fn parse_keeps_severity1_only_caps_three_and_normalizes_paths() {
        let raw = r#"{"diagnostics": {
            "./app.ts": [
                {"severity":1,"range":{"start":{"line":4,"character":9}},"message":"bad type"},
                {"severity":2,"range":{"start":{"line":0,"character":0}},"message":"warning skipped"},
                {"severity":1,"range":{"start":{"line":1,"character":2}},"message":"unused"},
                {"severity":1,"range":{"start":{"line":2,"character":3}},"message":"third"},
                {"severity":1,"range":{"start":{"line":8,"character":8}},"message":"fourth dropped"}
            ]
        }}"#;
        let state = state_with_diagnostics("app.ts", raw);
        let rows = parse_diagnostics(&state, "app.ts");
        assert_eq!(rows.len(), 3, "capped at the ref's 3");
        assert_eq!(rows[0], (4, 9, "bad type".to_owned()), "severity 1 only");
        assert_eq!(rows[2], (2, 3, "third".to_owned()), "the 4th is dropped");
        // Nothing for an absent path.
        assert!(parse_diagnostics(&state, "other.ts").is_empty());
    }

    #[test]
    fn edit_block_renders_the_error_rows_under_the_block() {
        let raw = r#"{"diagnostics": {"src/a.ts": [
            {"severity":1,"range":{"start":{"line":2,"character":4}},"message":"cannot find x"}
        ]}}"#;
        let state = state_with_diagnostics("src/a.ts", raw);
        let styles = crate::theme::ThemeState::v2_dark().styles();
        let render = with_diagnostics(edit_block(&state, &styles, 80), &state, &styles, 80);
        let text: Vec<String> =
            render.placed.iter().flat_map(|p| p.lines.iter().map(|l| l.text())).collect();
        let joined = text.join("\n");
        assert!(joined.contains("Error [3:5] cannot find x"), "1-based range: {joined}");
        let height = render.height;
        let bare = edit_block(&state, &styles, 80).height;
        assert!(height > bare, "the diagnostics row stacked under the block");
    }
}

#[cfg(test)]
mod g370_websearch_tests {
    use super::*;

    fn websearch_state(provider: Option<&str>) -> ToolState {
        ToolState {
            status: ToolStatus::Completed,
            input: [("query".to_owned(), "rust tui".to_owned())].into_iter().collect(),
            metadata: provider
                .map(|provider| [("provider".to_owned(), provider.to_owned())])
                .unwrap_or_default()
                .into_iter()
                .collect(),
            ..ToolState::default()
        }
    }

    #[test]
    fn websearch_label_carries_the_provider_name() {
        let theme = crate::theme::ThemeState::v2_dark().styles();
        // "opencode" → "OpenCode"; raw providers titlecase.
        for (provider, expected) in [
            ("opencode", "Web Search via OpenCode \"rust tui\""),
            ("tavily", "Web Search via Tavily \"rust tui\""),
        ] {
            let render = tool_detail_row(
                "websearch",
                &websearch_state(Some(provider)),
                false,
                0,
                &theme,
                80,
            );
            let text: Vec<String> =
                render.placed.iter().flat_map(|p| p.lines.iter().map(|l| l.text())).collect();
            assert!(text.join("\n").contains(expected), "{provider}: {text:?}");
        }
        // No provider metadata → the plain label.
        let render = tool_detail_row("websearch", &websearch_state(None), false, 0, &theme, 80);
        let text: Vec<String> =
            render.placed.iter().flat_map(|p| p.lines.iter().map(|l| l.text())).collect();
        assert!(text.join("\n").contains("Web Search \"rust tui\""), "no provider: {text:?}");
    }
}
