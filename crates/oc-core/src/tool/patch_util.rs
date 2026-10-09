//! Port of `packages/util/src/patch.ts` — the `*** Begin Patch` / `*** End
//! Patch` language: parse (add/update/delete hunks with `*** Move to:` and
//! `*** End of File` markers, heredoc unwrapping) and derive (exact →
//! rstrip → trim → unicode-normalized seek, cumulative replacements).

/// `Patch.BoundaryError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BoundaryError {
    #[error("The first line of the patch must be '*** Begin Patch'")]
    First,
    #[error("The last line of the patch must be '*** End Patch'")]
    Last,
}

/// `Patch.InvalidHunkError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct InvalidHunkError {
    pub line: String,
    pub line_number: usize,
    pub reason: Option<String>,
}

impl std::fmt::Display for InvalidHunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            Some(reason) => write!(f, "Invalid hunk at line {}: {}", self.line_number, reason),
            None => write!(
                f,
                "Invalid hunk at line {}: '{}' is not a valid hunk header. Valid hunk headers: '*** Add File: {{path}}', '*** Delete File: {{path}}', '*** Update File: {{path}}'",
                self.line_number, self.line
            ),
        }
    }
}

/// `Patch.ParseError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error(transparent)]
    Boundary(#[from] BoundaryError),
    #[error(transparent)]
    InvalidHunk(#[from] InvalidHunkError),
}

/// `Patch.Hunk`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hunk {
    Add { path: String, contents: String },
    Delete { path: String },
    Update { path: String, move_path: Option<String>, chunks: Vec<UpdateFileChunk> },
}

impl Hunk {
    /// The hunk's file path.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Hunk::Add { path, .. } | Hunk::Delete { path } | Hunk::Update { path, .. } => path,
        }
    }
}

/// `Patch.UpdateFileChunk`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateFileChunk {
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub change_context: Option<String>,
    pub end_of_file: bool,
}

/// `Patch.FileUpdate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUpdate {
    pub content: String,
    pub bom: bool,
}

/// `stripHeredoc` — unwrap `cat <<'EOF' … EOF` / `<<EOF … EOF` wrappers.
fn strip_heredoc(input: &str) -> String {
    let after_cat = input.strip_prefix("cat").map_or(input, |rest| {
        if rest.starts_with(char::is_whitespace) {
            rest.trim_start()
        } else {
            input
        }
    });
    let Some(mut rest) = after_cat.strip_prefix("<<") else { return input.to_string() };
    let quote = match rest.chars().next() {
        Some(q @ ('\'' | '"')) => {
            rest = &rest[q.len_utf8()..];
            Some(q)
        }
        _ => None,
    };
    let name_end =
        rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    if name_end == 0 {
        return input.to_string();
    }
    let name = &rest[..name_end];
    rest = &rest[name_end..];
    if let Some(quote) = quote {
        let Some(next) = rest.strip_prefix(quote) else { return input.to_string() };
        rest = next;
    }
    if !rest.starts_with(char::is_whitespace) {
        return input.to_string();
    }
    let rest = rest.trim_start();
    let Some(body_and_tag) = rest.strip_suffix(name) else { return input.to_string() };
    // `…\n<body>\n<name>` — one newline separates body from the closing tag.
    body_and_tag.strip_suffix('\n').map_or_else(|| input.to_string(), |body| body.to_string())
}

fn is_boundary(line: &str) -> bool {
    line == "*** End Patch"
        || line.starts_with("*** Add File: ")
        || line.starts_with("*** Delete File: ")
        || line.starts_with("*** Update File: ")
}

