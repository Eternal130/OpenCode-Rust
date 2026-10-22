//! Skill URL source download channel — the port of v2
//! `skill/discovery.ts` (`SkillDiscovery.Service.pull`).
//!
//! Config `skills` entries that are parseable `http(s)://` URLs are URL
//! sources (v2 `config/plugin/skill.ts:89-101` — everything else,
//! including scp-style `git@host:x` and `ftp://` strings, stays a
//! directory source). The engine's config-load seam awaits
//! [`pull_url_sources`] BEFORE the (sync) [`super::ConfigSkillTable::discover`]
//! and feeds the downloaded roots back as extra scanned sources; the
//! scan itself (`{*.md,**/SKILL.md}` + frontmatter) is unchanged.
//!
//! Observable protocol per pass, per URL source (v2 `discovery.ts`):
//!
//! - GET `{base}/index.json` EVERY pass — `{skills:[{name,version?,files}]}`
//!   (2 retries, ~200 ms jittered exponential backoff, non-2xx = failure).
//!   Index failure ⇒ the source yields NO skills (logged; there is no
//!   offline fallback scan of the cache).
//! - Guards run BEFORE any file fetch: skill names and relative file
//!   paths must be safe (`isSafeSegment` / `isSafeRelativePath`), file
//!   URLs must resolve same-origin under `{base}/{skill}/`, and every
//!   destination must stay contained under the skill root. A skill with
//!   any unsafe entry is dropped whole, without a single file request.
//! - Eligibility: the files list must include `SKILL.md` or `{name}.md`.
//! - Cache layout: `<cache_root>/skills/<sha1(base)>/<skill-name>/`.
//! - Version stamp `.opencode-version`: same version ⇒ zero file
//!   downloads (the index is still fetched); version changed ⇒ all
//!   files download into `<root>.tmp-<token>` staging, the stamp is
//!   written, and the staging dir atomically swaps into place (backup +
//!   rollback on failure) — a failed refresh keeps the old version.
//! - Per-file download skips files already on disk. Per-file failure ⇒
//!   log; a versioned refresh aborts keeping the old version.
//! - Concurrency: v2 caps pulls at ≤ 4 skills / ≤ 8 files; this port
//!   executes the same protocol serially (see the caps note below the
//!   imports) — observably equivalent, request sets identical up to
//!   order.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// v2 concurrency caps (discovery.ts:13-14): ≤ 4 skills and ≤ 8 files
/// in flight per pull.
///
/// This port runs sources, skills, and files SERIALLY (in-flight ≤ 1 at
/// every level, within the v2 caps; the observable protocol — request
/// sets, cache layout, version gating — is identical, and the v2 tests
/// themselves compare SORTED request lists). Serial loops also keep the
/// pull future's auto traits clean: closure-carrying streams trip
/// rustc's higher-ranked `Send` leak check through the engine's spawned
/// `prompt` futures.
/// v2 `HttpClient.retryTransient({ times: 2, ... })` — retries after the
/// first attempt (3 attempts total).
const RETRIES: u32 = 2;
/// v2 `Schedule.exponential(200).pipe(Schedule.jittered)` — base backoff
/// between retries, jittered to ±20 %.
const RETRY_BASE_MS: u64 = 200;

/// URL-source classification (v2 `config/plugin/skill.ts:90`):
/// `URL.canParse(item) && /^(https?:)$/.test(protocol)` — only parseable
/// http/https URLs are URL sources; anything else (`git@host:x`,
/// `ftp://x`, relative paths) falls through to directory semantics.
#[must_use]
pub fn is_url_source(item: &str) -> bool {
    match reqwest::Url::parse(item) {
        Ok(url) => matches!(url.scheme(), "http" | "https"),
        Err(_) => false,
    }
}

/// Pulls every URL source among `entries` (raw config `skills` items,
/// deduplicated by first occurrence, v2 `Skill.Source` `add`) into the
/// hashed cache under `cache_root` and returns the downloaded skill
/// directories in source order — the extra scanned sources for
/// [`super::ConfigSkillTable::discover`].
pub async fn pull_url_sources(entries: &[String], cache_root: &Path) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut sources: Vec<String> = Vec::new();
    for entry in entries {
        if !is_url_source(entry) {
            continue;
        }
        if seen.insert(entry.as_str()) {
            sources.push(entry.clone());
        }
    }
    if sources.is_empty() {
        return Vec::new();
    }
    // v2 refreshes sources serially (`for (const source of current)`);
    // the concurrency caps apply within one source.
    let client = reqwest::Client::new();
    let mut directories: Vec<PathBuf> = Vec::new();
    for source in &sources {
        directories.extend(pull(&client, source, cache_root).await);
    }
    directories
}

