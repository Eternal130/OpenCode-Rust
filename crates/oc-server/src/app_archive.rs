//! App asset archive — the R-04 embed pipeline (ROADMAP §R-04 decision,
//! 2026-10-11).
//!
//! The reference app (anomalyco/opencode `packages/app`, Solid/Vite/PWA)
//! has no standalone published artifact: the release workflow builds it
//! from a pinned ref, brotli-compresses every dist file, and packs them
//! into a self-describing JSON archive that is embedded into the binary
//! at BUILD time ([`embedded_archive`], fed by `build.rs` via
//! `OPENCODE_APP_ARCHIVE`) or pointed at at RUN time
//! (`OPENCODE_APP_ASSETS` = archive file OR unpacked dist directory —
//! the local-iteration path). Decompression is lazy per file through a
//! small bounded cache; [`AssetStore::get`] is the only surface
//! `assets.rs` consumes, so its header logic stays byte-identical.
//!
//! Archive shape:
//!
//! ```json
//! {
//!   "format": "opencode-rust-app-archive/1",
//!   "provenance": { "ref_commit": "...", "built_at": "...", "app_commit": "..." },
//!   "files": { "<path>": "<base64(brotli(bytes))>" }
//! }
//! ```
//!
//! Compression happens ONLY in CI (`tools/web-assets/package.ts`, bun +
//! node:zlib brotli q6); this crate depends on the decompressor alone.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

use base64::Engine as _;
use dashmap::DashMap;
use serde::Deserialize;

/// The `format` self-identification string every archive must carry.
pub const FORMAT: &str = "opencode-rust-app-archive/1";

/// The build-time embedded archive (build.rs stages it into `OUT_DIR`;
/// an empty file — every dev build without `OPENCODE_APP_ARCHIVE` —
/// means "no archive" and the store stays empty).
const EMBEDDED_ARCHIVE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded-app-archive.bin"));

/// Where an [`AppArchive`] came from (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Compile-time embed (release pipeline).
    Embedded,
    /// Runtime `OPENCODE_APP_ASSETS` override (file or dist directory).
    Override,
}

/// Build provenance carried inside the archive.
#[derive(Debug, Clone, Deserialize)]
pub struct Provenance {
    /// The pinned reference-app commit the dist was built from.
    pub ref_commit: String,
    /// ISO timestamp of the CI packaging run.
    pub built_at: String,
    /// The app source commit (`ref_commit` while the ref IS the source).
    pub app_commit: String,
}

#[derive(Debug, Deserialize)]
struct ArchiveDoc {
    format: String,
    provenance: Provenance,
    files: HashMap<String, String>,
}

/// Archive load failures (structural: unreadable, bad JSON, wrong
/// `format`, undecodable base64). Per-file brotli corruption surfaces
/// at [`AppArchive::get`] as a miss instead — the archive is CI-built
/// and any corruption there fails the workflow before it can ship.
#[derive(Debug, thiserror::Error)]
pub enum AppArchiveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("bad archive: {0}")]
    Bad(String),
}

enum Entry {
    /// base64(brotli(bytes)) from a packaged archive — decompressed on
    /// first use, cached bounded.
    Brotli(Arc<[u8]>),
    /// Plain bytes from an unpacked dist directory (dev override path).
    Plain(Arc<[u8]>),
}

/// A parsed (not yet decompressed) app archive.
pub struct AppArchive {
    source: Source,
    provenance: Provenance,
    files: HashMap<String, Entry>,
    cache: parking_lot::Mutex<DecompressCache>,
}

/// Tiny bounded most-recent-last cache (capacity ≤ 32 keeps the linear
/// scan far cheaper than a dependency).
struct DecompressCache {
    capacity: usize,
    entries: Vec<(Arc<str>, Arc<[u8]>)>,
}

impl DecompressCache {
    fn new(capacity: usize) -> Self {
        Self { capacity, entries: Vec::new() }
    }

    fn lookup(&mut self, path: &str) -> Option<Arc<[u8]>> {
        let position = self.entries.iter().position(|(key, _)| &**key == path)?;
        let entry = self.entries.remove(position);
        self.entries.push(entry);
        let (_, bytes) = self.entries.last().expect("just pushed");
        Some(Arc::clone(bytes))
    }

    fn store(&mut self, path: &str, bytes: Arc<[u8]>) {
        if self.capacity == 0 {
            return;
        }
        self.entries.retain(|(key, _)| &**key != path);
        while self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        self.entries.push((Arc::from(path), bytes));
    }
}