/// `Patch.parse`.
pub fn parse(patch_text: &str) -> Result<Vec<Hunk>, ParseError> {
    let lines: Vec<String> = strip_heredoc(patch_text.trim())
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    let begin = if lines.first().is_some_and(|line| line.trim() == "*** Begin Patch") {
        0
    } else {
        usize::MAX
    };
    let end = if lines.last().is_some_and(|line| line.trim() == "*** End Patch") {
        lines.len() - 1
    } else {
        usize::MAX
    };
    if begin == usize::MAX {
        return Err(BoundaryError::First.into());
    }
    if end == usize::MAX || begin >= end {
        return Err(BoundaryError::Last.into());
    }

    let mut hunks = Vec::new();
    let mut index = begin + 1;
    while index < end {
        let line = &lines[index];
        let header = line.trim();
        if index == begin + 1
            && header.starts_with("*** Environment ID:")
            && !header["*** Environment ID:".len()..].trim().is_empty()
        {
            index += 1;
            continue;
        }
        if let Some(path) = header.strip_prefix("*** Add File: ") {
            let path = path.trim().to_string();
            let (contents, next) = parse_add(&lines, index + 1, end, &path)?;
            hunks.push(Hunk::Add { path, contents });
            index = next;
            continue;
        }
        if let Some(path) = header.strip_prefix("*** Delete File: ") {
            let path = path.trim().to_string();
            let next_line = lines.get(index + 1).map(String::as_str);
            if index + 1 < end {
                if let Some(next) = next_line.map(str::trim) {
                    if !is_boundary(next) {
                        if next.starts_with("*** ") {
                            return Err(InvalidHunkError {
                                line: next.to_string(),
                                line_number: index + 2,
                                reason: None,
                            }
                            .into());
                        }
                        return Err(InvalidHunkError {
                            line: next.to_string(),
                            line_number: index + 2,
                            reason: Some(format!(
                                "Unexpected line after Delete File '{path}': '{next}'. Delete hunks do not contain body lines"
                            )),
                        }
                        .into());
                    }
                }
            }
            hunks.push(Hunk::Delete { path });
            index += 1;
            continue;
        }
        if let Some(path) = header.strip_prefix("*** Update File: ") {
            let path = path.trim().to_string();
            let mut next = index + 1;
            while lines.get(next).is_some_and(|line| line.trim_end() == "*** End of File") {
                next += 1;
            }
            let mut move_path = None;
            let movement = lines.get(next).map(|line| line.trim_end().to_string());
            if movement.as_deref() == Some("*** Move to:")
                || movement.as_deref().is_some_and(|m| m.starts_with("*** Move to: "))
            {
                let target = movement
                    .as_deref()
                    .and_then(|m| m.strip_prefix("*** Move to: "))
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if target.is_empty() {
                    return Err(InvalidHunkError {
                        line: lines[next].trim().to_string(),
                        line_number: next + 1,
                        reason: Some(format!("Move destination for '{path}' must not be empty")),
                    }
                    .into());
                }
                move_path = Some(target);
                next += 1;
            }
            let (chunks, after) = parse_update(&lines, next, end, &path, index)?;
            hunks.push(Hunk::Update { path, move_path, chunks });
            index = after;
            continue;
        }
        return Err(InvalidHunkError {
            line: header.to_string(),
            line_number: index + 1,
            reason: None,
        }
        .into());
    }
    Ok(hunks)
}

type StepResult<T> = Result<T, ParseError>;

fn parse_add(
    lines: &[String],
    start: usize,
    end: usize,
    path: &str,
) -> StepResult<(String, usize)> {
    let mut content: Vec<&str> = Vec::new();
    let mut index = start;
    while index < end && !is_boundary(lines[index].trim()) {
        let Some(plus) = lines[index].strip_prefix('+') else {
            let line = lines[index].trim();
            return Err(InvalidHunkError {
                line: line.to_string(),
                line_number: index + 1,
                reason: Some(format!(
                    "Invalid Add File line for '{path}': expected a line starting with '+', got '{line}'"
                )),
            }
            .into());
        };
        content.push(plus);
        index += 1;
    }
    Ok((content.join("\n"), index))
}

