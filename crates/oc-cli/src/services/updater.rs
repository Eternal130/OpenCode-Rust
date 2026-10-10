//! The updater (v2 `cli/src/services/updater.ts`, reduced to the Rust
//! artifact stream) + the real `opencode upgrade`/`uninstall` bodies
//! (Phase 10 packaging lane) + the release channel (lane C).
//!
//! Differences from v2, all REGISTERED:
//!
//! * **Update channel (lane C)**: `opencode-rust upgrade` with no
//!   target now queries the port's OWN GitHub releases feed
//!   ([`crate::services::update_feed`]) and installs the newest
//!   channel-eligible release; a policy-driven background checker
//!   (`crate::services::autoupdate`) notifies or auto-installs. The
//!   failure-isolation contract (silent no-op on any network failure,
//!   sha256-verified all-or-nothing installs) is deliberate hardening
//!   BEYOND upstream — v2 has no sha256 verify.
//! * The `github` method downloads from the Rust port's OWN release
//!   location (`OPENCODE_RELEASE_URL`), never opencode.ai — the 2.0.24
//!   pin must not adopt newer TS releases.
//! * `local-file` is a Rust-port extension (install from a tarball or
//!   binary on disk); package-manager methods (`npm`, `brew`, …) are
//!   detected for diagnostics but not driven.
//!
//! Replacement is atomic on unix: stage a sibling file, `chmod 755`,
//! `rename` over the running binary (the old inode lives until exit).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::services::update_feed::Candidate;

/// The release tag this binary was built from (`v2.0.24.beta.1`), or
/// `"dev"` for local/source builds (build.rs stamps it from
/// `OC_RELEASE_TAG`; the release workflow sets it to the git tag).
/// A `dev` tag disables the release channel: `upgrade` explains, the
/// background checker never starts.
pub const RELEASE_TAG: &str = match option_env!("OC_RELEASE_TAG") {
    Some(tag) => tag,
    None => "dev",
};

/// Cache-dir temp-file prefix for in-flight downloads (PID-unique
/// names; swept on the next run so a killed download leaves nothing
/// durable behind).
const TEMP_PREFIX: &str = "update-download-";

/// The Rust-port extension methods.
pub const EXTRA_METHODS: [&str; 2] = ["local-file", "github"];

/// Every accepted `--method` value (v2's seven + the two extensions).
pub const ALL_METHODS: [&str; 9] =
    ["curl", "npm", "pnpm", "bun", "yarn", "vp", "brew", "local-file", "github"];

/// Environment variable holding the full release-tarball URL for the
/// `github` method (may contain `{version}`/`{target}` placeholders).
pub const RELEASE_URL_ENV: &str = "OPENCODE_RELEASE_URL";

/// Failures of [`upgrade`] — one clear message each, like v2's
/// `UpgradeError` (title/detail/command/retry collapsed).
#[derive(Debug)]
pub struct UpgradeError(pub String);

impl std::fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for UpgradeError {}

/// The curl-method install location (v2 `curlBinary`):
/// `$HOME/.opencode/bin/opencode-rust`.
#[must_use]
pub fn curl_binary() -> PathBuf {
    oc_util::global_roots::resolve().home.join(".opencode").join("bin").join("opencode-rust")
}

/// `Updater.method()` (reduced): `curl` when the running binary IS the
/// install-script location; `brew` under a Cellar prefix; otherwise
/// `None` (the handler asks for `--method`). Package-manager package
/// detection (npm/pnpm/bun/yarn/vp) is intentionally absent — the Rust
/// artifact stream does not ship that way.
#[must_use]
pub fn detect_method(exe: &Path) -> Option<&'static str> {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let curl = curl_binary();
    let curl = curl.canonicalize().unwrap_or(curl);
    if exe == curl {
        return Some("curl");
    }
    if exe.components().any(|part| part.as_os_str() == "Cellar") {
        return Some("brew");
    }
    None
}

/// Where the `github` method downloads from. `Err` carries the "set
/// `OPENCODE_RELEASE_URL`" guidance when unset.
pub fn release_url(version: &str) -> Result<String, UpgradeError> {
    let Some(template) = std::env::var(RELEASE_URL_ENV).ok().filter(|value| !value.is_empty())
    else {
        return Err(UpgradeError(format!(
            "The github upgrade method needs {RELEASE_URL_ENV} (the Rust port's release tarball \
             URL, optionally with {{version}}/{{target}} placeholders); refusing to guess — \
             opencode.ai serves the TS release stream this port must not adopt"
        )));
    };
    let target = release_target();
    Ok(template.replace("{version}", version).replace("{target}", &target))
}

/// The release artifact target name for this machine.
#[must_use]
pub fn release_target() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// One upgrade request, fully resolved.
#[derive(Debug, Clone)]
pub struct UpgradeRequest {
    /// Method name (from `--method` or detection).
    pub method: String,
    /// The positional target: a version (`2.0.24`) or, for
    /// `local-file`, a path.
    pub target: Option<String>,
}

