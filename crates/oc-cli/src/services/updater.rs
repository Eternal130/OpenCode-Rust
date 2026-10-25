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
//!   binary on disk); package-manager methods (`npm`, `pnpm`, `bun`,
//!   `yarn`, `vp`, `brew`) run the REAL v2 install commands (v2
//!   `updater.ts` `commands` map) — the manager's registry is the
//!   trust root, so there is no sha256 step on this path (v2 has
//!   none either).
//! * **Sibling refresh (p4 follow-up)**: the updater refreshes the
//!   `oc-pty` sibling too — release archives co-package it
//!   (`package/oc-pty`), and replacing only the main binary would
//!   leave a protocol-version-skewed sibling erroring every
//!   persistent-terminal op. Archives without the member (legacy
//!   beta tarballs, raw binaries) skip the sibling silently.
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
#[allow(dead_code)] // kept public for tests/diagnostics; the upgrade paths spell names out
pub const EXTRA_METHODS: [&str; 2] = ["local-file", "github"];

/// Every accepted `--method` value (v2's seven + the two extensions).
pub const ALL_METHODS: [&str; 9] =
    ["curl", "npm", "pnpm", "bun", "yarn", "vp", "brew", "local-file", "github"];

/// Environment variable holding the full release-tarball URL for the
/// `github` method (may contain `{version}`/`{target}` placeholders).
pub const RELEASE_URL_ENV: &str = "OPENCODE_RELEASE_URL";

/// Environment variable overriding the package name the
/// package-manager methods install (v2 resolves it from the
/// `opencode.ai/update/api` release metadata; the Rust port has no
/// such feed, so the name defaults to the published npm package and
/// is overridable for tests/alternative distributions).
pub const PACKAGE_ENV: &str = "OPENCODE_PACKAGE";

/// The default package the package-manager methods install (v2's
/// `@opencode-ai/cli`, the stream the ref updater drives).
pub const DEFAULT_PACKAGE: &str = "@opencode-ai/cli";

/// The package name package-manager methods install.
#[must_use]
pub fn package_name() -> String {
    std::env::var(PACKAGE_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PACKAGE.to_owned())
}

/// The package managers whose upgrade bodies are actually driven
/// (v2 `commands` in `upgrade`): each installs `<pkg>@<version>`.
#[allow(dead_code)] // exercised by the PATH-shim tests
pub const PACKAGE_MANAGERS: [&str; 5] = ["npm", "pnpm", "bun", "yarn", "vp"];

/// Exact install argv per manager (v2 `updater.ts` `commands` +
/// bun/brew branches). Version must already be `v`-stripped.
///
/// * npm keeps the old package with `--force` (uninstalling can
///   unlink the replacement command) — v2's dominant branch for the
///   `cli` artifact when the installed package is not `cli-node`.
/// * pnpm passes `--allow-build` (the package ships postinstall).
/// * bun gets a private `--cache-dir` (it never prunes old versions
///   from its shared cache) and `--trust` for postinstall.
/// * vp uses `update` when the same package is already installed;
///   `install --force` is only for cross-package migrations, which
///   this port cannot detect (no installed-package probe), so the
///   plain `update` shape is the default.
#[must_use]
pub fn package_manager_command(method: &str, package: &str, version: &str) -> Option<Vec<String>> {
    let target = format!("{package}@{version}");
    let command = match method {
        "npm" => vec![
            "npm".to_owned(),
            "install".to_owned(),
            "--global".to_owned(),
            "--force".to_owned(),
            target,
        ],
        "pnpm" => vec![
            "pnpm".to_owned(),
            "add".to_owned(),
            "--global".to_owned(),
            format!("--allow-build={package}"),
            target,
        ],
        "bun" => vec![
            "bun".to_owned(),
            "install".to_owned(),
            "--global".to_owned(),
            "--trust".to_owned(),
            target,
        ],
        "yarn" => vec!["yarn".to_owned(), "global".to_owned(), "add".to_owned(), target],
        "vp" => vec!["vp".to_owned(), "update".to_owned(), "-g".to_owned(), target],
        "brew" => vec!["brew".to_owned(), "upgrade".to_owned(), package.to_owned()],
        _ => return None,
    };
    Some(command)
}

