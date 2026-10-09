//! Port of `tool/plugin/edit.ts` — the model-facing exact-edit leaf with the
//! three-pass matcher (exact → unicode-normalized → line-based trimmed),
//! newline normalization to the dominant ending, BOM preservation, and
//! permission-assert-before-failure ordering (core-runner-map §4.5).

use oc_schema::session_support::FileDiffStatus;
use oc_schema::tool::Content;
use serde_json::Value;

use super::file_diff::file_diff;
use super::{BuiltinTool, Context, FailureCause, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, Kind, ResolveInput};
use crate::file_mutation::bom_join;

/// `normalizeForMatch` — curly quotes/dashes/space variants to ASCII.
#[must_use]
pub fn normalize_for_match(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        out.push(match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{00A0}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            c if ('\u{2002}'..='\u{200A}').contains(&c) => ' ',
            c => c,
        });
    }
    out
}

/// A byte-range match in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    start: usize,
    end: usize,
}

/// Pass 1 + 2 substrate: plain `indexOf` occurrences over a haystack.
fn find_occurrences(content: &str, search: &str) -> Vec<Match> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(found) = content[offset..].find(search) {
        let start = offset + found;
        result.push(Match { start, end: start + search.len() });
        offset = end_of(&result);
    }
    result
}

fn end_of(matches: &[Match]) -> usize {
    matches.last().map_or(0, |m| m.end)
}

/// Pass 2: occurrences of the NORMALIZED search in the normalized source,
/// with byte offsets mapped back onto the ORIGINAL source (normalization is
/// char→char, so the char streams align 1:1; byte widths differ).
fn find_normalized_occurrences(content: &str, search: &str) -> Vec<Match> {
    let normalized_search: Vec<char> = normalize_for_match(search).chars().collect();
    if normalized_search.is_empty() {
        return Vec::new();
    }
    // (byte offset, normalized char) for the source.
    let source: Vec<(usize, char)> = normalize_for_match(content)
        .chars()
        .zip(content.char_indices().map(|(index, _)| index))
        .map(|(c, index)| (index, c))
        .collect();
    let end_bytes: Vec<usize> =
        content.char_indices().map(|(index, c)| index + c.len_utf8()).collect();
    let mut result = Vec::new();
    let mut start = 0;
    while source.len() - start >= normalized_search.len() {
        let matched =
            (0..normalized_search.len()).all(|i| source[start + i].1 == normalized_search[i]);
        if matched {
            let last = start + normalized_search.len() - 1;
            result.push(Match { start: source[start].0, end: end_bytes[last] });
            start += normalized_search.len();
        } else {
            start += 1;
        }
    }
    result
}

/// Pass 3: line-based `trimEnd` + unicode-normalized matches, skipping
/// overlapping candidates.
fn find_line_occurrences(content: &str, search: &str) -> Vec<Match> {
    let trailing_newline = search.ends_with('\n');
    let mut expected: Vec<&str> = search.split('\n').collect();
    if trailing_newline {
        expected.pop();
    }
    // [...content.matchAll(/[^\n]*(?:\n|$)/g)] — every non-empty line span.
    struct Span {
        start: usize,
        span_end: usize,
        text_end: usize,
        body: String,
        newline: bool,
    }
    let mut lines: Vec<Span> = Vec::new();
    let mut position = 0;
    for segment in content.split_inclusive('\n') {
        if segment.is_empty() {
            continue;
        }
        let newline = segment.ends_with('\n');
        let body = segment.strip_suffix('\n').unwrap_or(segment);
        let text_end = position + body.len() - usize::from(body.ends_with('\r'));
        lines.push(Span {
            start: position,
            span_end: position + segment.len(),
            text_end,
            body: body.to_string(),
            newline,
        });
        position += segment.len();
    }
    let mut candidates: Vec<Match> = Vec::new();
    for index in 0..lines.len() {
        if index + expected.len() > lines.len() {
            break;
        }
        let window = &lines[index..index + expected.len()];
        let all_match = window.iter().zip(&expected).all(|(line, want)| {
            normalize_for_match(trim_end(&line.body)) == normalize_for_match(trim_end(want))
        });
        if !all_match {
            continue;
        }
        let last = window.last().expect("non-empty window");
        if trailing_newline && !last.newline {
            continue;
        }
        candidates.push(Match {
            start: window[0].start,
            end: if trailing_newline { last.span_end } else { last.text_end },
        });
    }
    // Drop candidates overlapping an already-accepted match.
    let mut result: Vec<Match> = Vec::new();
    for candidate in candidates {
        if result.iter().any(|kept| kept.end > candidate.start && kept.start < candidate.end) {
            continue;
        }
        result.push(candidate);
    }
    result
}

