//! Port of `tool/plugin/read.ts` + `tool/read-filesystem.ts` — the read
//! leaf with text/media/listing surfaces, the exact `{n}: {text}` line
//! format, missing-file suggestions, NFC/typography filename recovery, and
//! the AGENTS.md side effect (core-runner-map §4.6).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use oc_schema::tool::Content;
use serde_json::Value;

use super::{BuiltinTool, Context, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, ReadOptions};

/// `MAX_READ_LINES`.
pub const MAX_READ_LINES: usize = 2_000;
/// `MAX_READ_BYTES` — 50 KiB per page.
pub const MAX_READ_BYTES: usize = 50 * 1024;
/// `MAX_MEDIA_INGEST_BYTES` — 20 MiB.
pub const MAX_MEDIA_INGEST_BYTES: u64 = 20 * 1024 * 1024;
/// `MAX_LINE_LENGTH`.
const MAX_LINE_LENGTH: usize = 2_000;
const MAX_LINE_SUFFIX: &str = "... (line truncated to 2000 chars)";
/// `MEDIA_MIMES`.
pub const MEDIA_MIMES: [&str; 5] =
    ["image/png", "image/jpeg", "image/gif", "image/webp", "application/pdf"];
const AGENTS_FILENAME: &str = "AGENTS.md";

pub fn description() -> String {
    "Read the contents of a file or directory. Supports text files, images, and PDFs. Images and PDFs are presented directly to the model. Each text line is prefixed by its 1-based line number as <line>: <content>. The prefix is for reference and is not part of the file content. Directory entries are returned one per line. Use offset and limit to read large files or directories in sections. Prefer one larger read over many small slices, and use grep to find specific content in large files.".to_string()
}

pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "File or directory to read" },
            "offset": { "type": "integer", "minimum": 0, "description": "The line or directory entry to start reading from (1-based)" },
            "limit": { "type": "integer", "minimum": 0, "description": "The maximum number of lines or directory entries to read (defaults to and capped at 2000)" }
        },
        "required": ["path"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "read",
        description: description(),
        input: schema(),
        permission: None,
        codemode: false,
        namespace: None,
        execute: std::sync::Arc::new(ReadExecutor),
    }
}

// ---------------------------------------------------------------------------
// ReadToolFileSystem (read-filesystem.ts)
// ---------------------------------------------------------------------------

/// `ReadToolFileSystem.ReadError` messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotFound(PathBuf),
    /// `BinaryFileError`.
    Binary(String),
    /// `MediaIngestLimitError`.
    MediaLimit {
        resource: String,
        maximum_bytes: u64,
    },
    /// `OffsetOutOfRangeError`.
    OffsetOutOfRange {
        offset: u64,
        lines: u64,
    },
    /// `PathKindError`.
    PathKind {
        resource: String,
        expected: &'static str,
    },
    Io(String),
}

impl ReadError {
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            ReadError::NotFound(_) => "file not found".to_string(),
            ReadError::Binary(resource) => format!("Cannot read binary file: {resource}"),
            ReadError::MediaLimit { resource, maximum_bytes } => {
                format!("Media exceeds {maximum_bytes} byte ingestion limit: {resource}")
            }
            ReadError::OffsetOutOfRange { offset, lines } => format!(
                "Offset {offset} is out of range for this file ({lines} {})",
                if *lines == 1 { "line" } else { "lines" }
            ),
            ReadError::PathKind { resource, expected } => {
                format!("Path is not {expected}: {resource}")
            }
            ReadError::Io(message) => message.clone(),
        }
    }
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntry {
    pub path: String,
    pub kind: &'static str,
}

/// The read output union.
#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutput {
    File { uri: String, name: Option<String>, content: String, mime: String, base64: bool },
    TextPage { content: String, mime: String, offset: u64, truncated: bool, next: Option<u64> },
    ListPage { entries: Vec<ListEntry>, truncated: bool, next: Option<u64> },
}

/// `Mime.detect` — magic-byte sniffing for the supported media set.
fn detect_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"%PDF") {
        Some("application/pdf")
    } else {
        None
    }
}

