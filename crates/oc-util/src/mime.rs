//! Shared extension → MIME lookup — the G-92 convergence point.
//!
//! Before G-92 three private tables drifted apart: the oc-server asset
//! table (`FSUtil.mimeType`, the `mime-types` `lookup` + octet-stream
//! fallback), the oc-core read-tool table, and the oc-tui attachment
//! table (`mimeTypes`, local-attachment.ts:61). This module is their
//! union; each call site keeps its own fallback policy (octet-stream,
//! `None`, …) and, where a site's ref table is a restricted subset
//! (the attachment image/pdf filter), the site keeps the subset.
//!
//! Behavior notes vs the pre-G-92 tables:
//! * `xml` resolves to `application/xml` (the `mime-types` value the
//!   oc-server table already used); the oc-core read tool previously
//!   returned `text/xml` — a drift from its own `mime-types lookup`
//!   reference, now aligned.
//! * The oc-server asset path inherits the union's extra text/code
//!   mappings (previously octet-stream); its ref (`FSUtil.mimeType` →
//!   the full `mime-types` table) always had them, so this closes a
//!   divergence rather than opening one.

/// The universal fallback (`mime-types` contract).
pub const OCTET_STREAM: &str = "application/octet-stream";

/// The union table: a lowercase, dotless extension → MIME.
#[must_use]
pub fn from_extension(extension: &str) -> Option<&'static str> {
    let lower = extension.to_ascii_lowercase();
    let lower = lower.strip_prefix('.').unwrap_or(&lower);
    match lower {
        "html" | "htm" => Some("text/html"),
        "js" | "mjs" | "cjs" => Some("text/javascript"),
        "ts" | "tsx" => Some("text/typescript"),
        "css" => Some("text/css"),
        "json" | "map" => Some("application/json"),
        "txt" | "text" | "md" | "markdown" => Some("text/plain"),
        "xml" => Some("application/xml"),
        "yaml" | "yml" => Some("text/yaml"),
        "py" => Some("text/x-python"),
        "rs" => Some("text/rust"),
        "toml" => Some("text/toml"),
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        "ico" => Some("image/vnd.microsoft.icon"),
        "webmanifest" => Some("application/manifest+json"),
        "wasm" => Some("application/wasm"),
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "mp4" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mp3" => Some("audio/mpeg"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

/// Extracts the extension after the last `.` (`FSUtil.mimeType` /
/// `path.extname` semantics) and looks it up.
#[must_use]
pub fn from_path(path: &str) -> Option<&'static str> {
    path.rsplit_once('.').and_then(|(_, tail)| from_extension(tail))
}

/// `Mime.detect` (core mime.ts:3-21) — magic-byte sniff; PNG requires the
/// FULL 8-byte signature (`\x89PNG\r\n\x1a\n`), GIF is the 4-byte `GIF8`
/// prefix; `text/plain` for textual payloads (no NUL bytes, strict UTF-8,
/// ≤30% control bytes); fallback `application/octet-stream`. Empty
/// payloads are text. The single canonical sniffer — read.rs / prompt.rs
/// delegate here (G-382⑥: the private copies had drifted to a 4-byte PNG
/// and 6-byte `GIF87a`/`GIF89a` match).
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
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && matches!(&bytes[8..12], b"avif" | b"avis") {
        return "image/avif";
    }
    if is_text(bytes) {
        "text/plain"
    } else {
        "application/octet-stream"
    }
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
    let controls = bytes.iter().filter(|byte| **byte < 9 || (**byte > 13 && **byte < 32)).count();
    // ratio over counts: intentional usize->f64 (clippy::cast_precision_loss)
    #[allow(clippy::cast_precision_loss)]
    {
        controls as f64 / bytes.len() as f64 <= 0.3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_png_needs_full_signature_gif_is_4_bytes() {
        // mime.ts:4 matches all 8 PNG bytes; a 4-byte prefix alone is NOT
        // png (and 0x89 is not UTF-8, so it falls to octet-stream).
        assert_eq!(detect_mime(&[0x89, 0x50, 0x4E, 0x47]), "application/octet-stream");
        assert_eq!(detect_mime(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]), "image/png");
        // mime.ts:6 matches the `GIF8` prefix regardless of version bytes.
        assert_eq!(detect_mime(b"GIF8"), "image/gif");
        assert_eq!(detect_mime(b"GIF89a"), "image/gif");
    }

    #[test]
    fn union_table_lookups() {
        assert_eq!(from_extension("html"), Some("text/html"));
        assert_eq!(from_extension(".PNG"), Some("image/png"));
        assert_eq!(from_extension("JPEG"), Some("image/jpeg"));
        assert_eq!(from_extension("map"), Some("application/json"));
        assert_eq!(from_extension("webmanifest"), Some("application/manifest+json"));
        assert_eq!(from_extension("woff2"), Some("font/woff2"));
        assert_eq!(from_extension("wasm"), Some("application/wasm"));
        assert_eq!(from_extension("mp3"), Some("audio/mpeg"));
        assert_eq!(from_extension("pdf"), Some("application/pdf"));
        assert_eq!(from_extension("md"), Some("text/plain"));
        assert_eq!(from_extension("rs"), Some("text/rust"));
        assert_eq!(from_extension("xml"), Some("application/xml"));
        assert_eq!(from_extension("exe"), None);
        assert_eq!(from_extension(""), None);
    }

    #[test]
    fn path_extraction() {
        assert_eq!(from_path("index.html"), Some("text/html"));
        assert_eq!(from_path("a/b/app.js"), Some("text/javascript"));
        assert_eq!(from_path("style.CSS"), Some("text/css"));
        assert_eq!(from_path("noext"), None);
        assert_eq!(from_path("data.bin"), None);
    }
}
