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

/// `FIRST_CHUNK` — the 256 KiB rope-chunk read granularity
/// (read-filesystem.ts:16).
const FIRST_CHUNK: usize = 256 * 1024;

/// One settled `textPage` result (read-filesystem.ts:290-313).
struct PageResult {
    entries: Vec<String>,
    offset: u64,
    next: Option<u64>,
    /// Byte offset (within the input) just past the last consumed line.
    consumed: usize,
}

/// The text-page budget (read-filesystem.ts `textPage`). Returns `None`
/// when the input is not eof and no `next` boundary settled (the caller
/// must read another chunk and retry).
fn text_page(bytes: &[u8], eof: bool, offset: u64, limit: u64) -> Option<PageResult> {
    let decoded = String::from_utf8_lossy(bytes);
    let mut split: Vec<&str> = decoded.split('\n').collect();
    // eof: a trailing newline ends the last line instead of starting an
    // empty one; non-eof: the last element is a (possibly empty) partial
    // line and is dropped.
    if !eof || split.last() == Some(&"") {
        split.pop();
    }
    let available: Vec<String> = split
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();

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
        && (!eof || offset - 1 + (entries.len() as u64) < available.len() as u64)
    {
        next = Some(offset + entries.len() as u64);
    }
    if !eof && next.is_none() {
        return None;
    }

    let consumed_lines = if next.is_none() { available.len() } else { (next.unwrap() - 1) as usize };
    let consumed = if consumed_lines == 0 {
        0
    } else {
        nth_newline(bytes, consumed_lines).unwrap_or(bytes.len())
    };
    Some(PageResult { entries, offset, next, consumed })
}

/// `textLeaf` — newline count of a chunk.
fn newline_count(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|&&byte| byte == b'\n').count() as u64
}

/// `nthNewline` — byte index just past the `count`-th `\n`.
fn nth_newline(bytes: &[u8], count: usize) -> Option<usize> {
    let mut found = 0;
    bytes.iter().enumerate().find_map(|(index, &byte)| {
        if byte != b'\n' {
            return None;
        }
        found += 1;
        (found == count).then_some(index + 1)
    })
}

/// `lineCount` — newlines plus a final unterminated line
/// (read-filesystem.ts:318).
fn total_line_count(newlines: u64, last: Option<u8>) -> u64 {
    match last {
        None | Some(b'\n') => newlines,
        Some(_) => newlines + 1,
    }
}

/// `textOffset` (rope order-statistic query) over the chunk list: byte
/// offset just past the `newline`-th newline, i.e. where line
/// `newline + 1` starts.
fn text_offset(leaves: &[Vec<u8>], newline: u64) -> usize {
    if newline == 0 {
        return 0;
    }
    let mut remaining = newline;
    let mut offset = 0usize;
    for leaf in leaves {
        let leaf_lines = newline_count(leaf);
        if remaining <= leaf_lines {
            return offset + nth_newline(leaf, remaining as usize).unwrap_or(leaf.len());
        }
        remaining -= leaf_lines;
        offset += leaf.len();
    }
    offset
}

/// Concatenate every leaf from byte `start` onward (the `selected`
/// buffer of the ref loop).
fn selected_from(leaves: &[Vec<u8>], start: usize) -> Vec<u8> {
    let mut position = 0usize;
    let mut selected = Vec::new();
    for leaf in leaves {
        let leaf_start = position;
        position += leaf.len();
        if position <= start {
            continue;
        }
        let skip = start.saturating_sub(leaf_start);
        selected.extend_from_slice(&leaf[skip..]);
    }
    selected
}

/// Per-leaf NUL scan over the consumed prefix only (read-filesystem.ts
/// :243-250) — the binary flag covers the consumed range, not the whole
/// file.
fn consumed_is_binary(leaves: &[Vec<u8>], consumed: usize) -> bool {
    let mut checked = 0usize;
    leaves.iter().any(|leaf| {
        let length = leaf.len().min(consumed.saturating_sub(checked));
        checked += leaf.len();
        length > 0 && leaf[..length].contains(&0)
    })
}

/// `makeTextPage` (read-filesystem.ts:265-283).
fn make_text_page(
    path: &Path,
    resource: &str,
    entries: Vec<String>,
    offset: u64,
    next: Option<u64>,
    binary: bool,
    lines: u64,
) -> Result<ReadOutput, ReadError> {
    if binary {
        return Err(ReadError::Binary(resource.to_string()));
    }
    if entries.is_empty() && offset != 1 {
        return Err(ReadError::OffsetOutOfRange { offset, lines });
    }
    Ok(ReadOutput::TextPage {
        content: entries.join("\n"),
        mime: mime_from_path(path),
        offset,
        truncated: next.is_some(),
        next,
    })
}