/// `mimeType(path)` — extension fallback (mime-types `lookup`).
fn mime_from_path(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("txt" | "text" | "md" | "markdown") => "text/plain".to_string(),
        Some("json") => "application/json".to_string(),
        Some("js" | "mjs" | "cjs") => "text/javascript".to_string(),
        Some("ts" | "tsx") => "text/typescript".to_string(),
        Some("css") => "text/css".to_string(),
        Some("html" | "htm") => "text/html".to_string(),
        Some("xml") => "text/xml".to_string(),
        Some("yaml" | "yml") => "text/yaml".to_string(),
        Some("py") => "text/x-python".to_string(),
        Some("rs") => "text/rust".to_string(),
        Some("toml") => "text/toml".to_string(),
        Some("pdf") => "application/pdf".to_string(),
        Some("png") => "image/png".to_string(),
        Some("jpg" | "jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn file_url(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy())
}

/// `clampLine`.
fn clamp_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() > MAX_LINE_LENGTH {
        let prefix: String = chars[..MAX_LINE_LENGTH].iter().collect();
        format!("{prefix}{MAX_LINE_SUFFIX}")
    } else {
        line.to_string()
    }
}

/// Directory listing with the v2 ordering (directories first, then by
/// path) and paging.
fn list_page(dir: &Path, offset: u64, limit: u64) -> Result<ReadOutput, ReadError> {
    let entries = std::fs::read_dir(dir).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadError::NotFound(dir.to_path_buf()),
        _ => ReadError::Io(error.to_string()),
    })?;
    let mut visible: Vec<ListEntry> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => "directory",
            Ok(file_type) if file_type.is_symlink() => "symlink",
            Ok(_) => "file",
            Err(_) => continue,
        };
        let path = if kind == "directory" { format!("{name}/") } else { name };
        visible.push(ListEntry { path, kind });
    }
    visible.sort_by(|a, b| match (a.kind, b.kind) {
        ("directory", "directory") => a.path.cmp(&b.path),
        ("directory", _) => std::cmp::Ordering::Less,
        (_, "directory") => std::cmp::Ordering::Greater,
        _ => a.path.cmp(&b.path),
    });
    let start = (offset.saturating_sub(1)) as usize;
    let end = start.saturating_add(limit as usize).min(visible.len());
    let selected: Vec<ListEntry> =
        visible.get(start..end).map(|slice| slice.to_vec()).unwrap_or_default();
    let truncated = start + selected.len() < visible.len();
    Ok(ReadOutput::ListPage {
        next: truncated.then_some(offset + selected.len() as u64),
        entries: selected,
        truncated,
    })
}

/// The text-page budget (read-filesystem.ts `textPage`, eof-always variant
/// over the whole file).
fn text_page(
    bytes: &[u8],
    offset: u64,
    limit: u64,
) -> Result<(Vec<String>, Option<u64>), ReadError> {
    let decoded = String::from_utf8_lossy(bytes).into_owned();
    let mut split: Vec<&str> = decoded.split('\n').collect();
    if decoded.ends_with('\n') {
        split.pop();
    }
    let available: Vec<&str> = split.into_iter().collect();
    let mut entries: Vec<String> = Vec::new();
    let mut size = 0usize;
    let mut next: Option<u64> = None;
    for (index, value) in available.iter().skip(offset.saturating_sub(1) as usize).enumerate() {
        let line = offset + index as u64;
        if entries.len() >= limit as usize || size >= MAX_READ_BYTES {
            next = Some(line);
            break;
        }
        let text = clamp_line(value);
        let line_size = text.len() + usize::from(!entries.is_empty());
        if size + line_size > MAX_READ_BYTES {
            next = Some(line);
            break;
        }
        entries.push(text);
        size += line_size;
    }
    if next.is_none()
        && entries.len() >= limit as usize
        && offset - 1 + (entries.len() as u64) < available.len() as u64
    {
        next = Some(offset + entries.len() as u64);
    }
    Ok((entries, next))
}