impl AppArchive {
    /// Parses a packaged archive (`format` checked, base64 decoded,
    /// files kept compressed). `source` is diagnostics only.
    ///
    /// # Errors
    /// - [`AppArchiveError::Bad`] on wrong/missing `format`, invalid
    ///   JSON, or undecodable base64 payload.
    /// - [`AppArchiveError::Io`] never (bytes are already in memory),
    ///   kept for symmetry with [`Self::load_directory`].
    pub fn from_bytes(bytes: &[u8], source: Source) -> Result<Self, AppArchiveError> {
        let doc: ArchiveDoc = serde_json::from_slice(bytes)
            .map_err(|error| AppArchiveError::Bad(format!("invalid json: {error}")))?;
        if doc.format != FORMAT {
            return Err(AppArchiveError::Bad(format!(
                "unsupported format {:?} (expected {FORMAT:?})",
                doc.format
            )));
        }
        let mut files = HashMap::with_capacity(doc.files.len());
        for (path, encoded) in doc.files {
            let compressed =
                base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes()).map_err(
                    |error| AppArchiveError::Bad(format!("file {path:?}: bad base64: {error}")),
                )?;
            files.insert(path, Entry::Brotli(Arc::from(compressed)));
        }
        Ok(Self {
            source,
            provenance: doc.provenance,
            files,
            cache: parking_lot::Mutex::new(DecompressCache::new(32)),
        })
    }

    /// Reads an unpacked dist directory (the `OPENCODE_APP_ASSETS` dev
    /// override): every file becomes a plain, undecorated entry.
    /// Provenance is synthesized (`local`) — there is no CI packaging.
    ///
    /// # Errors
    /// [`AppArchiveError::Io`] on any unreadable file / unwalkable dir.
    pub fn load_directory(root: &Path, source: Source) -> Result<Self, AppArchiveError> {
        let mut files = HashMap::new();
        walk_files(root, root, &mut files)?;
        Ok(Self {
            source,
            provenance: Provenance {
                ref_commit: "local".to_owned(),
                built_at: String::new(),
                app_commit: "local".to_owned(),
            },
            files,
            cache: parking_lot::Mutex::new(DecompressCache::new(32)),
        })
    }

    /// The file at `path`, decompressing brotli entries on demand
    /// (results cached, bounded). `None` = not in the archive (or the
    /// entry failed to decompress — treated as a miss by the serving
    /// path, which cannot distinguish anyway).
    #[must_use]
    pub fn get(&self, path: &str) -> Option<Arc<[u8]>> {
        if let Some(hit) = self.cache.lock().lookup(path) {
            return Some(hit);
        }
        let entry = self.files.get(path)?;
        let plain = match entry {
            Entry::Plain(bytes) => Arc::clone(bytes),
            Entry::Brotli(compressed) => Arc::from(decompress(compressed)?),
        };
        self.cache.lock().store(path, Arc::clone(&plain));
        Some(plain)
    }

    /// Number of files in the archive (not yet decompressed).
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// The one-line boot banner (stderr, printed once per boot when an
    /// archive is active). Exact shape pinned by test.
    #[must_use]
    pub fn provenance_line(&self) -> String {
        format!("opencode-rust: web ui: {} files (ref {})", self.len(), self.provenance.ref_commit)
    }
}

fn decompress(compressed: &[u8]) -> Option<Vec<u8>> {
    let mut reader = compressed;
    let mut plain = Vec::new();
    brotli_decompressor::BrotliDecompress(&mut reader, &mut plain).ok()?;
    Some(plain)
}

fn walk_files(
    root: &Path,
    dir: &Path,
    files: &mut HashMap<String, Entry>,
) -> Result<(), AppArchiveError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let full = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_files(root, &full, files)?;
        } else if file_type.is_file() {
            let relative =
                full.strip_prefix(root).unwrap_or(&full).to_string_lossy().replace('\\', "/");
            let bytes = std::fs::read(&full)?;
            files.insert(relative, Entry::Plain(Arc::from(bytes)));
        }
        // Symlinks and other specials are skipped: a built dist has none.
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AssetStore — AppState's `assets` field
// ---------------------------------------------------------------------------

/// The serving-path asset store: directly-inserted entries (tests, or
/// embedders composing their own shell) shadow archive files; an
/// installed [`AppArchive`] backs everything else. Empty store = the
/// API-only binary (no archive embedded or overridden).
pub struct AssetStore {
    direct: DashMap<String, Arc<[u8]>>,
    archive: Option<Arc<AppArchive>>,
}