/// The index manifest (`Index` schema, discovery.ts:54-56).
#[derive(serde::Deserialize)]
struct Index {
    skills: Vec<IndexSkill>,
}

/// One index entry (`IndexSkill`, discovery.ts:48-52).
#[derive(serde::Deserialize)]
struct IndexSkill {
    name: String,
    #[serde(default)]
    version: Option<String>,
    files: Vec<String>,
}

/// One URL source pull: fetch + validate the index, guard every skill,
/// then download/refresh under the hashed cache slot.
async fn pull(client: &reqwest::Client, url: &str, cache_root: &Path) -> Vec<PathBuf> {
    let base = if url.ends_with('/') { url.to_owned() } else { format!("{url}/") };
    let source = match reqwest::Url::parse(&base) {
        Ok(url) => url,
        Err(_) => return Vec::new(),
    };
    let index = source.join("index.json").expect("relative join on a valid base");
    let data: Index = match fetch_with_retry(client, &index).await {
        Ok(body) => match serde_json::from_slice(&body) {
            Ok(index) => index,
            Err(error) => {
                eprintln!("[skill-url] failed to parse skill index at {index}: {error}");
                return Vec::new();
            }
        },
        Err(error) => {
            eprintln!("[skill-url] failed to fetch skill index at {index}: {error}");
            return Vec::new();
        }
    };
    let source_root = cache_root.join("skills").join(sha1_hex(&base));
    let planned = data
        .skills
        .iter()
        .filter_map(|skill| plan_skill(skill, &source, &source_root))
        .collect::<Vec<_>>();
    let mut directories = Vec::new();
    for skill in planned {
        if let Some(root) = pull_skill(client, skill).await {
            directories.push(root);
        }
    }
    directories
}

/// One file of a planned skill: the resolved same-origin URL plus its
/// relative path (staging builds destinations off the relative form).
struct PlannedFile {
    url: reqwest::Url,
    file: String,
    destination: PathBuf,
}

/// One guard-checked skill ready to download (discovery.ts:108-144).
struct PlannedSkill {
    name: String,
    root: PathBuf,
    version: Option<String>,
    files: Vec<PlannedFile>,
}

/// Applies every pre-fetch guard (discovery.ts:108-144): safe skill
/// name, eligibility (`SKILL.md` or `{name}.md`), containment under the
/// source slot, and — per file — safe relative path, same-origin URL,
/// and containment under the skill root. Any violation drops the whole
/// skill (`None`) before a single file request is issued.
fn plan_skill(
    skill: &IndexSkill,
    source: &reqwest::Url,
    source_root: &Path,
) -> Option<PlannedSkill> {
    if !is_safe_segment(&skill.name) {
        return None;
    }
    if !skill.files.iter().any(|file| file == "SKILL.md" || file == &format!("{}.md", skill.name)) {
        return None;
    }
    let root = source_root.join(&skill.name);
    if !contains(source_root, &root) || root == source_root {
        return None;
    }
    let skill_url = source.join(&format!("{}/", encode_uri_component(&skill.name))).ok()?;
    let mut files = Vec::with_capacity(skill.files.len());
    for file in &skill.files {
        if !is_safe_relative_path(file) {
            return None;
        }
        let resource = skill_url.join(file).ok()?;
        if resource.origin() != source.origin() {
            return None;
        }
        let destination = root.join(file);
        if !contains(&root, &destination) || destination == root {
            return None;
        }
        files.push(PlannedFile { url: resource, file: file.clone(), destination });
    }
    Some(PlannedSkill { name: skill.name.clone(), root, version: skill.version.clone(), files })
}

/// One planned skill's pass through the cache (discovery.ts:145-197):
/// version-gated download or staged atomic refresh, then the final
/// eligibility check that decides whether the root answers.
async fn pull_skill(client: &reqwest::Client, skill: PlannedSkill) -> Option<PathBuf> {
    match skill.version.as_deref() {
        None => download_all(client, &skill.files).await,
        Some(version) => {
            let current = std::fs::read_to_string(skill.root.join(".opencode-version")).ok();
            if current.as_deref() != Some(version) {
                refresh_versioned(client, &skill, version).await;
            }
            // Same version ⇒ zero file downloads this pass (the index
            // fetch above already happened).
        }
    }
    if eligible(&skill.root, &skill.name) {
        Some(skill.root)
    } else {
        None
    }
}

/// `SKILL.md` or `{name}.md` present under `root` (discovery.ts:169-170,
/// 193-195).
fn eligible(root: &Path, name: &str) -> bool {
    root.join("SKILL.md").exists() || root.join(format!("{name}.md")).exists()
}