/// `opencode upgrade` — returns the process exit code.
///
/// * Explicit target: the v2-shaped paths (`local-file` from disk,
///   `github`/`curl` via `OPENCODE_RELEASE_URL`, package-manager
///   methods explain themselves).
/// * No target (lane C): query the release feed and install the
///   newest channel-eligible release; `dev` builds are told they are
///   source builds; a dead feed is a CLEAR error and exit 1 (manual
///   action — the exception to the silent-failure contract the
///   background checker follows).
pub async fn upgrade(request: UpgradeRequest) -> i32 {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("opencode"));
    let Some(target) = request.target.as_deref().map(str::trim).filter(|t| !t.is_empty()) else {
        return upgrade_to_latest(&request.method, &exe).await;
    };
    let method = match request.method.as_str() {
        "auto" => detect_method(&exe).map(str::to_owned).unwrap_or_default(),
        other => other.to_owned(),
    };
    if method.is_empty() {
        eprintln!(
            "Error: Could not detect the installation method. Pass --method to choose how to upgrade OpenCode ({}).",
            ALL_METHODS.join(", ")
        );
        return 1;
    }
    println!("Using method: {method}");
    match method.as_str() {
        "local-file" => upgrade_from_file(Path::new(target), &exe),
        "github" | "curl" => upgrade_from_release(method == "curl", target, &exe).await,
        "brew" => {
            eprintln!(
                "Error: This binary lives under a Homebrew Cellar; upgrade it with `brew upgrade opencode` (the Rust port does not drive package managers)."
            );
            1
        }
        other => {
            eprintln!(
                "Error: Method {other:?} installs the TS release stream; the Rust port only supports {}.",
                EXTRA_METHODS.join(", ")
            );
            1
        }
    }
}

/// No-target upgrade (lane C): the release channel. Manual path —
/// errors print clearly and exit non-zero; the installed binary is
/// only ever replaced by a fully verified download.
async fn upgrade_to_latest(requested_method: &str, exe: &Path) -> i32 {
    let current = crate::util::VERSION;
    // Method gating mirrors the explicit paths: only our own artifact
    // streams can serve a "latest" from the feed.
    match requested_method {
        "local-file" => {
            eprintln!("Error: local-file upgrades need a path argument (a tarball, zip, or binary on disk).");
            return 1;
        }
        "brew" => {
            eprintln!(
                "Error: This binary lives under a Homebrew Cellar; upgrade it with `brew upgrade opencode` (the Rust port does not drive package managers)."
            );
            return 1;
        }
        "npm" | "pnpm" | "bun" | "yarn" | "vp" => {
            eprintln!(
                "Error: Method {requested_method:?} installs the TS release stream; the Rust port only supports {}.",
                EXTRA_METHODS.join(", ")
            );
            return 1;
        }
        // `auto` still refuses to drive a Cellar build it can detect.
        "auto" if detect_method(exe) == Some("brew") => {
            eprintln!(
                "Error: This binary lives under a Homebrew Cellar; upgrade it with `brew upgrade opencode` (the Rust port does not drive package managers)."
            );
            return 1;
        }
        _ => {}
    }
    if RELEASE_TAG == "dev" {
        println!("OpenCode upgrade skipped: {current} is a source build (dev); the release channel serves packaged builds only.");
        println!("Rebuild from source, or install a release tarball first (see tools/RELEASE.md).");
        return 0;
    }
    let Some(current_ver) = crate::services::update_feed::ReleaseVer::parse(RELEASE_TAG) else {
        println!("OpenCode upgrade skipped: build tag {RELEASE_TAG:?} is not a release tag; cannot compare against the release channel.");
        return 0;
    };
    println!("Using method: github (release channel)");
    match crate::services::update_feed::latest_release(&current_ver).await {
        Err(error) => {
            eprintln!("Error: Could not check for updates: {error}");
            eprintln!("Check your network, then run opencode-rust upgrade again.");
            1
        }
        Ok(None) => {
            println!("Already on the latest release (v{current_ver})");
            0
        }
        Ok(Some(candidate)) => {
            println!("Upgrading v{current_ver} → v{}", candidate.version);
            match install_candidate(&candidate, exe).await {
                Ok(()) => {
                    println!("Upgrade complete");
                    println!(
                        "Replaced {} (running processes keep the old version until restarted).",
                        exe.display()
                    );
                    0
                }
                Err(error) => {
                    eprintln!("Error: Upgrade failed: {error}");
                    eprintln!("Fix the issue above, then run opencode-rust upgrade again.");
                    1
                }
            }
        }
    }
}

/// `local-file`: `target` is a tarball/zip/raw binary on disk.
fn upgrade_from_file(source: &Path, exe: &Path) -> i32 {
    if !source.is_file() {
        eprintln!("Error: Upgrade source not found at {}", source.display());
        return 1;
    }
    println!("Upgrading from {}", source.display());
    match stage_binary(source) {
        Ok(binary) => replace_and_report(&binary, exe),
        Err(error) => {
            eprintln!("Error: Upgrade failed: {error}");
            1
        }
    }
}

