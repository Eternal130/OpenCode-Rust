//! Static asset serving (`cli/services/web-ui.ts` + `cli/app-assets.ts`).
//!
//! Route precedence: `/api`, `/api/*`, `/auth/*`, `/openapi.json` never reach
//! here (API dispatch owns them). Everything else is served BEFORE auth so a
//! signed-out browser still gets the app shell. Key = pathname minus the
//! leading `/`; exact hit serves the bytes, everything else falls back to
//! `index.html` (SPA) except `_assets/*` / `icons/*` misses (404 no-store).
//! GET/HEAD only (405); HEAD answers 200 with headers and an empty body.
//!
//! HTML responses carry a CSP whose `script-src` gains the SHA-256 hash of
//! the inline `oc-theme-preload-script` (hand-rolled port of the v2 regex —
//! this crate has no regex dependency).

use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::Response;
use sha2::{Digest, Sha256};

use crate::state::AppState;

/// The pinned `OpenAPI` document this server must serve byte-identical
/// (`tools/openapi-gen/openapi.json`).
pub static OPENAPI_JSON: &[u8] = include_bytes!("../../../tools/openapi-gen/openapi.json");

/// Serves `/openapi.json`: the pinned document, content-type application/json.
#[must_use]
pub fn openapi_response() -> Response {
    response_with(
        StatusCode::OK,
        "application/json",
        OPENAPI_JSON,
        &[("cache-control", "no-cache")],
    )
}

/// The asset dispatch (`serveUI`) — `request` is already auth-exempt.
#[must_use]
pub fn serve(path: &str, method: &Method, state: &AppState) -> Response {
    let key = path.strip_prefix('/').unwrap_or(path);
    let requested = state.assets.get(key).map(|entry| entry.value().clone());

    if (key.starts_with("_assets/") || key.starts_with("icons/")) && requested.is_none() {
        let mut response = crate::errors::empty(StatusCode::NOT_FOUND);
        response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return response;
    }

    let name = if requested.is_some() { key } else { "index.html" };
    let Some(file) =
        requested.or_else(|| state.assets.get("index.html").map(|entry| entry.value().clone()))
    else {
        return crate::errors::empty(StatusCode::NOT_FOUND);
    };

    if *method != Method::GET && *method != Method::HEAD {
        return crate::errors::empty(StatusCode::METHOD_NOT_ALLOWED);
    }

    let html = name == "index.html";
    let revalidate = html || name == "sw.js" || name == "registerSW.js";
    let content_type = mime_type(name);
    let cache_control = if revalidate { "no-cache" } else { "public, max-age=31536000, immutable" };
    let csp = if html { csp_for_html(&file) } else { csp(None) };

    if *method == Method::HEAD {
        return head_response(StatusCode::OK, content_type, cache_control, &csp, file.len());
    }
    response_with_csp(StatusCode::OK, content_type, &file, cache_control, &csp)
}

// ---------------------------------------------------------------------------
// CSP (`web-ui.ts csp/cspForHtml`)
// ---------------------------------------------------------------------------

/// The base CSP; `hash` (base64 SHA-256 of the preload script) is appended
/// to `script-src` when present.
#[must_use]
pub fn csp(hash: Option<&str>) -> String {
    let mut value = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'".to_owned();
    if let Some(hash) = hash {
        value.push_str(" 'sha256-");
        value.push_str(hash);
        value.push('\'');
    }
    value.push_str(
        "; style-src 'self' 'unsafe-inline'; img-src 'self' data: https: blob:; \
         font-src 'self' data:; media-src 'self' data:; connect-src * data: blob:; \
         worker-src 'self' blob:",
    );
    value
}

/// Computes the CSP for an HTML body: the SHA-256 (base64) of the inline
/// `<script id="oc-theme-preload-script">` content, when present.
#[must_use]
pub fn csp_for_html(body: &[u8]) -> String {
    csp(preload_script_hash(body).as_deref())
}

/// The v2 regex, hand-rolled:
/// `<script\b(?![^>]*\bsrc\s*=)[^>]*\bid=(["'])oc-theme-preload-script\1[^>]*>([\s\S]*?)<\/script>`
/// (case-insensitive; group 2 hashed). First match wins.
#[must_use]
pub fn preload_script_hash(body: &[u8]) -> Option<String> {
    let digest = Sha256::digest(preload_script_content(body)?);
    Some(base64_encode_standard(&digest))
}

