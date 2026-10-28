//! Port of `packages/core/src/ripgrep/binary.ts` — ripgrep binary
//! acquisition. Resolution order (binary.ts:100-136, extended with the
//! port-side `OPENCODE_RG_PATH`/bundled seams already used by the tool
//! layer): `OPENCODE_RG_PATH` → bundled `<exe_dir>/rg` → `rg` on `PATH`
//! → global-cache bin (`<cache>/opencode-rust/bin/rg`) → managed
//! auto-download of GitHub release 15.1.0 with archive extraction.
//!
//! Downloads are runtime-only: the test seam ([`resolve_with`]) takes an
//! injected fetch, so no test ever touches the network.

use std::path::{Path, PathBuf};

use tokio::process::Command;
use tokio::sync::OnceCell;

/// `VERSION` (binary.ts:14).
pub const RG_VERSION: &str = "15.1.0";

/// Kill switch for the managed download (port-side offline/CI guard,
/// mirroring the bun manager's `OPENCODE_DISABLE_BUN_DOWNLOAD`).
pub const DISABLE_DOWNLOAD_ENV: &str = "OPENCODE_DISABLE_RG_DOWNLOAD";

/// `OPENCODE_RG_PATH` — explicit binary override.
pub const RG_PATH_ENV: &str = "OPENCODE_RG_PATH";

/// One `PLATFORM` row (binary.ts:16-25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformConfig {
    /// Rust-style release triple, e.g. `x86_64-unknown-linux-musl`.
    pub platform: &'static str,
    /// `tar.gz` (unix) or `zip` (windows).
    pub extension: &'static str,
}

/// `PLATFORM[`${arch}-${platform}`]` for the running process.
#[must_use]
pub fn platform_config() -> Option<PlatformConfig> {
    platform_config_for(std::env::consts::ARCH, std::env::consts::OS)
}

/// Pure form of [`platform_config`] (testable for unsupported rows).
#[must_use]
pub fn platform_config_for(arch: &str, os: &str) -> Option<PlatformConfig> {
    match (arch, os) {
        ("aarch64", "macos") => Some(cfg_row("aarch64-apple-darwin", "tar.gz")),
        ("aarch64", "linux") => Some(cfg_row("aarch64-unknown-linux-gnu", "tar.gz")),
        ("x86_64", "macos") => Some(cfg_row("x86_64-apple-darwin", "tar.gz")),
        ("x86_64", "linux") => Some(cfg_row("x86_64-unknown-linux-musl", "tar.gz")),
        ("aarch64", "windows") => Some(cfg_row("aarch64-pc-windows-msvc", "zip")),
        ("x86", "windows") => Some(cfg_row("i686-pc-windows-msvc", "zip")),
        ("x86_64", "windows") => Some(cfg_row("x86_64-pc-windows-msvc", "zip")),
        _ => None,
    }
}

const fn cfg_row(platform: &'static str, extension: &'static str) -> PlatformConfig {
    PlatformConfig { platform, extension }
}

impl PlatformConfig {
    /// `ripgrep-<version>-<platform>.<ext>` (binary.ts:120).
    #[must_use]
    pub fn filename(&self) -> String {
        format!("ripgrep-{RG_VERSION}-{}.{}", self.platform, self.extension)
    }

    /// The GitHub release download URL (binary.ts:121).
    #[must_use]
    pub fn url(&self) -> String {
        format!(
            "https://github.com/BurntSushi/ripgrep/releases/download/{RG_VERSION}/{}",
            self.filename()
        )
    }

    fn exe_name(&self) -> &'static str {
        if self.extension == "zip" { "rg.exe" } else { "rg" }
    }
}

/// Where a resolved ripgrep came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgSource {
    EnvOverride,
    Bundled,
    Path,
    GlobalCache,
    Download,
}

/// Binary resolution failures (binary.ts error messages).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RgError {
    /// `unsupported platform for ripgrep: <key>`.
    UnsupportedPlatform(String),
    /// Empty download body (`failed to download ripgrep from <url>`).
    EmptyDownload(String),
    /// Download transport failure.
    Download(String),
    /// `ripgrep extraction failed ...` / missing executable in archive.
    Extract(String),
    Io(String),
}

