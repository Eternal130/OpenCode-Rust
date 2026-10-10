//! Composer attachment resolution (R-07) — the port of
//! `component/prompt/local-attachment.ts` + the label rules of
//! `prompt/attachment.ts`: pasted text that names local image (or
//! PDF) file(s) becomes inline `data:` attachments instead of literal
//! text, exactly v2's terminal-paste path. The *host clipboard* read
//! arm of v2's `prompt.paste` (Bun `createHostClipboard`, preferred
//! `image/png`) needs platform clipboard access that stays out of
//! scope here (see `crate::clipboard`'s OSC 52 note) — bracketed
//! paste of a file path is the supported paste entry.
//!
//! v2 refs: `resolvePastedAttachments` (local-attachment.ts:29),
//! `promptAttachmentLabel` (prompt/attachment.ts:76).

/// `MAX_PASTED_FILEPATHS` — bounds filesystem work per paste.
pub const MAX_PASTED_FILEPATHS: usize = 32;

/// `MAX_LOCAL_ATTACHMENT_BYTES` — 20 MiB per read; also bounds the
/// total staged data of a multi-path paste.
pub const MAX_LOCAL_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;

/// The extension → mime table (`mimeTypes`, local-attachment.ts:61).
#[must_use]
pub fn mime_from_extension(extension: &str) -> Option<&'static str> {
    let lower = extension.to_ascii_lowercase();
    match lower.as_str() {
        ".avif" => Some("image/avif"),
        ".gif" => Some("image/gif"),
        ".jpeg" | ".jpg" => Some("image/jpeg"),
        ".pdf" => Some("application/pdf"),
        ".png" => Some("image/png"),
        ".svg" => Some("image/svg+xml"),
        ".webp" => Some("image/webp"),
        _ => None,
    }
}

/// One staged attachment from a paste (the `resolvePastedAttachments`
/// return shape): SVGs stage as text, binaries as data URIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PastedAttachment {
    Text { filename: String, content: String },
    File { filename: String, uri: String },
}

/// The filesystem seam (`LocalFiles`) — production reads `std::fs`
/// bounded; tests inject an in-memory map. `mime` returns `None` for
/// unknown extensions (v2 falls through to `application/octet-stream`
/// which then fails the image/pdf filter).
pub trait LocalFiles {
    fn read_bytes(&self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>>;
    fn mime(&self, path: &str) -> Option<String>;
}

/// The production `LocalFiles`: bounded `std::fs::read` (the size
/// check before reading keeps the 20 MiB cap honest on huge files).
pub struct FsLocalFiles;

impl LocalFiles for FsLocalFiles {
    fn read_bytes(&self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        let meta = std::fs::metadata(path)?;
        if meta.len() > max_bytes as u64 || meta.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "too large"));
        }
        std::fs::read(path)
    }

    fn mime(&self, path: &str) -> Option<String> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;
        mime_from_extension(&format!(".{extension}")).map(str::to_owned)
    }
}

/// `readLocalAttachmentWith`: read one candidate if its extension
/// marks it a supported attachment; `None` = not an attachment (the
/// paste falls through to text).
fn read_local_attachment(
    files: &dyn LocalFiles,
    path: &str,
    max_bytes: usize,
) -> Option<PastedAttachment> {
    let mime = files.mime(path)?;
    if !mime.starts_with("image/") && mime != "application/pdf" {
        return None;
    }
    if mime == "image/svg+xml" {
        let bytes = files.read_bytes(path, max_bytes).ok()?;
        let content = String::from_utf8(bytes).ok()?;
        if content.len() > max_bytes {
            return None;
        }
        return Some(PastedAttachment::Text { filename: filename_of(path), content });
    }
    let bytes = files.read_bytes(path, max_bytes).ok()?;
    if bytes.len() > max_bytes {
        return None;
    }
    Some(PastedAttachment::File {
        filename: filename_of(path),
        uri: format!("data:{mime};base64,{}", crate::clipboard::base64_encode::encode(&bytes)),
    })
}

/// `path.basename`.
fn filename_of(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_owned()
}