impl AssetStore {
    #[must_use]
    pub fn new() -> Self {
        Self { direct: DashMap::new(), archive: None }
    }

    /// Inserts a plain (already-uncompressed) entry. Direct entries take
    /// precedence over archive files.
    pub fn insert(&self, key: String, bytes: Vec<u8>) {
        self.direct.insert(key, Arc::from(bytes));
    }

    /// The bytes for `key` (the `assets.rs` lookup — decompresses
    /// archive entries on demand through the bounded cache).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Arc<[u8]>> {
        if let Some(entry) = self.direct.get(key) {
            return Some(Arc::clone(entry.value()));
        }
        self.archive.as_ref()?.get(key)
    }

    /// Installs the archive backing every non-direct lookup.
    pub fn set_archive(&mut self, archive: AppArchive) {
        self.archive = Some(Arc::new(archive));
    }

    /// The active archive's boot banner, when one is installed.
    #[must_use]
    pub fn provenance_line(&self) -> Option<String> {
        self.archive.as_ref().map(|archive| archive.provenance_line())
    }
}

impl Default for AssetStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Boot wiring
// ---------------------------------------------------------------------------

/// The staged build-time archive, when the release pipeline embedded
/// one (`OPENCODE_APP_ARCHIVE` at build time; `None` in dev builds).
#[must_use]
pub fn embedded_archive() -> Option<&'static [u8]> {
    (!EMBEDDED_ARCHIVE.is_empty()).then_some(EMBEDDED_ARCHIVE)
}

/// Populates `store` for boot (R-04 wiring, called by `boot_and_serve`
/// before the state is shared):
///
/// 1. `OPENCODE_APP_ASSETS` (file = packaged archive, directory =
///    unpacked dist) — the runtime override, for local iteration;
/// 2. else the build-time embedded archive (release binaries);
/// 3. else nothing — the store stays empty and `serve` is API-only
///    (today's behavior; every default test build).
///
/// A set-but-unloadable override is a HARD error (printed to stderr):
/// an operator who asked for the UI must never get a silently UI-less
/// binary.
///
/// # Errors
/// [`io::Error`] from the load path (unreadable path, bad archive).
pub fn install_at_boot(store: &mut AssetStore) -> io::Result<()> {
    match install_at_boot_inner(store) {
        Ok(()) => Ok(()),
        Err(error) => {
            eprintln!("opencode-rust: web ui: asset load failed: {error}");
            Err(io::Error::other(error))
        }
    }
}

