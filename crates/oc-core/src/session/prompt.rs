//! Server-side prompt attachment materialization (core/session/prompt.ts
//! :93-231 + mime.ts) — the pipeline the `session.prompt` submit path
//! runs over the wire `PromptInput.FileAttachment` array.
//!
//! Ref pipeline (`materializeAttachment`, prompt.ts:93-133):
//!
//! 1. `attachmentLabel` (:206-208) — errors name the attachment
//!    (`input.name ?? (data: ? "inline attachment" : uri)`), never echo
//!    its bytes.
//! 2. `data:` URIs decode inline (`decodeDataURL`, :210-225): canonical
//!    base64 (`bytes→base64 === payload`) or `decodeURIComponent` for
//!    non-base64 payloads.
//! 3. `file://` URIs read through `readFileAttachment` (:151-201):
//!    `?start=`/`?end=` positive ints, stat, directory listing
//!    materialization (dirs first, `name + path.sep`, joined `\n`,
//!    `application/x-directory`), pre-read size cap, plain file read.
//! 4. `MAX_ATTACHMENT_BYTES` (20 MiB, :203) caps the resolved bytes.
//! 5. `Mime.detect` (mime.ts) — magic bytes, `text/plain` for textual
//!    payloads, else `application/octet-stream`.
//! 6. `mime === "text/plain" && start !== undefined` slices the decoded
//!    UTF-8 lines `[start-1, end)` (:114-123).
//! 7. `normalizeImageAttachment` (:135-149): `image/*` payloads go
//!    through `Image.normalize` (`crate::image::normalize`); a missing
//!    resizer (`None` limits) is the `Image.ResizerUnavailableError`
//!    passthrough arm.
//!
//! The oc-server `handlers/attachment.rs` module delegates here; the
//! resolved `Prompt.FileAttachment` shape is what the inbox stores.

use base64::Engine as _;
use oc_schema::prompt::{FileAttachment, FileSource, InputFileAttachment};

/// `MAX_ATTACHMENT_BYTES` (prompt.ts:203): 20 MiB.
pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;

/// `AttachmentError` — the ref error messages, byte-exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentError {
    /// `Invalid attachment data URL: ${label}` (:223).
    InvalidDataUrl { label: String },
    /// `Invalid attachment URI: ${uri}` (:155).
    InvalidUri { uri: String },
    /// `Unsupported attachment URI: ${uri}` (:158).
    UnsupportedUri { uri: String },
    /// `Invalid file URI: ${uri}` (:167).
    InvalidFileUri { uri: String },
    /// `Unable to read attachment: ${uri}` (:171/:175/:199).
    ReadFailed { uri: String },
    /// `Attachment is not a file: ${uri}` (:191).
    NotAFile { uri: String },
    /// `Attachment exceeds the ${limit} byte limit: ${label}` (:108-110,
    /// :193-195 — the stat-time check names the uri, the decoded-bytes
    /// check names the label).
    TooLarge { label: String, limit: usize },
    /// `normalizeImageAttachment` failures (`error.message`, :146).
    Image(String),
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDataUrl { label } => write!(f, "Invalid attachment data URL: {label}"),
            Self::InvalidUri { uri } => write!(f, "Invalid attachment URI: {uri}"),
            Self::UnsupportedUri { uri } => write!(f, "Unsupported attachment URI: {uri}"),
            Self::InvalidFileUri { uri } => write!(f, "Invalid file URI: {uri}"),
            Self::ReadFailed { uri } => write!(f, "Unable to read attachment: {uri}"),
            Self::NotAFile { uri } => write!(f, "Attachment is not a file: {uri}"),
            Self::TooLarge { label, limit } => {
                write!(f, "Attachment exceeds the {limit} byte limit: {label}")
            }
            Self::Image(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AttachmentError {}

/// `Mime.detect` (mime.ts): magic-byte sniff; `text/plain` for textual
/// payloads (no NUL bytes, strict UTF-8, ≤30% control bytes); fallback
/// `application/octet-stream`. Empty payloads are text.
#[must_use]
pub fn detect_mime(bytes: &[u8]) -> &'static str {
    fn starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
        bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix
    }
    if starts_with(bytes, &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return "image/png";
    }
    if starts_with(bytes, &[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg";
    }
    if starts_with(bytes, &[0x47, 0x49, 0x46, 0x38]) {
        return "image/gif";
    }
    if starts_with(bytes, &[0x42, 0x4D]) {
        return "image/bmp";
    }
    if starts_with(bytes, &[0x25, 0x50, 0x44, 0x46, 0x2D]) {
        return "application/pdf";
    }
    if bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
    {
        return "image/webp";
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        return "image/avif";
    }
    if is_text(bytes) { "text/plain" } else { "application/octet-stream" }
}

/// `isText` (mime.ts): empty ⇒ text; NUL bytes ⇒ binary; strict UTF-8
/// (`TextDecoder { fatal: true }`); ≤30% control bytes
/// (`< 9 || (> 13 && < 32)`).
fn is_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    if bytes.contains(&0) {
        return false;
    }
    if std::str::from_utf8(bytes).is_err() {
        return false;
    }
    let controls =
        bytes.iter().filter(|byte| **byte < 9 || (**byte > 13 && **byte < 32)).count();
    controls as f64 / bytes.len() as f64 <= 0.3
}