/// `normalizePastedFilepath` (local-attachment.ts:81): strip wrapping
/// quotes, decode `file://` URLs, unescape `\x` sequences off win32.
#[must_use]
pub fn normalize_pasted_filepath(value: &str, platform: &str) -> String {
    let raw = value.trim_matches(|c| c == '\'' || c == '"');
    if let Some(url) = decode_file_url(raw, platform) {
        return url;
    }
    if platform == "win32" {
        return raw.to_owned();
    }
    // `raw.replace(/\\(.)/g, "$1")` — a backslash yields the next
    // character (a trailing backslash yields nothing).
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            out.extend(chars.next());
        } else {
            out.push(character);
        }
    }
    out
}

/// `decodeFileURL` (local-attachment.ts:89) — `file://` → path,
/// rejecting `%2f`/`%5c` escapes and non-local hosts.
#[must_use]
pub fn decode_file_url(value: &str, platform: &str) -> Option<String> {
    if !value.starts_with("file://") {
        return None;
    }
    let rest = &value["file://".len()..];
    if rest.contains("%2f") || rest.contains("%2F") || rest.contains("%5c") || rest.contains("%5C")
    {
        return None;
    }
    let (host, encoded) = rest.split_once('/')?;
    if !host.is_empty() && host != "localhost" {
        if platform != "win32" {
            return None;
        }
        // Windows UNC (`\\host\share\...`) stays out of the Rust TUI's
        // supported surface (v2 keeps it; we run on unix).
        return None;
    }
    let decoded = percent_decode(&format!("/{encoded}"))?;
    Some(decoded)
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `parsePastedFilepaths` (local-attachment.ts:107): shell-style
/// splitting with quote/escape handling; `file://` pastes drop
/// comment lines first. More than [`MAX_PASTED_FILEPATHS`] paths, or
/// an unterminated quote, yields none (v2's `[]`).
#[must_use]
pub fn parse_pasted_filepaths(value: &str, platform: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    let push = |result: &mut Vec<String>, current: &mut String| {
        if current.is_empty() {
            return;
        }
        let decoded = decode_file_url(current, platform).unwrap_or_else(|| current.clone());
        result.push(decoded);
        current.clear();
    };

    let input = if value.contains("file://") {
        value
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        value.to_owned()
    };

    let chars: Vec<char> = input.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        if let Some(open) = quote {
            if character == open {
                quote = None;
                index += 1;
                continue;
            }
            if character == '\\' && platform != "win32" && open == '"' && index + 1 < chars.len() {
                current.push(chars[index + 1]);
                index += 2;
                continue;
            }
            current.push(character);
            index += 1;
            continue;
        }
        if character == '\'' || character == '"' {
            quote = Some(character);
            index += 1;
            continue;
        }
        if character == '\\' && platform != "win32" && index + 1 < chars.len() {
            current.push(chars[index + 1]);
            index += 2;
            continue;
        }
        if character.is_whitespace() {
            push(&mut result, &mut current);
            if result.len() > MAX_PASTED_FILEPATHS {
                return Vec::new();
            }
            index += 1;
            continue;
        }
        current.push(character);
        index += 1;
    }
    if quote.is_some() {
        return Vec::new();
    }
    push(&mut result, &mut current);
    if result.len() > MAX_PASTED_FILEPATHS {
        return Vec::new();
    }
    result
}

/// `resolvePastedAttachments` (local-attachment.ts:29): pasted text
/// naming one file (or several whitespace-separated paths) becomes
/// attachments; `None` = plain text, insert verbatim. HTTP(S) URLs
/// never resolve (v2's explicit skip).
pub fn resolve_pasted_attachments(
    text: &str,
    platform: &str,
    files: &dyn LocalFiles,
) -> Option<Vec<PastedAttachment>> {
    let pasted = text.trim();
    let filepath = normalize_pasted_filepath(pasted, platform);
    if filepath.starts_with("http://") || filepath.starts_with("https://") {
        return None;
    }

    let mut attachments = Vec::new();
    let single = read_local_attachment(files, &filepath, MAX_LOCAL_ATTACHMENT_BYTES);
    if let Some(attachment) = single {
        attachments.push(attachment);
    } else {
        let filepaths = parse_pasted_filepaths(pasted, platform);
        if filepaths.len() <= 1 {
            return None;
        }
        let mut remaining = MAX_LOCAL_ATTACHMENT_BYTES;
        for candidate in &filepaths {
            let next = read_local_attachment(files, candidate, remaining)?;
            remaining = remaining.saturating_sub(match &next {
                PastedAttachment::Text { content, .. } => content.len(),
                PastedAttachment::File { uri, .. } => uri.len(),
            });
            attachments.push(next);
        }
    }
    Some(attachments)
}