fn install_at_boot_inner(store: &mut AssetStore) -> Result<(), AppArchiveError> {
    if let Ok(raw) = std::env::var("OPENCODE_APP_ASSETS") {
        if !raw.is_empty() {
            let path = Path::new(&raw);
            let archive = if path.is_dir() {
                AppArchive::load_directory(path, Source::Override)
            } else {
                AppArchive::from_bytes(&std::fs::read(path)?, Source::Override)
            }?;
            eprintln!("{}", archive.provenance_line());
            store.set_archive(archive);
            return Ok(());
        }
    }
    if let Some(bytes) = embedded_archive() {
        let archive = AppArchive::from_bytes(bytes, Source::Embedded)?;
        eprintln!("{}", archive.provenance_line());
        store.set_archive(archive);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compresses with the CI script's parameters (brotli q6) so tests
    /// exercise the exact production payload shape.
    fn brotli6(plain: &[u8]) -> Vec<u8> {
        let params = brotli::enc::BrotliEncoderParams {
            quality: 6,
            ..brotli::enc::BrotliEncoderParams::default()
        };
        let mut reader = io::Cursor::new(plain.to_vec());
        let mut packed = Vec::new();
        brotli::enc::BrotliCompress(&mut reader, &mut packed, &params).expect("compress");
        packed
    }

    fn fixture_json(files: &[(&str, &[u8])]) -> Vec<u8> {
        let owned: Vec<(String, Vec<u8>)> =
            files.iter().map(|(path, plain)| ((*path).to_owned(), plain.to_vec())).collect();
        archive_json(owned)
    }

    fn archive_json(files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
        use base64::engine::general_purpose::STANDARD;
        let mut encoded = serde_json::Map::new();
        for (path, plain) in files {
            encoded.insert(path, serde_json::Value::String(STANDARD.encode(brotli6(&plain))));
        }
        serde_json::json!({
            "format": FORMAT,
            "provenance": {
                "ref_commit": "e7a34f09bfd9134dfade5a8ddb843f7030bc9a69",
                "built_at": "2026-10-11T00:00:00Z",
                "app_commit": "e7a34f09bfd9134dfade5a8ddb843f7030bc9a69"
            },
            "files": encoded,
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn app_archive_round_trips_brotli_files() {
        let archive = AppArchive::from_bytes(
            &fixture_json(&[
                ("index.html", b"<html>shell</html>"),
                ("_assets/app.js", b"console.log(1)"),
            ]),
            Source::Embedded,
        )
        .expect("archive");
        assert_eq!(archive.len(), 2);
        assert_eq!(&*archive.get("index.html").expect("index"), b"<html>shell</html>");
        assert_eq!(&*archive.get("_assets/app.js").expect("js"), b"console.log(1)");
        assert!(archive.get("missing.css").is_none());
        // Repeat hits ride the cache (same content, still cheap).
        assert_eq!(&*archive.get("index.html").expect("cached index"), b"<html>shell</html>");
    }

    #[test]
    fn app_archive_rejects_wrong_format_and_bad_base64() {
        let wrong_format = br#"{"format":"other/1","provenance":{"ref_commit":"","built_at":"","app_commit":""},"files":{}}"#;
        assert!(AppArchive::from_bytes(wrong_format, Source::Embedded).is_err());
        let bad_base64 = br#"{"format":"opencode-rust-app-archive/1","provenance":{"ref_commit":"","built_at":"","app_commit":""},"files":{"x":"!!!"}}"#;
        assert!(AppArchive::from_bytes(bad_base64, Source::Embedded).is_err());
        assert!(AppArchive::from_bytes(b"not json", Source::Embedded).is_err());
    }

    #[test]
    fn app_archive_corrupt_brotli_is_a_miss_not_a_panic() {
        use base64::engine::general_purpose::STANDARD;
        let doc = serde_json::json!({
            "format": FORMAT,
            "provenance": { "ref_commit": "r", "built_at": "", "app_commit": "r" },
            "files": { "broken.js": STANDARD.encode(b"definitely not brotli") },
        });
        let archive =
            AppArchive::from_bytes(doc.to_string().as_bytes(), Source::Override).expect("archive");
        assert!(archive.get("broken.js").is_none());
    }

    #[test]
    fn app_archive_provenance_line_exact_shape() {
        let archive =
            AppArchive::from_bytes(&fixture_json(&[("index.html", b"x")]), Source::Embedded)
                .expect("archive");
        assert_eq!(
            archive.provenance_line(),
            "opencode-rust: web ui: 1 files (ref e7a34f09bfd9134dfade5a8ddb843f7030bc9a69)"
        );
    }

    #[test]
    fn app_archive_loads_dist_directory() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("index.html"), b"<html>dir</html>").expect("write");
        std::fs::create_dir(root.path().join("_assets")).expect("mkdir");
        std::fs::write(root.path().join("_assets").join("app.js"), b"1").expect("write");
        let archive = AppArchive::load_directory(root.path(), Source::Override).expect("archive");
        assert_eq!(archive.len(), 2);
        assert_eq!(&*archive.get("index.html").expect("index"), b"<html>dir</html>");
        assert_eq!(&*archive.get("_assets/app.js").expect("js"), b"1");
        assert_eq!(archive.provenance().ref_commit, "local");
    }

    #[test]
    fn app_archive_cache_evicts_under_capacity_without_losing_correctness() {
        let files: Vec<(String, Vec<u8>)> =
            (0..64u8).map(|index| (format!("f{index}.js"), vec![index])).collect();
        let archive = AppArchive::from_bytes(&archive_json(files.clone()), Source::Embedded)
            .expect("archive");
        // Push far past the 32-entry capacity, then re-read everything:
        // eviction only costs decompression, never correctness.
        for (path, bytes) in &files {
            assert_eq!(&*archive.get(path).expect("file"), bytes.as_slice());
        }
        for (path, bytes) in &files {
            assert_eq!(&*archive.get(path).expect("file again"), bytes.as_slice());
        }
    }

    #[test]
    fn asset_store_direct_entries_shadow_archive() {
        let mut store = AssetStore::new();
        store.set_archive(
            AppArchive::from_bytes(&fixture_json(&[("index.html", b"archive")]), Source::Embedded)
                .expect("archive"),
        );
        assert_eq!(&*store.get("index.html").expect("index"), b"archive");
        store.insert("index.html".to_owned(), b"direct".to_vec());
        assert_eq!(&*store.get("index.html").expect("shadowed"), b"direct");
        assert!(store.provenance_line().is_some());
        assert!(AssetStore::new().get("index.html").is_none());
        assert!(AssetStore::new().provenance_line().is_none());
    }
}
