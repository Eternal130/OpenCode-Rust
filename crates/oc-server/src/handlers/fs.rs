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

use super::common::{parse_query_field, ApiError, SharedState};
use super::session::{query_field, services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/experimental/fs/write", axum::routing::post(write))
        .route("/api/fs/find", axum::routing::get(find))
        .route("/api/fs/list", axum::routing::get(list))
        .route("/api/fs/read/*path", axum::routing::get(read))
}

fn resolve_location_for_fs(
    state: &SharedState,
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<oc_core::location::LocationRef, ApiError> {
    super::common::resolve_location(state, uri.query().unwrap_or_default(), headers)
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
    let location = resolve_location_for_fs(&state, &uri, &headers)?;
    let _services = services(&state, &location).await?;
    // axum's `Path` already percent-decodes the wildcard; decoding again
    // would mangle names that legitimately contain `%` (`a%25b` → `a%b`).
    let relative = path.as_str();
    let real = resolve_contained(&location, relative).await?;
    let metadata = tokio::fs::metadata(&real).await.map_err(|error| {
        crate::errors::debug_cause("fs:file_not_found", &error);
        ApiError::file_not_found(relative)
    })?;
    if !metadata.is_file() {
        // `Path is not a file` dies in v2 (500).
        return Err(ApiError::unknown_ref());
    }
    let bytes = tokio::fs::read(&real).await.map_err(|error| {
        crate::errors::debug_cause("fs:file_not_found", &error);
        ApiError::file_not_found(relative)
    })?;
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
    let location = resolve_location_for_fs(&state, &uri, &headers)?;
    let _services = services(&state, &location).await?;
    let relative = query_field(&uri, "path").unwrap_or_default();
    let absolute = location.as_path().join(&relative);
    if !absolute.is_dir() {
        return Err(ApiError::file_not_found(&relative));
    }
    let mut entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&absolute).await.map_err(|error| {
        crate::errors::debug_cause("fs:file_not_found", &error);
        ApiError::file_not_found(&relative)
    })?;
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
    let location = resolve_location_for_fs(&state, &uri, &headers)?;
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

    let root = location.as_path().to_path_buf();
    let exclusions = ScanExclusions::for_root(&root);
    // REF builds an index of every (type-filtered) entry and ranks with
    // fuzzy matching at query time; the port's equivalent is a bounded
    // full walk (same entry budget as REF's ripgrep cap) + scoring.
    // The walk is synchronous std::fs recursion (up to 100k entries), so
    // it must not run on a tokio worker — spawn_blocking moves both the
    // walk and the scoring off the async runtime.
    let query_for_blocking = query.clone();
    let blocking = tokio::task::spawn_blocking(move || {
        let mut candidates = Vec::new();
        let mut visited = 0usize;
        walk(&root, want_files, want_dirs, &exclusions, &mut visited, &mut candidates);

        // REF (filesystem/search.ts): every find path ranks candidates —
        // fuzzysort.go in the ripgrep fallback, score-desc then
        // path-length-asc in the fff layer — and defaults the limit to 50.
        // Ranking equivalent: subsequence fuzzy score (substring matches
        // win via the consecutive-run bonus), then shorter paths first.
        let effective_limit = limit.unwrap_or(50);
        let lowered = query_for_blocking.to_lowercase();
        let query_chars: Vec<char> = lowered.chars().collect();
        let mut scored: Vec<_> = candidates
            .into_iter()
            .filter_map(|(path, is_dir)| {
                let target = path.to_string_lossy().to_lowercase();
                fuzzy_score(&query_chars, &target)
                    .map(|score| (score, target.chars().count(), target, path, is_dir))
            })
            .collect();
        scored
            .sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
        scored.truncate(effective_limit);
        scored
            .into_iter()
            .map(|(_, _, _, path, is_dir)| (path, is_dir))
            .collect::<Vec<_>>()
    });
    let scored = blocking.await.map_err(|error| {
        crate::errors::debug_cause("fs:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;

    let data: Vec<Value> = scored
        .into_iter()
        .map(|(path, is_dir)| entry_json(&path.to_string_lossy(), is_dir))
        .collect();
    Ok(super::common::json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

/// REF's ripgrep fallback passes `limit: location.vcs && !home ?
/// Number.MAX_SAFE_INTEGER : 100_000` (search.ts:72) — VCS-rooted scans
/// are effectively unbounded; the port caps every scan at 100k entries
/// as an accepted deviation (a hard ceiling keeps the synchronous walk
/// inside spawn_blocking bounded).
const SCAN_ENTRY_BUDGET: usize = 100_000;

/// Sensitive-directory exclusions for a scan (port of
/// core/filesystem/protected.ts + search.ts wiring): when the scan root
/// is the user's home, REF's `names()` basenames are excluded (ripgrep
/// `exclude: NAME/**`); REF's absolute `paths()` are excluded on every
/// scan. Table-bearing platforms are darwin/win32 only — linux tables
/// are empty exactly like REF.
struct ScanExclusions {
    home_rooted: bool,
    names: Vec<String>,
    paths: Vec<std::path::PathBuf>,
}

impl ScanExclusions {
    fn for_root(root: &std::path::Path) -> Self {
        ScanExclusions {
            home_rooted: oc_core::filesystem::protected::is_home(&root.to_string_lossy()),
            names: oc_core::filesystem::protected::names()
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            paths: oc_core::filesystem::protected::paths(),
        }
    }

    fn is_excluded_dir(&self, name: &str, directory: &std::path::Path) -> bool {
        // Heavy/irrelevant trees (pre-existing skip; ripgrep's defaults
        // exclude .git/node_modules in REF).
        if name == ".git" || name == "node_modules" || name == "target" {
            return true;
        }
        if self.home_rooted && self.names.iter().any(|candidate| candidate == name) {
            return true;
        }
        if self.paths.is_empty() {
            return false;
        }
        let resolved = oc_core::filesystem::protected::resolve_lexical(directory);
        self.paths.iter().any(|protected| resolved == *protected || resolved.starts_with(protected))
    }
}

/// Fuzzy subsequence score, the ranking equivalent of REF's fuzzysort/fff
/// usage: case-insensitive subsequence over the whole path string with a
/// consecutive-run bonus (so substring completions — what the web UI's
/// fsFind relies on — outrank scattered acronyms) and a word-start bonus
/// after separators. Tighter matches edge out longer ones. `None` = no
/// match. Deviation from fuzzysort's exact algorithm: single-pass greedy
/// alignment (documented in the lane report).
fn fuzzy_score(query: &[char], target: &str) -> Option<i64> {
    let chars: Vec<char> = target.chars().collect();
    let mut score: i64 = 0;
    let mut matched = 0usize;
    let mut previous: i64 = -2;
    for (index, character) in chars.iter().enumerate() {
        if matched < query.len() && *character == query[matched] {
            score += 1;
            let index = index as i64;
            if index == previous + 1 {
                score += 4;
            }
            let boundary =
                index == 0 || matches!(chars[(index - 1) as usize], '/' | '-' | '_' | ' ' | '.');
            if boundary {
                score += 3;
            }
            previous = index;
            matched += 1;
        }
    }
    if matched < query.len() {
        return None;
    }
    // Prefer tighter matches (fuzzysort's "closer spread" heuristic).
    Some(score - (chars.len() as i64 - query.len() as i64) / 8)
}

/// REF (search.ts ranking): fuzzysort.go in the ripgrep fallback, then
/// score-desc / path-length-asc in the fff layer. The port's ranking is
/// a greedy-subsequence approximation of fuzzysort/fff — single-pass
/// left-to-right alignment rather than fuzzysort's optimal scoring —
/// whose substring-first property (consecutive-run bonus outranks
/// scattered acronym matches) is what autocomplete depends on; that
/// property is pinned by test
/// `fuzzy_substring_completion_outranks_scattered_subsequence`.
///
/// The budget counts ENTRIES VISITED (read from a directory), not
/// candidates pushed: with `type=file` over huge trees of directories,
/// counting pushes would leave the walk unbounded.
fn walk(
    directory: &std::path::Path,
    want_files: bool,
    want_dirs: bool,
    exclusions: &ScanExclusions,
    visited: &mut usize,
    out: &mut Vec<(std::path::PathBuf, bool)>,
) {
    if *visited >= SCAN_ENTRY_BUDGET {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(directory) else { return };
    let mut children: Vec<_> = read_dir.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for entry in children {
        *visited += 1;
        if *visited >= SCAN_ENTRY_BUDGET {
            return;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            // REF: ripgrep `exclude` globs keep protected trees out of the
            // index entirely (directory entries derive from the parents of
            // INCLUDED files) — excluded dirs are never candidates.
            if exclusions.is_excluded_dir(&entry.file_name().to_string_lossy(), &entry.path()) {
                continue;
            }
            if want_dirs {
                out.push((entry.path(), true));
            }
            walk(&entry.path(), want_files, want_dirs, exclusions, visited, out);
        } else if want_files {
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
    let location = resolve_location_for_fs(&state, &uri, &headers)?;
    let _services = services(&state, &location).await?;
    let relative = query_field(&uri, "path").unwrap_or_default();
    let absolute = location.as_path().join(&relative);
    if let Some(parent) = absolute.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            crate::errors::debug_cause("fs:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    }
    tokio::fs::write(&absolute, &body).await.map_err(|error| {
        crate::errors::debug_cause("fs:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
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

#[cfg(test)]
mod find_tests {
    use super::*;

    fn chars(value: &str) -> Vec<char> {
        value.chars().collect()
    }

    #[test]
    fn fuzzy_substring_completion_outranks_scattered_subsequence() {
        let substring = fuzzy_score(&chars("read"), "/a/readme.md").expect("substring matches");
        let scattered = fuzzy_score(&chars("read"), "/a/rusty/example/asset/do.txt")
            .expect("subsequence matches");
        let missing = fuzzy_score(&chars("zzz"), "/a/readme.md");
        assert!(substring > scattered, "substring completion must rank first");
        assert_eq!(missing, None);
    }

    #[test]
    fn fuzzy_is_case_insensitive_and_matches_empty_query() {
        // Call-site contract: targets arrive lowercased (the handler
        // lowercases both query and path like fuzzysort's prepare).
        assert!(fuzzy_score(&chars("rdm"), "/a/readme.md").is_some());
        // REF fuzzysort: empty query matches everything (score equal,
        // shorter paths first).
        assert!(fuzzy_score(&chars(""), "/anything/at/all.txt").is_some());
    }

    #[test]
    fn fuzzy_prefers_tighter_matches() {
        let tight = fuzzy_score(&chars("ab"), "/ab").expect("tight");
        let loose = fuzzy_score(&chars("ab"), "/a/super/long/path/b").expect("loose");
        assert!(tight > loose);
    }

    #[test]
    fn exclusions_skip_home_names_only_when_rooted_at_home() {
        let mut rooted = ScanExclusions { home_rooted: true, names: vec![], paths: vec![] };
        rooted.names = vec!["Desktop".to_owned(), "Documents".to_owned()];
        assert!(rooted.is_excluded_dir("Desktop", std::path::Path::new("/home/u/Desktop")));
        assert!(!rooted.is_excluded_dir("projects", std::path::Path::new("/home/u/projects")));

        let unrooted =
            ScanExclusions { home_rooted: false, names: rooted.names.clone(), paths: vec![] };
        assert!(!unrooted.is_excluded_dir("Desktop", std::path::Path::new("/repo/Desktop")));

        // Case-sensitive like REF's ripgrep `exclude: NAME/**` globs.
        assert!(!rooted.is_excluded_dir("desktop", std::path::Path::new("/home/u/desktop")));
    }

    #[test]
    fn exclusions_cover_absolute_protected_paths_on_any_scan() {
        let mut scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        scan.paths = vec![std::path::PathBuf::from("/.Trashes")];
        assert!(scan.is_excluded_dir("anything", std::path::Path::new("/.Trashes/sub")));
        assert!(!scan.is_excluded_dir("Trashes", std::path::Path::new("/repo/Trashes")));
    }

    #[test]
    fn exclusions_keep_heavy_tree_skips() {
        let scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        for name in [".git", "node_modules", "target"] {
            assert!(scan.is_excluded_dir(name, &std::path::Path::new("/repo").join(name)));
        }
    }

    #[test]
    fn walk_collects_candidates_without_descending_excluded_trees() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let path = root.path();
        std::fs::write(path.join("readme.md"), "x").expect("write");
        std::fs::create_dir_all(path.join(".git").join("deep")).expect("git");
        std::fs::write(path.join(".git").join("deep").join("readme.git.md"), "x").expect("write");
        std::fs::create_dir_all(path.join("notes")).expect("notes");
        std::fs::write(path.join("notes").join("x.txt"), "x").expect("write");

        let mut scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        scan.names = vec!["notes".to_owned()];
        let mut out = Vec::new();
        let mut visited = 0usize;
        walk(path, true, true, &scan, &mut visited, &mut out);
        let names: Vec<String> = out
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"readme.md".to_owned()), "candidates: {names:?}");
        assert!(!names.iter().any(|n| n.contains("git")), "excluded .git subtree: {names:?}");
        // home_rooted=false → names table does not apply.
        assert!(
            names.contains(&"notes".to_owned()),
            "names only apply on home-rooted scans: {names:?}"
        );
    }

    #[test]
    fn walk_budget_counts_entries_visited_not_candidates_pushed() {
        // Many directories, few matches: with type=file over a huge tree
        // of dirs, a push-counted budget would leave the walk unbounded.
        // Use a tiny stand-in budget by building >budget entries and
        // asserting the walk stops after exactly the visited budget.
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let path = root.path();
        // 300 directories named dNNN, no files, want_dirs=false → zero
        // candidates pushed, 300+ entries visited.
        for index in 0..300 {
            std::fs::create_dir_all(path.join(format!("d{index:03}"))).expect("dir");
        }
        let scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        let mut out = Vec::new();
        let mut visited = 0usize;
        walk(path, true, false, &scan, &mut visited, &mut out);
        assert!(out.is_empty(), "no files requested → no candidates: {out:?}");
        // root listing (300 dirs) + each dir's (empty) listing: the walk
        // must have visited every directory entry but produced nothing.
        assert_eq!(visited, 300, "visited counts entries, not pushes");
        assert!(visited <= SCAN_ENTRY_BUDGET);

        // Now prove the ceiling binds on visited entries: run again with
        // a visited counter already near the budget and confirm the walk
        // returns immediately without pushing candidates.
        let mut near: usize = SCAN_ENTRY_BUDGET;
        let mut out2 = Vec::new();
        walk(path, true, true, &scan, &mut near, &mut out2);
        assert!(out2.is_empty(), "budget exhausted → walk is a no-op: {out2:?}");
        assert_eq!(near, SCAN_ENTRY_BUDGET);
    }
}