/// `promptAttachmentLabel` (prompt/attachment.ts:76): the chip text a
/// new attachment gets — `[Image N]` / `[PDF N]`, numbered past the
/// highest existing `[Kind N]` mention, unless the same payload was
/// attached before (then its mention is reused).
#[must_use]
pub fn prompt_attachment_label(
    existing: &[super::FileMention],
    payload: &Option<super::AttachmentPayload>,
    name: Option<&str>,
) -> String {
    let Some(payload) = payload else {
        return name.unwrap_or("file").to_owned();
    };
    let kind = if payload.mime.starts_with("image/") { "Image" } else { "PDF" };
    // Reuse: the identical payload already staged → its mention.
    if let Some(file) = existing.iter().find(|file| {
        file.payload
            .as_ref()
            .is_some_and(|other| other.data == payload.data && other.mime == payload.mime)
    }) {
        return file.mention_text();
    }
    let pattern_prefix = format!("[{kind} ");
    let count = existing.iter().fold(0usize, |highest, file| {
        file.mention_text()
            .strip_prefix(&pattern_prefix)
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|digits| digits.parse::<usize>().ok())
            .map_or(highest, |n| n.max(highest))
    });
    format!("[{kind} {}]", count + 1)
}

/// In-memory [`LocalFiles`] for tests: path → bytes (+ per-path mime
/// override via the extension map).
#[cfg(test)]
pub(crate) struct MapFiles {
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
}