fn base64_encode_standard(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Extracts the first matching inline script's content (the regex's group 2).
fn preload_script_content(body: &[u8]) -> Option<&[u8]> {
    let text = body;
    let mut cursor = 0;
    while let Some(offset) = find_case_insensitive(&text[cursor..], b"<script") {
        let start = cursor + offset;
        cursor = start + 1;
        // `\b`: the byte after `<script` must not be a word char.
        let after = text.get(start + 7).copied();
        if let Some(byte) = after {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                continue;
            }
        }
        let tag_open = start + 7;
        // Tag ends at the first '>'; `(?![^>]*\bsrc\s*=)` and `id=(["'])...\1`
        // both constrain themselves to the pre-'>' region.
        let Some(gt_rel) = find_byte(&text[tag_open..], b'>') else {
            continue;
        };
        let tag = &text[tag_open..tag_open + gt_rel];
        let content_start = tag_open + gt_rel + 1;

        // `src\s*=` present as a whole attribute → skip this candidate.
        if has_attribute(tag, b"src") {
            continue;
        }
        let Some(id_value) = attribute_value(tag, b"id") else {
            continue;
        };
        if id_value != b"oc-theme-preload-script" {
            continue;
        }
        // `([\s\S]*?)<\/script>` — lazy, case-insensitive close tag.
        let Some(close_rel) = find_case_insensitive(&text[content_start..], b"</script") else {
            continue;
        };
        return Some(&text[content_start..content_start + close_rel]);
    }
    None
}

fn find_byte(haystack: &[u8], needle: u8) -> Option<usize> {
    haystack.iter().position(|byte| *byte == needle)
}

fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(l, r)| l.eq_ignore_ascii_case(r))
}

fn find_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&offset| eq_ignore_ascii_case(&haystack[offset..offset + needle.len()], needle))
}

/// Whether a tag body carries `name` as a whole attribute with an optional
/// `= value` (`\bname\s*=`).
fn has_attribute(tag: &[u8], name: &[u8]) -> bool {
    attribute_offset(tag, name, true).is_some()
}

/// Reads `name="value"` / `name='value'` from a tag body (same quote on both
/// sides, per the `\1` backreference).
fn attribute_value<'a>(tag: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let offset = attribute_offset(tag, name, false)?;
    let mut cursor = offset + name.len();
    // `\s*=`
    while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= tag.len() || tag[cursor] != b'=' {
        return None;
    }
    cursor += 1;
    while cursor < tag.len() && tag[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let quote = tag.get(cursor).copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    cursor += 1;
    let end = tag[cursor..].iter().position(|byte| *byte == quote)? + cursor;
    Some(&tag[cursor..end])
}