fn parse_update(
    lines: &[String],
    start: usize,
    end: usize,
    path: &str,
    hunk: usize,
) -> StepResult<(Vec<UpdateFileChunk>, usize)> {
    let mut chunks: Vec<UpdateFileChunk> = Vec::new();
    let mut index = start;
    let mut after_end_of_file = false;
    while index < end {
        let line = &lines[index];
        let update_line = line.trim_end();
        if after_end_of_file {
            if update_line.is_empty() {
                index += 1;
                continue;
            }
            if update_line == "@@" || update_line.starts_with("@@ ") {
                after_end_of_file = false;
            } else if is_boundary(update_line) {
                break;
            } else {
                return Err(InvalidHunkError {
                    line: line.clone(),
                    line_number: index + 1,
                    reason: Some(format!(
                        "Expected update hunk to start with a @@ context marker, got: '{line}'"
                    )),
                }
                .into());
            }
        }
        if update_line == "*** End of File" {
            let last = chunks.last();
            if last.is_some_and(|chunk| chunk.old_lines.is_empty() && chunk.new_lines.is_empty()) {
                return Err(InvalidHunkError {
                    line: update_line.to_string(),
                    line_number: index + 1,
                    reason: Some("Update hunk does not contain any lines".to_string()),
                }
                .into());
            }
            if let Some(chunk) = chunks.last_mut() {
                chunk.end_of_file = true;
                after_end_of_file = true;
            }
            index += 1;
            continue;
        }
        if is_boundary(update_line) {
            break;
        }
        if update_line == "@@" || update_line.starts_with("@@ ") {
            if chunks
                .last()
                .is_some_and(|chunk| chunk.old_lines.is_empty() && chunk.new_lines.is_empty())
            {
                return Err(InvalidHunkError {
                    line: line.clone(),
                    line_number: index + 1,
                    reason: Some(format!(
                        "Unexpected line found in update hunk: '{line}'. Every line should start with ' ' (context line), '+' (added line), or '-' (removed line)"
                    )),
                }
                .into());
            }
            chunks.push(UpdateFileChunk {
                old_lines: Vec::new(),
                new_lines: Vec::new(),
                change_context: (update_line != "@@")
                    .then(|| update_line["@@ ".len()..].to_string()),
                end_of_file: false,
            });
            index += 1;
            continue;
        }
        if chunks.is_empty() {
            chunks.push(UpdateFileChunk::default());
        }
        let Some(chunk) = chunks.last_mut() else { unreachable!() };
        if line.is_empty() {
            chunk.old_lines.push(String::new());
            chunk.new_lines.push(String::new());
            index += 1;
            continue;
        }
        if let Some(text) = line.strip_prefix(' ') {
            chunk.old_lines.push(text.to_string());
            chunk.new_lines.push(text.to_string());
            index += 1;
            continue;
        }
        if let Some(text) = line.strip_prefix('-') {
            chunk.old_lines.push(text.to_string());
            index += 1;
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            chunk.new_lines.push(text.to_string());
            index += 1;
            continue;
        }
        let populated = !chunk.old_lines.is_empty() || !chunk.new_lines.is_empty();
        return Err(InvalidHunkError {
            line: line.clone(),
            line_number: index + 1,
            reason: Some(if populated {
                format!("Expected update hunk to start with a @@ context marker, got: '{line}'")
            } else {
                format!(
                    "Unexpected line found in update hunk: '{line}'. Every line should start with ' ' (context line), '+' (added line), or '-' (removed line)"
                )
            }),
        }
        .into());
    }
    if chunks.is_empty() {
        return Err(InvalidHunkError {
            line: lines[hunk].trim().to_string(),
            line_number: hunk + 1,
            reason: Some(format!("Update file hunk for path '{path}' is empty")),
        }
        .into());
    }
    if chunks.last().is_some_and(|chunk| chunk.old_lines.is_empty() && chunk.new_lines.is_empty()) {
        let line = lines.get(index).map_or(String::new(), |l| l.trim().to_string());
        return Err(InvalidHunkError {
            line_number: index + 1,
            reason: Some(if line == "*** End Patch" {
                "Update hunk does not contain any lines".to_string()
            } else {
                format!(
                    "Unexpected line found in update hunk: '{line}'. Every line should start with ' ' (context line), '+' (added line), or '-' (removed line)"
                )
            }),
            line,
        }
        .into());
    }
    Ok((chunks, index))
}

/// The seek comparison ladder: exact → rstrip → trim → unicode-normalized.
type Compare = fn(&str, &str) -> bool;

fn cmp_exact(left: &str, right: &str) -> bool {
    left == right
}

fn cmp_rstrip(left: &str, right: &str) -> bool {
    left.trim_end() == right.trim_end()
}

fn cmp_trim(left: &str, right: &str) -> bool {
    left.trim() == right.trim()
}

fn cmp_normalized(left: &str, right: &str) -> bool {
    crate::tool::edit::normalize_for_match(left.trim())
        == crate::tool::edit::normalize_for_match(right.trim())
}

const LADDER: [Compare; 4] = [cmp_exact, cmp_rstrip, cmp_trim, cmp_normalized];

fn matches_at(lines: &[String], pattern: &[String], offset: usize, compare: Compare) -> bool {
    pattern.iter().enumerate().all(|(index, line)| {
        compare(lines.get(offset + index).map(String::as_str).unwrap_or(""), line)
    })
}