impl std::fmt::Display for RgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform(key) => write!(f, "unsupported platform for ripgrep: {key}"),
            Self::EmptyDownload(url) => write!(f, "failed to download ripgrep from {url}"),
            Self::Download(message) => write!(f, "ripgrep download failed: {message}"),
            Self::Extract(message) => write!(f, "{message}"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

/// Resolve the ripgrep binary synchronously (no download):
/// `OPENCODE_RG_PATH` → bundled `<exe_dir>/rg` → `rg` from `PATH`.
/// Kept for call sites that spawn without awaiting acquisition.
#[must_use]
pub fn binary() -> PathBuf {
    if let Some(path) = env_override() {
        return path;
    }
    if let Some(bundled) = bundled_binary() {
        return bundled;
    }
    PathBuf::from(if std::env::consts::OS == "windows" { "rg.exe" } else { "rg" })
}

/// `OPENCODE_RG_PATH` when it points at an existing file.
fn env_override() -> Option<PathBuf> {
    std::env::var_os(RG_PATH_ENV).map(PathBuf::from).filter(|path| path.is_file())
}

/// Bundled `<exe_dir>/rg[.exe]`.
fn bundled_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if std::env::consts::OS == "windows" { "rg.exe" } else { "rg" };
    let bundled = dir.join(name);
    bundled.is_file().then_some(bundled)
}

/// `which(name)` (util/which.ts) — `PATH` scan for an executable file.
#[must_use]
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut candidates: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // ref appends `global.bin` to the search path before the explicit
    // cache check (binary.ts:55,70) — same practical result, so a plain
    // PATH scan plus the explicit target check below covers it.
    candidates.retain(|candidate| !candidate.as_os_str().is_empty());
    let exe_ext = if std::env::consts::OS == "windows" { ".exe" } else { "" };
    for dir in candidates {
        let candidate = dir.join(format!("{name}{exe_ext}"));
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// The injectable inputs of the full resolution (test seam).
pub struct ResolveDeps {
    /// `OPENCODE_RG_PATH` value (already file-checked by the caller).
    pub env_override: Option<PathBuf>,
    /// Bundled `<exe_dir>/rg`.
    pub bundled: Option<PathBuf>,
    /// `which("rg")` result.
    pub path_binary: Option<PathBuf>,
    /// The global-cache bin dir (`Global.bin`).
    pub global_bin: PathBuf,
}

impl ResolveDeps {
    /// Real-environment inputs (download-agnostic).
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            env_override: env_override(),
            bundled: bundled_binary(),
            path_binary: which(if std::env::consts::OS == "windows" { "rg.exe" } else { "rg" }),
            global_bin: oc_util::global_roots::resolve().bin(),
        }
    }
}

/// Cached process-wide resolution (ref `Effect.cached` around the
/// resolution pipeline, binary.ts:99).
static RESOLVED: OnceCell<PathBuf> = OnceCell::const_new();

/// Full async resolution with a per-process cache; falls back to a bare
/// `rg` PATH spawn when every source fails (best-effort parity with the
/// previous synchronous behavior).
pub async fn resolve_cached() -> PathBuf {
    // The env override is honored per call (not cached) so re-pointing
    // `OPENCODE_RG_PATH` takes effect immediately — the cache only pins
    // the runtime discovery chain (PATH / global cache / download).
    if let Some(path) = env_override() {
        return path;
    }
    RESOLVED
        .get_or_init(|| async {
            match resolve(|url| http_download(url)).await {
                Ok((path, _)) => path,
                Err(_) => binary(),
            }
        })
        .await
        .clone()
}

/// Uncached resolution with the real HTTP downloader (runtime only;
/// tests use [`resolve_with`] instead). Honors
/// [`DISABLE_DOWNLOAD_ENV`].
pub async fn resolve<F, Fut>(fetch: F) -> Result<(PathBuf, RgSource), RgError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    let deps = ResolveDeps::from_env();
    if std::env::var_os(DISABLE_DOWNLOAD_ENV).is_some() {
        return resolve_without_download(&deps);
    }
    resolve_with(&deps, fetch).await
}

fn resolve_without_download(deps: &ResolveDeps) -> Result<(PathBuf, RgSource), RgError> {
    if let Some(path) = &deps.env_override {
        return Ok((path.clone(), RgSource::EnvOverride));
    }
    if let Some(path) = &deps.bundled {
        return Ok((path.clone(), RgSource::Bundled));
    }
    if let Some(path) = &deps.path_binary {
        return Ok((path.clone(), RgSource::Path));
    }
    let target = exe_target(&deps.global_bin);
    if target.is_file() {
        return Ok((target, RgSource::GlobalCache));
    }
    Err(RgError::Download("download disabled".to_string()))
}