/// `read` — the filesystem surface.
pub fn read_file(
    path: &Path,
    resource: &str,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<ReadOutput, ReadError> {
    // JS `page.offset || 1` — a zero offset is falsy and reads as 1.
    let paged_offset = offset.filter(|value| *value > 0).unwrap_or(1);
    let paged_limit = limit.map_or(MAX_READ_LINES as u64, |v| v.min(MAX_READ_LINES as u64));
    let symlink_metadata = std::fs::symlink_metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadError::NotFound(path.to_path_buf()),
        _ => ReadError::Io(error.to_string()),
    })?;
    let metadata = if symlink_metadata.file_type().is_symlink() {
        std::fs::metadata(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ReadError::NotFound(path.to_path_buf()),
            _ => ReadError::Io(error.to_string()),
        })?
    } else {
        symlink_metadata
    };
    if metadata.is_dir() {
        return list_page(path, paged_offset, paged_limit);
    }

    let bytes = std::fs::read(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadError::NotFound(path.to_path_buf()),
        _ => ReadError::Io(error.to_string()),
    })?;

    if let Some(media) = detect_mime(&bytes) {
        if metadata.len() > MAX_MEDIA_INGEST_BYTES {
            return Err(ReadError::MediaLimit {
                resource: resource.to_string(),
                maximum_bytes: MAX_MEDIA_INGEST_BYTES,
            });
        }
        return Ok(ReadOutput::File {
            uri: file_url(path),
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            content: base64::engine::general_purpose::STANDARD.encode(&bytes),
            mime: media.to_string(),
            base64: true,
        });
    }

    let paged = metadata.len() as usize > MAX_READ_BYTES || offset.is_some() || limit.is_some();
    if !paged {
        if bytes.contains(&0) {
            return Err(ReadError::Binary(resource.to_string()));
        }
        let decoded = String::from_utf8_lossy(&bytes).into_owned();
        let clamped = decoded.split('\n').map(clamp_line).collect::<Vec<_>>().join("\n");
        return Ok(ReadOutput::File {
            uri: file_url(path),
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            content: clamped,
            mime: mime_from_path(path),
            base64: false,
        });
    }

    if bytes.contains(&0) {
        // Binary check over the consumed span (eof-always ⇒ whole file).
        return Err(ReadError::Binary(resource.to_string()));
    }
    let (entries, next) = text_page(&bytes, paged_offset, paged_limit)?;
    let line_count = {
        let decoded = String::from_utf8_lossy(&bytes);
        let mut lines = decoded.split('\n').count() as u64;
        if decoded.ends_with('\n') {
            lines = lines.saturating_sub(1);
        }
        lines
    };
    if entries.is_empty() && paged_offset != 1 {
        return Err(ReadError::OffsetOutOfRange { offset: paged_offset, lines: line_count });
    }
    Ok(ReadOutput::TextPage {
        content: entries.join("\n"),
        mime: mime_from_path(path),
        offset: paged_offset,
        truncated: next.is_some(),
        next,
    })
}

// ---------------------------------------------------------------------------
// toModelContent (read.ts)
// ---------------------------------------------------------------------------

/// The structured `Output` union (read.ts `Output` = FileContent |
/// TextPage | ListPage) — REF-exact field names and casing for
/// codemode/plugin consumers.
#[must_use]
pub fn structured_output(output: &ReadOutput) -> Value {
    match output {
        ReadOutput::File { uri, name, content, mime, base64 } => {
            let mut value = serde_json::json!({
                "type": "file",
                "uri": uri,
                "content": content,
                "encoding": if *base64 { "base64" } else { "utf8" },
                "mime": mime,
            });
            if let Some(name) = name {
                value["name"] = Value::String(name.clone());
            }
            value
        }
        ReadOutput::TextPage { content, mime, offset, truncated, next } => {
            let mut value = serde_json::json!({
                "type": "text-page",
                "content": content,
                "mime": mime,
                "offset": offset,
                "truncated": truncated,
            });
            if let Some(next) = next {
                value["next"] = serde_json::json!(next);
            }
            value
        }
        ReadOutput::ListPage { entries, truncated, next } => {
            let mut value = serde_json::json!({
                "type": "list-page",
                "entries": entries.iter().map(|entry| serde_json::json!({
                    "path": entry.path,
                    "type": entry.kind,
                })).collect::<Vec<_>>(),
                "truncated": truncated,
            });
            if let Some(next) = next {
                value["next"] = serde_json::json!(next);
            }
            value
        }
    }
}