/// JS `trimEnd` (trailing whitespace only).
fn trim_end(value: &str) -> &str {
    value.trim_end_matches(|c: char| c.is_whitespace())
}

const CRLF: &str = "\r\n";

pub fn description() -> String {
    "Edit the contents of a file by finding and replacing exact text. When editing text from Read output, preserve the exact indentation (tabs or spaces) and omit the line-number prefix, such as `1: `. Never include the prefix in oldString or newString. The edit fails if oldString is not found. By default, oldString must identify a UNIQUE location. Multiple matches FAIL unless replaceAll is true. Add more surrounding context to disambiguate, or set replaceAll to true to replace every occurrence. Use replaceAll when the change should apply to every occurrence, such as renaming a variable.".to_string()
}

/// The edit input schema (v2 shape).
pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File to edit" },
            "oldString": { "type": "string", "description": "Exact text to find and replace" },
            "newString": { "type": "string", "description": "Text to replace oldString with (must differ from oldString)" },
            "replaceAll": { "type": "boolean", "description": "Whether to replace every occurrence of oldString. When false, oldString must match exactly once. Defaults to false." }
        },
        "required": ["path", "oldString", "newString"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "edit",
        description: description(),
        input: schema(),
        permission: Some("edit"),
        codemode: false,
        execute: std::sync::Arc::new(EditExecutor),
    }
}

struct EditExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for EditExecutor {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a Value,
        context: &'a Context,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        Box::pin(async move { run(services, input, context).await })
    }
}

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let path = input["path"].as_str().unwrap_or_default().to_string();
    let old_string_input = input["oldString"].as_str().unwrap_or_default().to_string();
    let new_string_input = input["newString"].as_str().unwrap_or_default().to_string();
    let replace_all = input["replaceAll"].as_bool().unwrap_or(false);

    if old_string_input == new_string_input {
        return Err(ToolFailure::message(
            "No changes to apply: oldString and newString are identical.",
        ));
    }
    if old_string_input.is_empty() {
        return Err(ToolFailure::message(
            "oldString must not be empty. Use write to create or overwrite a file.",
        ));
    }

    let target =
        services.access.resolve(&ResolveInput { path: path.clone(), kind: Some(Kind::File) });
    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };
    services
        .access
        .authorize_external(std::slice::from_ref(&target), &invocation, None)
        .await
        .map_err(|error| blanket(&path, error))?;

    let lock_target = services.mutation.resolve_path(&path).to_string_lossy().into_owned();
    let lock_targets = vec![lock_target];
    services
        .mutation
        .with_lock(&lock_targets, async move {
            execute_edit(
                services,
                &path,
                &old_string_input,
                &new_string_input,
                replace_all,
                context,
                &target,
            )
            .await
        })
        .await
}

fn blanket(path: &str, error: crate::permission::PermissionError) -> ToolFailure {
    ToolFailure {
        message: format!("Unable to edit {path}"),
        cause: Some(super::permission_cause(&error)),
    }
}