/// `github`/`curl` with an EXPLICIT target: download the
/// `OPENCODE_RELEASE_URL` tarball (placeholders filled), verify against
/// the `<url>.sha256` sidecar when one is reachable, replace. The
/// explicit-target path stays sidecar-optional so offline/harness
/// installs (`OPENCODE_RELEASE_URL` at a bare local tarball) keep
/// working exactly as before.
async fn upgrade_from_release(via_installer_semantics: bool, target: &str, exe: &Path) -> i32 {
    let version = target.trim_start_matches('v');
    let url = match release_url(version) {
        Ok(url) => url,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let _ = via_installer_semantics; // both names share the download+replace body
    println!("Downloading {url}");
    match install_from_url_pair(&url, &format!("{url}.sha256"), exe, false).await {
        Ok(verified) => {
            if verified == VerifyOutcome::Skipped {
                println!("Note: no sha256 sidecar at {url}.sha256; installed without checksum verification.");
            }
            println!("Upgrade complete");
            println!(
                "Replaced {} (running processes keep the old version until restarted).",
                exe.display()
            );
            0
        }
        Err(error) => {
            eprintln!("Error: Could not complete the upgrade: {error}");
            eprintln!("Check your network, then run opencode upgrade again.");
            1
        }
    }
}

/// Whether a verified install actually verified (feed/auto installs
/// ALWAYS verify — `require_sidecar` rejects an unreachable sidecar —
/// while explicit-target installs fall back to unverified with a note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Tarball digest matched the sidecar's.
    Verified,
    /// No usable sidecar reachable; `require_sidecar == false` let it
    /// through (legacy explicit-target behavior).
    Skipped,
}

/// Feed-driven install (manual latest + background auto): sha256
/// verification is REQUIRED — a missing/unreachable/bad sidecar keeps
/// the current binary running.
pub async fn install_candidate(candidate: &Candidate, exe: &Path) -> Result<(), String> {
    install_from_url_pair(&candidate.tarball_url, &candidate.sha256_url, exe, true)
        .await
        .map(|_| ())
}

/// The all-or-nothing install: download to a PID-unique cache temp,
/// sha256-verify against the sidecar, extract, atomically replace.
/// Any failed step cleans the temp up and leaves `exe` untouched.
async fn install_from_url_pair(
    tarball_url: &str,
    sha256_url: &str,
    exe: &Path,
    require_sidecar: bool,
) -> Result<VerifyOutcome, String> {
    let cache = oc_util::global_roots::resolve().cache;
    std::fs::create_dir_all(&cache)
        .map_err(|error| format!("create {}: {error}", cache.display()))?;
    sweep_stale_downloads(&cache);
    let temp = cache.join(format!("{TEMP_PREFIX}{}.tar.gz", std::process::id()));
    let outcome =
        download_verify_install(tarball_url, sha256_url, &temp, exe, require_sidecar).await;
    let _ = std::fs::remove_file(&temp);
    outcome
}

async fn download_verify_install(
    tarball_url: &str,
    sha256_url: &str,
    temp: &Path,
    exe: &Path,
    require_sidecar: bool,
) -> Result<VerifyOutcome, String> {
    download_file(tarball_url, temp).await?;
    let verified = match fetch_text(sha256_url).await {
        Ok(sidecar) => {
            // A fetched-but-unreadable sidecar always refuses: either
            // the sidecar is tampered/garbage or something intercepts
            // the request — installing unverified is not an option
            // when one EXISTS.
            let Some(expected) = sidecar_expected_hex(&sidecar) else {
                return Err(format!("unreadable sha256 sidecar at {sha256_url}"));
            };
            verify_sha256_file(temp, &expected)?;
            VerifyOutcome::Verified
        }
        Err(error) => {
            if require_sidecar {
                return Err(format!("could not fetch sha256 sidecar: {error}"));
            }
            VerifyOutcome::Skipped
        }
    };
    let payload = stage_binary(temp)?;
    replace_binary(&payload, exe)?;
    // stage_binary only stages a copy for archive inputs; never
    // delete a user-supplied raw binary/file source.
    if payload
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"oc-upgrade-payload-"))
    {
        let _ = std::fs::remove_file(&payload);
    }
    Ok(verified)
}

/// Delete leftover `update-download-*` temps from crashed/killed runs
/// (PID-unique names mean they are never ours).
fn sweep_stale_downloads(cache: &Path) {
    if let Ok(entries) = std::fs::read_dir(cache) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.as_encoded_bytes().starts_with(TEMP_PREFIX.as_bytes()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Expected digest from a `sha256sum`-format sidecar
/// (`<hex>  <filename>`); tolerates a bare hex line and upper case.
/// `None` when no 64-hex token is present.
#[must_use]
pub fn sidecar_expected_hex(sidecar: &str) -> Option<String> {
    sidecar.split_whitespace().find_map(|token| {
        (token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| token.to_ascii_lowercase())
    })
}

/// Compare a file's sha256 against an expected hex digest
/// (case-insensitive). Mismatch refuses the install.
fn verify_sha256_file(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let actual = oc_util::hash::sha256_hex(&bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch for {}: sidecar says {expected}, download is {actual} — refusing to install",
            path.display()
        ))
    }
}