fn seek(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() || lines.len() < pattern.len() {
        return None;
    }
    if eof {
        let offset = lines.len() - pattern.len();
        if offset < start {
            return None;
        }
        return LADDER
            .iter()
            .find_map(|compare| matches_at(lines, pattern, offset, *compare).then_some(offset));
    }
    LADDER.iter().find_map(|compare| {
        (start..=lines.len() - pattern.len())
            .find(|offset| matches_at(lines, pattern, *offset, *compare))
    })
}

fn compute_replacements(
    lines: &[String],
    path: &str,
    chunks: &[UpdateFileChunk],
) -> Result<Vec<(usize, usize, Vec<String>)>, String> {
    let mut replacements: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut line_index = 0usize;
    for chunk in chunks {
        if let Some(context) = &chunk.change_context {
            let found = seek(lines, std::slice::from_ref(context), line_index, false)
                .ok_or_else(|| format!("Failed to find context '{context}' in {path}"))?;
            line_index = found + 1;
        }
        if chunk.old_lines.is_empty() {
            replacements.push((lines.len(), 0, chunk.new_lines.clone()));
            continue;
        }
        let mut old_lines = chunk.old_lines.clone();
        let mut new_lines = chunk.new_lines.clone();
        let mut found = seek(lines, &old_lines, line_index, chunk.end_of_file);
        if found.is_none() && old_lines.last().is_some_and(|line| line.is_empty()) {
            old_lines.pop();
            if new_lines.last().is_some_and(|line| line.is_empty()) {
                new_lines.pop();
            }
            found = seek(lines, &old_lines, line_index, chunk.end_of_file);
        }
        let found = match found {
            Some(found) => found,
            None if chunk.old_lines.iter().all(|line| line.is_empty()) => {
                let expected = if chunk.old_lines.len() == 1 {
                    "an expected blank line".to_string()
                } else {
                    format!("{} consecutive blank lines", chunk.old_lines.len())
                };
                return Err(format!("Failed to find {expected} in {path}"));
            }
            None => {
                return Err(format!(
                    "Failed to find expected lines in {path}:\n{}",
                    chunk.old_lines.join("\n")
                ));
            }
        };
        replacements.push((found, old_lines.len(), new_lines));
        line_index = found + old_lines.len();
    }
    replacements.sort_by_key(|(start, _, _)| *start);
    Ok(replacements)
}

/// `Patch.derive` — apply the chunks to `original`.
pub fn derive(
    path: &str,
    chunks: &[UpdateFileChunk],
    original: &str,
) -> Result<FileUpdate, String> {
    let (bom, stripped) = crate::file_mutation::bom_split(original);
    let source = stripped;
    let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let replacements = compute_replacements(&lines, path, chunks)?;
    for (start, remove, insert) in replacements.into_iter().rev() {
        lines.splice(start..start + remove, insert);
    }
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    let joined = lines.join("\n");
    let (next_bom, next_text) = crate::file_mutation::bom_split(&joined);
    Ok(FileUpdate { content: next_text.to_string(), bom: bom || next_bom })
}

