//! Prompt attachment materialization (R-07): the server half of
//! `core/session/prompt.ts`'s `materializeAttachment` — the client
//! POSTs `files` in the **`PromptInput.FileAttachment`** shape (a
//! `data:`/`file://` URI, ref `packages/schema/src/prompt-input.ts`);
//! the inbox stores the **resolved** `Prompt.FileAttachment`
//! (`{data: base64, mime, source}`, ref `packages/schema/src/prompt.ts`),
//! which the session runner lowers into provider image blocks
//! (`oc-core/src/session/runner/to_llm_message.rs`).
//!
//! Port scope (the image lane): `data:` URIs decode inline;
//! `file://` URIs read the file (bounded, ref
//! `MAX_ATTACHMENT_BYTES`) with mime detection by magic bytes
//! (`packages/core/src/mime.ts`). v2 additionally normalizes image
//! attachments through an image resizer
//! (`normalizeImageAttachment`) — when the resizer is unavailable v2
//! passes the bytes through unchanged; the Rust port has no resizer
//! service yet, so it always takes the passthrough arm. Text-range
//! (`?start=&end=`) and directory listings of `file://` URIs are not
//! part of the image lane and stay unported (surfaced as errors, not
//! silent drops).

use base64::Engine as _;
use oc_schema::prompt::{FileAttachment, FileSource, InputFileAttachment};

/// `MAX_ATTACHMENT_BYTES` (core/session/prompt.ts): 20 MiB.
pub const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;

/// Why a `file://` read failed (`AttachmentError` messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentError {
    InvalidUri(String),
    UnsupportedScheme(String),
    ReadFailed(String),
    TooLarge { uri: String, limit: u64 },
    InvalidBase64(String),
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUri(uri) => write!(f, "Invalid attachment URI: {uri}"),
            Self::UnsupportedScheme(uri) => write!(f, "Unsupported attachment URI: {uri}"),
            Self::ReadFailed(uri) => write!(f, "Unable to read attachment: {uri}"),
            Self::TooLarge { uri, limit } => {
                write!(f, "Attachment exceeds the {limit} byte limit: {uri}")
            }
            Self::InvalidBase64(uri) => write!(f, "Invalid data URL payload: {uri}"),
        }
    }
}

/// `Mime.detect` (core/src/mime.ts): sniff the magic bytes of the
/// image formats opencode accepts; unknown payloads fall back to
/// octet-stream (v2's `application/octet-stream` default).
#[must_use]
pub fn detect_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return "image/png";
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg";
    }
    if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return "image/gif";
    }
    if bytes.starts_with(&[0x42, 0x4D]) {
        return "image/bmp";
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return "image/avif";
        }
    }
    "application/octet-stream"
}