#[cfg(test)]
impl LocalFiles for MapFiles {
    fn read_bytes(&self, path: &str, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "missing"))?;
        if bytes.len() > max_bytes {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "too large"));
        }
        Ok(bytes.clone())
    }

    fn mime(&self, path: &str) -> Option<String> {
        let extension = std::path::Path::new(path).extension()?.to_str()?;
        mime_from_extension(&format!(".{extension}")).map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> MapFiles {
        MapFiles {
            files: [
                ("/tmp/shot.png".to_owned(), b"\x89PNG fake".to_vec()),
                ("/tmp/shot.jpeg".to_owned(), b"\xff\xd8\xff fake".to_vec()),
                ("/tmp/doc.txt".to_owned(), b"plain text".to_vec()),
                ("/tmp/pic.svg".to_owned(), b"<svg></svg>".to_vec()),
                ("/tmp/big.pdf".to_owned(), vec![0u8; 64]),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn single_image_path_resolves_to_a_data_uri() {
        let map = files();
        let resolved = resolve_pasted_attachments("/tmp/shot.png", "linux", &map).unwrap();
        assert_eq!(resolved.len(), 1);
        match &resolved[0] {
            PastedAttachment::File { filename, uri } => {
                assert_eq!(filename, "shot.png");
                assert_eq!(
                    uri,
                    &format!(
                        "data:image/png;base64,{}",
                        crate::clipboard::base64_encode::encode(b"\x89PNG fake")
                    )
                );
            }
            other @ PastedAttachment::Text { .. } => panic!("expected file, got {other:?}"),
        }
    }

    #[test]
    fn file_url_and_quoted_paths_decode() {
        let map = files();
        assert!(resolve_pasted_attachments("file:///tmp/shot.png", "linux", &map).is_some());
        assert!(resolve_pasted_attachments("'/tmp/shot.png'", "linux", &map).is_some());
        // Escaped spaces survive the shell-style split.
        assert_eq!(
            parse_pasted_filepaths("/tmp/my\\ shot.png /tmp/shot.png", "linux"),
            vec!["/tmp/my shot.png", "/tmp/shot.png"]
        );
    }

    #[test]
    fn non_attachment_text_and_urls_do_not_resolve() {
        let map = files();
        assert_eq!(resolve_pasted_attachments("hello world", "linux", &map), None);
        assert_eq!(resolve_pasted_attachments("/tmp/doc.txt", "linux", &map), None);
        assert_eq!(resolve_pasted_attachments("https://example.test/cat.png", "linux", &map), None);
        // One non-attachment path among several → none (v2 returns
        // undefined when any candidate fails).
        assert_eq!(resolve_pasted_attachments("/tmp/shot.png /tmp/doc.txt", "linux", &map), None);
    }

    #[test]
    fn svg_stages_as_text() {
        let map = files();
        let resolved = resolve_pasted_attachments("/tmp/pic.svg", "linux", &map).unwrap();
        assert_eq!(
            resolved[0],
            PastedAttachment::Text { filename: "pic.svg".into(), content: "<svg></svg>".into() }
        );
    }

    #[test]
    fn multi_path_pastes_stage_all() {
        let map = files();
        let resolved =
            resolve_pasted_attachments("/tmp/shot.png /tmp/shot.jpeg /tmp/big.pdf", "linux", &map)
                .unwrap();
        assert_eq!(resolved.len(), 3);
    }

    #[test]
    fn oversize_reads_reject() {
        let map = files();
        // The 20 MiB default budget accepts the 64-byte fixture…
        assert!(read_local_attachment(&map, "/tmp/big.pdf", MAX_LOCAL_ATTACHMENT_BYTES).is_some());
        // …an explicit small budget rejects it (v2's bounded read).
        assert!(read_local_attachment(&map, "/tmp/big.pdf", 8).is_none());
    }

    #[test]
    fn unterminated_quotes_yield_no_paths() {
        assert_eq!(parse_pasted_filepaths("'/tmp/a.png", "linux"), Vec::<String>::new());
        assert_eq!(parse_pasted_filepaths("a.png b.png", "linux"), vec!["a.png", "b.png"]);
        assert_eq!(parse_pasted_filepaths("\"/tmp/a b.png\"", "linux"), vec!["/tmp/a b.png"]);
    }

    #[test]
    fn file_url_pastes_drop_comment_lines() {
        let parsed = parse_pasted_filepaths("# comment\nfile:///tmp/a.png", "linux");
        assert_eq!(parsed, vec!["/tmp/a.png"]);
    }

    #[test]
    fn labels_number_past_existing_mentions_and_reuse_payloads() {
        use super::super::{AttachmentPayload, FileMention};
        let png = AttachmentPayload { mime: "image/png".into(), data: "AAECAw==".into() };
        let staged = vec![
            FileMention {
                name: "one.png".into(),
                start: 0,
                end: 9,
                payload: Some(AttachmentPayload {
                    mime: "image/png".into(),
                    data: "JVBERi0=".into(),
                }),
                mention: Some("[Image 1]".into()),
            },
            FileMention {
                name: "two.png".into(),
                start: 0,
                end: 9,
                payload: Some(png.clone()),
                mention: Some("[Image 2]".into()),
            },
        ];
        // Same payload again → its own staged mention is reused.
        assert_eq!(prompt_attachment_label(&staged, &Some(png), Some("two.png")), "[Image 2]");
        // New payload numbers past the highest existing image.
        let fresh = AttachmentPayload { mime: "image/png".into(), data: "////////".into() };
        assert_eq!(prompt_attachment_label(&staged, &Some(fresh), None), "[Image 3]");
        // PDFs number their own series.
        let pdf = AttachmentPayload { mime: "application/pdf".into(), data: "AAAA".into() };
        assert_eq!(prompt_attachment_label(&staged, &Some(pdf), None), "[PDF 1]");
        // Mentions without payloads (plain @-file chips) label by name.
        assert_eq!(prompt_attachment_label(&staged, &None, Some("notes.md")), "notes.md");
    }
}