/// `Patch.joinBom`.
#[must_use]
pub fn join_bom(text: &str, bom: bool) -> String {
    crate::file_mutation::bom_join(text, bom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_util_parses_add_update_delete_and_move() {
        let hunks = parse(
            "*** Begin Patch\n*** Add File: hello.txt\n+Hello world\n*** Update File: src/app.py\n*** Move to: src/main.py\n@@ def greet():\n-print(\"Hi\")\n+print(\"Hello, world!\")\n*** Delete File: obsolete.txt\n*** End Patch",
        )
        .expect("parse");
        assert_eq!(hunks.len(), 3);
        assert_eq!(
            hunks[0],
            Hunk::Add { path: "hello.txt".into(), contents: "Hello world".into() }
        );
        let Hunk::Update { path, move_path, chunks } = &hunks[1] else { panic!() };
        assert_eq!(path, "src/app.py");
        assert_eq!(move_path.as_deref(), Some("src/main.py"));
        assert_eq!(chunks[0].change_context.as_deref(), Some("def greet():"));
        assert_eq!(chunks[0].old_lines, vec!["print(\"Hi\")".to_string()]);
        assert_eq!(chunks[0].new_lines, vec!["print(\"Hello, world!\")".to_string()]);
        assert_eq!(hunks[2], Hunk::Delete { path: "obsolete.txt".into() });
    }

    #[test]
    fn patch_util_boundary_errors() {
        let error = parse("invalid patch").unwrap_err();
        assert_eq!(error.to_string(), "The first line of the patch must be '*** Begin Patch'");
        let error = parse("*** Begin Patch\n*** Add File: foo\n+hello").unwrap_err();
        assert_eq!(error.to_string(), "The last line of the patch must be '*** End Patch'");
        // A begin/end-only envelope parses to zero hunks — the TOOL rejects
        // it with "patch rejected: empty patch".
        assert!(parse("*** Begin Patch\n*** End Patch").expect("envelope").is_empty());
    }

    #[test]
    fn patch_util_rejects_empty_and_invalid_hunks() {
        let error = parse("*** Begin Patch\n*** Frobnicate File: foo\n*** End Patch").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Invalid hunk at line 2: '*** Frobnicate File: foo' is not a valid hunk header. Valid hunk headers: '*** Add File: {path}', '*** Delete File: {path}', '*** Update File: {path}'"
        );
        // Empty update hunk (chunk-less): the path-empty reason at the
        // header line (parse_update's chunks.length === 0 branch).
        let error =
            parse("*** Begin Patch\n*** Update File: empty.txt\n*** End Patch").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Invalid hunk at line 2: Update file hunk for path 'empty.txt' is empty"
        );
        // Parse succeeds with zero hunks (Environment ID only); the TOOL
        // rejects it with "patch rejected: empty patch".
        assert_eq!(
            parse("*** Begin Patch\n*** Environment ID: abc\n*** End Patch").expect("parse").len(),
            0
        );
    }

    #[test]
    fn patch_util_rejects_body_after_delete() {
        let error =
            parse("*** Begin Patch\n*** Delete File: x.txt\nsome body\n*** End Patch").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Invalid hunk at line 3: Unexpected line after Delete File 'x.txt': 'some body'. Delete hunks do not contain body lines"
        );
    }

    #[test]
    fn patch_util_unwraps_heredoc_patches() {
        let hunks =
            parse("cat <<'EOF'\n*** Begin Patch\n*** Add File: a.txt\n+x\n*** End Patch\nEOF")
                .expect("heredoc");
        assert_eq!(hunks.len(), 1);
    }

    #[test]
    fn patch_util_derive_exact_and_fuzzy_matches() {
        let original = "alpha\nbeta\nomega\n";
        let chunks = [UpdateFileChunk {
            old_lines: vec!["beta".into()],
            new_lines: vec!["BETA".into()],
            change_context: None,
            end_of_file: false,
        }];
        let update = derive("f", &chunks, original).expect("derive");
        assert_eq!(update.content, "alpha\nBETA\nomega\n");
        assert!(!update.bom);

        // Whitespace-tolerant match.
        let chunks = [UpdateFileChunk {
            old_lines: vec!["beta  ".into()],
            new_lines: vec!["B".into()],
            change_context: None,
            end_of_file: false,
        }];
        let update = derive("f", &chunks, original).expect("derive fuzzy");
        assert_eq!(update.content, "alpha\nB\nomega\n");

        // Missing lines error carries the expected block.
        let chunks = [UpdateFileChunk {
            old_lines: vec!["missing".into()],
            new_lines: vec!["x".into()],
            change_context: None,
            end_of_file: false,
        }];
        assert_eq!(
            derive("unchanged.txt", &chunks, original).unwrap_err(),
            "Failed to find expected lines in unchanged.txt:\nmissing"
        );
    }

    #[test]
    fn patch_util_derive_appends_with_empty_old_lines_and_eof() {
        let original = "a\nb\n";
        let chunks = [UpdateFileChunk {
            old_lines: vec![],
            new_lines: vec!["c".into()],
            change_context: None,
            end_of_file: false,
        }];
        let update = derive("f", &chunks, original).expect("derive");
        assert_eq!(update.content, "a\nb\nc\n");

        let chunks = [UpdateFileChunk {
            old_lines: vec!["b".into()],
            new_lines: vec!["B".into()],
            change_context: None,
            end_of_file: true,
        }];
        let update = derive("f", &chunks, original).expect("derive eof");
        assert_eq!(update.content, "a\nB\n");
    }

    #[test]
    fn patch_util_derive_preserves_bom() {
        let original = "\u{FEFF}a\nb\n";
        let chunks = [UpdateFileChunk {
            old_lines: vec!["a".into()],
            new_lines: vec!["A".into()],
            change_context: None,
            end_of_file: false,
        }];
        let update = derive("f", &chunks, original).expect("derive");
        assert!(update.bom);
        assert_eq!(join_bom(&update.content, update.bom), "\u{FEFF}A\nb\n");
    }
}