/// `toModelContent` — the exact model text for a read output.
#[must_use]
pub fn to_model_content(path: &str, offset: Option<u64>, output: &ReadOutput) -> String {
    match output {
        ReadOutput::File { content, mime, base64: true, .. } => {
            // Media handled by the executor (text + file part); text here is
            // only the summary line.
            let _ = (content, mime, path);
            String::new()
        }
        ReadOutput::File { content, .. } => {
            let text = content.strip_suffix('\n').unwrap_or(content);
            let lines: Vec<&str> =
                if content.is_empty() { Vec::new() } else { text.split('\n').collect() };
            let mut parts = vec![if lines.is_empty() {
                format!("Read file {path}, 0 lines")
            } else {
                format!("Read file {path}, lines 1-{}", lines.len())
            }];
            for (index, line) in lines.iter().enumerate() {
                parts.push(format!("{}: {line}", index + 1));
            }
            parts.join("\n")
        }
        ReadOutput::ListPage { entries, truncated, next } => {
            let start = offset.filter(|value| *value > 0).unwrap_or(1);
            let mut parts = vec![if entries.is_empty() {
                format!("Read directory {path}, 0 entries")
            } else {
                format!(
                    "Read directory {path}, entries {start}-{}",
                    start + entries.len() as u64 - 1
                )
            }];
            for entry in entries {
                parts.push(entry.path.clone());
            }
            if *truncated && next.is_some() {
                parts.push(format!(
                    "[Output truncated. Continue reading with offset: {}]",
                    next.unwrap_or(0)
                ));
            }
            parts.join("\n")
        }
        ReadOutput::TextPage { content, offset: page_offset, truncated, next, .. } => {
            let start = *page_offset;
            let lines: Vec<&str> =
                if content.is_empty() { Vec::new() } else { content.split('\n').collect() };
            let mut parts = vec![if lines.is_empty() {
                format!("Read file {path}, 0 lines")
            } else {
                format!("Read file {path}, lines {start}-{}", start + lines.len() as u64 - 1)
            }];
            for (index, line) in lines.iter().enumerate() {
                parts.push(format!("{}: {line}", start as usize + index));
            }
            if *truncated && next.is_some() {
                parts.push(format!(
                    "[Output truncated. Continue reading with offset: {}]",
                    next.unwrap_or(0)
                ));
            }
            parts.join("\n")
        }
    }
}

// ---------------------------------------------------------------------------
// The executor
// ---------------------------------------------------------------------------

struct ReadExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for ReadExecutor {
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

fn leaf_failure(path: &str, error: ReadError) -> ToolFailure {
    match error {
        ReadError::Binary(_)
        | ReadError::MediaLimit { .. }
        | ReadError::OffsetOutOfRange { .. }
        | ReadError::PathKind { .. } => ToolFailure {
            message: error.message(),
            cause: Some(super::FailureCause::Nested(error.message())),
        },
        ReadError::NotFound(absolute) => missing(path, &absolute),
        ReadError::Io(message) => ToolFailure {
            message: format!("Unable to read {path}"),
            cause: Some(super::FailureCause::Nested(message)),
        },
    }
}

/// `missing` — sibling suggestions.
fn missing(input: &str, absolute: &Path) -> ToolFailure {
    let base = absolute.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let parent = absolute.parent().unwrap_or_else(|| Path::new("/"));
    let mut suggestions: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let candidate = name.to_lowercase();
            if candidate.contains(&base) || base.contains(&candidate) {
                suggestions.push(join_display(input, &name));
            }
            if suggestions.len() == 3 {
                break;
            }
        }
    }
    let message = if suggestions.is_empty() {
        format!("File not found: {input}")
    } else {
        format!("File not found: {input}\n\nDid you mean one of these?\n{}", suggestions.join("\n"))
    };
    ToolFailure::message(message)
}

/// `join(dirname(input), entry)` — keeps the user's input spelling for the
/// directory part.
fn join_display(input: &str, entry: &str) -> String {
    let dirname = match input.rfind('/') {
        Some(index) => &input[..index],
        None => return entry.to_string(),
    };
    format!("{dirname}/{entry}")
}

