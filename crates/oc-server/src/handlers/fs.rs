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
    let services = services(&state, &location).await?;
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
    // The index is keyed by canonical root (invalidation: a different
    // canonical root is a different slot).
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let exclusions = ScanExclusions::for_root(&root);
    // G-31: VCS-rooted non-home scans are unbounded (ref search.ts:72
    // `location.vcs && !home ? MAX_SAFE_INTEGER : 100_000`); the
    // budget is applied at INDEX BUILD time via the project
    // resolver's vcs info (T1 — covers hg worktrees, sub-directory
    // locations stay budgeted).
    let budget = scan_budget(is_vcs_root(&services.project, &root), &exclusions);
    // T2: entries come from the per-root in-memory index (10s refresh,
    // build-on-first-miss) — no per-request walk.
    let entries = state
        .fs_index
        .entries(root, budget, exclusions, i64::try_from(super::common::now_ms(&state)).unwrap_or(0))
        .await;

    // REF (filesystem/search.ts): every find path ranks the index
    // entries — fuzzysort.go in the ripgrep fallback, score-desc then
    // path-length-asc in the fff layer — and defaults the limit to 50.
    // Ranking equivalent: subsequence fuzzy score (substring matches
    // win via the consecutive-run bonus), then shorter paths first.
    // Scoring runs on spawn_blocking like the index build.
    let query_for_blocking = query.clone();
    let blocking = tokio::task::spawn_blocking(move || {
        let effective_limit = limit.unwrap_or(50);
        let lowered = query_for_blocking.to_lowercase();
        let query_chars: Vec<char> = lowered.chars().collect();
        let mut scored: Vec<_> = entries
            .iter()
            .filter(|(_, is_dir)| (*is_dir && want_dirs) || (!*is_dir && want_files))
            .filter_map(|(path, is_dir)| {
                let target = path.to_string_lossy().to_lowercase();
                fuzzy_score(&query_chars, &target)
                    .map(|score| (score, target.chars().count(), target, path, is_dir))
            })
            .collect();
        scored
            .sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
        scored.truncate(effective_limit);
        scored.into_iter().map(|(_, _, _, path, is_dir)| (path.clone(), *is_dir)).collect::<Vec<_>>()
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
/// are effectively UNBOUNDED, so every entry of a repo is a fuzzy
/// candidate regardless of tree size; non-VCS (and home-rooted) scans
/// keep the 100k ceiling as a hard bound on the synchronous walk.
/// G-31 (T2): the budget is applied at INDEX BUILD time — VCS roots
/// get `usize::MAX` (ref semantics), everything else keeps
/// [`SCAN_ENTRY_BUDGET`] (entries beyond the budget dropped from the
/// index, same truncation as the old per-request walk).
const SCAN_ENTRY_BUDGET: usize = 100_000;

/// ref search.ts `REFRESH_INTERVAL` ("10 seconds").
const REFRESH_INTERVAL_MS: i64 = 10_000;

/// `location.vcs` for the scan root (search.ts:72's gate), resolved via
/// the PROJECT RESOLVER instead of sniffing `.git` (T1): the resolver
/// walks to the nearest `.git`/`.hg` marker and reports the checkout
/// directory + vcs kind, so hg worktrees qualify too. Conservative
/// semantics preserved: only the ROOT of the checkout (the resolver's
/// directory equals the scan root) counts — a location placed in a
/// sub-directory of a repository stays budgeted, matching the old
/// `.git`-existence-at-root sniff.
fn is_vcs_root(project: &oc_core::project::Resolved, root: &std::path::Path) -> bool {
    if project.vcs.is_none() {
        return false;
    }
    let canonical_root =
        std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    // The resolver's directory is already canonical (config::resolve_path
    // realpaths on success).
    project.directory == canonical_root
}

/// The per-scan entry budget: unlimited for VCS-rooted non-home
/// scans (ref's `MAX_SAFE_INTEGER`), the 100k ceiling everywhere
/// else.
fn scan_budget(vcs_root: bool, exclusions: &ScanExclusions) -> usize {
    if vcs_root && !exclusions.home_rooted {
        usize::MAX
    } else {
        SCAN_ENTRY_BUDGET
    }
}

/// Sensitive-directory exclusions for a scan (port of
/// core/filesystem/protected.ts + search.ts wiring): when the scan root
/// is the user's home, REF's `names()` basenames are excluded (ripgrep
/// `exclude: NAME/**`); REF's absolute `paths()` are excluded on every
/// scan. Table-bearing platforms are darwin/win32 only — linux tables
/// are empty exactly like REF.
#[derive(Clone)]
pub(crate) struct ScanExclusions {
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
    budget: usize,
    visited: &mut usize,
    out: &mut Vec<(std::path::PathBuf, bool)>,
) {
    if *visited >= budget {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(directory) else { return };
    let mut children: Vec<_> = read_dir.flatten().collect();
    children.sort_by_key(std::fs::DirEntry::file_name);
    for entry in children {
        *visited += 1;
        if *visited >= budget {
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
            walk(&entry.path(), want_files, want_dirs, exclusions, budget, visited, out);
        } else if want_files {
            out.push((entry.path(), false));
        }
    }
}

// ---------------------------------------------------------------------------
// G-31 T2 — the in-memory per-root search index (ref search.ts
// `scan`/`refresh`): entries built by the same walk, refreshed every
// 10s by a background task; searches hit the index instead of walking
// per request.
// ---------------------------------------------------------------------------

type IndexEntries = std::sync::Arc<Vec<(std::path::PathBuf, bool)>>;

/// How long an awaiter of an in-flight FIRST build waits before giving
/// up and serving whatever the slot holds (empty) — bounds the poll
/// loop so a wedged/slow builder can never hang a request forever
/// (review C1). The background refresher or a later request still
/// repairs the slot.
const AWAIT_BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Roots whose slot no request touched for this long are evicted (and
/// their refresher stopped): the process-wide map must not keep
/// re-walking vanished/idle locations forever (review I1; ref's index
/// dies with its Location scope). Rebuild-on-miss makes eviction safe.
const IDLE_EVICT_MS: i64 = 300_000;

struct IndexSlot {
    /// When this index settled (unix ms); a request older than
    /// `refresh_ms` past this triggers a rebuild.
    settled_at: i64,
    entries: IndexEntries,
    /// Single-flight claim for in-progress rebuilds (ref's `refreshing`
    /// Deferred): concurrent stale requests serve the LAST GOOD index
    /// instead of piling up walks. Held through a [`ClaimGuard`] so a
    /// cancelled (or panicking) builder still releases it.
    refreshing: std::sync::atomic::AtomicBool,
    /// Whether a background refresher task is live for this root.
    refresher_started: std::sync::atomic::AtomicBool,
    /// Last request access (unix ms) — feeds the idle eviction.
    last_used_ms: std::sync::atomic::AtomicI64,
    /// Latest request-supplied build parameters (review I2): the
    /// background refresher re-reads these per rebuild instead of
    /// freezing the values of whichever request spawned it.
    budget: std::sync::atomic::AtomicU64,
    exclusions: ScanExclusions,
}

impl IndexSlot {
    fn empty() -> Self {
        IndexSlot {
            settled_at: i64::MIN,
            entries: std::sync::Arc::new(Vec::new()),
            refreshing: std::sync::atomic::AtomicBool::new(false),
            refresher_started: std::sync::atomic::AtomicBool::new(false),
            last_used_ms: std::sync::atomic::AtomicI64::new(i64::MIN),
            budget: std::sync::atomic::AtomicU64::new(0),
            exclusions: ScanExclusions { home_rooted: false, names: Vec::new(), paths: Vec::new() },
        }
    }
}

/// Releases a slot's `refreshing` claim when dropped — including on
/// future cancellation (client disconnect mid-build) and panics — so a
/// leaked claim can never wedge every later search for that root
/// (review C1).
struct ClaimGuard<'a> {
    slots: &'a dashmap::DashMap<std::path::PathBuf, IndexSlot>,
    root: std::path::PathBuf,
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.slots.get(&self.root) {
            slot.refreshing.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

/// The process-wide fs search index (rides `AppState`): one slot per
/// canonical scan root, each refreshed every [`REFRESH_INTERVAL_MS`] by
/// a background `tokio` task (the walk itself on `spawn_blocking`, so
/// refreshes never block the async runtime).
///
/// Invalidation choice (documented): slots are keyed by canonical root
/// and rebuilt when a request finds them older than the 10s freshness
/// window OR missing (build-on-first-miss — the first request after a
/// location (re)load walks synchronously like ref's un-initialized
/// first `find` awaits its scan). No separate config-reload hook: a
/// moved/reloaded location resolves to a different canonical root and
/// therefore a fresh slot; slots self-evict when their root vanishes
/// or after [`IDLE_EVICT_MS`] without a request (rebuild-on-miss makes
/// eviction safe), so the map tracks LIVE roots, not every root ever
/// queried.
pub struct SearchIndex {
    clock: std::sync::Arc<dyn oc_util::determinism::Clock>,
    refresh_ms: i64,
    /// Bound for awaiting an in-flight first build ([`AWAIT_BUILD_TIMEOUT`]
    /// in production; shorter in tests).
    await_timeout: std::time::Duration,
    /// Production (`new`) starts per-root background refreshers; the
    /// test constructor disables them (tiny test windows would spin).
    background: bool,
    slots: dashmap::DashMap<std::path::PathBuf, IndexSlot>,
    /// Total index builds (test observable: "no walk per request").
    builds: std::sync::atomic::AtomicU64,
}

impl SearchIndex {
    #[must_use]
    pub fn new(clock: std::sync::Arc<dyn oc_util::determinism::Clock>) -> Self {
        SearchIndex {
            clock,
            refresh_ms: REFRESH_INTERVAL_MS,
            await_timeout: AWAIT_BUILD_TIMEOUT,
            background: true,
            slots: dashmap::DashMap::new(),
            builds: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Test constructor: explicit freshness window (also the refresher
    /// period) and await bound, no background refreshers (call
    /// `spawn_refresher` directly to exercise the loop).
    #[must_use]
    pub fn with_refresh_ms(
        clock: std::sync::Arc<dyn oc_util::determinism::Clock>,
        refresh_ms: i64,
    ) -> Self {
        SearchIndex {
            background: false,
            refresh_ms,
            await_timeout: std::time::Duration::from_millis(200),
            ..Self::new(clock)
        }
    }

    /// Total walk-driven index builds across all roots.
    pub fn builds(&self) -> u64 {
        self.builds.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The request path (ref `find` → `refresh` → `search`): serve a
    /// fresh slot's entries; on miss or a stale slot, rebuild
    /// (build-on-first-miss; ref awaits the first scan) and then serve.
    /// Concurrent stale requests collapse onto one rebuild; losers
    /// serve the last good index (or await a first build, bounded).
    pub(crate) async fn entries(
        self: &std::sync::Arc<Self>,
        root: std::path::PathBuf,
        budget: usize,
        exclusions: ScanExclusions,
        now_ms: i64,
    ) -> IndexEntries {
        // Record the access + latest build parameters (the refresher
        // re-reads them per rebuild — review I2).
        let (fresh_entries, needs_refresher) = {
            let mut slot = self.slots.entry(root.clone()).or_insert_with(IndexSlot::empty);
            slot.last_used_ms.store(now_ms, std::sync::atomic::Ordering::Relaxed);
            slot.budget.store(budget as u64, std::sync::atomic::Ordering::Relaxed);
            slot.exclusions = exclusions.clone();
            let fresh = now_ms < slot.settled_at + self.refresh_ms;
            let refreshing = slot.refreshing.load(std::sync::atomic::Ordering::Acquire);
            let initialized = slot.settled_at != i64::MIN;
            let needs_refresher =
                self.background && !slot.refresher_started.swap(true, std::sync::atomic::Ordering::AcqRel);
            (fresh || (refreshing && initialized), needs_refresher)
        };
        if needs_refresher {
            self.spawn_refresher(root.clone());
        }
        if fresh_entries {
            // Fresh cache, or a refresh in flight over a last good
            // index (ref serves the previous map mid-scan).
            if let Some(slot) = self.slots.get(&root) {
                return slot.entries.clone();
            }
        }
        let Some(slot) = self.slots.get(&root) else {
            return std::sync::Arc::new(Vec::new());
        };
        if slot.refreshing.load(std::sync::atomic::Ordering::Acquire) {
            // First build in flight: ref's un-initialized find AWAITS
            // the initial scan — bounded (review C1) so a wedged
            // builder cannot hang the request.
            drop(slot);
            return self.await_first_build(&root).await;
        }
        drop(slot);
        // Claim the single-flight rebuild; the guard releases the claim
        // on completion, cancellation, or panic (review C1).
        let guard = {
            let mut slot = self.slots.entry(root.clone()).or_insert_with(IndexSlot::empty);
            if slot.refreshing.swap(true, std::sync::atomic::Ordering::AcqRel) {
                None
            } else {
                Some(ClaimGuard { slots: &self.slots, root: root.clone() })
            }
        };
        if let Some(guard) = guard {
            self.rebuild(&root, budget, &exclusions).await;
            drop(guard);
            return self.slots.get(&root).map(|slot| slot.entries.clone()).unwrap_or_default();
        }
        // Lost the single-flight claim: await (first build) or serve
        // the last good index, bounded.
        self.await_first_build(&root).await
    }

    /// Bounded wait for an in-flight build: polls the claim with the
    /// 2ms sleep until the claim clears (or a last-good index exists)
    /// and gives up at [`AWAIT_BUILD_TIMEOUT`] — serving whatever the
    /// slot holds rather than hanging (review C1).
    async fn await_first_build(&self, root: &std::path::Path) -> IndexEntries {
        let deadline = tokio::time::Instant::now() + self.await_timeout;
        loop {
            let Some(slot) = self.slots.get(root) else { break };
            if !slot.refreshing.load(std::sync::atomic::Ordering::Acquire)
                || slot.settled_at != i64::MIN
            {
                return slot.entries.clone();
            }
            drop(slot);
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        self.slots.get(root).map(|slot| slot.entries.clone()).unwrap_or_default()
    }

    /// One walk + swap (ref `scan`): builds on `spawn_blocking` (the
    /// std::fs recursion must not run on a tokio worker), replaces the
    /// slot's entries only on success — a failed refresh keeps the
    /// last good index (ref keeps the previous map alive mid-scan).
    /// A vanished root self-evicts its slot afterwards (review I1).
    async fn rebuild(&self, root: &std::path::Path, budget: usize, exclusions: &ScanExclusions) {
        self.builds.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scan_root = root.to_path_buf();
        let exclusions = exclusions.clone();
        let walked = tokio::task::spawn_blocking(move || {
            // The index carries files AND directories (ref indexes
            // files plus every parent directory); type filtering
            // happens at query time.
            let mut candidates = Vec::new();
            let mut visited = 0usize;
            walk(&scan_root, true, true, &exclusions, budget, &mut visited, &mut candidates);
            candidates
        })
        .await;
        if let Ok(entries) = walked {
            if let Some(mut slot) = self.slots.get_mut(root) {
                slot.entries = std::sync::Arc::new(entries);
                slot.settled_at = self.clock.unix_ms();
            }
        }
        // I1 self-eviction: the location/root is gone — drop the slot
        // (and with it the reason for the refresher to keep walking).
        if !root.exists() {
            self.slots.remove(root);
        }
    }

    /// The 10-second background refresher (ref `refresh`'s forked scan
    /// loop): one task per indexed root, holding only a `Weak` handle
    /// so it dies with the server state. On refresh error the last
    /// good index stays (rebuild swaps only on success). Build
    /// parameters are re-read from the slot's latest request-supplied
    /// values on EVERY tick (review I2), and the loop stops (evicting
    /// the slot) when the root vanished or went idle (review I1).
    fn spawn_refresher(self: &std::sync::Arc<Self>, root: std::path::PathBuf) {
        let weak = std::sync::Arc::downgrade(self);
        let period = std::time::Duration::from_millis(
            u64::try_from(self.refresh_ms).unwrap_or(10_000).max(1),
        );
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(period);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tick.tick().await; // consume the immediate first tick
            loop {
                tick.tick().await;
                let Some(index) = weak.upgrade() else { break };
                let Some(slot) = index.slots.get(&root) else { break };
                let last_used = slot.last_used_ms.load(std::sync::atomic::Ordering::Relaxed);
                let budget = usize::try_from(
                    slot.budget.load(std::sync::atomic::Ordering::Relaxed),
                )
                .unwrap_or(usize::MAX);
                let exclusions = slot.exclusions.clone();
                drop(slot);
                // I1: idle roots are evicted, not walked forever.
                if index.clock.unix_ms().saturating_sub(last_used) > IDLE_EVICT_MS {
                    index.slots.remove(&root);
                    break;
                }
                index.rebuild(&root, budget, &exclusions).await;
                if !index.slots.contains_key(&root) {
                    break; // evicted by the rebuild (vanished root)
                }
            }
            // Let a later request re-spawn the refresher (the loop only
            // exits with the slot gone or the state dying, but keep the
            // flag honest anyway).
            if let Some(index) = weak.upgrade() {
                if let Some(slot) = index.slots.get(&root) {
                    slot.refresher_started.store(false, std::sync::atomic::Ordering::Release);
                }
            }
        });
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
        walk(path, true, true, &scan, SCAN_ENTRY_BUDGET, &mut visited, &mut out);
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
        walk(path, true, false, &scan, SCAN_ENTRY_BUDGET, &mut visited, &mut out);
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
        walk(path, true, true, &scan, SCAN_ENTRY_BUDGET, &mut near, &mut out2);
        assert!(out2.is_empty(), "budget exhausted → walk is a no-op: {out2:?}");
        assert_eq!(near, SCAN_ENTRY_BUDGET);
    }

    /// G-31: VCS-rooted scans are unbounded (ref search.ts:72
    /// `location.vcs && !home ? MAX_SAFE_INTEGER : 100_000`) — a tree
    /// larger than a small stand-in budget is NOT truncated under the
    /// VCS-root budget, while the plain budget stops at exactly that
    /// many visited entries.
    #[test]
    fn vcs_root_scan_budget_is_unbounded() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let path = root.path();
        // A repo checkout: .git marker (dir contents stay excluded)
        // plus 40 files the small budget would cut off.
        std::fs::create_dir_all(path.join(".git")).expect("git");
        std::fs::write(path.join(".git").join("config"), "x").expect("write");
        for index in 0..40 {
            std::fs::write(path.join(format!("f{index:02}.txt")), "x").expect("write");
        }
        let scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        let budget = |vcs_root: bool| scan_budget(vcs_root, &scan);
        assert_eq!(budget(true), usize::MAX, "VCS root → unbounded");

        // Bounded walk with a 10-entry stand-in budget stops early.
        let mut out = Vec::new();
        let mut visited = 0usize;
        walk(path, true, false, &scan, 10, &mut visited, &mut out);
        assert_eq!(visited, 10, "the plain budget truncates at 10 entries");
        assert!(out.len() < 40, "truncated: {}/40 files", out.len());

        // The SAME tree under the VCS-root budget collects every file.
        let mut out = Vec::new();
        let mut visited = 0usize;
        walk(path, true, false, &scan, budget(true), &mut visited, &mut out);
        assert_eq!(out.len(), 40, "VCS-root budget must not truncate: got {}", out.len());
        // .git stays excluded from candidates even unbounded.
        assert!(!out.iter().any(|(p, _)| p.to_string_lossy().contains(".git")));
    }

    /// The budget gate: home-rooted scans keep the 100k ceiling even
    /// at a repo root, and non-repo roots are bounded.
    #[test]
    fn scan_budget_gates_on_vcs_root_and_home() {
        let scan = ScanExclusions { home_rooted: false, names: vec![], paths: vec![] };
        assert_eq!(scan_budget(true, &scan), usize::MAX);
        assert_eq!(scan_budget(false, &scan), SCAN_ENTRY_BUDGET);

        let home_scan = ScanExclusions { home_rooted: true, names: vec![], paths: vec![] };
        assert_eq!(
            scan_budget(true, &home_scan),
            SCAN_ENTRY_BUDGET,
            "!home gates the unbounded arm"
        );
    }

    fn resolved(directory: &std::path::Path, vcs: Option<oc_core::project::Vcs>) -> oc_core::project::Resolved {
        oc_core::project::Resolved {
            previous: None,
            id: "test".to_owned(),
            directory: directory.to_path_buf(),
            canonical: directory.to_path_buf(),
            vcs,
        }
    }

    /// T1: VCS-root status comes from the project resolver — git AND hg
    /// checkout roots qualify; sub-directory locations and vcs-less
    /// projects stay budgeted (root-of-worktree-only semantics).
    #[test]
    fn is_vcs_root_uses_project_resolver() {
        let repo = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        // A git checkout root (the resolver would report this directory).
        std::fs::create_dir_all(repo.path().join(".git")).expect("git");
        let root = std::fs::canonicalize(repo.path()).expect("canonical");
        let git = resolved(&root, Some(oc_core::project::Vcs::Git { store: root.join(".git") }));
        assert!(is_vcs_root(&git, &root), "git checkout root → vcs root");
        assert!(is_vcs_root(&git, repo.path()), "non-canonical root input still matches");

        // hg root qualifies too (covered by the resolver's Vcs::Hg arm).
        let hg = resolved(&root, Some(oc_core::project::Vcs::Hg { store: root.join(".hg") }));
        assert!(is_vcs_root(&hg, &root), "hg checkout root → vcs root");

        // Sub-directory of the checkout: resolver directory != scan root
        // → budgeted (conservative, same as the old root-only sniff).
        let sub = repo.path().join("sub");
        std::fs::create_dir_all(&sub).expect("sub");
        assert!(!is_vcs_root(&git, &sub), "sub-directory stays budgeted");

        // No vcs at all.
        let plain = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let none = resolved(plain.path(), None);
        assert!(!is_vcs_root(&none, plain.path()));
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;
    use oc_util::determinism::Clock;

    /// Stepped clock so freshness windows are deterministic.
    struct FakeClock(std::sync::Mutex<i64>);
    impl Clock for FakeClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH + std::time::Duration::from_millis(*self.0.lock().unwrap() as u64)
        }
    }

    fn index(clock: &std::sync::Arc<FakeClock>) -> std::sync::Arc<SearchIndex> {
        std::sync::Arc::new(SearchIndex::with_refresh_ms(
            std::sync::Arc::clone(clock) as std::sync::Arc<dyn Clock>,
            10_000,
        ))
    }

    fn scan(_root: &std::path::Path) -> ScanExclusions {
        ScanExclusions { home_rooted: false, names: vec![], paths: vec![] }
    }

    fn names(entries: &IndexEntries) -> Vec<String> {
        entries
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[tokio::test]
    async fn searches_hit_the_index_without_rewalking() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(root.path().join("readme.md"), "x").expect("write");

        // Build-on-first-miss: exactly ONE walk.
        let first = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000)
            .await;
        assert_eq!(index.builds(), 1);
        assert!(names(&first).contains(&"readme.md".to_owned()));

        // Requests inside the freshness window serve the cached index:
        // no additional builds however many searches arrive.
        for now in [1_500, 5_000, 9_999] {
            let served = index
                .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), now)
                .await;
            assert_eq!(index.builds(), 1, "no walk per request at now={now}");
            assert!(names(&served).contains(&"readme.md".to_owned()));
        }
    }

    #[tokio::test]
    async fn refresh_after_the_window_picks_up_new_files() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(root.path().join("old.txt"), "x").expect("write");
        let _ = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000)
            .await;

        // A file added AFTER the build is invisible until the 10s
        // window lapses (ref's index is only refreshed per interval)…
        std::fs::write(root.path().join("new.txt"), "x").expect("write");
        let stale = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 5_000)
            .await;
        assert!(!names(&stale).contains(&"new.txt".to_owned()), "still the old index");

        // …and the first request past the window rebuilds (this is the
        // same rebuild the background refresher performs every 10s).
        let fresh = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 11_001)
            .await;
        assert_eq!(index.builds(), 2);
        assert!(names(&fresh).contains(&"new.txt".to_owned()), "refresh picked up new.txt");
    }

    #[tokio::test]
    async fn non_vcs_budget_is_applied_at_index_build_time() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(0)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        for i in 0..20 {
            std::fs::write(root.path().join(format!("f{i:02}.txt")), "x").expect("write");
        }
        // scan_budget(false, _) = SCAN_ENTRY_BUDGET stand-in of 5:
        // entries beyond the budget are dropped from the INDEX itself.
        let entries =
            index.entries(root.path().to_path_buf(), 5, scan(root.path()), 0).await;
        assert!(entries.len() < 20, "budget truncates the index build: got {}", entries.len());
        // A later query (fresh window) sees the SAME truncated set.
        let again =
            index.entries(root.path().to_path_buf(), 5, scan(root.path()), 1_000).await;
        assert_eq!(entries.len(), again.len());
    }

    #[tokio::test]
    async fn concurrent_first_requests_collapse_onto_one_build() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(0)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(root.path().join("a.txt"), "x").expect("write");
        let mut joins = Vec::new();
        for _ in 0..8 {
            let index = std::sync::Arc::clone(&index);
            let root = root.path().to_path_buf();
            let scan = scan(&root);
            joins.push(tokio::spawn(async move {
                index.entries(root, SCAN_ENTRY_BUDGET, scan, 0).await
            }));
        }
        for join in joins {
            let entries = join.await.expect("join");
            assert!(names(&entries).contains(&"a.txt".to_owned()));
        }
        // The single-flight claim bounds builds to one per staleness
        // window even under a thundering herd of first requests.
        assert_eq!(index.builds(), 1, "one build, not one per request");
    }

    /// C1: a first-miss build whose request future is CANCELLED
    /// (client disconnect) must not leak the single-flight claim — the
    /// next request rebuilds itself instead of spinning forever in the
    /// awaiter loop.
    #[tokio::test]
    async fn cancelled_first_build_releases_the_single_flight_claim() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        for i in 0..200 {
            std::fs::write(root.path().join(format!("f{i:03}.txt")), "x").expect("write");
        }
        // Cancel the first-miss build almost immediately.
        let mut first = Box::pin(index.entries(
            root.path().to_path_buf(),
            SCAN_ENTRY_BUDGET,
            scan(root.path()),
            1_000,
        ));
        tokio::select! {
            _ = &mut first => {}
            _ = tokio::time::sleep(std::time::Duration::from_millis(1)) => {}
        }
        drop(first); // cancellation vector: Drop runs the ClaimGuard

        // The claim is free: the next request completes promptly (no
        // unbounded wait on a leaked claim) and serves real entries.
        let served = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            index.entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_001),
        )
        .await
        .expect("next request must not hang on a leaked claim");
        assert!(!served.is_empty(), "index rebuilt and served");
    }

    /// C1: the awaiter path is BOUNDED — a slot whose first build never
    /// completes cannot hang the request past the await deadline (the
    /// test constructor pins a 200ms deadline).
    #[tokio::test]
    async fn awaiter_gives_up_at_the_deadline() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = index(&clock);
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        // Wedge the slot: claim held, never initialized.
        index.slots.entry(root.path().to_path_buf()).or_insert_with(IndexSlot::empty).refreshing =
            std::sync::atomic::AtomicBool::new(true);
        let start = std::time::Instant::now();
        let served = tokio::time::timeout(
            std::time::Duration::from_secs(30), // wall-clock bound of the test itself
            index.entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000),
        )
        .await
        .expect("bounded wait must return, not hang");
        assert!(served.is_empty(), "nothing built → empty entries, served anyway");
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "gave up at the deadline");
    }

    /// I1: a vanished root self-evicts — the rebuild drops the slot and
    /// the refresher loop stops (no eternal re-walk of dead locations).
    #[tokio::test]
    async fn vanished_root_evicts_its_slot_and_stops_refreshing() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = std::sync::Arc::new(SearchIndex::with_refresh_ms(
            std::sync::Arc::clone(&clock) as std::sync::Arc<dyn Clock>,
            50,
        ));
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(root.path().join("a.txt"), "x").expect("write");
        let _ = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000)
            .await;
        assert!(index.slots.contains_key(root.path()));
        index.spawn_refresher(root.path().to_path_buf());

        let root_path = root.path().to_path_buf();
        root.close().expect("remove the root directory");
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(
            !index.slots.contains_key(&root_path),
            "vanished root must self-evict"
        );
        let builds_after_eviction = index.builds();

        // And rebuild-on-miss keeps eviction safe: a (re)created root
        // builds a fresh slot on the next request.
        std::fs::create_dir_all(&root_path).expect("recreate");
        std::fs::write(root_path.join("b.txt"), "x").expect("write");
        let rebuilt = index
            .entries(root_path.clone(), SCAN_ENTRY_BUDGET, scan(&root_path), 2_000)
            .await;
        assert!(names(&rebuilt).contains(&"b.txt".to_owned()));
        assert!(index.builds() > builds_after_eviction);
    }

    /// I1: idle roots (no request within [`IDLE_EVICT_MS`]) are evicted
    /// by their refresher instead of being re-walked forever.
    #[tokio::test]
    async fn idle_root_is_evicted_by_the_refresher() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = std::sync::Arc::new(SearchIndex::with_refresh_ms(
            std::sync::Arc::clone(&clock) as std::sync::Arc<dyn Clock>,
            50,
        ));
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        std::fs::write(root.path().join("a.txt"), "x").expect("write");
        let _ = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000)
            .await;
        index.spawn_refresher(root.path().to_path_buf());
        // No further access: advance the clock past the idle TTL — the
        // next refresher tick must evict rather than rebuild.
        *clock.0.lock().unwrap() = 1_000 + IDLE_EVICT_MS + 1;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(!index.slots.contains_key(root.path()), "idle root must be evicted");
    }

    /// I2: the refresher re-reads build parameters per tick — a budget
    /// change supplied by a later request governs subsequent background
    /// rebuilds instead of the frozen spawn-time value.
    #[tokio::test]
    async fn refresher_reads_current_params_per_rebuild() {
        let clock = std::sync::Arc::new(FakeClock(std::sync::Mutex::new(1_000)));
        let index = std::sync::Arc::new(SearchIndex::with_refresh_ms(
            std::sync::Arc::clone(&clock) as std::sync::Arc<dyn Clock>,
            50,
        ));
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        for i in 0..10 {
            std::fs::write(root.path().join(format!("f{i:02}.txt")), "x").expect("write");
        }
        // First access with a wide budget: all 10 files indexed.
        let wide = index
            .entries(root.path().to_path_buf(), SCAN_ENTRY_BUDGET, scan(root.path()), 1_000)
            .await;
        assert_eq!(wide.len(), 10);
        // A later request supplies a TIGHT budget (fresh window → no
        // rebuild, but the slot's params are updated)…
        let _ = index
            .entries(root.path().to_path_buf(), 3, scan(root.path()), 1_010)
            .await;
        // …so the refresher's next background rebuild uses budget 3.
        index.spawn_refresher(root.path().to_path_buf());
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let rebuilt = index
            .slots
            .get(root.path())
            .map(|slot| slot.entries.clone())
            .expect("slot alive (recently used)");
        assert!(rebuilt.len() < 10, "refresher rebuilt with the current budget: {}", rebuilt.len());
    }
}