/// Fetch `url` into `dest` (reqwest, rustls; 5 s connect / 300 s
/// total for the release tarball itself). Mirrors the v2 installer's
/// curl requirements.
async fn download_file(url: &str, dest: &Path) -> Result<(), String> {
    let bytes = download_bytes(url, Duration::from_secs(300)).await?;
    std::fs::write(dest, bytes).map_err(|error| error.to_string())
}

/// GET a small text resource (sidecars) with the short budget.
async fn fetch_text(url: &str) -> Result<String, String> {
    let bytes = download_bytes(url, Duration::from_secs(15)).await?;
    String::from_utf8(bytes).map_err(|error| format!("sidecar is not UTF-8: {error}"))
}

async fn download_bytes(url: &str, total: Duration) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(total)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client.get(url).send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    Ok(bytes.to_vec())
}

/// Extract the `opencode` binary from an artifact (raw ELF, gzip
/// tarball, or zip) into a staging path.
fn stage_binary(artifact: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt as _;
    let magic = read_magic(artifact)?;
    if magic.starts_with(b"\x7fELF") || is_shell_script(magic) {
        return Ok(artifact.to_path_buf());
    }
    let staging = std::env::temp_dir().join(format!(
        "oc-upgrade-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    std::fs::create_dir_all(&staging).map_err(|error| format!("create staging: {error}"))?;
    let _cleanup = Staging(&staging);
    if magic.starts_with(&[0x1f, 0x8b]) {
        let status = std::process::Command::new("tar")
            .args(["-xzf"])
            .arg(artifact)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|error| format!("run tar: {error} (tar is required to upgrade)"))?;
        if !status.success() {
            return Err(format!("tar exited {status}"));
        }
    } else if magic.starts_with(b"PK\x03\x04") {
        let status = std::process::Command::new("unzip")
            .args(["-q", "-o"])
            .arg(artifact)
            .arg("-d")
            .arg(&staging)
            .status()
            .map_err(|error| format!("run unzip: {error} (unzip is required to upgrade)"))?;
        if !status.success() {
            return Err(format!("unzip exited {status}"));
        }
    } else {
        return Err(format!("{} is neither a binary nor a tar.gz/zip archive", artifact.display()));
    }
    let extracted = find_binary(&staging).ok_or_else(|| {
        format!(
            "no opencode-rust binary inside {} (expected an entry named opencode-rust or opencode)",
            artifact.display()
        )
    })?;
    // The staging dir is deleted on return — move the payload somewhere
    // stable (and executable) before handing it back.
    let payload = std::env::temp_dir().join(format!(
        "oc-upgrade-payload-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ));
    std::fs::copy(&extracted, &payload)
        .map_err(|error| format!("stage payload at {}: {error}", payload.display()))?;
    std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("chmod 755 {}: {error}", payload.display()))?;
    Ok(payload)
}

/// Payload binary names inside release artifacts: `opencode-rust`
/// (lane B naming, what release tarballs carry as
/// `package/opencode-rust`) first, legacy `opencode` (the beta.1
/// tarballs) as fallback.
const BINARY_NAMES: [&str; 2] = ["opencode-rust", "opencode"];

/// Depth-limited search for the payload binary: `opencode-rust`
/// (or legacy `opencode`) at the root or one level down (release
/// archives wrap it in a `package/` directory).
fn find_binary(dir: &Path) -> Option<PathBuf> {
    for name in BINARY_NAMES {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        for name in BINARY_NAMES {
            let candidate = entry.path().join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Install `binary` over `exe` (copy to a sibling temp, chmod 755,
/// atomic rename — safe while the process is running on unix).
fn replace_binary(binary: &Path, exe: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let staged = exe
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!(".opencode-new-{}", std::process::id()));
    std::fs::copy(binary, &staged)
        .map_err(|error| format!("stage replacement at {}: {error}", staged.display()))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("chmod 755 {}: {error}", staged.display()))?;
    std::fs::rename(&staged, exe).map_err(|error| format!("replace {}: {error}", exe.display()))
}

fn replace_and_report(binary: &Path, exe: &Path) -> i32 {
    match replace_binary(binary, exe) {
        Ok(()) => {
            println!("Upgrade complete");
            println!(
                "Replaced {} (running processes keep the old version until restarted).",
                exe.display()
            );
            0
        }
        Err(error) => {
            eprintln!("Error: Upgrade failed: {error}");
            eprintln!("Fix the issue above, then run opencode upgrade again.");
            1
        }
    }
}

fn read_magic(path: &Path) -> Result<[u8; 4], String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut magic = [0u8; 4];
    let read = file.read(&mut magic).map_err(|error| error.to_string())?;
    if read < 4 {
        return Err(format!("{} is too small to be an upgrade artifact", path.display()));
    }
    Ok(magic)
}

fn is_shell_script(magic: [u8; 4]) -> bool {
    magic.starts_with(b"#!")
}

/// Best-effort staging cleanup.
struct Staging<'a>(&'a Path);

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

// ---------------------------------------------------------------------------
// Uninstall (v2 handlers/uninstall.ts, unix-shaped)
// ---------------------------------------------------------------------------

/// One planned removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub label: &'static str,
    pub path: PathBuf,
    pub keep: bool,
}

/// The uninstall plan: the four global roots (+ keep flags) and the
/// registration files found in state.
#[must_use]
pub fn removal_plan(keep_config: bool, keep_data: bool) -> Vec<Removal> {
    let roots = oc_util::global_roots::resolve();
    vec![
        Removal { label: "Data", path: roots.data.clone(), keep: keep_data },
        Removal { label: "Cache", path: roots.cache.clone(), keep: false },
        Removal { label: "Config", path: roots.config.clone(), keep: keep_config },
        Removal { label: "State", path: roots.state.clone(), keep: false },
    ]
}

/// `service(-<channel>)?.json` registrations in the state dir (all
/// channels share these files — stop their owners before deleting).
#[must_use]
pub fn registration_files() -> Vec<PathBuf> {
    let state = oc_util::global_roots::resolve().state;
    let Ok(entries) = std::fs::read_dir(&state) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            let stem_ok = name == "service.json" || name.starts_with("service-");
            stem_ok
                && std::path::Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();
    files
}

/// Stop every registered background service (best-effort, per file).
pub async fn stop_services(files: &[PathBuf]) {
    for file in files {
        oc_server::lifecycle::stop(oc_server::lifecycle::StopOptions {
            file: Some(file.clone()),
            pty: oc_server::lifecycle::PtyStop::Clear,
            timing: oc_server::lifecycle::EnsureTiming::default(),
        })
        .await;
    }
}

/// Shell config files that may carry the install PATH entry (v2
/// `shellConfigs`, reduced to the unix shells).
#[must_use]
pub fn shell_configs() -> Vec<PathBuf> {
    let roots = oc_util::global_roots::resolve();
    let home = &roots.home;
    let xdg =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    let shell = std::env::var("SHELL").unwrap_or_default();
    let shell = shell.rsplit('/').next().unwrap_or("bash").to_owned();
    let candidates: Vec<PathBuf> = match shell.as_str() {
        "fish" => vec![home.join(".config/fish/config.fish"), xdg.join("fish/config.fish")],
        "zsh" => {
            let zdot = std::env::var_os("ZDOTDIR").map_or_else(|| home.clone(), PathBuf::from);
            vec![
                zdot.join(".zshrc"),
                zdot.join(".zshenv"),
                xdg.join("zsh/.zshrc"),
                xdg.join("zsh/.zshenv"),
            ]
        }
        "ash" | "sh" => vec![home.join(".ashrc"), home.join(".profile")],
        _ => vec![
            home.join(".bashrc"),
            home.join(".bash_profile"),
            xdg.join("bash/.bashrc"),
            xdg.join("bash/.bash_profile"),
        ],
    };
    let mut unique: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        if candidate.is_file() && !unique.contains(&candidate) {
            unique.push(candidate);
        }
    }
    unique
}

