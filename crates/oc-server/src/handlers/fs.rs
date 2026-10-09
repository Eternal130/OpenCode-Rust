//! fs handlers (v2 `handlers/fs.ts`, 55 LOC): read (binary octet-stream,
//! core-detected mime; v2 resolve containment — lexical normalize, then
//! canonicalize vs the location root, escapes die 500), list/find
//! (dirs-first, locale-compare, trailing `/` on dirs; list entries are
//! location-relative like v2, find keeps the registered absolute-shape
//! gap), and the deliberately-escaping write (raw body, path query).

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{parse_query_field, percent_decode, ApiError, SharedState};
use super::session::{query_field, services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/experimental/fs/write", axum::routing::post(write))
        .route("/api/fs/find", axum::routing::get(find))
        .route("/api/fs/list", axum::routing::get(list))
        .route("/api/fs/read/*path", axum::routing::get(read))
}

fn resolve_location_for_fs(
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<oc_core::location::LocationRef, ApiError> {
    super::common::resolve_location(uri.query().unwrap_or_default(), headers)
}

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

fn mime_from_path(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("txt" | "text" | "md" | "markdown") => "text/plain",
        Some("json") => "application/json",
        Some("js" | "mjs" | "cjs") => "text/javascript",
        Some("ts" | "tsx") => "text/typescript",
        Some("css") => "text/css",
        Some("html" | "htm") => "text/html",
        Some("xml") => "text/xml",
        Some("yaml" | "yml") => "text/yaml",
        Some("py") => "text/x-python",
        Some("rs") => "text/rust",
        Some("toml") => "text/toml",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Node `path.resolve` semantics (v2 `FileSystem.resolve` first fence):
/// make `input` absolute against `base` (an absolute input REPLACES the
/// base), then lexically resolve `.`/`..` components without touching the
/// filesystem. At a root, `..` stays the root (`path.resolve("/..") === "/"`).
fn lexical_resolve(base: &std::path::Path, input: &str) -> std::path::PathBuf {
    let joined = if std::path::Path::new(input).is_absolute() {
        std::path::PathBuf::from(input)
    } else {
        base.join(input)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// `FSUtil.contains` on string paths (component-prefix, not textual).
fn contains(parent: &std::path::Path, child: &std::path::Path) -> bool {
    oc_util::pathx::contains_directory(&parent.to_string_lossy(), &child.to_string_lossy())
}

/// The boot-resolved location root (v2 `FileSystem` layer root): the
/// canonicalized location directory for plain locations; the configured
/// directory as-is for workspace-placed ones (the #44568 boot-only skip).
/// Resolved lazily per request — the directory exists by the time services
/// resolved, so the canonical identity matches the boot-time probe.
fn location_root(location: &oc_core::location::LocationRef) -> std::path::PathBuf {
    if location.workspace_id.is_some() {
        return location.as_path().to_path_buf();
    }
    std::fs::canonicalize(location.as_path()).unwrap_or_else(|_| location.as_path().to_path_buf())
}

/// v2 `FileSystem.resolve` for `fs.read`: the double fence. Errors mirror
/// the reference — a lexical or canonical escape dies (500
/// `UnknownError`); a missing path fails `NotFound` (404
/// `FileNotFoundError`).
async fn resolve_contained(
    location: &oc_core::location::LocationRef,
    relative: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let absolute = lexical_resolve(location.as_path(), relative);
    if !contains(location.as_path(), &absolute) {
        return Err(ApiError::unknown_ref());
    }
    let real = match tokio::fs::canonicalize(&absolute).await {
        Ok(real) => real,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                || error.kind() == std::io::ErrorKind::NotADirectory =>
        {
            return Err(ApiError::file_not_found(relative));
        }
        Err(_) => return Err(ApiError::unknown_ref()),
    };
    if !contains(&location_root(location), &real) {
        return Err(ApiError::unknown_ref());
    }
    Ok(real)
}

// ---------------------------------------------------------------------------
// GET /api/fs/read/*path — raw bytes, core-detected mime (contained; the
// list/find/write escapes are deliberate v2 behavior, deep-map §3)
// ---------------------------------------------------------------------------

async fn read(
    State(state): State<SharedState>,
    Path(path): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location_for_fs(&uri, &headers)?;
    let _services = services(&state, &location).await?;
    let relative = percent_decode(&path);
    let real = resolve_contained(&location, &relative).await?;
    let metadata =
        tokio::fs::metadata(&real).await.map_err(|_| ApiError::file_not_found(&relative))?;
    if !metadata.is_file() {
        // `Path is not a file` dies in v2 (500).
        return Err(ApiError::unknown_ref());
    }
    let bytes = tokio::fs::read(&real).await.map_err(|_| ApiError::file_not_found(&relative))?;
    let mime = detect_mime(&bytes).map_or_else(
        || {
            if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) || utf8_valid(&bytes) {
                "text/plain".to_owned()
            } else {
                mime_from_path(&real)
            }
        },
        String::from,
    );
    let _ = services;
    Ok(super::common::bytes_response(bytes, &mime))
}

fn utf8_valid(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

// ---------------------------------------------------------------------------
// GET /api/fs/list, /api/fs/find — dirs-first, locale-compare, trailing /
// ---------------------------------------------------------------------------

/// Node `path.relative(base, target)` for absolute inputs: common-prefix
/// split, then `..` per remaining base component + the target remainder.
fn path_relative(base: &std::path::Path, target: &std::path::Path) -> String {
    let base: Vec<_> = base.components().collect();
    let target: Vec<_> = target.components().collect();
    let mut shared = 0;
    while shared < base.len() && shared < target.len() && base[shared] == target[shared] {
        shared += 1;
    }
    let mut out = std::path::PathBuf::new();
    for _ in shared..base.len() {
        out.push("..");
    }
    for component in &target[shared..] {
        out.push(component.as_os_str());
    }
    out.to_string_lossy().into_owned()
}

fn entry_json(path: &str, is_dir: bool) -> Value {
    json!({
        "path": path,
        "type": if is_dir { "directory" } else { "file" },
    })
}

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location_for_fs(&uri, &headers)?;
    let _services = services(&state, &location).await?;
    let relative = query_field(&uri, "path").unwrap_or_default();
    let absolute = location.as_path().join(&relative);
    if !absolute.is_dir() {
        return Err(ApiError::file_not_found(&relative));
    }
    let mut entries = Vec::new();
    let mut read_dir =
        tokio::fs::read_dir(&absolute).await.map_err(|_| ApiError::file_not_found(&relative))?;
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let is_dir = entry.file_type().await.is_ok_and(|kind| kind.is_dir());
        let name = entry.file_name().to_string_lossy().into_owned();
        // v2 (filesystem.ts list): entries carry paths RELATIVE to the
        // location directory (`.` for the location root itself), dirs with
        // a trailing separator.
        let child = absolute.join(&name);
        let relative = path_relative(location.as_path(), &child);
        let mut path = if relative.is_empty() { ".".to_owned() } else { relative };
        if is_dir {
            path.push('/');
        }
        entries.push((!is_dir, name.to_lowercase(), entry_json(&path, is_dir)));
    }
    // Dirs first, then locale-compare names.
    entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let data: Vec<Value> = entries.into_iter().map(|(_, _, value)| value).collect();
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn find(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location_for_fs(&uri, &headers)?;
    let _services = services(&state, &location).await?;
    let Some(query) = query_field(&uri, "query") else {
        return Err(ApiError::invalid_request("query is required"));
    };
    let limit = query_field(&uri, "limit").and_then(|value| value.parse::<usize>().ok());
    let kind = query_field(&uri, "type");
    let (want_files, want_dirs) = match kind.as_deref() {
        Some("file") => (true, false),
        Some("directory") => (false, true),
        _ => (true, true),
    };

    let lowered = query.to_lowercase();
    let root = location.as_path().to_path_buf();
    let mut matches = Vec::new();
    walk(&root, &lowered, want_files, want_dirs, limit, &mut matches);

    let data: Vec<Value> = matches
        .into_iter()
        .map(|(path, is_dir)| entry_json(&path.to_string_lossy(), is_dir))
        .collect();
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

fn walk(
    directory: &std::path::Path,
    lowered_query: &str,
    want_files: bool,
    want_dirs: bool,
    limit: Option<usize>,
    out: &mut Vec<(std::path::PathBuf, bool)>,
) {
    if let Some(limit) = limit {
        if out.len() >= limit {
            return;
        }
    }
    let Ok(read_dir) = std::fs::read_dir(directory) else { return };
    let mut children: Vec<_> = read_dir.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for entry in children {
        let Ok(kind) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if kind.is_dir() {
            if want_dirs && name.contains(lowered_query) {
                out.push((entry.path(), true));
            }
            // Skip heavy/irrelevant trees.
            if name == ".git" || name == "node_modules" || name == "target" {
                continue;
            }
            walk(&entry.path(), lowered_query, want_files, want_dirs, limit, out);
        } else if want_files && name.contains(lowered_query) {
            out.push((entry.path(), false));
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/experimental/fs/write — raw octet-stream body, path query
// (deliberately escapes the workspace: staging into server tmp, §3)
// ---------------------------------------------------------------------------

async fn write(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let location = resolve_location_for_fs(&uri, &headers)?;
    let _services = services(&state, &location).await?;
    let relative = query_field(&uri, "path").unwrap_or_default();
    let absolute = location.as_path().join(&relative);
    if let Some(parent) = absolute.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|_| ApiError::unknown_ref())?;
    }
    tokio::fs::write(&absolute, &body).await.map_err(|_| ApiError::unknown_ref())?;
    Ok(super::common::json_location_data(
        &location.as_path().to_string_lossy(),
        json!({ "path": absolute.to_string_lossy() }),
    ))
}

/// Wildcard query re-parse helper (keeps `parse_query_field` referenced).
#[allow(dead_code)]
fn _query(uri: &Uri, name: &str) -> Option<String> {
    parse_query_field(uri.query().unwrap_or_default(), name)
}
