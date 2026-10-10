//! Managed pinned bun download (the MIGRATION §2.3 distribution
//! decision: **managed pinned download + checksum + graceful
//! degradation** — no embedding, single-binary stays light; a missing
//! bun disables the plugin subsystem with a clear log, never crashes
//! the host).
//!
//! * Resolution order (cache > system > `PATH`, per the Phase 9/10
//!   decision): `OC_PLUGIN_BUN` override → `~/.cache/opencode-rust/bin/bun`
//!   → `/usr/local/bin/bun` → `PATH` bun → `~/.bun/bin/bun`.
//! * [`ensure`] downloads the PINNED bun 1.3.10 zip when (and only
//!   when) the caller actually wants plugins, verifies its sha256
//!   against the constant below (captured from the real artifact — see
//!   `tools/RELEASE.md` for the capture method), unzips into the
//!   cache bin dir with mode 755, atomically.
//! * Offline / checksum mismatch / missing tooling are graceful: an
//!   [`EnsureError`] the caller turns into one clear log line, and the
//!   subsystem stays disabled. `OPENCODE_DISABLE_BUN_DOWNLOAD=1` is
//!   the kill switch (sandboxes, CI, air-gapped hosts).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The pinned bun version (upgrades change the node-compatibility
/// surface plugins see — Phase 9 binding constraint ⑤).
pub const BUN_VERSION: &str = "1.3.10";

/// The release artifact target this module manages (linux x64 only;
/// other platforms resolve a system bun or stay disabled).
pub const BUN_TARGET: &str = "linux-x64";

/// The pinned download URL (oven-sh GitHub release).
pub const BUN_URL: &str =
    "https://github.com/oven-sh/bun/releases/download/bun-v1.3.10/bun-linux-x64.zip";

/// sha256 of [`BUN_URL`]'s payload, captured from the real download
/// (2026-10-10, 39,328,732 bytes; capture method documented in
/// `tools/RELEASE.md`). Any other bytes are rejected.
pub const BUN_SHA256: &str = "f57bc0187e39623de716ba3a389fda5486b2d7be7131a980ba54dc7b733d2e08";

/// Environment override for the bun binary (test hook; shared with
/// the plugin host).
pub const BUN_ENV: &str = "OC_PLUGIN_BUN";

/// Kill switch for the managed download (sandboxes / offline hosts).
pub const DISABLE_DOWNLOAD_ENV: &str = "OPENCODE_DISABLE_BUN_DOWNLOAD";

/// How long concurrent bootstraps wait on the install lock.
const ENSURE_LOCK_TIMEOUT: Duration = Duration::from_secs(180);

/// Where a found bun came from (diagnostics for the disable log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BunSource {
    /// `OC_PLUGIN_BUN` override.
    EnvOverride,
    /// `~/.cache/opencode-rust/bin/bun` (managed pinned download).
    Cache,
    /// `/usr/local/bin/bun`.
    System,
    /// `bun` on the `PATH`.
    Path,
    /// `~/.bun/bin/bun` (the official user install).
    UserInstall,
}

impl BunSource {
    /// One-line description for logs.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::EnvOverride => "OC_PLUGIN_BUN override",
            Self::Cache => "managed cache",
            Self::System => "/usr/local/bin/bun",
            Self::Path => "PATH",
            Self::UserInstall => "~/.bun/bin/bun",
        }
    }
}

/// Failures of [`ensure`] — every one degrades to "plugins disabled",
/// logged once, never a crash.
#[derive(Debug)]
pub enum EnsureError {
    /// The kill switch is set (or the platform is unsupported).
    Disabled(String),
    /// The zip could not be fetched (offline, missing curl/wget, …).
    Download(String),
    /// The download's sha256 did not match the pin.
    Checksum { expected: String, actual: String },
    /// The zip could not be turned into a bun binary.
    Extract(String),
    /// Filesystem trouble installing into the cache bin dir.
    Install(String),
}

impl std::fmt::Display for EnsureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled(reason) => write!(f, "bun bootstrap disabled: {reason}"),
            Self::Download(detail) => {
                write!(f, "could not download bun {BUN_VERSION}: {detail}")
            }
            Self::Checksum { expected, actual } => write!(
                f,
                "bun {BUN_VERSION} download failed checksum verification: expected {expected}, got {actual}"
            ),
            Self::Extract(detail) => write!(f, "could not extract bun {BUN_VERSION}: {detail}"),
            Self::Install(detail) => write!(f, "could not install bun {BUN_VERSION}: {detail}"),
        }
    }
}

