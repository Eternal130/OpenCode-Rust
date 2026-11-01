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

#[cfg(test)]
mod tests {
    use super::*;

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