/// Best-effort download of every file; failures are logged per file
/// (discovery.ts:153-156 — every file is attempted, none short-circuit).
async fn download_all(client: &reqwest::Client, files: &[PlannedFile]) {
    for file in files {
        download(client, file.url.clone(), file.destination.clone()).await;
    }
}

/// `download` (discovery.ts:78-89): skip files already on disk, fetch,
/// write with parent dirs. `false` = failure (the caller decides
/// whether that aborts anything).
async fn download(client: &reqwest::Client, url: reqwest::Url, destination: PathBuf) -> bool {
    if destination.exists() {
        return true;
    }
    match fetch_with_retry(client, &url).await {
        Ok(body) => {
            if let Some(parent) = destination.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&destination, &body) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!(
                        "[skill-url] failed to write skill file {}: {error}",
                        destination.display()
                    );
                    false
                }
            }
        }
        Err(error) => {
            eprintln!("[skill-url] failed to download skill file {url}: {error}");
            false
        }
    }
}

/// The versioned refresh (discovery.ts:157-192): stage every file under
/// `<root>.tmp-<token>`, stamp, then swap atomically with a backup and
/// rollback. Any failure keeps the old version and is logged.
async fn refresh_versioned(client: &reqwest::Client, skill: &PlannedSkill, version: &str) {
    let token = staging_token();
    let staging = suffixed_sibling(&skill.root, "tmp", token);
    let backup = suffixed_sibling(&skill.root, "old", token);
    if let Err(error) = stage_and_swap(client, skill, version, &staging, &backup).await {
        eprintln!("[skill-url] failed to refresh skill {}: {error}", skill.name);
    }
    // Effect.ensuring: the staging dir never survives a refresh.
    let _ = std::fs::remove_dir_all(&staging);
}

/// `<root>.<kind>-<token>` next to the skill root (discovery.ts:158-160).
fn suffixed_sibling(root: &Path, kind: &str, token: u64) -> PathBuf {
    PathBuf::from(format!("{}.{}-{token:016x}", root.display(), kind))
}

async fn stage_and_swap(
    client: &reqwest::Client,
    skill: &PlannedSkill,
    version: &str,
    staging: &Path,
    backup: &Path,
) -> Result<(), String> {
    // discovery.ts:162-167: every file downloads into staging (no
    // short-circuit), then the every-downloaded check decides.
    let mut downloaded_all = true;
    for file in &skill.files {
        if !download(client, file.url.clone(), staging.join(&file.file)).await {
            downloaded_all = false;
        }
    }
    if !downloaded_all {
        return Err("a file download failed; the old version is kept".into());
    }
    if !eligible(staging, &skill.name) {
        return Err("staging has no SKILL.md or name file; the old version is kept".into());
    }
    std::fs::write(staging.join(".opencode-version"), version)
        .map_err(|error| format!("version stamp write failed: {error}"))?;
    let cached = skill.root.exists();
    if cached {
        std::fs::rename(&skill.root, backup)
            .map_err(|error| format!("backup rename failed: {error}"))?;
    }
    match std::fs::rename(staging, &skill.root) {
        Ok(()) => {
            if cached {
                let _ = std::fs::remove_dir_all(backup);
            }
            Ok(())
        }
        Err(error) => {
            // Rollback: restore the backup over the failed swap.
            if cached {
                let _ = std::fs::rename(backup, &skill.root);
            }
            Err(format!("staging swap failed: {error}"))
        }
    }
}

/// The retried GET (discovery.ts:69-76): network errors and transient
/// statuses (429, 5xx) consume the retry budget with ~200 ms jittered
/// exponential backoff; other non-2xx statuses fail immediately
/// (`filterStatusOk` without a retry).
///
/// Jitter comes from the clock (not `rand::thread_rng`) so the future
/// never carries a `!Send` RNG handle.
async fn fetch_with_retry(client: &reqwest::Client, url: &reqwest::Url) -> Result<Vec<u8>, String> {
    let mut attempt = 0u32;
    loop {
        let failure = match client.get(url.clone()).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                if (200..300).contains(&status) {
                    match response.bytes().await {
                        Ok(body) => return Ok(body.to_vec()),
                        Err(error) => format!("body read failed: {error}"),
                    }
                } else if status == 429 || status >= 500 {
                    format!("HTTP {status} (transient)")
                } else {
                    return Err(format!("HTTP {status}"));
                }
            }
            Err(error) => format!("request failed: {error}"),
        };
        if attempt >= RETRIES {
            return Err(failure);
        }
        // ±20 % jitter (integer math: a per-mille factor in 800..1200)
        // over the exponential base — v2 Schedule.exponential(200) +
        // Schedule.jittered.
        let base_ms = RETRY_BASE_MS << attempt;
        let factor_permille = 800 + u64::from(subsec_nanos() % 400);
        tokio::time::sleep(Duration::from_millis(base_ms * factor_permille / 1000)).await;
        attempt += 1;
    }
}