/// Tail-compress install output like v2 `conciseDetail`: strip
/// control chars, keep the last 12 non-empty lines, clip to ~2000
/// chars.
#[must_use]
pub fn concise_detail(output: &str) -> String {
    let normalized = output.replace('\r', "\n");
    let lines: Vec<&str> =
        normalized.lines().map(str::trim_end).filter(|line| !line.trim().is_empty()).collect();
    let tail: Vec<&str> = lines.iter().rev().take(12).rev().copied().collect();
    let joined = tail.join("\n");
    let mut detail = if joined.chars().count() > 2_000 {
        let skipped = joined.chars().count() - 1_999;
        format!("…{}", joined.chars().skip(skipped).collect::<String>())
    } else {
        joined
    };
    if lines.len() > 12 {
        detail.push_str("\n\nOutput shortened to the last 12 lines.");
    }
    detail
}

/// Run one package-manager install command (v2 `runUpgrade`):
/// 5-minute budget, output captured, non-zero exit or spawn failure
/// becomes a clear `UpgradeError` carrying the command and the
/// stderr/stdout tail. Package-manager installs have NO sha256 step
/// in v2 — the manager's own registry integrity is the trust root —
/// so this path is deliberately separate from the verified
/// download+sidecar body used by the `github`/`curl` methods.
pub async fn run_package_manager_command(command: &[String]) -> Result<(), UpgradeError> {
    let display = command.join(" ");
    let spawned = tokio::process::Command::new(&command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(Duration::from_secs(300), spawned).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            return Err(UpgradeError(format!(
                "{display} could not install OpenCode: failed to run {}: {error}. Fix the issue above, then run opencode upgrade again.",
                command[0]
            )));
        }
        Err(_) => {
            return Err(UpgradeError(format!(
                "{display} could not install OpenCode: the command timed out after 5 minutes. Fix the issue above, then run opencode upgrade again."
            )));
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let detail = if concise_detail(&stderr).is_empty() {
        concise_detail(&stdout)
    } else {
        concise_detail(&stderr)
    };
    let detail = if detail.is_empty() {
        format!(
            "The command exited with code {} without any error output.",
            output.status.code().unwrap_or(-1)
        )
    } else {
        detail
    };
    Err(UpgradeError(format!(
        "{display} could not install OpenCode: {detail}. Fix the issue above, then run opencode upgrade again."
    )))
}

/// Upgrade via a package manager (v2 `upgrade` package-manager
/// branches): resolve the package, build the exact argv, run it.
pub async fn upgrade_via_package_manager(method: &str, version: &str) -> Result<(), UpgradeError> {
    let version = version.trim().trim_start_matches('v');
    let package = package_name();
    let Some(command) = package_manager_command(method, &package, version) else {
        return Err(UpgradeError(format!("Unknown package-manager method: {method}")));
    };
    println!("Running: {}", command.join(" "));
    run_package_manager_command(&command).await
}

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
        "brew" | "npm" | "pnpm" | "bun" | "yarn" | "vp" => {
            // v2 drives the real package-manager command (`npm install
            // --global <pkg>@<ver>`, …). No sha256 step here: the
            // manager's registry is the trust root (matches ref).
            let version = target;
            match upgrade_via_package_manager(&method, version).await {
                Ok(()) => {
                    println!("Upgrade complete ({method}).");
                    0
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    1
                }
            }
        }
        other => {
            eprintln!(
                "Error: Unknown upgrade method {other:?}; supported methods: {}.",
                ALL_METHODS.join(", ")
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
        "brew" | "npm" | "pnpm" | "bun" | "yarn" | "vp" => {
            eprintln!(
                "Error: Method {requested_method:?} needs an explicit version (e.g. `opencode upgrade 2.0.24 --method {requested_method}`); the release channel only serves the Rust artifact stream."
            );
            return 1;
        }
        // `auto` still refuses to drive a Cellar build it can detect.
        "auto" if detect_method(exe) == Some("brew") => {
            eprintln!(
                "Error: This binary lives under a Homebrew Cellar; upgrade it with `brew upgrade opencode`."
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
        Ok(staged) => replace_and_report(&staged, exe),
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
    install_staged(&payload, exe)?;
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

/// Extract the `opencode-rust`/`opencode` binary — plus the
/// `oc-pty` sibling when the archive carries one — from an artifact
/// (raw ELF, gzip tarball, or zip) into staging payloads.
fn stage_binary(artifact: &Path) -> Result<StagedBinary, String> {
    let magic = read_magic(artifact)?;
    if magic.starts_with(b"\x7fELF") || is_shell_script(magic) {
        return Ok(StagedBinary { binary: artifact.to_path_buf(), pty: None });
    }
    let staging =
        std::env::temp_dir().join(format!("oc-upgrade-{}-{}", std::process::id(), unique_nanos()));
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
    // The oc-pty sibling (every release tarball since the phase-3
    // gate ruling co-packages it): co-extract so the install can
    // refresh BOTH members in lockstep. Archives without the member
    // (legacy beta tarballs) silently skip — OC_PTY_DAEMON=0 covers
    // those installs.
    let pty =
        find_member(&staging, PTY_MEMBER).map(|pty| stage_payload(&pty, "pty")).transpose()?;
    let binary = stage_payload(&extracted, "binary")?;
    Ok(StagedBinary { binary, pty })
}

/// A staged upgrade payload: the main binary plus, when the artifact
/// carried one, the extracted `oc-pty` sibling.
#[derive(Debug)]
struct StagedBinary {
    /// The `opencode-rust`/`opencode` payload to install over `exe`.
    binary: PathBuf,
    /// The `oc-pty` payload to install beside `exe`, when the
    /// archive contained the member (`None` for raw-binary inputs
    /// and legacy archives — any installed sibling stays untouched).
    pty: Option<PathBuf>,
}

impl StagedBinary {
    /// Remove the staged payload temps (`oc-upgrade-payload-*` only —
    /// raw-binary inputs hand back the SOURCE path and are never
    /// deleted).
    fn cleanup(&self) {
        for path in std::iter::once(&self.binary).chain(self.pty.as_ref()) {
            if path
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b"oc-upgrade-payload-"))
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The persistent-terminal daemon's member/sibling name: release
/// archives carry it as `package/oc-pty`, and the spawn resolution
/// (`oc_core::persistent_pty::transport`) expects it NEXT TO the
/// main executable.
const PTY_MEMBER: &str = "oc-pty";

/// Monotonic-enough suffix keeping staged payload names unique
/// within a process.
fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos())
}

/// Copy an extracted member to a PID-unique payload temp with mode
/// 755 — the extraction staging dir is deleted on return, so
/// anything that must outlive it moves somewhere stable first.
fn stage_payload(extracted: &Path, tag: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt as _;
    let payload = std::env::temp_dir().join(format!(
        "oc-upgrade-payload-{tag}-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::copy(extracted, &payload)
        .map_err(|error| format!("stage payload at {}: {error}", payload.display()))?;
    std::fs::set_permissions(&payload, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("chmod 755 {}: {error}", payload.display()))?;
    Ok(payload)
}

/// Install a staged payload pair: replace `exe` with the main
/// binary, then — when the artifact carried one — refresh the
/// `oc-pty` sibling next to it with the SAME atomic mechanics. The
/// pair must move in lockstep (p4): a stale sibling after an upgrade
/// means protocol-version skew and every persistent-terminal op
/// errors until a manual resync.
fn install_staged(staged: &StagedBinary, exe: &Path) -> Result<(), String> {
    let result = replace_binary(&staged.binary, exe).and_then(|()| match &staged.pty {
        Some(pty) => replace_binary(pty, &pty_sibling(exe)),
        None => Ok(()),
    });
    staged.cleanup();
    result
}

/// Where the `oc-pty` sibling lives for an install rooted at `exe`.
fn pty_sibling(exe: &Path) -> PathBuf {
    exe.parent().unwrap_or(Path::new(".")).join(PTY_MEMBER)
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

/// Depth-limited search for a single named member (`oc-pty`) at the
/// root or one level down — the same layout `find_binary` covers.
fn find_member(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join(name);
        if candidate.is_file() {
            return Some(candidate);
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

fn replace_and_report(staged: &StagedBinary, exe: &Path) -> i32 {
    match install_staged(staged, exe) {
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
        let staged = stage_binary(&binary).expect("stage raw");
        assert_eq!(staged.binary, binary);
        assert!(staged.pty.is_none(), "raw binaries carry no sibling");

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
        assert!(staged.binary.is_file());
        let body = std::fs::read(&staged.binary).expect("read");
        assert_eq!(body, b"\x7fELF-fake");
        assert_ne!(staged.binary, tarball, "the payload must be extracted out of the archive");
        assert!(staged.pty.is_none(), "this legacy-shaped archive carries no sibling");
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
        assert_eq!(std::fs::read(&staged.binary).expect("read"), b"\x7fELF-rust-fake");
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
        assert_eq!(std::fs::read(&staged.binary).expect("read"), b"\x7fELF-new");
    }

    /// A release-shaped tarball carrying BOTH members:
    /// `package/opencode-rust` plus, when `pty` is given, the
    /// `package/oc-pty` sibling every current release archive ships
    /// (phase-3 gate ruling).
    fn build_release_tarball(dir: &Path, main: &[u8], pty: Option<&[u8]>) -> PathBuf {
        let package = dir.join("package");
        std::fs::create_dir_all(&package).expect("mkdir");
        std::fs::write(package.join("opencode-rust"), main).expect("write main");
        if let Some(pty) = pty {
            std::fs::write(package.join("oc-pty"), pty).expect("write pty");
        }
        let tarball = dir.join("release.tar.gz");
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
    fn install_staged_replaces_the_pty_sibling_in_lockstep() {
        // p4: an upgrade must refresh BOTH members — a stale oc-pty
        // after an upgrade means protocol-version skew and every
        // persistent-terminal op errors until a manual resync.
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("dir");
        let tarball = build_release_tarball(dir.path(), b"\x7fELF-new-main", Some(b"pty-new"));
        let staged = stage_binary(&tarball).expect("stage");
        let pty = staged.pty.clone().expect("sibling staged from the archive");
        assert_eq!(std::fs::read(&staged.binary).expect("read"), b"\x7fELF-new-main");
        assert_eq!(std::fs::read(&pty).expect("read"), b"pty-new");

        let install = tempfile::tempdir().expect("install");
        let exe = install.path().join("opencode-rust");
        std::fs::write(&exe, b"old-main").expect("write");
        let sibling = install.path().join("oc-pty");
        std::fs::write(&sibling, b"old-pty").expect("write");

        install_staged(&staged, &exe).expect("install");
        assert_eq!(std::fs::read(&exe).expect("read"), b"\x7fELF-new-main");
        assert_eq!(
            std::fs::read(&sibling).expect("read"),
            b"pty-new",
            "sibling refreshes in lockstep"
        );
        let mode = std::fs::metadata(&sibling).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "the sibling installs executable");
        // Staged payload temps are cleaned up after the install.
        assert!(!staged.binary.exists() && !pty.exists(), "payload temps removed");
    }

    #[test]
    fn install_staged_skips_the_sibling_when_the_archive_has_none() {
        // Legacy beta archives carry no oc-pty member: refresh the
        // main binary, leave any installed sibling untouched (silent
        // skip — the OC_PTY_DAEMON=0 hatch covers those installs).
        let dir = tempfile::tempdir().expect("dir");
        let tarball = build_release_tarball(dir.path(), b"\x7fELF-new-main", None);
        let staged = stage_binary(&tarball).expect("stage");
        assert!(staged.pty.is_none(), "no archive member → no staged sibling");

        let install = tempfile::tempdir().expect("install");
        let exe = install.path().join("opencode-rust");
        std::fs::write(&exe, b"old-main").expect("write");
        let sibling = install.path().join("oc-pty");
        std::fs::write(&sibling, b"old-pty").expect("write");

        install_staged(&staged, &exe).expect("install");
        assert_eq!(std::fs::read(&exe).expect("read"), b"\x7fELF-new-main");
        assert_eq!(std::fs::read(&sibling).expect("read"), b"old-pty", "sibling untouched");
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
    async fn verified_install_refreshes_the_pty_sibling_too() {
        // End-to-end over the download → sha256-verify → replace path:
        // the co-packaged `package/oc-pty` member lands beside the
        // replaced main binary in the SAME install (p4).
        let _roots = sandbox_roots();
        let dir = tempfile::tempdir().expect("dir");
        let tarball_path =
            build_release_tarball(dir.path(), b"\x7fELF-new-binary", Some(b"pty-new"));
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
        let sibling = install_dir.path().join("oc-pty");
        std::fs::write(&sibling, b"old-pty").expect("write");

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
        assert_eq!(
            std::fs::read(&sibling).expect("read"),
            b"pty-new",
            "sibling refreshed in the same install"
        );
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

    // ------------------------------------------------------------------
    // Package-manager upgrade execution (v2 runUpgrade) — fake
    // binaries on PATH, never a real install
    // ------------------------------------------------------------------

    /// Write a fake `<name>` executable into `dir` that appends its
    /// argv (one per line) to `log` and exits `code` after printing
    /// `stderr_text` to stderr.
    fn fake_manager(dir: &Path, name: &str, log: &Path, code: i32, stderr_text: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        let script = dir.join(name);
        let stderr_escaped = stderr_text.replace('\\', "\\\\").replace('\'', "'\\''");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\nprintf '%s\\n' '{}' >&2\nexit {code}\n",
                log.display(),
                stderr_escaped
            ),
        )
        .expect("write fake manager");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake manager");
    }

    /// Env sandbox pointing PATH exclusively at `dir` (keeps /usr/bin
    /// out so a REAL npm can never leak in).
    struct PathGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);

    fn shim_path(dir: &Path) -> PathGuard {
        let saved = vec![("PATH", std::env::var_os("PATH"))];
        std::env::set_var("PATH", dir);
        PathGuard(saved)
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn argv_of(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log).expect("log").lines().map(str::to_owned).collect()
    }

    #[test]
    fn package_manager_commands_match_v2() {
        let npm = package_manager_command("npm", "@opencode-ai/cli", "2.0.24").expect("npm");
        assert_eq!(npm, vec!["npm", "install", "--global", "--force", "@opencode-ai/cli@2.0.24"]);
        assert_eq!(
            package_manager_command("pnpm", "pkg", "1.2.3").expect("pnpm"),
            vec!["pnpm", "add", "--global", "--allow-build=pkg", "pkg@1.2.3"]
        );
        assert_eq!(
            package_manager_command("bun", "pkg", "1.2.3").expect("bun"),
            vec!["bun", "install", "--global", "--trust", "pkg@1.2.3"]
        );
        assert_eq!(
            package_manager_command("yarn", "pkg", "1.2.3").expect("yarn"),
            vec!["yarn", "global", "add", "pkg@1.2.3"]
        );
        assert_eq!(
            package_manager_command("vp", "pkg", "1.2.3").expect("vp"),
            vec!["vp", "update", "-g", "pkg@1.2.3"]
        );
        assert_eq!(
            package_manager_command("brew", "opencode", "1.2.3").expect("brew"),
            vec!["brew", "upgrade", "opencode"]
        );
        assert!(package_manager_command("curl", "pkg", "1.2.3").is_none());
        assert!(package_manager_command("github", "pkg", "1.2.3").is_none());
    }

    #[tokio::test]
    async fn npm_upgrade_runs_the_real_argv_via_a_path_shim() {
        let dir = tempfile::tempdir().expect("dir");
        let log = dir.path().join("argv.log");
        fake_manager(dir.path(), "npm", &log, 0, "");
        let _guard = shim_path(dir.path());

        let saved = std::env::var(PACKAGE_ENV).ok();
        std::env::remove_var(PACKAGE_ENV);
        upgrade_via_package_manager("npm", "v2.0.24").await.expect("fake install succeeds");
        match saved {
            Some(saved) => std::env::set_var(PACKAGE_ENV, saved),
            None => std::env::remove_var(PACKAGE_ENV),
        }

        let expected: Vec<String> =
            ["install", "--global", "--force", &format!("{DEFAULT_PACKAGE}@2.0.24")]
                .into_iter()
                .map(str::to_owned)
                .collect();
        assert_eq!(argv_of(&log), expected);
    }

    #[tokio::test]
    async fn every_manager_runs_its_own_argv() {
        let dir = tempfile::tempdir().expect("dir");
        let _guard = shim_path(dir.path());
        let saved = std::env::var(PACKAGE_ENV).ok();
        std::env::set_var(PACKAGE_ENV, "fake-pkg");
        for method in PACKAGE_MANAGERS {
            let log = dir.path().join(format!("{method}.log"));
            fake_manager(dir.path(), method, &log, 0, "");
            upgrade_via_package_manager(method, "3.0.0").await.expect("fake install");
            let argv = argv_of(&log);
            assert!(argv.contains(&"fake-pkg@3.0.0".to_owned()), "{method}: {argv:?}");
            // Pinned versions, never floating tags.
            assert!(!argv.iter().any(|arg| arg == "latest"), "{method}");
        }
        match saved {
            Some(saved) => std::env::set_var(PACKAGE_ENV, saved),
            None => std::env::remove_var(PACKAGE_ENV),
        }
    }

    #[tokio::test]
    async fn non_zero_exit_propagates_stderr_tail() {
        let dir = tempfile::tempdir().expect("dir");
        let log = dir.path().join("argv.log");
        fake_manager(
            dir.path(),
            "npm",
            &log,
            3,
            "npm ERR! code EACCES\nnpm ERR! permission denied",
        );
        let _guard = shim_path(dir.path());

        let error = upgrade_via_package_manager("npm", "1.0.0").await.expect_err("exit 3");
        assert!(error.0.contains("could not install OpenCode"), "{}", error.0);
        assert!(error.0.contains("npm install --global"), "{}", error.0);
        assert!(error.0.contains("EACCES"), "stderr tail survives: {}", error.0);
        assert!(error.0.contains("permission denied"), "{}", error.0);
    }

    #[tokio::test]
    async fn non_zero_exit_without_output_still_explains() {
        let dir = tempfile::tempdir().expect("dir");
        let log = dir.path().join("argv.log");
        fake_manager(dir.path(), "vp", &log, 2, "");
        let _guard = shim_path(dir.path());

        let error = upgrade_via_package_manager("vp", "1.0.0").await.expect_err("exit 2");
        assert!(error.0.contains("exited with code 2"), "{}", error.0);
    }

    #[tokio::test]
    async fn missing_binary_is_a_clear_error() {
        // PATH points at an empty dir: every manager name is missing.
        let dir = tempfile::tempdir().expect("dir");
        let _guard = shim_path(dir.path());

        let error = run_package_manager_command(&[
            "definitely-not-a-real-manager".to_owned(),
            "x".to_owned(),
        ])
        .await
        .expect_err("spawn fails");
        assert!(error.0.contains("failed to run"), "{}", error.0);
        assert!(error.0.contains("could not install OpenCode"), "{}", error.0);
    }

    #[tokio::test]
    async fn upgrade_entrypoint_returns_zero_on_success_and_one_on_failure() {
        let dir = tempfile::tempdir().expect("dir");
        let log_ok = dir.path().join("ok.log");
        fake_manager(dir.path(), "yarn", &log_ok, 0, "");
        let log_bad = dir.path().join("bad.log");
        fake_manager(dir.path(), "pnpm", &log_bad, 9, "boom");
        let _guard = shim_path(dir.path());

        let code =
            upgrade(UpgradeRequest { method: "yarn".to_owned(), target: Some("4.5.6".to_owned()) })
                .await;
        assert_eq!(code, 0);
        assert!(argv_of(&log_ok).iter().any(|arg| arg.contains("4.5.6")), "{:?}", argv_of(&log_ok));

        let code =
            upgrade(UpgradeRequest { method: "pnpm".to_owned(), target: Some("4.5.6".to_owned()) })
                .await;
        assert_eq!(code, 1);
    }

    #[test]
    fn concise_detail_matches_v2_rules() {
        // v2 only trimEnds each line: leading whitespace survives.
        assert_eq!(concise_detail("  hello  \n\nworld\r\n"), "  hello\nworld");
        let many = (1..=20).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let detail = concise_detail(&many);
        assert!(detail.starts_with('9'), "keeps the last 12 lines: {detail}");
        assert!(detail.ends_with("Output shortened to the last 12 lines."));
        let long = format!("{}\n", "x".repeat(3_000));
        assert!(concise_detail(&long).starts_with('…'), "long output is clipped");
    }
}