impl std::error::Error for EnsureError {}

/// Locate a usable bun without touching the network. `None` = absent
/// (the caller disables the plugin subsystem).
#[must_use]
pub fn resolve() -> Option<PathBuf> {
    locate().map(|(path, _)| path)
}

/// [`resolve`] with provenance.
#[must_use]
pub fn locate() -> Option<(PathBuf, BunSource)> {
    if let Some(path) = std::env::var_os(BUN_ENV) {
        let path = PathBuf::from(path);
        if is_executable(&path) {
            return Some((path, BunSource::EnvOverride));
        }
        return None; // an explicit override that fails is final (tests)
    }
    let cache = crate::global_roots::resolve().bin().join("bun");
    if is_executable(&cache) {
        return Some((cache, BunSource::Cache));
    }
    let system = PathBuf::from("/usr/local/bin/bun");
    if is_executable(&system) {
        return Some((system, BunSource::System));
    }
    if let Some(path) = on_path() {
        return Some((path, BunSource::Path));
    }
    let user = home_bun();
    if is_executable(&user) {
        return Some((user, BunSource::UserInstall));
    }
    None
}

/// Resolve a bun, downloading the pinned build when none exists.
///
/// This is the "plugins were requested" path: the caller only invokes
/// it when plugin entries/installs make a bun necessary. Failures are
/// [`EnsureError`]s — log and disable, never crash.
pub fn ensure() -> Result<PathBuf, EnsureError> {
    if let Some(path) = resolve() {
        return Ok(path);
    }
    if let Some(reason) = disable_reason() {
        return Err(EnsureError::Disabled(reason));
    }
    install_pinned()
}

fn log_debug(message: &str) {
    // One stderr line, only when the host asked for boot logs — a
    // resolved bun is the normal case and must stay silent otherwise.
    if std::env::var_os("OPENCODE_PRINT_LOGS").is_some() {
        eprintln!("opencode bunman: {message}");
    }
}

/// Why the managed download must not run, if it must not.
fn disable_reason() -> Option<String> {
    let value = std::env::var_os(DISABLE_DOWNLOAD_ENV)?;
    let value = value.to_string_lossy().to_ascii_lowercase();
    if value.is_empty() || value == "0" || value == "false" {
        return None;
    }
    Some(format!("{DISABLE_DOWNLOAD_ENV}={value}"))
}

/// Download + verify + install the pinned bun (lock-held).
fn install_pinned() -> Result<PathBuf, EnsureError> {
    let bin_dir = crate::global_roots::resolve().bin();
    std::fs::create_dir_all(&bin_dir)
        .map_err(|error| EnsureError::Install(format!("create {}: {error}", bin_dir.display())))?;
    // Concurrent hosts bootstrap through one lock; the loser finds the
    // binary already installed on its next resolve.
    let lock_path = bin_dir.join(".bun-install.lock");
    let _lock =
        crate::flock::FileLock::exclusive(&lock_path, ENSURE_LOCK_TIMEOUT).map_err(|error| {
            EnsureError::Install(format!("acquire {}: {error}", lock_path.display()))
        })?;
    if let Some(path) = resolve() {
        return Ok(path);
    }
    let zip = bin_dir.join(format!("bun-{BUN_VERSION}-download.zip"));
    eprintln!(
        "opencode: bun not found; downloading pinned bun {BUN_VERSION} to {}",
        bin_dir.display()
    );
    download(BUN_URL, &zip).map_err(EnsureError::Download)?;
    let installed = install_zip(&zip, &bin_dir);
    let _ = std::fs::remove_file(&zip);
    let bun = installed?;
    log_debug(&format!("bun {BUN_VERSION} installed at {}", bun.display()));
    Ok(bun)
}

/// Verify `zip` against the pinned sha and install its bun member —
/// the post-download half of [`ensure`], separable for tests.
fn install_zip(zip: &Path, bin_dir: &Path) -> Result<PathBuf, EnsureError> {
    let actual = sha256_file(zip).map_err(|error| EnsureError::Download(error.to_string()))?;
    if actual != BUN_SHA256 {
        return Err(EnsureError::Checksum { expected: BUN_SHA256.to_owned(), actual });
    }
    extract_into(zip, bin_dir).map_err(EnsureError::Extract)
}