/// The clock's sub-second nanoseconds — the jitter entropy source
/// (`rand::thread_rng` is `!Send` and would poison the pull future's
/// auto traits; a wall-clock fraction is sufficient for backoff jitter
/// and staging tokens).
fn subsec_nanos() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos())
        .unwrap_or_default()
}

/// A collision-unlikely staging/backup token (wall-clock nanoseconds).
fn staging_token() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| {
            since
                .as_secs()
                .wrapping_mul(1_000_000_000)
                .wrapping_add(u64::from(since.subsec_nanos()))
        })
        .unwrap_or_default()
}

// ===========================================================================
// Guards (discovery.ts:16-46)
// ===========================================================================

/// `isSafeSegment`: non-empty, not `.`/`..`, no separators or NUL.
fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
}

/// `isSafeRelativePath`: non-empty; no `\`, NUL, `?`, `#`; not
/// URL-parseable; not absolute (posix or win32); every `/`-segment
/// percent-decodes to a safe segment.
fn is_safe_relative_path(value: &str) -> bool {
    if value.is_empty()
        || value.contains('\\')
        || value.contains('\0')
        || value.contains('?')
        || value.contains('#')
    {
        return false;
    }
    if reqwest::Url::parse(value).is_ok() {
        return false;
    }
    if value.starts_with('/') || is_win32_absolute(value) {
        return false;
    }
    value.split('/').all(|segment| {
        decode_uri_component(segment).is_some_and(|decoded| is_safe_segment(&decoded))
    })
}

/// The `path.win32.isAbsolute` drive-root cases (the `\\` cases are
/// already rejected by the backslash check): `X:/`-prefixed values.
fn is_win32_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// `FSUtil.contains`: `child` stays at/under `parent`. All inputs here
/// are lexically joined from safe relative segments, so `starts_with`
/// matches the reference's `path.relative` check.
fn contains(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent)
}

/// `decodeURIComponent`: strict percent-decoding — malformed `%`
/// sequences and non-UTF-8 output are `None` (the reference catches the
/// thrown URIError and calls the segment unsafe).
fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 3 > bytes.len() {
                return None;
            }
            let high = hex_digit(bytes[index + 1])?;
            let low = hex_digit(bytes[index + 2])?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// `encodeURIComponent`: the unreserved set stays, everything else
/// becomes `%XX` UTF-8 (JS leaves `A-Za-z0-9-_.!~*'()` raw).
fn encode_uri_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        let character = byte as char;
        if character.is_ascii_alphanumeric()
            || matches!(character, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            encoded.push(character);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `Hash.fast` — sha1 hex (the cache slot key under `<cache>/skills/`).
fn sha1_hex(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, value.as_bytes());
    let mut hex = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    hex
}