/// `alternatePath` — recover a uniquely-matching sibling under NFC +
/// typography canonicalization.
fn alternate_path(absolute: &Path) -> Option<PathBuf> {
    let canonical = |name: &str| -> String {
        name.chars()
            .map(|c| match c {
                '\u{00A0}' | '\u{202F}' => ' ',
                '\u{2018}' | '\u{2019}' => '\'',
                _ => c,
            })
            .collect()
    };
    let base = canonical(&absolute.file_name()?.to_string_lossy());
    let dir = absolute.parent()?;
    let mut matches: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let kind = entry.file_type().ok()?;
        if !kind.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if canonical(&name) == base {
            matches.push(dir.join(name));
        }
    }
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let path = input["path"].as_str().unwrap_or_default().to_string();
    let offset = input["offset"].as_u64();
    let limit = input["limit"].as_u64();
    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };

    let requested =
        services.access.authorize_read(&path, &invocation, None).await.map_err(|error| {
            ToolFailure {
                message: format!("Unable to read {path}"),
                cause: Some(super::permission_cause(&error)),
            }
        })?;

    let outcome = match read_file(&requested.absolute, &requested.resource, offset, limit) {
        Ok(output) => (output, requested, path.clone()),
        Err(ReadError::NotFound(_)) => {
            let Some(alternate) = alternate_path(&requested.absolute) else {
                return Err(missing(&path, &requested.absolute));
            };
            let sibling = services
                .access
                .authorize_read(
                    &alternate.to_string_lossy(),
                    &invocation,
                    Some(&ReadOptions { sibling_of: requested.clone() }),
                )
                .await
                .map_err(|error| ToolFailure {
                    message: format!("Unable to read {path}"),
                    cause: Some(super::permission_cause(&error)),
                })?;
            match read_file(&sibling.absolute, &sibling.resource, offset, limit) {
                Ok(ReadOutput::ListPage { .. }) => return Err(missing(&path, &requested.absolute)),
                Ok(output) => (
                    output,
                    sibling,
                    join_display(
                        &path,
                        &alternate
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    ),
                ),
                Err(ReadError::NotFound(_)) => return Err(missing(&path, &requested.absolute)),
                Err(error) => return Err(leaf_failure(&path, error)),
            }
        }
        Err(error) => return Err(leaf_failure(&path, error)),
    };
    let (output, target, display_path) = outcome;

    // AGENTS.md side effect: walk UP from the read dir to the location root
    // (exclusive); inject discovered files as durable synthetic instructions.
    if target.external_directory.is_none() {
        if let Some(instructions) = &services.instructions {
            let start = match &output {
                ReadOutput::ListPage { .. } => target.absolute.clone(),
                _ => target
                    .absolute
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| target.absolute.clone()),
            };
            let mut candidates: Vec<PathBuf> = Vec::new();
            let root = services.directory.clone();
            let mut current: Option<&Path> = Some(&start);
            while let Some(dir) = current {
                if dir == root {
                    break;
                }
                let candidate = dir.join(AGENTS_FILENAME);
                if candidate.is_file() && candidate != target.absolute {
                    candidates.push(candidate);
                }
                current = dir.parent();
            }
            // fs.up walks start→root; candidates order is nearest-first.
            if !candidates.is_empty() {
                instructions.load(&context.session_id, &candidates);
            }
        }
    }

    if let ReadOutput::File { content, mime, base64: true, .. } = &output {
        if !MEDIA_MIMES.contains(&mime.as_str()) {
            return Err(leaf_failure(&path, ReadError::Binary(target.resource.clone())));
        }
        let summary = if mime == "application/pdf" {
            "PDF read successfully"
        } else {
            "Image read successfully"
        };
        return Ok(ToolOutcome {
            content: vec![
                Content::Text { text: summary.to_string() },
                Content::File {
                    uri: format!("data:{mime};base64,{content}"),
                    mime: mime.clone(),
                    name: Some(display_path.clone()),
                },
            ],
            metadata: Some(record_of(serde_json::json!({ "truncated": false }))),
            output: Some(structured_output(&output)),
        });
    }

    let text = to_model_content(&display_path, offset, &output);
    let truncated = match &output {
        ReadOutput::File { .. } => false,
        ReadOutput::TextPage { truncated, .. } | ReadOutput::ListPage { truncated, .. } => {
            *truncated
        }
    };
    Ok(ToolOutcome {
        content: vec![Content::Text { text }],
        metadata: Some(record_of(serde_json::json!({ "truncated": truncated }))),
        output: Some(structured_output(&output)),
    })
}

pub(crate) fn record_of(value: Value) -> oc_schema::common::Record {
    serde_json::from_value(value).unwrap_or_default()
}