/// Fetch `url` to `dest` via curl (wget fallback) — the same tooling
/// the v2 installer itself requires.
fn download(url: &str, dest: &Path) -> Result<(), String> {
    let curl = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "2", "--connect-timeout", "15"])
        .arg("-o")
        .arg(dest)
        .arg(url)
        .status();
    match curl {
        Ok(status) if status.success() => return Ok(()),
        Ok(status) => {
            eprintln!("opencode: curl exited {status}; trying wget");
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("opencode: curl not found; trying wget");
        }
        Err(error) => return Err(format!("run curl: {error}")),
    }
    let wget = std::process::Command::new("wget")
        .args(["-q", "--tries=2", "--timeout=15", "-O"])
        .arg(dest)
        .arg(url)
        .status();
    match wget {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("wget exited {status} for {url}")),
        Err(error) => Err(format!("neither curl nor wget could fetch {url}: {error}")),
    }
}

/// Streaming sha256 (the zip is ~40 MiB; do not hold it in memory).
fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    use std::fmt::Write as _;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    Ok(out)
}

/// Unzip `zip` and install its `bun-<target>/bun` member into `bin_dir`
/// as `bun` (mode 755, written to a temp name then atomically renamed).
fn extract_into(zip: &Path, bin_dir: &Path) -> Result<PathBuf, String> {
    let staging = std::env::temp_dir().join(format!(
        "oc-bunman-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    std::fs::create_dir_all(&staging).map_err(|error| format!("create staging: {error}"))?;
    let cleanup = Staging(&staging);
    let unzip = std::process::Command::new("unzip")
        .arg("-q")
        .arg("-o")
        .arg(zip)
        .arg("-d")
        .arg(&staging)
        .status();
    match unzip {
        Ok(status) if status.success() => {}
        Ok(status) => return Err(format!("unzip exited {status}")),
        Err(error) => return Err(format!("unzip is required to install bun: {error}")),
    }
    // The release zip layout: bun-linux-x64/bun. Probe the pinned
    // member first, then any single `bun` file one level deep.
    let extracted = staging.join(BUN_TARGET).join("bun");
    let extracted = if extracted.is_file() {
        extracted
    } else {
        let mut found = None;
        if let Ok(entries) = std::fs::read_dir(&staging) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("bun");
                if candidate.is_file() {
                    found = Some(candidate);
                    break;
                }
            }
        }
        found.ok_or_else(|| format!("no bun binary inside {}", zip.display()))?
    };
    let target = bin_dir.join("bun");
    let staged = bin_dir.join(".bun-staged");
    std::fs::copy(&extracted, &staged)
        .map_err(|error| format!("copy to {}: {error}", staged.display()))?;
    set_executable(&staged)?;
    std::fs::rename(&staged, &target)
        .map_err(|error| format!("rename into {}: {error}", target.display()))?;
    drop(cleanup);
    Ok(target)
}

/// Best-effort staging cleanup.
struct Staging<'a>(&'a Path);

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("chmod 755 {}: {error}", path.display()))
}

/// `bun` via the `PATH` entries (no `which` dependency).
fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("bun"))
        .find(|candidate| is_executable(candidate))
}