/// The core resolution pipeline (binary.ts:100-136):
/// env override → bundled → PATH → global cache → download+extract.
pub async fn resolve_with<F, Fut>(
    deps: &ResolveDeps,
    fetch: F,
) -> Result<(PathBuf, RgSource), RgError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    if let Some(path) = &deps.env_override {
        return Ok((path.clone(), RgSource::EnvOverride));
    }
    if let Some(path) = &deps.bundled {
        return Ok((path.clone(), RgSource::Bundled));
    }
    if let Some(path) = &deps.path_binary {
        return Ok((path.clone(), RgSource::Path));
    }
    let target = exe_target(&deps.global_bin);
    if target.is_file() {
        return Ok((target, RgSource::GlobalCache));
    }

    let config = platform_config()
        .ok_or_else(|| RgError::UnsupportedPlatform(format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)))?;
    let url = config.url();
    let bytes = fetch(url.clone()).await.map_err(RgError::Download)?;
    if bytes.is_empty() {
        return Err(RgError::EmptyDownload(url));
    }
    std::fs::create_dir_all(&deps.global_bin).map_err(|error| RgError::Io(error.to_string()))?;
    let archive = deps.global_bin.join(config.filename());
    tokio::fs::write(&archive, &bytes)
        .await
        .map_err(|error| RgError::Io(error.to_string()))?;
    let outcome = extract(&archive, &config, &target).await;
    let _ = tokio::fs::remove_file(&archive).await;
    outcome?;
    Ok((target, RgSource::Download))
}

/// `<global_bin>/rg[.exe]` (binary.ts:71).
fn exe_target(global_bin: &Path) -> PathBuf {
    global_bin.join(if std::env::consts::OS == "windows" { "rg.exe" } else { "rg" })
}

/// `extract` (binary.ts:73-97) — unpack into a temp dir under the global
/// bin, copy the release executable out, chmod 755 on unix.
async fn extract(archive: &Path, config: &PlatformConfig, target: &Path) -> Result<(), RgError> {
    let dir = tempfile_in(global_bin_parent(target)?).await?;
    let scope = TempDirGuard(&dir);

    if config.extension == "zip" {
        // ref shells out to powershell `Expand-Archive` (binary.ts:81-91).
        let shell = which("powershell.exe").or_else(|| which("pwsh.exe"));
        let shell = shell.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "powershell.exe".to_string());
        let script = format!(
            "$global:ProgressPreference = 'SilentlyContinue'; Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive.to_string_lossy().replace('\'', "''"),
            dir.to_string_lossy().replace('\'', "''"),
        );
        run_extract(&shell, &["-NoProfile", "-NonInteractive", "-Command", &script]).await?;
    } else {
        // `tar -xzf <archive> -C <dir>` (binary.ts:94).
        run_extract("tar", &["-xzf", &archive.to_string_lossy(), "-C", &dir.to_string_lossy()])
            .await?;
    }

    let extracted = dir
        .join(format!("ripgrep-{RG_VERSION}-{}", config.platform))
        .join(config.exe_name());
    if !extracted.is_file() {
        return Err(RgError::Extract(format!(
            "ripgrep archive did not contain executable: {}",
            extracted.display()
        )));
    }
    tokio::fs::copy(&extracted, target)
        .await
        .map_err(|error| RgError::Io(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(target)
            .map_err(|error| RgError::Io(error.to_string()))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(target, permissions).map_err(|error| RgError::Io(error.to_string()))?;
    }
    let _ = scope; // cleanup on drop
    Ok(())
}

fn global_bin_parent(target: &Path) -> Result<PathBuf, RgError> {
    target
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| RgError::Io("global bin dir unavailable".to_string()))
}

/// `makeTempDirectoryScoped({ directory: global.bin, prefix: "ripgrep-" })`.
async fn tempfile_in(base: PathBuf) -> Result<PathBuf, RgError> {
    tokio::fs::create_dir_all(&base)
        .await
        .map_err(|error| RgError::Io(error.to_string()))?;
    for _ in 0..16 {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let candidate = base.join(format!("ripgrep-{suffix}"));
        match tokio::fs::create_dir(&candidate).await {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(RgError::Io(error.to_string())),
        }
    }
    Err(RgError::Io("could not create ripgrep temp dir".to_string()))
}

/// Removes the extraction temp dir when dropped.
struct TempDirGuard<'a>(&'a Path);
impl Drop for TempDirGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

async fn run_extract(program: &str, args: &[&str]) -> Result<(), RgError> {
    let output = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|error| RgError::Extract(format!("ripgrep extraction failed: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let message = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("ripgrep extraction failed with code {}", output.status.code().unwrap_or(-1))
        };
        return Err(RgError::Extract(message));
    }
    Ok(())
}

/// Real HTTP fetch of a release archive (runtime only — never in tests).
async fn http_download(url: String) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();
    let response = client.get(&url).send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let bytes = response.bytes().await.map_err(|error| error.to_string())?;
    Ok(bytes.to_vec())
}