/// Finds a whole-word attribute name; when `with_value` is set the attribute
/// must be followed by `\s*=` (the `src` lookahead shape).
fn attribute_offset(tag: &[u8], name: &[u8], with_value: bool) -> Option<usize> {
    let mut cursor = 0;
    while cursor < tag.len() {
        // Skip to the next word start.
        if !tag[cursor].is_ascii_alphanumeric() && tag[cursor] != b'_' {
            cursor += 1;
            continue;
        }
        let word_start = cursor;
        while cursor < tag.len()
            && (tag[cursor].is_ascii_alphanumeric() || tag[cursor] == b'_' || tag[cursor] == b'-')
        {
            cursor += 1;
        }
        let word = &tag[word_start..cursor];
        if eq_ignore_ascii_case(word, name) {
            if !with_value {
                return Some(word_start);
            }
            let mut probe = cursor;
            while probe < tag.len() && tag[probe].is_ascii_whitespace() {
                probe += 1;
            }
            if probe < tag.len() && tag[probe] == b'=' {
                return Some(word_start);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// MIME + response helpers
// ---------------------------------------------------------------------------

/// `FSUtil.mimeType`: extension lookup with an octet-stream fallback.
#[must_use]
pub fn mime_type(name: &str) -> &'static str {
    let extension = name.rsplit_once('.').map(|(_, tail)| tail.to_ascii_lowercase());
    match extension.as_deref() {
        Some("html" | "htm") => "text/html",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/vnd.microsoft.icon",
        Some("webmanifest") => "application/manifest+json",
        Some("txt") => "text/plain",
        Some("xml") => "application/xml",
        Some("wasm") => "application/wasm",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn response_with(
    status: StatusCode,
    content_type: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) -> Response {
    let content_type = HeaderValue::from_str(content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut response = Response::builder().status(status);
    {
        let map = response.headers_mut().expect("valid builder");
        map.insert(header::CONTENT_TYPE, content_type);
        for (name, value) in headers {
            if let (Ok(name), Ok(value)) =
                (axum::http::HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value))
            {
                map.insert(name, value);
            }
        }
    }
    response.body(axum::body::Body::from(body.to_vec())).unwrap_or_default()
}

fn response_with_csp(
    status: StatusCode,
    content_type: &str,
    body: &[u8],
    cache_control: &str,
    csp: &str,
) -> Response {
    response_with(
        status,
        content_type,
        body,
        &[
            ("cache-control", cache_control),
            ("content-security-policy", csp),
            ("x-content-type-options", "nosniff"),
        ],
    )
}

fn head_response(
    status: StatusCode,
    content_type: &str,
    cache_control: &str,
    csp: &str,
    length: usize,
) -> Response {
    let mut response = response_with_csp(status, content_type, &[], cache_control, csp);
    if let Ok(value) = HeaderValue::from_str(&length.to_string()) {
        response.headers_mut().insert(header::CONTENT_LENGTH, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_assets_openapi_bytes_match_pinned_sha() {
        use sha2::Digest;
        use std::fmt::Write as _;
        let digest = Sha256::digest(OPENAPI_JSON);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(hex, "{byte:02x}");
        }
        assert_eq!(hex, crate::routes_gen::OPENAPI_SHA256, "pinned openapi.json drifted");
    }

    #[test]
    fn server_assets_csp_base() {
        assert_eq!(
            csp(None),
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: https: blob:; font-src 'self' data:; media-src 'self' data:; \
             connect-src * data: blob:; worker-src 'self' blob:"
        );
    }

    const FIXTURE: &str = r#"<html><head>
<script src="/_assets/app.js"></script>
<script type="module" src="/_assets/other.js"></script>
<script id="oc-theme-preload-script">
  (() => { document.documentElement.dataset.theme = "dark" })()
</script>
<script>window.other = 1</script>
</head><body></body></html>"#;

    #[test]
    fn server_assets_csp_hashes_the_preload_script() {
        let expected = base64_encode_standard(&Sha256::digest(
            b"\n  (() => { document.documentElement.dataset.theme = \"dark\" })()\n",
        ));
        let csp = csp_for_html(FIXTURE.as_bytes());
        assert!(csp.contains(&format!("'sha256-{expected}'")), "csp was {csp}");
    }

    #[test]
    fn server_assets_csp_ignores_src_scripts_and_requires_exact_id() {
        // A src= script with the target id must NOT be hashed (negative lookahead).
        let src_only = br#"<script id="oc-theme-preload-script" src="/x.js">inline</script>"#;
        assert_eq!(preload_script_hash(src_only), None);
        // Same id on a different element shape still works case-insensitively.
        let upper = br"<SCRIPT ID='oc-theme-preload-script'>payload</SCRIPT>";
        assert_eq!(
            preload_script_hash(upper).as_deref(),
            Some(base64_encode_standard(&Sha256::digest(b"payload")).as_str())
        );
        // No preload script → no hash.
        assert_eq!(preload_script_hash(b"<html><body>x</body></html>"), None);
        // Unterminated script → no match.
        assert_eq!(preload_script_hash(b"<script id=\"oc-theme-preload-script\">forever"), None);
        // word-boundary: <scriptx ...> is not a script tag.
        assert_eq!(
            preload_script_hash(b"<scriptx id=\"oc-theme-preload-script\">payload</scriptx>"),
            None
        );
    }

    #[test]
    fn server_assets_lazy_close_tag_picks_first() {
        let nested = b"<script id=\"oc-theme-preload-script\">a</script>noise</script>";
        assert_eq!(
            preload_script_hash(nested).as_deref(),
            Some(base64_encode_standard(&Sha256::digest(b"a")).as_str())
        );
    }

    #[test]
    fn server_assets_mime_types() {
        assert_eq!(mime_type("index.html"), "text/html");
        assert_eq!(mime_type("a/b/app.js"), "text/javascript");
        assert_eq!(mime_type("style.CSS"), "text/css");
        assert_eq!(mime_type("manifest.webmanifest"), "application/manifest+json");
        assert_eq!(mime_type("logo.svg"), "image/svg+xml");
        assert_eq!(mime_type("noext"), "application/octet-stream");
        assert_eq!(mime_type("data.bin"), "application/octet-stream");
    }
}