/// Remove the `~/.opencode/bin` PATH lines (and their `# opencode`
/// comment) from one shell config (v2 `cleanShellConfig`).
#[must_use]
pub fn clean_shell_config(content: &str) -> String {
    fn is_entry(line: &str) -> bool {
        let trimmed = line.trim();
        (trimmed.starts_with("export PATH=") || trimmed.starts_with("fish_add_path "))
            && line.contains(".opencode/bin")
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let mut kept: Vec<&str> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if is_entry(line) {
            continue;
        }
        if line.trim() == "# opencode" && lines.get(index + 1).is_some_and(|next| is_entry(next)) {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

/// Execute the plan: remove every non-kept directory, collect
/// per-entry errors instead of aborting (v2 behavior).
pub fn execute_removals(plan: &[Removal]) -> Vec<String> {
    let mut errors = Vec::new();
    for removal in plan {
        if removal.keep || !removal.path.exists() {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(&removal.path) {
            errors.push(format!("{}: {error}", removal.label));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_detection_curl_only_at_the_install_location() {
        let home = tempfile::tempdir().expect("home");
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", home.path());
        let elsewhere = home.path().join("somewhere/opencode");
        std::fs::create_dir_all(elsewhere.parent().expect("parent")).expect("mkdir");
        std::fs::write(&elsewhere, "x").expect("write");
        assert_eq!(detect_method(&elsewhere), None);
        let installed = curl_binary();
        std::fs::create_dir_all(installed.parent().expect("parent")).expect("mkdir");
        std::fs::write(&installed, "x").expect("write");
        // The tempdir is a symlinked /tmp path on some hosts — compare
        // through the same canonicalization detect_method applies.
        let exe = std::fs::canonicalize(&installed).expect("canonical");
        assert_eq!(detect_method(&exe), Some("curl"));
        if let Some(saved) = saved {
            std::env::set_var("HOME", saved);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn release_url_requires_the_env_and_fills_placeholders() {
        let saved = std::env::var(RELEASE_URL_ENV).ok();
        std::env::remove_var(RELEASE_URL_ENV);
        assert!(release_url("9.9.9").is_err(), "unset env must be a clean error");
        std::env::set_var(
            RELEASE_URL_ENV,
            "https://example.com/opencode-{version}-{target}.tar.gz",
        );
        let url = release_url("9.9.9").expect("templated");
        assert_eq!(url, format!("https://example.com/opencode-9.9.9-{}.tar.gz", release_target()));
        assert!(url.contains("9.9.9") && !url.contains("{version}"));
        match saved {
            Some(saved) => std::env::set_var(RELEASE_URL_ENV, saved),
            None => std::env::remove_var(RELEASE_URL_ENV),
        }
    }

    #[test]
    fn stage_binary_accepts_a_raw_binary_and_finds_nested_entries() {
        let dir = tempfile::tempdir().expect("dir");
        let binary = dir.path().join("opencode");
        std::fs::write(&binary, b"\x7fELF-fake").expect("write");
        assert_eq!(stage_binary(&binary), Ok(binary.clone()));

        // tar.gz with a wrapped package/opencode layout.
        let staging = tempfile::tempdir().expect("staging");
        let package = staging.path().join("package");
        std::fs::create_dir_all(&package).expect("mkdir");
        std::fs::write(package.join("opencode"), b"\x7fELF-fake").expect("write");
        let tarball = staging.path().join("release.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["-czf"])
            .arg(&tarball)
            .arg("-C")
            .arg(staging.path())
            .arg("package")
            .status()
            .expect("tar exists on the gate hosts");
        assert!(status.success());
        let staged = stage_binary(&tarball).expect("stage");
        assert!(staged.is_file());
        let body = std::fs::read(&staged).expect("read");
        assert_eq!(body, b"\x7fELF-fake");
        assert_ne!(staged, tarball, "the payload must be extracted out of the archive");
    }

    /// A release-shaped tarball (`package/opencode-rust`, the lane B
    /// naming the release workflow packages).
    fn build_tarball(dir: &Path, member: &str, body: &[u8]) -> PathBuf {
        let package = dir.join("package");
        std::fs::create_dir_all(&package).expect("mkdir");
        std::fs::write(package.join(member), body).expect("write member");
        let tarball = dir.join("artifact.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["-czf"])
            .arg(&tarball)
            .arg("-C")
            .arg(dir)
            .arg("package")
            .status()
            .expect("tar exists on the gate hosts");
        assert!(status.success());
        tarball
    }

    #[test]
    fn stage_binary_accepts_the_opencode_rust_member() {
        let dir = tempfile::tempdir().expect("dir");
        let tarball = build_tarball(dir.path(), "opencode-rust", b"\x7fELF-rust-fake");
        let staged = stage_binary(&tarball).expect("stage");
        assert_eq!(std::fs::read(&staged).expect("read"), b"\x7fELF-rust-fake");
    }

    #[test]
    fn stage_binary_prefers_opencode_rust_over_legacy_opencode() {
        let dir = tempfile::tempdir().expect("dir");
        let package = dir.path().join("package");
        std::fs::create_dir_all(&package).expect("mkdir");
        std::fs::write(package.join("opencode"), b"\x7fELF-legacy").expect("write");
        std::fs::write(package.join("opencode-rust"), b"\x7fELF-new").expect("write");
        let tarball = dir.path().join("both.tar.gz");
        let status = std::process::Command::new("tar")
            .args(["-czf", &tarball.to_string_lossy()])
            .arg("-C")
            .arg(dir.path())
            .arg("package")
            .status()
            .expect("tar");
        assert!(status.success());
        let staged = stage_binary(&tarball).expect("stage");
        assert_eq!(std::fs::read(&staged).expect("read"), b"\x7fELF-new");
    }

    #[test]
    fn sidecar_expected_hex_tolerates_sha256sum_formats() {
        let digest = "a".repeat(64);
        assert_eq!(
            sidecar_expected_hex(&format!("{digest}  opencode-2.0.24-linux-x64.tar.gz")),
            Some(digest.clone()),
            "sha256sum two-space format"
        );
        assert_eq!(sidecar_expected_hex(&digest), Some(digest.clone()), "bare hex");
        assert_eq!(
            sidecar_expected_hex(&digest.to_ascii_uppercase()),
            Some(digest),
            "upper case is normalized"
        );
        assert_eq!(sidecar_expected_hex("not a digest"), None);
        assert_eq!(sidecar_expected_hex(""), None);
        assert_eq!(sidecar_expected_hex(&"a".repeat(63)), None);
    }

    #[test]
    fn verify_sha256_file_refuses_a_mismatch() {
        let dir = tempfile::tempdir().expect("dir");
        let file = dir.path().join("tarball");
        std::fs::write(&file, b"payload").expect("write");
        let good = oc_util::hash::sha256_hex(b"payload");
        assert_eq!(verify_sha256_file(&file, &good), Ok(()));
        assert_eq!(verify_sha256_file(&file, &good.to_ascii_uppercase()), Ok(()));
        let bad = oc_util::hash::sha256_hex(b"tampered");
        let error = verify_sha256_file(&file, &bad).expect_err("mismatch refuses");
        assert!(error.contains("sha256 mismatch"), "{error}");
        // The artifact survives a refusal.
        assert_eq!(std::fs::read(&file).expect("read"), b"payload");
    }

    // ------------------------------------------------------------------
    // Verified install over a local HTTP fixture (no real network)
    // ------------------------------------------------------------------

    /// Env sandbox relocating the global roots (cache dir under a
    /// tempdir). nextest isolates each test process, so env mutation
    /// is race-free.
    struct Roots {
        saved: Vec<(&'static str, Option<String>)>,
        _home: tempfile::TempDir,
    }

    fn sandbox_roots() -> Roots {
        let vars = ["HOME", "OPENCODE_TEST_HOME", "XDG_CACHE_HOME", "OPENCODE_DB"];
        let saved = vars.iter().map(|&name| (name, std::env::var(name).ok())).collect::<Vec<_>>();
        for &(name, _) in &saved {
            std::env::remove_var(name);
        }
        let home = tempfile::tempdir().expect("home");
        std::env::set_var("OPENCODE_TEST_HOME", home.path());
        Roots { saved, _home: home }
    }

    impl Drop for Roots {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// Serve `requests` HTTP round trips, mapping URL paths to bodies.
    fn serve_paths(bodies: Vec<(&'static str, Vec<u8>)>, requests: usize) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let path = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_owned();
                let body = bodies
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map_or_else(|| b"missing".to_vec(), |(_, body)| body.clone());
                let status = if body == b"missing" { "404 Not Found" } else { "200 OK" };
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                );
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn verified_install_replaces_and_cleans_up() {
        let _roots = sandbox_roots();
        let dir = tempfile::tempdir().expect("dir");
        let tarball_path = build_tarball(dir.path(), "opencode-rust", b"\x7fELF-new-binary");
        let tarball = std::fs::read(&tarball_path).expect("read");
        let digest = oc_util::hash::sha256_hex(&tarball);
        let base = serve_paths(
            vec![
                ("/t.tar.gz", tarball),
                (
                    "/t.tar.gz.sha256",
                    format!("{digest}  opencode-9.9.9-linux-x64.tar.gz").into_bytes(),
                ),
            ],
            2,
        );
        let install_dir = tempfile::tempdir().expect("install");
        let exe = install_dir.path().join("opencode-rust");
        std::fs::write(&exe, b"old").expect("write");

        let outcome = install_from_url_pair(
            &format!("{base}/t.tar.gz"),
            &format!("{base}/t.tar.gz.sha256"),
            &exe,
            true,
        )
        .await
        .expect("verified install");
        assert_eq!(outcome, VerifyOutcome::Verified);
        assert_eq!(std::fs::read(&exe).expect("read"), b"\x7fELF-new-binary");
        // PID-unique temp is gone; nothing update-* remains in cache.
        let cache = oc_util::global_roots::resolve().cache;
        let leftovers: Vec<String> = std::fs::read_dir(&cache)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| {
                entry.file_name().as_encoded_bytes().starts_with(TEMP_PREFIX.as_bytes())
            })
            .map(|entry| entry.path().display().to_string())
            .collect();
        assert!(leftovers.is_empty(), "temps cleaned: {leftovers:?}");
    }

    #[tokio::test]
    async fn tampered_sha256_refuses_the_install() {
        let _roots = sandbox_roots();
        let dir = tempfile::tempdir().expect("dir");
        let tarball_path = build_tarball(dir.path(), "opencode-rust", b"\x7fELF-new-binary");
        let tarball = std::fs::read(&tarball_path).expect("read");
        let wrong = oc_util::hash::sha256_hex(b"something else entirely");
        let base = serve_paths(
            vec![
                ("/t.tar.gz", tarball),
                (
                    "/t.tar.gz.sha256",
                    format!("{wrong}  opencode-9.9.9-linux-x64.tar.gz").into_bytes(),
                ),
            ],
            2,
        );
        let install_dir = tempfile::tempdir().expect("install");
        let exe = install_dir.path().join("opencode-rust");
        std::fs::write(&exe, b"old").expect("write");

        let error = install_from_url_pair(
            &format!("{base}/t.tar.gz"),
            &format!("{base}/t.tar.gz.sha256"),
            &exe,
            true,
        )
        .await
        .expect_err("tampered sidecar must refuse");
        assert!(error.contains("sha256 mismatch"), "{error}");
        assert_eq!(std::fs::read(&exe).expect("read"), b"old", "binary untouched");
        let cache = oc_util::global_roots::resolve().cache;
        assert!(
            !std::fs::read_dir(&cache).into_iter().flatten().flatten().any(|entry| entry
                .file_name()
                .as_encoded_bytes()
                .starts_with(TEMP_PREFIX.as_bytes())),
            "temps cleaned after refusal"
        );
    }

    #[tokio::test]
    async fn missing_sidecar_blocks_required_installs_only() {
        let _roots = sandbox_roots();
        let dir = tempfile::tempdir().expect("dir");
        let tarball_path = build_tarball(dir.path(), "opencode-rust", b"\x7fELF-new-binary");
        let tarball = std::fs::read(&tarball_path).expect("read");
        // Sidecar path 404s: feed installs (require) refuse, explicit
        // target installs (optional) proceed unverified.
        let base = serve_paths(vec![("/t.tar.gz", tarball)], 4);
        let install_dir = tempfile::tempdir().expect("install");
        let exe = install_dir.path().join("opencode-rust");
        std::fs::write(&exe, b"old").expect("write");

        let error = install_from_url_pair(
            &format!("{base}/t.tar.gz"),
            &format!("{base}/t.tar.gz.sha256"),
            &exe,
            true,
        )
        .await
        .expect_err("required sidecar must be fetched");
        assert!(error.contains("sidecar"), "{error}");
        assert_eq!(std::fs::read(&exe).expect("read"), b"old");

        let outcome = install_from_url_pair(
            &format!("{base}/t.tar.gz"),
            &format!("{base}/t.tar.gz.sha256"),
            &exe,
            false,
        )
        .await
        .expect("optional sidecar skips verification");
        assert_eq!(outcome, VerifyOutcome::Skipped);
        assert_eq!(std::fs::read(&exe).expect("read"), b"\x7fELF-new-binary");
    }

    #[tokio::test]
    async fn no_target_upgrade_on_a_dev_build_exits_cleanly() {
        let _roots = sandbox_roots();
        assert_eq!(RELEASE_TAG, "dev", "test builds carry no release tag");
        let code = upgrade(UpgradeRequest { method: "auto".to_owned(), target: None }).await;
        assert_eq!(code, 0, "dev builds skip with exit 0");
        // local-file with no path stays a clear error.
        let code = upgrade(UpgradeRequest { method: "local-file".to_owned(), target: None }).await;
        assert_eq!(code, 1);
    }

    #[test]
    fn replace_binary_swaps_atomically() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("dir");
        let exe = dir.path().join("opencode");
        std::fs::write(&exe, b"old").expect("write");
        let incoming = dir.path().join("incoming");
        std::fs::write(&incoming, b"new").expect("write");
        replace_binary(&incoming, &exe).expect("replace");
        assert_eq!(std::fs::read(&exe).expect("read"), b"new");
        let mode = std::fs::metadata(&exe).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn shell_config_cleaning_matches_v2_rules() {
        let config = "# comment\nexport PATH=$HOME/.opencode/bin:$PATH\n# opencode\nfish_add_path $HOME/.opencode/bin\nexport PATH=/usr/bin:$PATH\n";
        let cleaned = clean_shell_config(config);
        assert!(!cleaned.contains(".opencode/bin"), "{cleaned}");
        assert!(cleaned.contains("export PATH=/usr/bin:$PATH"), "{cleaned}");
        assert_eq!(cleaned, "# comment\nexport PATH=/usr/bin:$PATH\n");
    }

    #[test]
    fn removal_plan_honors_keep_flags() {
        let plan = removal_plan(true, false);
        assert!(plan.iter().find(|r| r.label == "Config").expect("config").keep);
        assert!(!plan.iter().find(|r| r.label == "Data").expect("data").keep);
        let plan = removal_plan(false, true);
        assert!(!plan.iter().find(|r| r.label == "Config").expect("config").keep);
        assert!(plan.iter().find(|r| r.label == "Data").expect("data").keep);
        // Cache and State never keep.
        assert!(!plan.iter().find(|r| r.label == "Cache").expect("cache").keep);
        assert!(!plan.iter().find(|r| r.label == "State").expect("state").keep);
    }

    #[test]
    fn execute_removals_collects_errors_and_respects_keep() {
        let dir = tempfile::tempdir().expect("dir");
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).expect("mkdir");
        std::fs::write(data.join("db"), "x").expect("write");
        let plan = vec![
            Removal { label: "Data", path: data.clone(), keep: false },
            Removal { label: "Missing", path: dir.path().join("absent"), keep: false },
            Removal { label: "Kept", path: dir.path().join("kept"), keep: true },
        ];
        let errors = execute_removals(&plan);
        assert_eq!(errors, Vec::<String>::new());
        assert!(!data.exists(), "non-kept dir removed");

        // A directory occupied by a file-parent causes remove_dir_all to
        // fail for a *file* path — exercise the error arm.
        let file = dir.path().join("plain-file");
        std::fs::write(&file, "x").expect("write");
        let plan = vec![Removal { label: "File", path: file, keep: false }];
        let errors = execute_removals(&plan);
        assert_eq!(errors.len(), 1, "remove_dir_all on a file reports, not panics");
        assert!(errors[0].starts_with("File:"), "{}", errors[0]);
    }
}