/// `attachmentLabel` (prompt.ts:206-208) — the data URL is the whole
/// file; errors and logs must name the attachment, not echo its bytes.
fn attachment_label(input: &InputFileAttachment) -> String {
    input.name.clone().unwrap_or_else(|| {
        if input.uri.starts_with("data:") {
            "inline attachment".to_string()
        } else {
            input.uri.clone()
        }
    })
}

/// `decodeDataURL` (prompt.ts:210-225): canonical base64 or
/// `decodeURIComponent` percent-decoding for non-base64 payloads.
fn decode_data_url(uri: &str, label: &str) -> Result<Vec<u8>, AttachmentError> {
    let invalid = || AttachmentError::InvalidDataUrl { label: label.to_string() };
    let Some(comma) = uri.find(',') else { return Err(invalid()) };
    let metadata = &uri[5..comma];
    let payload = &uri[comma + 1..];
    let base64_encoded =
        metadata.split(';').any(|part| part.to_ascii_lowercase() == "base64");
    if !base64_encoded {
        // decodeURIComponent: strict UTF-8 percent-decoding.
        return percent_decode(payload).ok_or_else(invalid).map(|text| text.into_bytes());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|_| invalid())?;
    // Non-canonical base64 is rejected (`bytes.toString("base64") !== payload`).
    if base64::engine::general_purpose::STANDARD.encode(&bytes) != payload {
        return Err(invalid());
    }
    Ok(bytes)
}

/// Percent-decode a UTF-8 payload (`decodeURIComponent`).
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The resolved half of one attachment (`readFileAttachment`'s return).
struct Resolved {
    bytes: Vec<u8>,
    source: FileSource,
    start: Option<usize>,
    end: Option<usize>,
    name: Option<String>,
    mime: Option<String>,
}

/// `positiveInt` (prompt.ts:227-231): integers > 0 only.
fn positive_int(value: Option<&str>) -> Option<usize> {
    let value = value?;
    let parsed: i64 = value.parse().ok()?;
    u64::try_from(parsed).ok().filter(|parsed| *parsed > 0).map(|parsed| parsed as usize)
}

/// `fileURLToPath` for the Unix paths the server accepts: percent-decode,
/// reject `%2f`/`%5c` escapes and non-localhost authorities.
fn file_url_to_path(uri: &str) -> Result<String, AttachmentError> {
    let url =
        uri.strip_prefix("file://").ok_or_else(|| AttachmentError::InvalidFileUri {
            uri: uri.to_string(),
        })?;
    let path = match url.split_once('/') {
        Some((host, _)) if !host.is_empty() && host != "localhost" => {
            return Err(AttachmentError::InvalidFileUri { uri: uri.to_string() });
        }
        Some((_, rest)) => format!("/{rest}"),
        None => {
            return Err(AttachmentError::InvalidFileUri { uri: uri.to_string() });
        }
    };
    if path.contains("%2f") || path.contains("%2F") || path.contains("%5c") || path.contains("%5C")
    {
        return Err(AttachmentError::InvalidFileUri { uri: uri.to_string() });
    }
    percent_decode(&path).ok_or_else(|| AttachmentError::InvalidFileUri { uri: uri.to_string() })
}

/// `readFileAttachment` (prompt.ts:151-201).
fn read_file_attachment(uri: &str) -> Result<Resolved, AttachmentError> {
    // `new URL(uri)` — a scheme-less or malformed URI is invalid (:153-156).
    let scheme_end = uri.find(':').ok_or_else(|| AttachmentError::InvalidUri {
        uri: uri.to_string(),
    })?;
    if scheme_end == 0
        || !uri[..scheme_end]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return Err(AttachmentError::InvalidUri { uri: uri.to_string() });
    }
    if &uri[..scheme_end] != "file" {
        return Err(AttachmentError::UnsupportedUri { uri: uri.to_string() });
    }
    // `url.searchParams` (`?start=&end=`) before the search/hash are
    // stripped for `fileURLToPath` (:159-168).
    let before_fragment = uri.split('#').next().unwrap_or(uri);
    let (path_part, query) = before_fragment.split_once('?').unwrap_or((before_fragment, ""));
    let mut start = None;
    let mut end = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = value.replace('+', " ");
        let value = percent_decode(&value).unwrap_or(value);
        if key == "start" {
            start = positive_int(Some(&value));
        } else if key == "end" {
            end = positive_int(Some(&value));
        }
    }
    let target = file_url_to_path(path_part)?;
    let info = std::fs::metadata(&target).map_err(|_| AttachmentError::ReadFailed {
        uri: uri.to_string(),
    })?;
    let basename = std::path::Path::new(&target)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    if info.is_dir() {
        // Directory listing (:172-190): file/directory entries only,
        // directories first (then locale name order), `name + path.sep`
        // per directory, joined `\n`, `application/x-directory`.
        let entries = std::fs::read_dir(&target).map_err(|_| AttachmentError::ReadFailed {
            uri: uri.to_string(),
        })?;
        let mut items: Vec<(bool, String)> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_type = entry.file_type().ok()?;
                // Node's `readdir` withFileTypes reports symlinks as
                // `symlink`; only `file`/`directory` survive the filter.
                if file_type.is_dir() {
                    Some((true, entry.file_name().to_string_lossy().into_owned()))
                } else if file_type.is_file() {
                    Some((false, entry.file_name().to_string_lossy().into_owned()))
                } else {
                    None
                }
            })
            .collect();
        items.sort_by(|a, b| {
            // `type === "directory" ? -1 : 1` on type mismatch, else
            // `name.localeCompare` — code-point order approximation.
            b.0
                .cmp(&a.0)
                .then_with(|| a.1.cmp(&b.1))
        });
        let separator = std::path::MAIN_SEPARATOR.to_string();
        let listing = items
            .into_iter()
            .map(|(is_dir, name)| if is_dir { format!("{name}{separator}") } else { name })
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(Resolved {
            bytes: listing.into_bytes(),
            source: FileSource::Uri { uri: uri.to_string() },
            start: None,
            end: None,
            name: basename,
            mime: Some("application/x-directory".to_string()),
        });
    }
    if !info.is_file() {
        return Err(AttachmentError::NotAFile { uri: uri.to_string() });
    }
    // The stat-time size cap (:192-196) names the uri, not the label.
    if info.len() as usize > MAX_ATTACHMENT_BYTES {
        return Err(AttachmentError::TooLarge {
            label: uri.to_string(),
            limit: MAX_ATTACHMENT_BYTES,
        });
    }
    let bytes = std::fs::read(&target).map_err(|_| AttachmentError::ReadFailed {
        uri: uri.to_string(),
    })?;
    Ok(Resolved {
        bytes,
        source: FileSource::Uri { uri: uri.to_string() },
        start,
        end,
        name: basename,
        mime: None,
    })
}