/// Decode a `data:<mime>;base64,<payload>` URL into `(mime, bytes)`.
fn decode_data_url(uri: &str) -> Result<(String, Vec<u8>), AttachmentError> {
    let rest =
        uri.strip_prefix("data:").ok_or_else(|| AttachmentError::InvalidUri(uri.to_owned()))?;
    let (header, payload) =
        rest.split_once(',').ok_or_else(|| AttachmentError::InvalidUri(uri.to_owned()))?;
    let mime = header.split(';').next().unwrap_or_default();
    if !header.to_ascii_lowercase().contains("base64") {
        // v2's decodeDataURL only handles the base64 encoding the
        // client produces.
        return Err(AttachmentError::InvalidUri(uri.to_owned()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|_| AttachmentError::InvalidBase64(uri.to_owned()))?;
    Ok((mime.to_owned(), bytes))
}

/// `fileURLToPath` for the Unix paths the server accepts (Windows
/// UNC forms stay with the TUI's paste parser).
fn file_url_to_path(uri: &str) -> Result<String, AttachmentError> {
    let url =
        uri.strip_prefix("file://").ok_or_else(|| AttachmentError::InvalidUri(uri.to_owned()))?;
    // Reject authority components other than localhost (v2 keeps the
    // remote-host form for Windows only, which we do not serve).
    let path = match url.split_once('/') {
        Some((host, _)) if !host.is_empty() && host != "localhost" => {
            return Err(AttachmentError::UnsupportedScheme(uri.to_owned()));
        }
        Some((_, rest)) => format!("/{rest}"),
        None => return Err(AttachmentError::InvalidUri(uri.to_owned())),
    };
    percent_decode(&path).ok_or_else(|| AttachmentError::InvalidUri(uri.to_owned()))
}

/// Percent-decode a path (rejecting `%2f`/`%5c` escapes like v2's
/// `decodeFileURL` guard).
fn percent_decode(value: &str) -> Option<String> {
    if value.contains("%2f")
        || value.contains("%2F")
        || value.contains("%5c")
        || value.contains("%5C")
    {
        return None;
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
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

/// Materialize one input attachment (the `data:`/`file://` arms of
/// `materializeAttachment`; text-range reads and directory listings
/// are outside the image lane).
pub fn materialize_attachment(
    input: &InputFileAttachment,
) -> Result<FileAttachment, AttachmentError> {
    let uri = &input.uri;
    let (bytes, source, name, mime) = if uri.starts_with("data:") {
        let (mime, bytes) = decode_data_url(uri)?;
        (bytes, FileSource::Inline, None, Some(mime))
    } else if uri.starts_with("file://") {
        let path = file_url_to_path(uri)?;
        if path.contains('?') || path.contains('#') {
            // `?start=&end=` text ranges: not the image lane.
            return Err(AttachmentError::UnsupportedScheme(uri.clone()));
        }
        let meta =
            std::fs::metadata(&path).map_err(|_| AttachmentError::ReadFailed(uri.clone()))?;
        if meta.is_dir() {
            return Err(AttachmentError::UnsupportedScheme(uri.clone()));
        }
        if meta.len() > MAX_ATTACHMENT_BYTES {
            return Err(AttachmentError::TooLarge {
                uri: uri.clone(),
                limit: MAX_ATTACHMENT_BYTES,
            });
        }
        let bytes = std::fs::read(&path).map_err(|_| AttachmentError::ReadFailed(uri.clone()))?;
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        (bytes, FileSource::Uri { uri: uri.clone() }, name, None)
    } else {
        return Err(AttachmentError::UnsupportedScheme(uri.clone()));
    };
    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(AttachmentError::TooLarge { uri: uri.clone(), limit: MAX_ATTACHMENT_BYTES });
    }
    let mime = mime.unwrap_or_else(|| detect_mime(&bytes).to_owned());
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(FileAttachment {
        data,
        mime,
        source,
        name: input.name.clone().or(name),
        description: input.description.clone(),
        mention: input.mention.clone(),
    })
}

/// Materialize the `files` array of a `session.prompt` body.
/// `None` (absent/null) passes through as `None`; a bad entry fails
/// the whole request (v2's `AttachmentError` rejects the prompt).
pub fn materialize_files(
    inputs: Option<&[InputFileAttachment]>,
) -> Result<Option<Vec<FileAttachment>>, AttachmentError> {
    let Some(inputs) = inputs else { return Ok(None) };
    if inputs.is_empty() {
        return Ok(None);
    }
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        files.push(materialize_attachment(input)?);
    }
    Ok(Some(files))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_PNG: &[u8] = include_bytes!("../../tests/fixtures/r07-tiny-4x2.png");

    #[test]
    fn detects_magic_bytes() {
        assert_eq!(detect_mime(TINY_PNG), "image/png");
        assert_eq!(detect_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), "image/jpeg");
        assert_eq!(detect_mime(b"GIF89a...."), "image/gif");
        assert_eq!(detect_mime(b"BM\x00"), "image/bmp");
        assert_eq!(detect_mime(b"\x00\x00\x00\x20ftypavif"), "image/avif");
        assert_eq!(detect_mime(b"plain text"), "application/octet-stream");
    }

    #[test]
    fn materializes_inline_data_urls() {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(TINY_PNG);
        let input = InputFileAttachment {
            uri: format!("data:image/png;base64,{encoded}"),
            name: Some("clipboard.png".into()),
            description: None,
            mention: None,
        };
        let file = materialize_attachment(&input).unwrap();
        assert_eq!(file.mime, "image/png");
        assert_eq!(file.data, encoded);
        assert_eq!(file.source, FileSource::Inline);
        assert_eq!(file.name.as_deref(), Some("clipboard.png"), "input name wins");
    }

    #[test]
    fn materializes_file_urls_with_sniffed_mime() {
        // Per-test scratch dir (user-writable regardless of who ran
        // the suite before — a fixed shared /tmp path collides across
        // uids).
        let dir = std::env::temp_dir().join(format!("oc-r07-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tiny.png");
        std::fs::write(&path, TINY_PNG).unwrap();
        let uri = format!("file://{}", path.display());
        let input =
            InputFileAttachment { uri: uri.clone(), name: None, description: None, mention: None };
        let file = materialize_attachment(&input).unwrap();
        assert_eq!(file.mime, "image/png", "sniffed from bytes, not the extension");
        assert_eq!(file.source, FileSource::Uri { uri });
        assert_eq!(file.name.as_deref(), Some("tiny.png"), "basename fills a missing name");
        std::fs::remove_file(&path).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn file_url_authority_rules() {
        assert_eq!(file_url_to_path("file:///tmp/a b.png").unwrap(), "/tmp/a b.png");
        assert_eq!(file_url_to_path("file://localhost/tmp/x.png").unwrap(), "/tmp/x.png");
        assert_eq!(
            file_url_to_path("file:///tmp/r%C3%A9sum%C3%A9.png").unwrap(),
            "/tmp/résumé.png"
        );
        assert!(matches!(
            file_url_to_path("file://nas/tmp/x.png"),
            Err(AttachmentError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            file_url_to_path("file:///a%2fb.png"),
            Err(AttachmentError::InvalidUri(_))
        ));
    }

    #[test]
    fn rejects_out_of_scope_and_broken_uris() {
        let bad = InputFileAttachment {
            uri: "https://example.test/a.png".into(),
            name: None,
            description: None,
            mention: None,
        };
        assert_eq!(
            materialize_attachment(&bad),
            Err(AttachmentError::UnsupportedScheme(bad.uri.clone()))
        );

        let broken = InputFileAttachment {
            uri: "data:image/png;base64,!!!not-base64!!!".into(),
            name: None,
            description: None,
            mention: None,
        };
        assert!(matches!(materialize_attachment(&broken), Err(AttachmentError::InvalidBase64(_))));

        let missing = InputFileAttachment {
            uri: "file:///nonexistent-r07/x.png".into(),
            name: None,
            description: None,
            mention: None,
        };
        assert!(matches!(materialize_attachment(&missing), Err(AttachmentError::ReadFailed(_))));
    }

    #[test]
    fn materialize_files_maps_none_and_rejects_partially() {
        assert_eq!(materialize_files(None).unwrap(), None);
        assert_eq!(materialize_files(Some(&[])).unwrap(), None);
        let good: InputFileAttachment =
            serde_json::from_value(serde_json::json!({"uri": "data:image/gif;base64,R0lGODlh"}))
                .unwrap();
        let files = materialize_files(Some(&[good])).unwrap().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].mime, "image/gif");
    }
}