// ===========================================================================
// Tests — the v2 `skill-discovery.test.ts` behavior matrix over a
// hand-rolled 127.0.0.1:0 HTTP catalog (the JWKS-lane TcpListener
// responder pattern).
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::MutexGuard;

    struct ServerState {
        skills: Value,
        files: BTreeMap<String, String>,
        requests: Vec<String>,
        fail_index: bool,
    }

    /// A mock skill catalog on 127.0.0.1:0: serves `/catalog/index.json`
    /// from the current skills array, `/catalog/...` file bodies from the
    /// current file map (404 when missing), and records every request's
    /// absolute URL (v2 `state.requests`).
    struct CatalogServer {
        base: String,
        state: Arc<Mutex<ServerState>>,
        handle: tokio::task::JoinHandle<()>,
        cache: tempfile::TempDir,
    }

    impl Drop for CatalogServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    impl CatalogServer {
        async fn start() -> Self {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
            let port = listener.local_addr().expect("addr").port();
            let origin = format!("http://127.0.0.1:{port}");
            let state = Arc::new(Mutex::new(ServerState {
                skills: json!([]),
                files: BTreeMap::new(),
                requests: Vec::new(),
                fail_index: false,
            }));
            let served = Arc::clone(&state);
            let record_origin = origin.clone();
            let handle = tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let mut buffer = [0u8; 8192];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let path = request.split_whitespace().nth(1).unwrap_or_default().to_string();
                    let url = format!("{record_origin}{path}");
                    let (status, body) = {
                        let mut state = served.lock();
                        state.requests.push(url);
                        if path == "/catalog/index.json" {
                            if state.fail_index {
                                ("500 Internal Server Error", "broken index".to_owned())
                            } else {
                                ("200 OK", format!("{{\"skills\":{}}}", state.skills))
                            }
                        } else if let Some(body) = state.files.get(&path) {
                            ("200 OK", body.clone())
                        } else {
                            ("404 Not Found", "Not Found".to_owned())
                        }
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            });
            let cache = tempfile::Builder::new()
                .prefix("oc-core-skill-url-cache-")
                .tempdir_in("/tmp/opencode")
                .expect("cache sandbox");
            Self { base: format!("{origin}/catalog/"), state, handle, cache }
        }

        /// One pull against the swapped-in catalog state (v2
        /// `catalog.pull`): resets the request log, pulls the catalog
        /// base URL against the persistent cache sandbox, returns the
        /// directories + the recorded requests.
        async fn pull(&self, skills: Value, files: &[(&str, &str)]) -> (Vec<PathBuf>, Vec<String>) {
            {
                let mut state = self.state.lock();
                state.skills = skills;
                state.files = files
                    .iter()
                    .map(|(path, body)| ((*path).to_owned(), (*body).to_owned()))
                    .collect();
                state.requests.clear();
            }
            let entries = vec![self.base.clone()];
            let directories = super::pull_url_sources(&entries, self.cache.path()).await;
            let requests = self.state.lock().requests.clone();
            (directories, requests)
        }

        fn set_fail_index(&self) {
            self.state.lock().fail_index = true;
        }

        fn requests(&self) -> Vec<String> {
            self.state.lock().requests.clone()
        }
    }

    /// The pull future must be `Send`: the engine seam awaits it inside
    /// `assemble`, whose future flows into `tokio::spawn`ed `prompt`
    /// calls (a regression guard for the higher-ranked Send leak check).
    #[test]
    fn pull_url_sources_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        let entries: Vec<String> = Vec::new();
        let cache = PathBuf::from("/tmp");
        assert_send(pull_url_sources(&entries, &cache));
    }

    /// `Hash.fast` — sha1 hex of the base URL (the cache slot key).
    fn sha1_hex(value: &str) -> String {
        super::sha1_hex(value)
    }

    fn cache_is_empty(cache: &Path) -> bool {
        std::fs::read_dir(cache).expect("cache readable").next().is_none()
    }

    // -----------------------------------------------------------------------
    // Guards: unsafe entries are rejected BEFORE any file fetch
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn rejects_skill_name_traversal_without_fetching_files() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) =
            catalog.pull(json!([{ "name": "../outside", "files": ["SKILL.md"] }]), &[]).await;
        assert!(dirs.is_empty());
        assert_eq!(requests, vec![format!("{}index.json", catalog.base)]);
        assert!(cache_is_empty(catalog.cache.path()));
    }

    #[tokio::test]
    async fn rejects_file_traversal_without_fetching_files() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(json!([{ "name": "deploy", "files": ["SKILL.md", "../outside.md"] }]), &[])
            .await;
        assert!(dirs.is_empty());
        assert_eq!(requests, vec![format!("{}index.json", catalog.base)]);
        assert!(cache_is_empty(catalog.cache.path()));
    }

    #[tokio::test]
    async fn rejects_absolute_file_paths_without_fetching_files() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(json!([{ "name": "deploy", "files": ["SKILL.md", "/tmp/outside.md"] }]), &[])
            .await;
        assert!(dirs.is_empty());
        assert_eq!(requests, vec![format!("{}index.json", catalog.base)]);
        assert!(cache_is_empty(catalog.cache.path()));
    }

    #[tokio::test]
    async fn rejects_cross_origin_file_urls_without_fetching_files() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(
                json!([{ "name": "deploy", "files": ["SKILL.md", "https://evil.example.test/outside.md"] }]),
                &[],
            )
            .await;
        assert!(dirs.is_empty());
        assert_eq!(requests, vec![format!("{}index.json", catalog.base)]);
        assert!(cache_is_empty(catalog.cache.path()));
    }

    // -----------------------------------------------------------------------
    // Eligibility: SKILL.md or {name}.md
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn ineligible_skill_files_yield_no_directories() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(
                json!([{ "name": "deploy", "files": ["intro.md"] }]),
                &[("/catalog/deploy/intro.md", "# Intro")],
            )
            .await;
        assert!(dirs.is_empty(), "no SKILL.md and no deploy.md ⇒ ineligible");
        assert_eq!(
            requests,
            vec![format!("{}index.json", catalog.base)],
            "no file fetches before eligibility"
        );
    }

    #[tokio::test]
    async fn bare_named_skill_file_is_eligible() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(
                json!([{ "name": "deploy", "files": ["deploy.md", "references/guide.md"] }]),
                &[
                    ("/catalog/deploy/deploy.md", "# Deploy"),
                    ("/catalog/deploy/references/guide.md", "# Guide"),
                ],
            )
            .await;
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].join("deploy.md").is_file());
        assert!(dirs[0].join("references").join("guide.md").is_file());
        let mut sorted = requests;
        sorted.sort();
        let mut expected = vec![
            format!("{}index.json", catalog.base),
            format!("{}deploy/deploy.md", catalog.base),
            format!("{}deploy/references/guide.md", catalog.base),
        ];
        expected.sort();
        assert_eq!(sorted, expected);
    }

    // -----------------------------------------------------------------------
    // Downloads + the hashed cache layout
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn downloads_safe_nested_files_under_the_skill_root() {
        let catalog = CatalogServer::start().await;
        let (dirs, requests) = catalog
            .pull(
                json!([{ "name": "deploy", "files": ["SKILL.md", "references/guide.md"] }]),
                &[
                    ("/catalog/deploy/SKILL.md", "# Deploy"),
                    ("/catalog/deploy/references/guide.md", "# Guide"),
                ],
            )
            .await;
        assert_eq!(dirs.len(), 1);
        let mut sorted = requests;
        sorted.sort();
        let mut expected = vec![
            format!("{}index.json", catalog.base),
            format!("{}deploy/SKILL.md", catalog.base),
            format!("{}deploy/references/guide.md", catalog.base),
        ];
        expected.sort();
        assert_eq!(sorted, expected);
        // The hashed cache slot: <cache>/skills/<sha1(base)>/<skill>.
        assert_eq!(
            dirs[0],
            catalog.cache.path().join("skills").join(sha1_hex(&catalog.base)).join("deploy")
        );
        assert_eq!(std::fs::read_to_string(dirs[0].join("SKILL.md")).unwrap(), "# Deploy");
        assert_eq!(
            std::fs::read_to_string(dirs[0].join("references").join("guide.md")).unwrap(),
            "# Guide"
        );
    }

    // -----------------------------------------------------------------------
    // Version semantics: gated skip + atomic refresh
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn refreshes_cached_files_when_the_version_changes() {
        let catalog = CatalogServer::start().await;
        let (first, _) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "1", "files": ["SKILL.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# Old")],
            )
            .await;
        let (second, second_requests) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "2", "files": ["SKILL.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# New")],
            )
            .await;
        assert_eq!(first, second, "the same cache root across pulls");
        assert_eq!(std::fs::read_to_string(first[0].join("SKILL.md")).unwrap(), "# New");
        assert!(second_requests.contains(&format!("{}deploy/SKILL.md", catalog.base)));
        // Version-gated skip: the same version issues ONLY index.json.
        let (third, third_requests) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "2", "files": ["SKILL.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# Ignored")],
            )
            .await;
        assert_eq!(third_requests, vec![format!("{}index.json", catalog.base)]);
        assert_eq!(third, first);
        assert_eq!(
            std::fs::read_to_string(first[0].join("SKILL.md")).unwrap(),
            "# New",
            "cached content untouched on the gated skip"
        );
    }

    #[tokio::test]
    async fn publishes_complete_updates_and_removes_stale_files() {
        let catalog = CatalogServer::start().await;
        let (first, _) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "1", "files": ["SKILL.md", "old.md"] }]),
                &[
                    ("/catalog/deploy/SKILL.md", "# Old"),
                    ("/catalog/deploy/old.md", "old reference"),
                ],
            )
            .await;
        let root = first[0].clone();
        // A refresh with a missing file aborts — the old version stays
        // intact (staging discarded, root untouched).
        let (second, _) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "2", "files": ["SKILL.md", "missing.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# Partial")],
            )
            .await;
        assert_eq!(std::fs::read_to_string(root.join("SKILL.md")).unwrap(), "# Old");
        assert_eq!(std::fs::read_to_string(root.join("old.md")).unwrap(), "old reference");
        assert_eq!(second, vec![root.clone()], "the cached root still answers");
        // A complete refresh swaps wholesale — stale files disappear.
        let (third, _) = catalog
            .pull(
                json!([{ "name": "deploy", "version": "3", "files": ["SKILL.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# New")],
            )
            .await;
        assert_eq!(std::fs::read_to_string(root.join("SKILL.md")).unwrap(), "# New");
        assert!(!root.join("old.md").exists());
        assert_eq!(third, vec![root.clone()]);
        // No staging/backup residue next to the root.
        let siblings: Vec<String> = std::fs::read_dir(root.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(siblings, vec!["deploy".to_owned()]);
    }

    // -----------------------------------------------------------------------
    // Index failure: no skills, no offline fallback
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn index_failure_yields_no_skills_without_offline_fallback() {
        let catalog = CatalogServer::start().await;
        // Seed the cache with a good pull first.
        let (seed, seed_requests) = catalog
            .pull(
                json!([{ "name": "deploy", "files": ["SKILL.md"] }]),
                &[("/catalog/deploy/SKILL.md", "# Deploy")],
            )
            .await;
        assert_eq!(seed.len(), 1);
        assert_eq!(
            seed_requests,
            vec![format!("{}index.json", catalog.base), format!("{}deploy/SKILL.md", catalog.base)]
        );
        // Break the index: the pull fails (after the retry budget) and
        // yields NOTHING — the seeded cache is not scanned offline.
        catalog.set_fail_index();
        let (dirs, requests) =
            catalog.pull(json!([{ "name": "deploy", "files": ["SKILL.md"] }]), &[]).await;
        assert!(dirs.is_empty(), "no offline fallback to the cached copy");
        assert_eq!(requests.len(), 1 + RETRIES as usize, "one attempt per retry of the budget");
        assert!(requests.iter().all(|request| request == &format!("{}index.json", catalog.base)));
        assert_eq!(
            std::fs::read_to_string(seed[0].join("SKILL.md")).unwrap(),
            "# Deploy",
            "the failed pull leaves the cache intact"
        );
    }

    // -----------------------------------------------------------------------
    // Non-http(s) entries are not URL sources
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn non_http_schemes_are_not_url_sources() {
        let catalog = CatalogServer::start().await;
        let entries =
            vec!["git@host:skills".to_owned(), "ftp://mirror.example.test/skills".to_owned()];
        let dirs = pull_url_sources(&entries, catalog.cache.path()).await;
        assert!(dirs.is_empty());
        assert!(catalog.requests().is_empty(), "no HTTP traffic for non-http(s) entries");
        // Classification matrix (v2 config/plugin/skill.ts:90).
        assert!(is_url_source("http://example.test/skills"));
        assert!(is_url_source("https://example.test/skills/"));
        assert!(!is_url_source("git@host:skills"), "scp-style strings are directory paths");
        assert!(!is_url_source("ftp://mirror.example.test/skills"));
        assert!(!is_url_source("./relative/skills"));
        assert!(!is_url_source("~/skills"));
    }

    #[test]
    fn non_url_skill_entries_keep_directory_semantics_in_discover() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-skill-url-discover-")
            .tempdir_in("/tmp/opencode")
            .expect("sandbox");
        let location = dir.path().join("project");
        let scp_style = location.join("git@host:skills");
        std::fs::create_dir_all(&scp_style).expect("scp-style dir");
        std::fs::write(
            scp_style.join("SKILL.md"),
            "---\nname: Remote\ndescription: scp-style path\n---\n# Remote",
        )
        .expect("skill file");
        let ftp_style = location.join("ftp:").join("mirror").join("skills");
        std::fs::create_dir_all(&ftp_style).expect("ftp-style dir");
        std::fs::write(ftp_style.join("quick.md"), "---\nname: Quick\n---\n# Quick")
            .expect("skill");
        let loaded = crate::config::Loader::new(
            location.clone(),
            crate::config::GlobalDirs {
                config: dir.path().join("cfg"),
                home: dir.path().join("home"),
            },
        )
        .load();
        let mut with_document = loaded.clone();
        with_document.entries.push(oc_schema::config::Entry::Document(Box::new(
            oc_schema::config::Document {
                info: serde_json::from_value(serde_json::json!({
                    "skills": ["git@host:skills", "ftp://mirror/skills"]
                }))
                .expect("info"),
                path: None,
            },
        )));
        let table = crate::engine::skills::ConfigSkillTable::discover(
            &with_document,
            &location,
            &dir.path().join("home"),
            &[],
        );
        let remote = crate::tool::skill::SkillTable::get(&table, "git@host:skills")
            .expect("scp-style entry stays a directory source");
        assert_eq!(remote.name.0, "Remote");
        let quick = crate::tool::skill::SkillTable::get(&table, "quick")
            .expect("ftp:// entry stays a directory source");
        assert_eq!(quick.name.0, "Quick");
    }

    #[test]
    fn discover_scans_downloaded_url_sources_as_extra_sources() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-skill-url-extra-")
            .tempdir_in("/tmp/opencode")
            .expect("sandbox");
        let local = dir.path().join("project").join("skills");
        std::fs::create_dir_all(local.join("local")).expect("local dir");
        std::fs::write(
            local.join("local").join("SKILL.md"),
            "---\nname: Local\n---\n# Local skill",
        )
        .expect("local skill");
        std::fs::write(
            dir.path().join("project").join("opencode.json"),
            serde_json::json!({ "skills": ["./skills"] }).to_string(),
        )
        .expect("project config");
        let downloaded = dir.path().join("cache").join("deploy");
        std::fs::create_dir_all(&downloaded).expect("downloaded dir");
        std::fs::write(downloaded.join("SKILL.md"), "---\nname: Deploy\n---\n# Deploy skill")
            .expect("downloaded skill");
        let loaded = crate::config::Loader::new(
            dir.path().join("project"),
            crate::config::GlobalDirs {
                config: dir.path().join("cfg"),
                home: dir.path().join("home"),
            },
        )
        .load();
        let table = crate::engine::skills::ConfigSkillTable::discover(
            &loaded,
            &dir.path().join("project"),
            &dir.path().join("home"),
            &[downloaded],
        );
        let deploy = crate::tool::skill::SkillTable::get(&table, "deploy")
            .expect("the downloaded root scans like a directory source");
        assert!(deploy.content.contains("# Deploy skill"));
        let local_skill = crate::tool::skill::SkillTable::get(&table, "local").expect("local");
        assert!(local_skill.content.contains("# Local skill"));
    }

    // -----------------------------------------------------------------------
    // The engine seam: the async pull runs before discover at assembly
    // -----------------------------------------------------------------------

    /// Env mutation must be serialized within the process (the same
    /// discipline as the v2-migration tests): assembly resolves the skill
    /// cache root through `global_roots::resolve()`, which reads
    /// XDG_CACHE_HOME.
    fn lock_env() -> MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct CacheEnv {
        _guard: MutexGuard<'static, ()>,
        saved: Option<String>,
    }

    impl CacheEnv {
        fn redirect(cache: &Path) -> Self {
            let guard = lock_env();
            let saved = std::env::var("XDG_CACHE_HOME").ok();
            std::env::set_var("XDG_CACHE_HOME", cache);
            Self { _guard: guard, saved }
        }
    }

    impl Drop for CacheEnv {
        fn drop(&mut self) {
            match &self.saved {
                Some(value) => std::env::set_var("XDG_CACHE_HOME", value),
                None => std::env::remove_var("XDG_CACHE_HOME"),
            }
        }
    }

    #[tokio::test]
    async fn engine_assembly_pulls_url_skill_sources_into_the_table() {
        let sandbox = tempfile::Builder::new()
            .prefix("oc-core-skill-url-engine-")
            .tempdir_in("/tmp/opencode")
            .expect("sandbox");
        let env = CacheEnv::redirect(&sandbox.path().join("cache"));
        let catalog = CatalogServer::start().await;
        // A global config document under the engine home carries the URL
        // source; assembly must pull it into the cache root before the
        // (sync) discover and scan the downloaded root.
        let global_config = sandbox.path().join("home").join(".config").join("opencode");
        std::fs::create_dir_all(&global_config).expect("global config dir");
        std::fs::write(
            global_config.join("opencode.json"),
            serde_json::json!({ "skills": [catalog.base] }).to_string(),
        )
        .expect("global config file");
        catalog.state.lock().files.insert(
            "/catalog/deploy/SKILL.md".to_owned(),
            "---\nname: Deploy\n---\n# Deploy".to_owned(),
        );
        catalog.state.lock().skills =
            serde_json::json!([{ "name": "deploy", "files": ["SKILL.md"] }]);
        std::fs::create_dir_all(sandbox.path().join("project").join(".git")).expect("project");
        let engine = crate::engine::Engine::boot(crate::engine::EngineOptions {
            activity_sweeper: false,
            ..crate::engine::EngineOptions::new(
                sandbox.path().join("data"),
                sandbox.path().join("home"),
            )
        })
        .await;
        let location = crate::location::LocationRef::new(sandbox.path().join("project"), None);
        let services = engine.location(location).await.expect("assemble");
        let deploy = crate::tool::skill::SkillTable::get(&*services.skills, "deploy")
            .expect("the pulled URL skill joins the table");
        assert!(deploy.content.contains("# Deploy"));
        let slot = sandbox
            .path()
            .join("cache")
            .join("opencode-rust")
            .join("skills")
            .join(sha1_hex(&catalog.base));
        assert_eq!(
            deploy.path.as_str(),
            slot.join("deploy").join("SKILL.md").to_str().unwrap(),
            "the skill scans out of the hashed cache slot"
        );
        drop(env);
    }
}