/// `materializeAttachment` (prompt.ts:93-133).
///
/// `image` supplies the `Image.Service` limits; `None` is the
/// `Image.ResizerUnavailableError` passthrough arm (:145).
pub fn materialize_attachment(
    input: &InputFileAttachment,
    image: Option<&crate::image::Limits>,
) -> Result<FileAttachment, AttachmentError> {
    let label = attachment_label(input);
    let resolved = if input.uri.starts_with("data:") {
        Resolved {
            bytes: decode_data_url(&input.uri, &label)?,
            source: FileSource::Inline,
            start: None,
            end: None,
            name: None,
            mime: None,
        }
    } else {
        read_file_attachment(&input.uri)?
    };
    // The decoded-bytes cap names the label (:107-111).
    if resolved.bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(AttachmentError::TooLarge { label, limit: MAX_ATTACHMENT_BYTES });
    }
    let mime =
        resolved.mime.clone().unwrap_or_else(|| detect_mime(&resolved.bytes).to_owned());
    // `mime === "text/plain" && start !== undefined` slices the decoded
    // UTF-8 lines `[start-1, end)` (:114-123; JS `slice(end)` with
    // `end === undefined` runs to the end).
    let content = if mime == "text/plain" && resolved.start.is_some() {
        let text = String::from_utf8_lossy(&resolved.bytes);
        let lines: Vec<&str> = text.split('\n').collect();
        let from = resolved.start.map(|start| start.saturating_sub(1)).unwrap_or(0);
        let to = resolved.end.unwrap_or(lines.len()).min(lines.len());
        let from = from.min(to);
        lines[from..to].join("\n").into_bytes()
    } else {
        resolved.bytes.clone()
    };
    let data = base64::engine::general_purpose::STANDARD.encode(&content);
    // `normalizeImageAttachment` (:135-149).
    let (data, mime) = if !mime.starts_with("image/") {
        (data, mime)
    } else if let Some(limits) = image {
        match crate::image::normalize(&label, &data, limits) {
            Ok(normalized) => {
                let mime = if normalized.mime.is_empty() { mime.clone() } else { normalized.mime };
                (normalized.base64, mime)
            }
            Err(error) => return Err(AttachmentError::Image(error.to_string())),
        }
    } else {
        (data, mime)
    };
    Ok(FileAttachment {
        data,
        mime,
        source: resolved.source,
        name: input.name.clone().or(resolved.name),
        description: input.description.clone(),
        mention: input.mention.clone(),
    })
}

/// Materialize the `files` array of a `session.prompt` body
/// (prompt.ts:53-55): `None` passes through, a bad entry fails the
/// whole prompt.
pub fn materialize_files(
    inputs: Option<&[InputFileAttachment]>,
    image: Option<&crate::image::Limits>,
) -> Result<Option<Vec<FileAttachment>>, AttachmentError> {
    let Some(inputs) = inputs else { return Ok(None) };
    if inputs.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        files.push(materialize_attachment(input, image)?);
    }
    Ok(Some(files))
}