/// `read` — the filesystem surface (read-filesystem.ts:112-262), ported
/// onto 256 KiB chunk reads so large files never load whole into memory
/// unless the requested page requires it.
pub fn read_file(
    path: &Path,
    resource: &str,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<ReadOutput, ReadError> {
    let symlink_metadata = std::fs::symlink_metadata(path).map_err(read_error(path))?;
    let metadata = if symlink_metadata.file_type().is_symlink() {
        std::fs::metadata(path).map_err(read_error(path))?
    } else {
        symlink_metadata
    };
    if metadata.is_dir() {
        let paged_offset = offset.filter(|value| *value > 0).unwrap_or(1);
        let paged_limit = limit.map_or(MAX_READ_LINES as u64, |v| v.min(MAX_READ_LINES as u64));
        return list_page(path, paged_offset, paged_limit);
    }

    use std::io::{Read, Seek, SeekFrom};
    let mut file =
        std::fs::File::open(path).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => ReadError::NotFound(path.to_path_buf()),
            _ => ReadError::Io(error.to_string()),
        })?;
    let read_at = move |start: u64, length: usize| -> Result<Vec<u8>, ReadError> {
        file.seek(SeekFrom::Start(start)).map_err(|error| ReadError::Io(error.to_string()))?;
        let mut buffer = vec![0u8; length];
        let mut filled = 0usize;
        while filled < length {
            let n = file
                .read(&mut buffer[filled..])
                .map_err(|error| ReadError::Io(error.to_string()))?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buffer.truncate(filled);
        Ok(buffer)
    };
    read_file_with(metadata.len(), path, resource, offset, limit, read_at)
}

fn read_error(path: &Path) -> impl Fn(std::io::Error) -> ReadError + '_ {
    move |error| match error.kind() {
        std::io::ErrorKind::NotFound => ReadError::NotFound(path.to_path_buf()),
        _ => ReadError::Io(error.to_string()),
    }
}

/// The chunk-paged read pipeline (read-filesystem.ts:118-262). `read_at`
/// is the positioned-chunk seam (tests count calls to bound memory
/// shape); `size` is the file length.
pub fn read_file_with(
    size: u64,
    path: &Path,
    resource: &str,
    offset: Option<u64>,
    limit: Option<u64>,
    mut read_at: impl FnMut(u64, usize) -> Result<Vec<u8>, ReadError>,
) -> Result<ReadOutput, ReadError> {
    // JS `page.offset || 1` — a zero offset is falsy and reads as 1.
    let paged_offset = offset.filter(|value| *value > 0).unwrap_or(1);
    let paged_limit = limit.map_or(MAX_READ_LINES as u64, |v| v.min(MAX_READ_LINES as u64));

    let first = read_at(0, FIRST_CHUNK)?;

    let media = detect_mime(&first);
    if let Some(media) = media {
        if size > MAX_MEDIA_INGEST_BYTES {
            return Err(ReadError::MediaLimit {
                resource: resource.to_string(),
                maximum_bytes: MAX_MEDIA_INGEST_BYTES,
            });
        }
        let whole = if first.len() as u64 >= size { first } else { read_at(0, size as usize)? };
        return Ok(ReadOutput::File {
            uri: file_url(path),
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            content: base64::engine::general_purpose::STANDARD.encode(&whole),
            mime: media.to_string(),
            base64: true,
        });
    }

    let paged = size as usize > MAX_READ_BYTES || offset.is_some() || limit.is_some();
    if !paged {
        // The whole file fits the first chunk and no paging was requested.
        if first.contains(&0) {
            return Err(ReadError::Binary(resource.to_string()));
        }
        let decoded = String::from_utf8_lossy(&first).into_owned();
        let clamped = decoded.split('\n').map(clamp_line).collect::<Vec<_>>().join("\n");
        return Ok(ReadOutput::File {
            uri: file_url(path),
            name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            content: clamped,
            mime: mime_from_path(path),
            base64: false,
        });
    }

    // Complete first chunk: the whole file is already in memory — page
    // directly with eof=true (read-filesystem.ts:157-165).
    if first.len() as u64 >= size {
        let Some(result) = text_page(&first, true, paged_offset, paged_limit) else {
            return Err(ReadError::Io("Read page did not settle for a complete first chunk".into()));
        };
        let binary = first[..result.consumed].contains(&0);
        let lines = total_line_count(newline_count(&first), first.last().copied());
        return make_text_page(
            path,
            resource,
            result.entries,
            result.offset,
            result.next,
            binary,
            lines,
        );
    }

    // The chunk loop (read-filesystem.ts:169-257): accumulate 256 KiB
    // rope leaves until the requested line settles (or eof), then page
    // over the selected span.
    let mut leaves: Vec<Vec<u8>> = vec![first];
    let mut total_bytes;
    let mut total_lines;
    {
        let head = &leaves[0];
        total_bytes = head.len() as u64;
        total_lines = newline_count(head);
    }
    let mut ended = false;
    loop {
        let eof = ended || total_bytes >= size;
        if total_lines >= paged_offset - 1 || eof {
            let start = text_offset(&leaves, paged_offset - 1);
            let selected = selected_from(&leaves, start);
            if let Some(result) = text_page(&selected, eof, 1, paged_limit) {
                let next = result.next.map(|n| paged_offset + n - 1);
                let consumed = start + result.consumed;
                let binary = consumed_is_binary(&leaves, consumed);
                let last = leaves.last().and_then(|leaf| leaf.last().copied());
                let lines = total_line_count(total_lines, last);
                return make_text_page(
                    path,
                    resource,
                    result.entries,
                    paged_offset,
                    next,
                    binary,
                    lines,
                );
            }
        }

        let next_chunk = read_at(total_bytes, FIRST_CHUNK)?;
        if next_chunk.is_empty() {
            ended = true;
            continue;
        }
        total_lines += newline_count(&next_chunk);
        total_bytes += next_chunk.len() as u64;
        leaves.push(next_chunk);
    }
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
        // v2 iterates `fs.readDirectory` (raw `readdir` order, read.ts:154 —
        // no sort); `std::fs::read_dir` yields the same dirent order, so the
        // suggestion order matches the reference byte-for-behavior.
        let names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
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