fn home_bun() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from);
    home.join(".bun").join("bin").join("bun")
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;
    use std::sync::MutexGuard;

    struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl Env {
        fn lock() -> Self {
            let guard = lock_env();
            let vars = [
                "HOME",
                "XDG_CACHE_HOME",
                "OPENCODE_TEST_HOME",
                "OPENCODE_CONFIG_DIR",
                BUN_ENV,
                DISABLE_DOWNLOAD_ENV,
                "OPENCODE_PRINT_LOGS",
                "PATH",
            ];
            let saved = vars.iter().map(|&name| (name, std::env::var_os(name))).collect::<Vec<_>>();
            for (name, _) in &saved {
                std::env::remove_var(name);
            }
            Self { _guard: guard, saved }
        }

        #[allow(clippy::unused_self)] // keeps call sites reading like a harness
        fn set(&self, name: &str, value: &str) {
            std::env::set_var(name, value);
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn fixture_zip() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bunman-fixture.zip")
    }

    fn fake_bun(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).expect("mkdir fake bun dir");
        let path = dir.join("bun");
        std::fs::write(&path, "#!/bin/sh\necho hi\n").expect("write fake bun");
        set_executable(&path).expect("chmod fake bun");
        path
    }

    #[test]
    fn pinned_constants_are_well_formed() {
        // Guards the pin: the sha must be 64 lowercase hex chars and the
        // URL must name the pinned version + target.
        assert_eq!(BUN_SHA256.len(), 64);
        assert!(BUN_SHA256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(BUN_URL.contains(&format!("bun-v{BUN_VERSION}")));
        assert!(BUN_URL.contains(BUN_TARGET));
        // The pinned sha was captured from the real artifact
        // (tools/RELEASE.md documents the method); the fixture zip's own
        // sha is e79eae4438439665fec3d59731c35bdcd24e7f8ed113da873b7669d6d06dfcac
        // and MUST NOT equal the pin.
        assert_ne!(BUN_SHA256, "e79eae4438439665fec3d59731c35bdcd24e7f8ed113da873b7669d06dfcac");
    }

    #[test]
    fn cache_wins_over_system_and_path() {
        let env = Env::lock();
        let home = tempfile::tempdir().expect("home");
        env.set("HOME", home.path().to_str().expect("path"));
        env.set("PATH", "/usr/local/bin:/usr/bin");
        let cache_bun = fake_bun(&home.path().join(".cache/opencode-rust/bin"));
        let (found, source) = locate().expect("cache bun located");
        assert_eq!(found, cache_bun);
        assert_eq!(source, BunSource::Cache);
    }

    #[test]
    fn env_override_beats_everything_and_is_final() {
        let env = Env::lock();
        let home = tempfile::tempdir().expect("home");
        env.set("HOME", home.path().to_str().expect("path"));
        env.set(BUN_ENV, "/definitely/not/bun");
        assert!(locate().is_none(), "a broken override must disable, not fall through");
        let override_dir = tempfile::tempdir().expect("override");
        let bun = fake_bun(override_dir.path());
        env.set(BUN_ENV, bun.to_str().expect("path"));
        let (found, source) = locate().expect("override located");
        assert_eq!(found, bun);
        assert_eq!(source, BunSource::EnvOverride);
    }

    #[test]
    fn fixture_installs_with_mode_755() {
        use std::os::unix::fs::PermissionsExt as _;
        let bin_dir = tempfile::tempdir().expect("bin dir");
        let bun = extract_into(&fixture_zip(), bin_dir.path()).expect("fixture extracts");
        assert_eq!(bun, bin_dir.path().join("bun"));
        assert!(bun.is_file());
        let mode = std::fs::metadata(&bun).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "installed bun must be executable");
        let body = std::fs::read_to_string(&bun).expect("read");
        assert!(body.contains("fixture-bun"), "the zip's bun member landed at bin/bun");
    }

    #[test]
    fn ensure_reports_disabled_and_never_touches_network() {
        let env = Env::lock();
        let home = tempfile::tempdir().expect("home");
        env.set("HOME", home.path().to_str().expect("path"));
        env.set("PATH", "");
        env.set(BUN_ENV, "/definitely/not/bun");
        env.set(DISABLE_DOWNLOAD_ENV, "1");
        match ensure() {
            Err(EnsureError::Disabled(reason)) => {
                assert!(reason.contains(DISABLE_DOWNLOAD_ENV), "reason names the switch: {reason}");
            }
            other => panic!("expected Disabled, got {other:?}"),
        }
    }

    #[test]
    fn checksum_verification_rejects_wrong_bytes() {
        // The fixture zip's bytes are NOT the pinned artifact: the real
        // post-download verify step must fail with the typed checksum
        // error and never reach extraction.
        let bin_dir = tempfile::tempdir().expect("bin dir");
        match install_zip(&fixture_zip(), bin_dir.path()) {
            Err(EnsureError::Checksum { expected, actual }) => {
                assert_eq!(expected, BUN_SHA256);
                assert_eq!(
                    actual,
                    "e79eae4438439665fec3d59731c35bdcd24e7f8ed113da873b7669d6d06dfcac"
                );
            }
            other => panic!("expected Checksum, got {other:?}"),
        }
        assert!(
            bin_dir.path().join("bun").symlink_metadata().is_err(),
            "a rejected checksum must not install anything"
        );
    }

    #[test]
    fn sha256_file_streams_known_vector() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("blob");
        std::fs::write(&path, b"abc").expect("write");
        assert_eq!(
            sha256_file(&path).expect("hash"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