async fn execute_edit(
    services: &ToolServices,
    path: &str,
    old_string_input: &str,
    new_string_input: &str,
    replace_all: bool,
    context: &Context,
    target: &crate::file_access::Target,
) -> Result<ToolOutcome, ToolFailure> {
    let original = match crate::file_mutation::read_text(&target.absolute) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Distinguish directory (WrongKind) from missing.
            if target.absolute.is_dir() {
                return Err(ToolFailure::message(format!(
                    "Path is a directory, not a file: {path}"
                )));
            }
            return Err(ToolFailure::message(format!("File not found: {path}")));
        }
        Err(error) => {
            if target.absolute.is_dir() {
                return Err(ToolFailure::message(format!(
                    "Path is a directory, not a file: {path}"
                )));
            }
            return Err(ToolFailure {
                message: format!("Unable to edit {path}"),
                cause: Some(FailureCause::Nested(error.to_string())),
            });
        }
    };

    let source = original.text;
    let ending = if source.contains(CRLF) { CRLF } else { "\n" };
    let old_string = old_string_input.replace(CRLF, "\n").replace('\n', ending);
    let new_string = new_string_input.replace(CRLF, "\n").replace('\n', ending);

    // Pass 1: exact. Pass 2: unicode-normalized. Pass 3: line-based.
    let exact = find_occurrences(&source, &old_string);
    let unicode = if exact.is_empty() {
        find_normalized_occurrences(&source, &old_string)
    } else {
        Vec::new()
    };
    let trailing = if exact.is_empty() && unicode.is_empty() {
        find_line_occurrences(&source, &old_string)
    } else {
        Vec::new()
    };
    let matches = if !exact.is_empty() {
        exact
    } else if !unicode.is_empty() {
        unicode
    } else {
        trailing
    };
    let replacements = matches.len();
    let selected: Vec<Match> =
        if replace_all { matches.clone() } else { matches.iter().take(1).copied().collect() };
    let mut replaced = source.clone();
    for m in selected.iter().rev() {
        replaced = format!("{}{}{}", &replaced[..m.start], new_string, &replaced[m.end..]);
    }

    let preview = if replacements > 0 && (replacements == 1 || replace_all) {
        Some(file_diff(&target.resource, &source, &replaced, FileDiffStatus::Modified))
    } else {
        None
    };

    // Permission assert BEFORE the zero/multi failure checks.
    let mut assert_input = crate::permission::AssertInput::new(
        context.session_id.clone(),
        "edit",
        vec![target.resource.clone()],
    );
    assert_input.agent = Some(context.agent.clone());
    assert_input.save = Some(vec!["*".to_string()]);
    if let Some(preview) = &preview {
        assert_input.metadata = Some(
            serde_json::to_value(serde_json::json!({ "files": [preview] }))
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        );
    }
    assert_input.source = super::tool_source(context);
    if let Err(error) = services.permission.assert(assert_input).await {
        return Err(blanket(path, error));
    }

    if replacements == 0 {
        return Err(ToolFailure::message(format!(
            "Could not find oldString in {path}. It must match exactly, including whitespace and indentation."
        )));
    }
    if replacements > 1 && !replace_all {
        return Err(ToolFailure::message(format!(
            "Found {replacements} matches for oldString, but expected exactly one. Add more surrounding context to make oldString unique, or set replaceAll to true to replace every occurrence."
        )));
    }

    let replacement_bom = replaced.starts_with('\u{FEFF}');
    let write_target = crate::file_mutation::MutationTarget {
        absolute: target.absolute.clone(),
        resource: target.resource.clone(),
    };
    services
        .mutation
        .write(&write_target, bom_join(&replaced, original.bom || replacement_bom).into_bytes())
        .await
        .map_err(|error| ToolFailure {
            message: format!("Unable to edit {path}"),
            cause: Some(FailureCause::Nested(error.to_string())),
        })?;
    let bom = original.bom || replacement_bom;
    let formatted = if services.formatter.file(&target.absolute) {
        crate::file_mutation::sync_text_bom(&target.absolute, bom)
            .map(|content| content.text)
            .unwrap_or_default()
    } else {
        crate::file_mutation::read_text(&target.absolute)
            .map(|content| content.text)
            .unwrap_or_default()
    };
    let files = vec![file_diff(&target.resource, &source, &formatted, FileDiffStatus::Modified)];
    Ok(ToolOutcome {
        content: vec![Content::Text {
            text: format!(
                "Edited {} ({replacements} replacement{})",
                files[0].file,
                if replacements == 1 { "" } else { "s" }
            ),
        }],
        metadata: Some(
            serde_json::to_value(serde_json::json!({ "files": files }))
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        ),
        output: Some(serde_json::json!({ "replacements": replacements })),
    })
}

/// The gpt-* codemod rule shared by the patch plugin's session hooks:
/// `true` keeps only `patch`, `false` keeps only `edit`/`write`.
pub(crate) fn use_patch_tool(model_id: &str) -> bool {
    model_id.contains("gpt-") && !model_id.contains("oss") && !model_id.contains("gpt-4")
}
