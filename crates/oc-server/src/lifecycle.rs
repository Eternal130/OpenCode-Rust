//! Service lifecycle (`cli/services/service-registration.ts`,
//! `client/service-probe.ts`, `client/promise/service.ts`,
//! `client/service-contender.ts`, `client/pty-handoff.ts`,
//! `cli/server-process.ts`) plus the axum `serve()` entry.
//!
//! Everything the daemon side and the client side share — registration
//! files, health probes, the ensure/stop loops, PTY handoff sidecars — lives
//! here. Timing constants match v2 exactly (probe 2s, stop poll 50ms × 100,
//! watcher 5s, EADDRINUSE retry 100ms × 15s, ensure wall clock 120s).

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::state::AppState;

/// The build identity this binary reports on `/api/info` (the
/// `x-opencode-build` response header) and requires of a managed
/// daemon (B4-H3): `<OC_RELEASE_TAG>@<git-hash>` stamped at compile
/// time. The parity pin keeps `version` at "2.0.24" across betas, so
/// the VERSION comparison alone cannot tell an upgraded client from
/// the stale daemon it spawned earlier — this stamp can. The header
/// (not a body field) keeps the info payload byte-shaped like v2's:
/// the served `/openapi.json` is sha-pinned in `goldens/p10` and
/// `reference_api_info.json` pins the reference body — headers are
/// invisible to both.
pub const BUILD_STAMP: &str = match option_env!("OC_BUILD_STAMP") {
    Some(stamp) => stamp,
    None => "dev@unknown",
};

/// The response header carrying [`BUILD_STAMP`].
pub const BUILD_HEADER: &str = "x-opencode-build";

// ===========================================================================
// Registration (service-registration.ts)
// ===========================================================================

/// Contents of the service registration file (`Service.Info`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    /// Unique service instance identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// `OpenCode` version served by the process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Base URL advertised by the service.
    pub url: String,
    /// Operating system process identifier.
    pub pid: u32,
    /// Private service password, when authentication is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// Per-channel registration filename (`ServiceConfig.filename`).
#[must_use]
pub fn registration_filename(channel: &str) -> String {
    match channel {
        "latest" | "dev" | "beta" | "next" => "service.json".to_owned(),
        other => {
            let sanitized: String = other
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            format!("service-{sanitized}.json")
        }
    }
}

/// The default registration file (`service-probe.ts fallback`).
#[must_use]
pub fn registration_fallback() -> PathBuf {
    let state = std::env::var("XDG_STATE_HOME").map_or_else(
        |_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
            PathBuf::from(home).join(".local").join("state")
        },
        PathBuf::from,
    );
    state.join("opencode").join("service.json")
}

/// Reads a registration; missing or corrupt files are both `None`
/// (the registering server self-evicts, clients rediscover).
#[must_use]
pub fn read_registration(file: Option<&Path>) -> Option<Registration> {
    let fallback;
    let file = if let Some(file) = file {
        file
    } else {
        fallback = registration_fallback();
        fallback.as_path()
    };
    let text = fs_read(file).ok()?;
    serde_json::from_str(&text).ok()
}

fn fs_read(file: &Path) -> io::Result<String> {
    std::fs::read_to_string(file)
}

/// Whether `found` is exactly this registration (all five fields — `owns`).
#[must_use]
pub fn registration_owns(found: &Registration, info: &Registration) -> bool {
    found.id == info.id
        && found.version == info.version
        && found.url == info.url
        && found.pid == info.pid
        && found.password == info.password
}

/// Whether two registrations describe the same service instance (`same`).
#[must_use]
pub fn registration_same(left: &Registration, right: &Registration) -> bool {
    left.id == right.id
        && left.version == right.version
        && left.url == right.url
        && left.pid == right.pid
}

/// Atomic registration write: `<file>.<id>.tmp` mode 0600 → rename, plus
/// the generation sidecar (`<file>.boot` with `boot_id` + process start
/// time) that later stops use to refuse signalling recycled PIDs.
pub fn write_registration(file: &Path, info: &Registration) -> io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = file
        .with_extension(format!("{}.tmp", info.id.clone().unwrap_or_else(|| "anon".to_owned())));
    write_private(&temp, serde_json::to_string(info).expect("registration serializes").as_bytes())?;
    std::fs::rename(&temp, file)?;
    crate::safekill::write_expectation(file, info.pid);
    Ok(())
}

/// `0o600` file write.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

/// `register()`: writes the registration, spawns the 5s self-heal watcher
/// (corrupt/replaced → shutdown), and returns the shutdown cleanup.
pub struct RegisterOptions {
    /// Bound server address (advertised URL).
    pub address: SocketAddr,
    /// Service password.
    pub password: String,
    /// Instance id (uuid).
    pub id: String,
    /// Registration file.
    pub file: PathBuf,
    /// `OpenCode` version.
    pub version: String,
    /// Shutdown latch the watcher fires when it no longer owns the file.
    pub shutdown: watch::Sender<bool>,
    /// Watcher cadence (5s).
    pub watch_interval: Duration,
}

/// The registration cleanup: removes the file if still owned.
#[derive(Debug, Clone)]
pub struct RegistrationCleanup {
    file: PathBuf,
    info: Registration,
}

impl RegistrationCleanup {
    /// Re-reads the registration; removes the file only when it still
    /// describes this instance.
    pub async fn run(self) {
        if let Some(found) = read_registration(Some(&self.file)) {
            if registration_owns(&found, &self.info) {
                let _ = tokio::fs::remove_file(&self.file).await;
            }
        }
    }
}

/// Registers the running service and starts the self-heal watcher.
pub fn register(options: RegisterOptions) -> io::Result<RegistrationCleanup> {
    let info = Registration {
        id: Some(options.id),
        version: Some(options.version),
        url: format!("http://{}", options.address),
        pid: std::process::id(),
        password: Some(options.password),
    };
    write_registration(&options.file, &info)?;

    let file = options.file.clone();
    let owned = info.clone();
    let shutdown = options.shutdown;
    let interval = options.watch_interval;
    tokio::spawn(async move {
        loop {
            match read_registration(Some(&file)) {
                None => {
                    eprintln!(
                        "managed service registration check failed; shutting down (registration: {})",
                        file.display()
                    );
                    let _ = shutdown.send(true);
                    return;
                }
                Some(found) => {
                    if !registration_owns(&found, &owned) {
                        eprintln!(
                            "managed service registration replaced; shutting down \
                             (observedServiceID: {:?}, observedPID: {}, observedVersion: {:?})",
                            found.id, found.pid, found.version
                        );
                        let _ = shutdown.send(true);
                        return;
                    }
                }
            }
            tokio::time::sleep(interval).await;
        }
    });

    Ok(RegistrationCleanup { file: options.file, info })
}

// ===========================================================================
// Health probe (service-probe.ts) — minimal HTTP/1.1 client, no TLS
// ===========================================================================

/// Connection details for a local service (`Endpoint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Base URL.
    pub url: String,
    /// Basic credentials, when configured.
    pub auth: Option<(String, String)>,
}

/// Probe classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    Ready,
    Waiting,
    Failed,
}

/// A classified probe result (`LocalService`).
#[derive(Debug, Clone)]
pub struct ProbedService {
    /// The registration that was probed.
    pub info: Registration,
    /// Endpoint derived from it.
    pub endpoint: Endpoint,
    /// Version the server reported.
    pub version: Option<String>,
    /// Build identity the server reported (`x-opencode-build`;
    /// `None` when the server predates the marker — v2 daemons and
    /// pre-B4-H3 builds of the port).
    pub build: Option<String>,
    /// Health classification.
    pub state: ProbeState,
    /// Protocol compatibility (404 on /api/info = ready but incompatible).
    pub compatible: bool,
}

/// One probe round (`probeResult`).
#[derive(Debug, Clone, Default)]
pub struct ProbeResult {
    /// `None` = undefined (could not classify).
    pub service: Option<ProbedService>,
    /// Whether the transport attempt timed out.
    pub timed_out: bool,
}

/// `headers(endpoint)`: Basic auth header value.
#[must_use]
pub fn endpoint_auth_header(endpoint: &Endpoint) -> Option<String> {
    use base64::Engine as _;
    let (username, password) = endpoint.auth.as_ref()?;
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
    ))
}

/// GET `<url>/api/info` with Basic auth, classified per `probeResult`.
pub async fn probe(info: &Registration, timeout: Duration) -> ProbeResult {
    let endpoint = Endpoint {
        url: info.url.clone(),
        auth: info.password.clone().map(|password| ("opencode".to_owned(), password)),
    };
    let url = format!("{}/api/info", info.url.trim_end_matches('/'));
    let mut headers = Vec::new();
    if let Some(auth) = endpoint_auth_header(&endpoint) {
        headers.push(("authorization".to_owned(), auth));
    }

    let started = std::time::Instant::now();
    let response = http_request("GET", &url, &headers, &[], timeout).await;
    let timed_out = response.is_err() && started.elapsed() >= timeout;
    let Ok(response) = response else {
        return ProbeResult { service: None, timed_out };
    };

    // 404 = protocol incompatibility, not an old version.
    if response.status == 404 {
        return ProbeResult {
            service: Some(ProbedService {
                info: info.clone(),
                endpoint,
                version: info.version.clone(),
                build: None,
                state: ProbeState::Ready,
                compatible: false,
            }),
            timed_out: false,
        };
    }

    let Some(server) = decode_server_info(&response.body) else {
        return ProbeResult { service: None, timed_out: false };
    };
    if server.pid != info.pid {
        return ProbeResult { service: None, timed_out: false };
    }
    if let Some(required) = &info.version {
        if &server.version != required {
            return ProbeResult { service: None, timed_out: false };
        }
    }
    let state = if (200..300).contains(&response.status) {
        ProbeState::Ready
    } else if response.status == 500 {
        ProbeState::Failed
    } else {
        ProbeState::Waiting
    };
    ProbeResult {
        service: Some(ProbedService {
            info: info.clone(),
            endpoint,
            version: Some(server.version),
            build: response.build,
            state,
            compatible: true,
        }),
        timed_out: false,
    }
}

struct ServerInfo {
    version: String,
    pid: u32,
}

/// `decodeInfo`: version must be a string, pid an integer ≥ 0.
fn decode_server_info(body: &[u8]) -> Option<ServerInfo> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let version = value.get("version")?.as_str()?.to_owned();
    let pid = value.get("pid")?.as_u64()?;
    let pid = u32::try_from(pid).ok()?;
    Some(ServerInfo { version, pid })
}

/// Minimal HTTP/1.1 client: `connection: close`, content-length or chunked
/// decoding. The probe only ever needs plain-HTTP GETs on loopback.
async fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
) -> Result<MiniResponse, String> {
    use std::fmt::Write as _;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (host, port, path) = parse_url(url)?;
    let request = tokio::time::timeout(timeout, async {
        let mut stream = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|error| format!("connect: {error}"))?;
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nhost: {host}:{port}\r\nconnection: close\r\ncontent-length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            let _ = write!(request, "{name}: {value}\r\n");
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| format!("write: {error}"))?;
        stream
            .write_all(body)
            .await
            .map_err(|error| format!("write body: {error}"))?;

        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = stream
                .read(&mut chunk)
                .await
                .map_err(|error| format!("read: {error}"))?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if raw.len() > 16 * 1024 * 1024 {
                return Err("response too large".to_owned());
            }
        }
        Ok(raw)
    })
    .await
    .map_err(|_| "request timed out".to_owned())??;

    parse_response(&request)
}

struct MiniResponse {
    status: u16,
    body: Vec<u8>,
    /// The `x-opencode-build` header value, when present (B4-H3).
    build: Option<String>,
}

fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| "only http:// supported".to_owned())?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped.find(']').ok_or_else(|| "invalid authority".to_owned())?;
        let host = &stripped[..end];
        let port = stripped[end + 1..]
            .strip_prefix(':')
            .and_then(|port| port.parse::<u16>().ok())
            .unwrap_or(80);
        (host.to_owned(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port))
                if port.bytes().all(|byte| byte.is_ascii_digit()) && !port.is_empty() =>
            {
                (host.to_owned(), port.parse::<u16>().map_err(|_| "invalid port".to_owned())?)
            }
            _ => (authority.to_owned(), 80),
        }
    };
    Ok((host, port, path.to_owned()))
}

fn parse_response(raw: &[u8]) -> Result<MiniResponse, String> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "truncated response".to_owned())?;
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(|| "empty response".to_owned())?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "malformed status".to_owned())?;
    let mut content_length = None;
    let mut chunked = false;
    let mut build = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                content_length = value.parse::<usize>().ok();
            } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            } else if name == BUILD_HEADER {
                build = Some(value.to_owned());
            }
        }
    }
    let rest = &raw[header_end + 4..];
    let body = if chunked {
        decode_chunked(rest)
    } else if let Some(length) = content_length {
        rest[..length.min(rest.len())].to_vec()
    } else {
        rest.to_vec()
    };
    Ok(MiniResponse { status, body, build })
}

fn decode_chunked(mut input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = input.windows(2).position(|window| window == b"\r\n") {
        let header = String::from_utf8_lossy(&input[..line_end]).into_owned();
        let size_text = header.split(';').next().unwrap_or_default().trim().to_owned();
        let Ok(size) = usize::from_str_radix(&size_text, 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = line_end + 2;
        let end = (start + size).min(input.len());
        out.extend_from_slice(&input[start..end]);
        input = &input[end.min(input.len())..];
        if input.starts_with(b"\r\n") {
            input = &input[2..];
        }
    }
    out
}

// ===========================================================================
// Incumbent + ensure loop (service.ts / service-contender.ts)
// ===========================================================================

/// `on_start` callback shape (`(reason, previousVersion?) => void`).
pub type StartCallback = Box<dyn Fn(&str, Option<&str>) + Send + Sync>;

/// Ensure/stop timing constants (`defaultEnsureTiming`).
#[derive(Debug, Clone, Copy)]
pub struct EnsureTiming {
    /// Registration poll cadence.
    pub poll_interval: Duration,
    /// Probe request timeout.
    pub request_timeout: Duration,
    /// Delay before a contender competes with an unanswered registration.
    pub spawn_delay: Duration,
    /// Backoff cap for repeated clean exits.
    pub max_spawn_delay: Duration,
    /// Total wall clock for one `ensure()` call.
    pub promise_timeout: Duration,
    /// Stop poll cadence.
    pub stop_poll_interval: Duration,
    /// Stop poll attempts.
    pub stop_poll_attempts: u32,
}

impl Default for EnsureTiming {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(25),
            request_timeout: Duration::from_secs(2),
            spawn_delay: Duration::from_secs(5),
            max_spawn_delay: Duration::from_secs(30),
            promise_timeout: Duration::from_secs(120),
            stop_poll_interval: Duration::from_millis(50),
            stop_poll_attempts: 100,
        }
    }
}

/// Whether a version satisfies the requirement (`matchesVersion`).
#[must_use]
pub fn matches_version(version: Option<&str>, required: Option<&str>) -> bool {
    match required {
        None => true,
        Some(_) => version == required,
    }
}

/// Whether a reported build satisfies the requirement (B4-H3): `None`
/// required accepts anything (the `checkVersion: false` paths keep
/// their reconnect-regardless semantics); a REQUIRED build demands an
/// exact match — a daemon that reports none (v2 daemons, pre-marker
/// builds of the port) counts as stale, which is what makes the
/// upgrade flow replace the pre-marker daemon it is looking at.
#[must_use]
pub fn matches_build(build: Option<&str>, required: Option<&str>) -> bool {
    match required {
        None => true,
        Some(_) => build == required,
    }
}

/// `incumbent()`: probes the exact URL the daemon would bind.
pub async fn incumbent(
    file: Option<&Path>,
    url: &str,
    required_version: Option<&str>,
    required_build: Option<&str>,
    timeout: Duration,
) -> Option<ProbedService> {
    let mut info = read_registration(file)?;
    info.url = url.to_owned();
    let found = probe(&info, timeout).await.service?;
    if !found.compatible {
        return None;
    }
    if !matches_version(found.version.as_deref(), required_version) {
        return None;
    }
    if !matches_build(found.build.as_deref(), required_build) {
        return None;
    }
    Some(found)
}

/// One spawned service attempt (stderr ring 8 KiB, detached).
struct Contender {
    child: std::process::Child,
    error: Option<String>,
    closed: bool,
    stderr: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl Contender {
    fn spawn(command: &[String], env: &[(String, String)]) -> io::Result<Self> {
        let (program, args) = command.split_first().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Missing service command")
        })?;
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .env_remove("RUST_BACKTRACE");
        for (name, value) in env {
            cmd.env(name, value);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take();
        let ring = Arc::new(std::sync::Mutex::new(Vec::new()));
        if let Some(stderr) = stderr {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                use std::io::Read;
                let mut stderr = stderr;
                let mut chunk = [0u8; 2048];
                loop {
                    match stderr.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            let mut guard =
                                ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            guard.extend_from_slice(&chunk[..read]);
                            let len = guard.len();
                            if len > 8 * 1024 {
                                guard.drain(..len - 8 * 1024);
                            }
                        }
                    }
                }
            });
        }
        Ok(Self { child, error: None, closed: false, stderr: ring })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn poll(&mut self) {
        if self.closed {
            return;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.closed = true;
                if !status.success() {
                    self.error = Some(format!("exited with {status}"));
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.closed = true;
                self.error = Some(error.to_string());
            }
        }
    }

    fn finished(&self) -> bool {
        self.closed
    }

    fn stderr_text(&self) -> String {
        let guard = self.stderr.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&guard).trim().to_owned()
    }

    /// `release()` (service-contender.ts:49): DETACH — stop tracking, never
    /// signal. A contender that won the race *is* the daemon: killing it on
    /// scope exit was murdering every freshly ensured background service the
    /// moment the first client's `ensure()` returned. The loser exits on its
    /// own (EADDRINUSE contender stand-down / registration self-heal).
    fn release(mut self) {
        let _ = self.child.stdin.take();
        let _ = self.child.stdout.take();
        drop(self.child);
    }
}

/// The startup attempts of one `ensure()` call.
struct ContenderPool {
    contenders: Vec<Contender>,
    failure: Option<String>,
    spawn_delay: Duration,
    last_spawn: Option<std::time::Instant>,
}

impl ContenderPool {
    fn new(timing: EnsureTiming) -> Self {
        Self {
            contenders: Vec::new(),
            failure: None,
            spawn_delay: timing.spawn_delay,
            last_spawn: None,
        }
    }

    fn service_answered(&mut self, timing: EnsureTiming) {
        self.spawn_delay = timing.spawn_delay;
    }

    fn evict(&mut self, pid: u32, timing: EnsureTiming) {
        self.contenders.retain(|item| item.pid() != pid && !item.finished());
        self.failure = None;
        self.last_spawn = None;
        let _ = timing;
    }

    fn recruit_now(&mut self) {
        self.last_spawn = Some(
            std::time::Instant::now()
                .checked_sub(self.spawn_delay)
                .unwrap_or_else(std::time::Instant::now),
        );
    }

    fn reap(&mut self, timing: EnsureTiming) -> Option<EnsureError> {
        for item in &mut self.contenders {
            item.poll();
        }
        let failed: Vec<_> = self
            .contenders
            .iter()
            .filter(|item| item.finished() && item.error.is_some())
            .map(|item| (item.stderr_text(), item.error.clone()))
            .collect();
        if self.failure.is_none() {
            self.failure = failed.first().map(|(stderr, error)| {
                let detail = match error {
                    Some(error) => format!("Failed to start server: {error}"),
                    None => "Failed to start server".to_owned(),
                };
                if stderr.is_empty() {
                    detail
                } else {
                    format!("{detail}\n{stderr}")
                }
            });
        }
        if self.contenders.iter().any(|item| item.finished() && item.error.is_none()) {
            self.spawn_delay = self.spawn_delay.saturating_mul(2).min(timing.max_spawn_delay);
        }
        self.contenders.retain(|item| !item.finished());
        if self.contenders.is_empty() {
            self.failure.clone().map(EnsureError::Spawn)
        } else {
            None
        }
    }

    fn should_recruit(&mut self, registered: bool) -> bool {
        if self.last_spawn.is_none() && registered {
            self.last_spawn = Some(std::time::Instant::now());
        }
        self.failure.is_none()
            && self.contenders.len() < 2
            && self.last_spawn.is_none_or(|last| last.elapsed() >= self.spawn_delay)
    }

    fn add(&mut self, contender: Contender) {
        self.last_spawn = Some(std::time::Instant::now());
        self.contenders.push(contender);
    }

    fn release_all(mut self) {
        for item in self.contenders.drain(..) {
            item.release();
        }
    }
}

/// Ensure-loop failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnsureError {
    /// Wall clock expired.
    #[error("Timed out waiting for the background service to start")]
    Timeout,
    /// The registered service reported a failed boot.
    #[error("Background service failed to start")]
    Failed,
    /// The health protocol is incompatible.
    #[error(
        "Background service uses an incompatible health protocol. Update this client or explicitly restart the service."
    )]
    Incompatible,
    /// A contender could not start.
    #[error("{0}")]
    Spawn(String),
}

/// Options for [`ensure`] and [`stop`].
#[derive(Default)]
pub struct EnsureOptions {
    /// Registration file (default: the XDG fallback).
    pub file: Option<PathBuf>,
    /// Required exact version.
    pub version: Option<String>,
    /// Required build identity (B4-H3): the daemon's `x-opencode-build`
    /// must match exactly; `None` skips the check. A daemon reporting
    /// no marker counts as a mismatch (pre-marker builds get replaced).
    pub build: Option<String>,
    /// Service command (default `opencode serve --service`).
    pub command: Option<Vec<String>>,
    /// Extra environment for the service process.
    pub env: Vec<(String, String)>,
    /// Reason callback fired once before spawning.
    pub on_start: Option<StartCallback>,
    /// Timing overrides (tests).
    pub timing: EnsureTiming,
}

/// `ensure()`: idempotent ensure-running — reuses a healthy compatible
/// server, replaces a version-mismatched one, otherwise spawns contenders
/// (≤ 2 alive) until a server becomes discoverable.
#[allow(clippy::too_many_lines)] // one clause per v2 branch of the same loop
pub async fn ensure(options: &EnsureOptions) -> Result<Endpoint, EnsureError> {
    let timing = options.timing;
    let deadline = std::time::Instant::now() + timing.promise_timeout;
    let mut pool = ContenderPool::new(timing);
    let mut timeouts: Option<(Registration, u32)> = None;
    let mut announced = false;

    let result = loop {
        if std::time::Instant::now() >= deadline {
            break Err(pool.take_failure().map_or(EnsureError::Timeout, EnsureError::Spawn));
        }

        let info = read_registration(options.file.as_deref());
        let mut timed_out = false;
        let registration = match info {
            Some(info) => {
                let probe_result = probe(&info, timing.request_timeout).await;
                timed_out = probe_result.timed_out;
                (info, probe_result)
            }
            None => (Registration::empty(), ProbeResult::default()),
        };
        let registered = registration.0.pid != 0 || !registration.0.url.is_empty();

        if timed_out && registered {
            let count = match &timeouts {
                Some((previous, count)) if registration_same(previous, &registration.0) => {
                    count + 1
                }
                _ => 1,
            };
            timeouts = Some((registration.0.clone(), count));
            if count >= 3 {
                announce_start(options, &mut announced, "missing", None);
                eprintln!("Background service is unresponsive; recovery cannot preserve persistent terminals");
                let file = options.file.clone().unwrap_or_else(registration_fallback);
                pty_handoff_clear(&file).await;
                terminate(&registration.0, options.file.as_deref(), timing).await;
                pool.evict(registration.0.pid, timing);
                pool.recruit_now();
                timeouts = None;
                continue;
            }
        } else {
            timeouts = None;
        }

        if let Some(service) = registration.1.service {
            pool.service_answered(timing);
            let version_matches =
                matches_version(service.version.as_deref(), options.version.as_deref());
            let build_matches = matches_build(service.build.as_deref(), options.build.as_deref());
            let compatible = service.compatible && version_matches && build_matches;
            if !service.compatible && version_matches {
                break Err(EnsureError::Incompatible);
            }
            if compatible && service.state == ProbeState::Ready {
                let file = options.file.clone().unwrap_or_else(registration_fallback);
                pty_handoff_complete(&file, &service.info).await;
                break Ok(service.endpoint);
            }
            if compatible && service.state == ProbeState::Failed {
                break Err(EnsureError::Failed);
            }
            if !compatible {
                // B4-H3: an identical version string with a DIFFERENT
                // (or missing) build marker is the stale-daemon case —
                // the parity pin keeps "2.0.24" across betas, so the
                // stamp is the only signal. The existing stop path
                // terminates the old daemon (PTY handoff included) and
                // the loop spawns a fresh contender below.
                let reason = if version_matches { "build-mismatch" } else { "version-mismatch" };
                announce_start(options, &mut announced, reason, service.version.as_deref());
                if service.state != ProbeState::Ready {
                    eprintln!("Background service is not ready; replacement cannot preserve persistent terminals");
                }
                let () = stop(StopOptions {
                    file: options.file.clone(),
                    pty: if service.state == ProbeState::Ready {
                        PtyStop::Handoff
                    } else {
                        PtyStop::Clear
                    },
                    timing,
                })
                .await;
                pool.evict(service.info.pid, timing);
            }
        } else {
            if let Some(failure) = pool.reap(timing) {
                break Err(failure);
            }
            if pool.should_recruit(registered) {
                announce_start(options, &mut announced, "missing", None);
                let command = options.command.clone().unwrap_or_else(|| {
                    vec!["opencode".to_owned(), "serve".to_owned(), "--service".to_owned()]
                });
                let file = options.file.clone().unwrap_or_else(registration_fallback);
                let mut env = options.env.clone();
                if let Some(extra) = pty_handoff_environment(&file, &options.env).await {
                    env = extra;
                }
                match Contender::spawn(&command, &env) {
                    Ok(contender) => pool.add(contender),
                    Err(error) => {
                        break Err(EnsureError::Spawn(format!("Failed to start server: {error}")));
                    }
                }
            }
        }
        tokio::time::sleep(timing.poll_interval).await;
    };

    pool.release_all();
    result
}

impl Registration {
    /// An unregistered placeholder (`url` empty, pid 0).
    #[must_use]
    pub fn empty() -> Self {
        Self { id: None, version: None, url: String::new(), pid: 0, password: None }
    }

    /// Whether this placeholder means "no registration".
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.url.is_empty() && self.pid == 0 && self.id.is_none()
    }
}

impl ContenderPool {
    fn take_failure(&mut self) -> Option<String> {
        self.failure.take()
    }
}

fn announce_start(
    options: &EnsureOptions,
    announced: &mut bool,
    reason: &str,
    previous_version: Option<&str>,
) {
    if *announced {
        return;
    }
    *announced = true;
    if let Some(on_start) = &options.on_start {
        on_start(reason, previous_version);
    }
}

// ===========================================================================
// Stop (service.ts stop/terminate)
// ===========================================================================

/// How to treat persistent terminals before stopping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyStop {
    /// Publish a handoff ticket.
    Handoff,
    /// Shut terminals down.
    Clear,
}

/// Options for [`stop`].
#[derive(Clone)]
pub struct StopOptions {
    /// Registration file.
    pub file: Option<PathBuf>,
    /// Terminal treatment.
    pub pty: PtyStop,
    /// Timing (stop polls).
    pub timing: EnsureTiming,
}

impl Default for StopOptions {
    fn default() -> Self {
        Self { file: None, pty: PtyStop::Clear, timing: EnsureTiming::default() }
    }
}

/// `stop()`: best-effort PTY handoff, then SIGTERM → poll → SIGKILL → poll,
/// then registration removal when still owned.
pub async fn stop(options: StopOptions) {
    let info = read_registration(options.file.as_deref());
    let file = options.file.clone().unwrap_or_else(registration_fallback);
    // Terminal handoff is best-effort; it must never keep the old service running.
    let handoff = match (&options.pty, &info) {
        (PtyStop::Handoff, Some(info)) => {
            pty_handoff_prepare(&file, info, options.timing.request_timeout).await
        }
        _ => None,
    };
    if handoff.is_none() {
        if let Err(cause) = pty_handoff_clear_quiet(&file).await {
            eprintln!("Failed to prepare persistent terminals for replacement: {cause}");
        }
    }
    if let Some(info) = info {
        terminate(&info, options.file.as_deref(), options.timing).await;
    }
}

/// `terminate()`: re-read (abort when changed), SAFE-signal, escalate, remove.
///
/// Signals go through [`crate::safekill::safe_signal`] with the
/// registration's generation expectation (boot sidecar) — a stale record
/// (machine rebooted / PID recycled / foreign non-opencode target) is
/// DELETED and logged, never signalled (production incident 2026-10).
async fn terminate(info: &Registration, file: Option<&Path>, timing: EnsureTiming) {
    let current = read_registration(file);
    if !current.is_some_and(|current| registration_same(&current, info)) {
        return;
    }
    let registration_path = file.map_or_else(registration_fallback, ToOwned::to_owned);
    let expect = crate::safekill::read_expectation(&registration_path)
        .unwrap_or(crate::safekill::Expectation::Foreign);
    match crate::safekill::safe_signal(info.pid, crate::safekill::Signal::Term, &expect) {
        crate::safekill::SignalOutcome::Delivered => {}
        outcome => {
            eprintln!(
                "Refusing to signal pid {} recorded by an old service registration ({:?}); removing the stale record",
                info.pid, outcome
            );
            let _ = tokio::fs::remove_file(&registration_path).await;
            let _ = tokio::fs::remove_file(crate::safekill::boot_sidecar(&registration_path)).await;
            return;
        }
    }
    if !wait_until_stopped(info.pid, Some(&expect), timing).await {
        if let crate::safekill::SignalOutcome::Delivered =
            crate::safekill::safe_signal(info.pid, crate::safekill::Signal::Kill, &expect)
        {
            if !wait_until_stopped(info.pid, Some(&expect), timing).await {
                eprintln!("Server process {} is still running", info.pid);
                return;
            }
        } else {
            eprintln!(
                "Refusing to escalate to SIGKILL for pid {} (record went stale mid-stop)",
                info.pid
            );
        }
    }
    let latest = read_registration(file);
    if !latest.is_some_and(|latest| registration_same(&latest, info)) {
        return;
    }
    let _ = tokio::fs::remove_file(&registration_path).await;
    let _ = tokio::fs::remove_file(crate::safekill::boot_sidecar(&registration_path)).await;
}

/// Whether `pid` is alive — NO signalling (pure `/proc` read; zombies count
/// as dead; with a generation expectation a recycled start time also reads
/// dead). Replaces the old `kill(pid, 0)` probe.
fn process_alive(pid: u32, expect: Option<&crate::safekill::Expectation>) -> bool {
    crate::safekill::process_alive(pid, expect)
}

async fn wait_until_stopped(
    pid: u32,
    expect: Option<&crate::safekill::Expectation>,
    timing: EnsureTiming,
) -> bool {
    let mut attempts = 0;
    loop {
        if !process_alive(pid, expect) {
            return true;
        }
        if attempts >= timing.stop_poll_attempts {
            return false;
        }
        attempts += 1;
        tokio::time::sleep(timing.stop_poll_interval).await;
    }
}

// ===========================================================================
// PTY handoff sidecar (pty-handoff.ts)
// ===========================================================================

/// `{source: {id?, pid, url}, handoff, expiresAt}` sidecar shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyHandoffRecord {
    /// The registration the handoff came from.
    pub source: PtyHandoffSource,
    /// The handoff ticket payload (`PersistentPty.Handoff | null`).
    #[serde(default)]
    pub handoff: Option<serde_json::Value>,
    /// Sidecar expiry (unix ms).
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyHandoffSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub pid: u32,
    pub url: String,
}

fn sidecar_path(file: &Path) -> PathBuf {
    let mut path = file.as_os_str().to_os_string();
    path.push(".pty-handoff");
    PathBuf::from(path)
}

fn now_ms() -> u64 {
    u64::try_from(crate::auth::unix_now_ms()).unwrap_or(0)
}

async fn read_sidecar(file: &Path) -> Option<PtyHandoffRecord> {
    let text = tokio::fs::read_to_string(sidecar_path(file)).await.ok()?;
    serde_json::from_str(&text).ok()
}

fn handoff_source_same(source: &PtyHandoffSource, info: &Registration) -> bool {
    source.id == info.id && source.pid == info.pid && source.url == info.url
}

/// `prepare()`: publish a handoff ticket before the owner stops. Best-effort
/// (`Ok(Some(()))` = sidecar published; `Ok(None)` = concurrent write or
/// nothing to do; `Err` = fallback shutdown also failed to publish).
async fn pty_handoff_prepare(file: &Path, info: &Registration, timeout: Duration) -> Option<()> {
    let existing = read_sidecar(file).await;
    if existing.as_ref().is_some_and(|record| {
        record.expires_at > now_ms() && handoff_source_same(&record.source, info)
    }) {
        return Some(());
    }
    let url = info.url.trim_end_matches('/').to_owned();
    let mut headers = Vec::new();
    if let Some(password) = &info.password {
        use base64::Engine as _;
        headers.push((
            "authorization".to_owned(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"))
            ),
        ));
    }
    // POST /api/experimental/persistent-pty/handoff
    let handoff = match http_request(
        "POST",
        &format!("{url}/api/experimental/persistent-pty/handoff"),
        &headers,
        b"{}",
        timeout,
    )
    .await
    {
        Ok(response) if (200..300).contains(&response.status) => parse_handoff_body(&response.body),
        _ => {
            // Another caller may already have prepared and stopped this server.
            let concurrent = read_sidecar(file).await;
            if concurrent.as_ref().is_some_and(|record| {
                record.expires_at > now_ms() && handoff_source_same(&record.source, info)
            }) {
                return Some(());
            }
            eprintln!(
                "Background service cannot hand off persistent terminals; shutting them down before replacement"
            );
            let _ = http_request(
                "POST",
                &format!("{url}/api/experimental/persistent-pty/shutdown"),
                &headers,
                b"{}",
                timeout,
            )
            .await;
            None
        }
    };
    publish_sidecar(file, info, handoff);
    Some(())
}

fn parse_handoff_body(body: &[u8]) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let handoff = value.get("handoff")?;
    if handoff.is_null() {
        return None;
    }
    if !is_handoff(handoff)
        || handoff.get("expiresAt").and_then(serde_json::Value::as_u64).unwrap_or(0) <= now_ms()
    {
        return None;
    }
    Some(handoff.clone())
}

fn is_handoff(value: &serde_json::Value) -> bool {
    value.get("directory").is_some_and(serde_json::Value::is_string)
        && value.get("instanceID").is_some_and(serde_json::Value::is_string)
        && value.get("ticket").is_some_and(serde_json::Value::is_string)
        && value.get("expiresAt").and_then(serde_json::Value::as_f64).is_some_and(f64::is_finite)
}

fn publish_sidecar(file: &Path, info: &Registration, handoff: Option<serde_json::Value>) {
    let expires_at = handoff
        .as_ref()
        .and_then(|value| value.get("expiresAt"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| now_ms() + 30_000);
    let record = PtyHandoffRecord {
        source: PtyHandoffSource { id: info.id.clone(), pid: info.pid, url: info.url.clone() },
        handoff,
        expires_at,
    };
    let path = sidecar_path(file);
    let temp = sidecar_path(&file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4())));
    if write_private(&temp, serde_json::to_string(&record).expect("sidecar serializes").as_bytes())
        .and_then(|()| std::fs::rename(&temp, &path))
        .is_err()
    {
        eprintln!("Failed to publish persistent terminal handoff");
    }
}

/// `environment()`: the env for a replacement contender —
/// `OPENCODE_PTY_HANDOFF=<json>` when a fresh sidecar matches the current
/// owner (or the owner is gone).
async fn pty_handoff_environment(
    file: &Path,
    env: &[(String, String)],
) -> Option<Vec<(String, String)>> {
    let record = read_sidecar(file).await?;
    if record.expires_at <= now_ms() {
        return None;
    }
    let current = read_registration(Some(file));
    if current.is_some_and(|current| !handoff_source_same(&record.source, &current)) {
        return None;
    }
    let handoff = record.handoff?;
    let mut merged = env.to_vec();
    merged.retain(|(name, _)| name != "OPENCODE_PTY_HANDOFF");
    merged.push(("OPENCODE_PTY_HANDOFF".to_owned(), handoff.to_string()));
    Some(merged)
}

/// `complete()`: clears the sidecar when its source is not the new owner.
pub async fn pty_handoff_complete(file: &Path, info: &Registration) {
    if let Some(record) = read_sidecar(file).await {
        if !handoff_source_same(&record.source, info) {
            pty_handoff_clear(file).await;
        }
    }
}

/// `clear()`: removes the sidecar.
pub async fn pty_handoff_clear(file: &Path) {
    let _ = tokio::fs::remove_file(sidecar_path(file)).await;
}

async fn pty_handoff_clear_quiet(file: &Path) -> Result<(), String> {
    match tokio::fs::remove_file(sidecar_path(file)).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Decodes an inherited `OPENCODE_PTY_HANDOFF` env value
/// (`Ignoring invalid PTY Restart handoff` on garbage).
#[must_use]
pub fn decode_inherited_handoff(raw: Option<&str>) -> Option<serde_json::Value> {
    let raw = raw?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if !is_handoff(&value)
        || value.get("expiresAt").and_then(serde_json::Value::as_u64).unwrap_or(0) <= now_ms()
    {
        eprintln!("Ignoring invalid PTY Restart handoff; persistent terminals will start fresh");
        return None;
    }
    Some(value)
}

// ===========================================================================
// serve() (process.ts listen/start)
// ===========================================================================

/// `waitForStdinClose()` (server-process.ts:199): the stdio ownership lease.
/// Resolves when stdin reaches EOF or is closed — the OS closes the pipe even
/// when the parent is killed before finalizers run, which is the whole point
/// of the lease. A blocking read runs on the dedicated runtime thread.
pub async fn wait_stdin_eof() {
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut chunk = [0u8; 4096];
    loop {
        match stdin.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// Options for [`serve`].
pub struct ServeOptions {
    /// Bind hostname (default 127.0.0.1).
    pub hostname: IpAddr,
    /// Bind port; `None` scans upward from 4096 on EADDRINUSE.
    pub port: Option<u16>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self { hostname: IpAddr::from([127, 0, 0, 1]), port: None }
    }
}

/// The exact EADDRINUSE failure text (`server-process.ts`).
#[must_use]
pub fn port_in_use_message(port: u16, hostname: &IpAddr) -> String {
    format!(
        "Managed service port {port} on {hostname} is already in use by another process. \
         Configure another port with `opencode service set port {port}` and start the service again."
    )
}

/// Whether an io error is EADDRINUSE.
#[must_use]
pub fn address_in_use(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AddrInUse
}

/// `listen()`: binds the exact port, or scans upward from 4096 when no port
/// was requested (v2 `process.ts:139-146`).
pub fn bind(options: &ServeOptions) -> io::Result<tokio::net::TcpListener> {
    if let Some(port) = options.port {
        tokio_from_std(SocketAddr::new(options.hostname, port))
    } else {
        let mut port = 4096u16;
        loop {
            let attempt = std::net::TcpListener::bind(SocketAddr::new(options.hostname, port));
            match attempt {
                Ok(listener) => {
                    listener.set_nonblocking(true)?;
                    return tokio::net::TcpListener::from_std(listener);
                }
                Err(error) if address_in_use(&error) && port < 65535 => port += 1,
                Err(error) => return Err(error),
            }
        }
    }
}

fn tokio_from_std(addr: SocketAddr) -> io::Result<tokio::net::TcpListener> {
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(listener)
}

/// Outcome of [`bind_or_incumbent`].
pub enum BindOutcome {
    /// The port was bound; serve on this listener.
    Bound(tokio::net::TcpListener),
    /// A compatible incumbent answered on the exact URL — the caller should
    /// stand down (v2 exits 0).
    IncumbentServing(ProbedService),
}

/// The CLI bind path (`server-process.ts`): on EADDRINUSE, poll the
/// incumbent every 100ms up to 15s, retrying the bind between probes.
/// A freshly spawned daemon stands down ONLY for an incumbent of the
/// required version AND build (B4-H3) — standing down for a stale
/// incumbent of the same pinned version would wedge the replacement.
pub async fn bind_or_incumbent(
    options: &ServeOptions,
    file: Option<&Path>,
    required_version: Option<&str>,
    required_build: Option<&str>,
) -> Result<BindOutcome, io::Error> {
    let url = format!("http://{}", SocketAddr::new(options.hostname, options.port.unwrap_or(0)));
    let initial = bind(options);
    let _cause = match initial {
        Ok(listener) => return Ok(BindOutcome::Bound(listener)),
        Err(error) if !address_in_use(&error) => return Err(error),
        Err(error) => error,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if let Some(found) =
            incumbent(file, &url, required_version, required_build, Duration::from_secs(2)).await
        {
            return Ok(BindOutcome::IncumbentServing(found));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        if std::time::Instant::now() >= deadline {
            break;
        }
        if let Ok(listener) = bind(options) {
            return Ok(BindOutcome::Bound(listener));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        port_in_use_message(options.port.unwrap_or(0), &options.hostname),
    ))
}

/// On-listen hook: runs after the listener is bound but BEFORE readiness
/// flips to ready (the Starting window is observable here).
pub type OnListen =
    Arc<dyn Fn(SocketAddr) -> futures::future::BoxFuture<'static, ()> + Send + Sync>;

/// A running server.
pub struct ServeHandle {
    /// The bound address.
    pub addr: SocketAddr,
    /// Shutdown latch (also observable by the registration watcher).
    pub shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<io::Result<()>>,
    /// The shared app state (readiness/engine observability; crate tests
    /// assert boot ordering through it).
    pub state: Arc<AppState>,
}

impl ServeHandle {
    /// Requests shutdown: readiness flips to Stopping, then the accept loop
    /// drains gracefully.
    pub fn begin_shutdown(&self) {
        crate::readiness::begin_stopping(&self.state.readiness);
        let _ = self.shutdown.send(true);
    }

    /// Requests shutdown and waits for the serve loop to finish.
    pub async fn shutdown_and_join(self) -> io::Result<()> {
        self.begin_shutdown();
        match self.task.await {
            Ok(result) => result,
            Err(error) => Err(io::Error::new(io::ErrorKind::BrokenPipe, error.to_string())),
        }
    }

    /// The bound address formatted as a URL.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

/// Binds + serves the router. Readiness flips `Ready` once listening (after
/// the optional [`OnListen`] hook ran during the `Starting` window); the
/// shutdown latch flips it to `Stopping` and drains the accept loop.
pub async fn serve(
    options: &ServeOptions,
    state: Arc<AppState>,
    router: axum::routing::Router,
    on_listen: Option<OnListen>,
) -> io::Result<ServeHandle> {
    let listener = bind(options)?;
    let addr = listener.local_addr()?;
    *state.local_addr.write() = Some(addr);

    // The shutdown latch lives on the shared state (gate-6 F2) so stream
    // bodies (SSE, PTY WS) can select on it; the ServeHandle keeps a clone
    // as the graceful-drain trigger.
    let shutdown = state.shutdown.clone();
    let mut graceful_rx = shutdown.subscribe();
    let graceful = async move {
        let _ = graceful_rx.wait_for(|flag| *flag).await;
    };
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(graceful)
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error.to_string()))
    });

    if let Some(on_listen) = on_listen {
        on_listen(addr).await;
    }
    crate::readiness::ready(&state.readiness);

    Ok(ServeHandle { addr, shutdown, task, state })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::net::TcpListener;

    fn registration() -> Registration {
        Registration {
            id: Some("srv_1".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:49375".to_owned(),
            pid: 4242,
            password: Some("secret".to_owned()),
        }
    }

    #[test]
    fn server_registration_filenames_per_channel() {
        assert_eq!(registration_filename("latest"), "service.json");
        assert_eq!(registration_filename("dev"), "service.json");
        assert_eq!(registration_filename("beta"), "service.json");
        assert_eq!(registration_filename("next"), "service.json");
        assert_eq!(registration_filename("local"), "service-local.json");
        assert_eq!(registration_filename("we ird/ch"), "service-we-ird-ch.json");
    }

    #[test]
    fn server_registration_atomic_write_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = registration();
        write_registration(&file, &info).expect("write");
        assert_eq!(read_registration(Some(&file)), Some(info));

        // Corrupt file reads as None.
        std::fs::write(&file, "{not json").expect("corrupt");
        assert_eq!(read_registration(Some(&file)), None);
        // Missing file reads as None.
        assert_eq!(read_registration(Some(&tmp.path().join("gone.json"))), None);
    }

    #[test]
    fn server_registration_write_is_private() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        write_registration(&file, &registration()).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn server_registration_owns_and_same() {
        let info = registration();
        assert!(registration_owns(&info.clone(), &info));
        let mut replaced = info.clone();
        replaced.pid = 99;
        assert!(!registration_owns(&replaced, &info));
        // same() ignores the password.
        let mut rotated = info.clone();
        rotated.password = Some("rotated".to_owned());
        assert!(!registration_owns(&rotated, &info));
        assert!(registration_same(&rotated, &info));
        let mut different = info.clone();
        different.url = "http://elsewhere".to_owned();
        assert!(!registration_same(&different, &info));
    }

    #[test]
    fn server_lifecycle_decode_server_info() {
        assert!(decode_server_info(br#"{"version":"2.0.24","pid":42}"#).is_some());
        assert!(decode_server_info(br#"{"pid":42}"#).is_none());
        assert!(decode_server_info(br#"{"version":1,"pid":42}"#).is_none());
        assert!(decode_server_info(br#"{"version":"2","pid":-1}"#).is_none());
        assert!(decode_server_info(br#"{"version":"2","pid":"x"}"#).is_none());
    }

    #[test]
    fn server_lifecycle_url_parsing() {
        let (host, port, path) = parse_url("http://127.0.0.1:8080/api/info").expect("parse");
        assert_eq!((host.as_str(), port, path.as_str()), ("127.0.0.1", 8080, "/api/info"));
        let (host, port, path) = parse_url("http://localhost/x").expect("parse");
        assert_eq!((host.as_str(), port, path.as_str()), ("localhost", 80, "/x"));
        let (host, port, _) = parse_url("http://[::1]:9000/y").expect("parse");
        assert_eq!((host.as_str(), port), ("::1", 9000));
        assert!(parse_url("https://example.com").is_err());
    }

    #[test]
    fn server_lifecycle_response_parsing() {
        let response =
            parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}").expect("parse");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
        assert_eq!(response.build, None, "no marker header present");
        let response = parse_response(
            b"HTTP/1.1 500\r\ntransfer-encoding: chunked\r\n\r\n2\r\nok\r\n0\r\n\r\n",
        )
        .expect("parse");
        assert_eq!(response.status, 500);
        assert_eq!(response.body, b"ok");
        // B4-H3: the build marker rides the x-opencode-build header
        // (case-insensitive), whatever the status.
        let response = parse_response(
            b"HTTP/1.1 200 OK\r\nX-OpenCode-Build: v2.0.24.beta.4@a1b2c3d\r\ncontent-length: 2\r\n\r\n{}",
        )
        .expect("parse");
        assert_eq!(response.build.as_deref(), Some("v2.0.24.beta.4@a1b2c3d"));
        let response = parse_response(
            b"HTTP/1.1 503 Service Unavailable\r\nx-opencode-build: dev@deadbee\r\ncontent-length: 2\r\n\r\n{}",
        )
        .expect("parse");
        assert_eq!(response.build.as_deref(), Some("dev@deadbee"));
    }

    #[test]
    fn server_lifecycle_matches_version() {
        assert!(matches_version(Some("1"), None));
        assert!(matches_version(None, None));
        assert!(matches_version(Some("1"), Some("1")));
        assert!(!matches_version(None, Some("1")));
        assert!(!matches_version(Some("2"), Some("1")));
    }

    /// B4-H3: the build marker comparison — a required stamp demands an
    /// exact match; a MISSING marker (v2 daemons, pre-marker builds of
    /// the port) counts as stale so the upgrade flow replaces the very
    /// daemon that predates the marker.
    #[test]
    fn server_lifecycle_matches_build() {
        assert!(matches_build(Some("a@1"), None), "no requirement accepts anything");
        assert!(matches_build(None, None));
        assert!(matches_build(Some("a@1"), Some("a@1")));
        assert!(!matches_build(Some("b@2"), Some("a@1")), "different build is stale");
        assert!(!matches_build(None, Some("a@1")), "missing marker is stale");
        assert!(BUILD_STAMP.contains('@'), "the stamp is <tag>@<hash>: {BUILD_STAMP}");
    }

    /// A minimal `/api/info` responder for the ensure tests: reports the
    /// given version + pid, carries the build marker in the response
    /// header (B4-H3), and 404s everything else.
    async fn stub_info_server(version: String, pid: u32, build: Option<String>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let version = version.clone();
                let build = build.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buffer = [0u8; 2048];
                    let _ = socket.read(&mut buffer).await;
                    let head = String::from_utf8_lossy(&buffer);
                    let (status, body) = if head.starts_with("GET /api/info") {
                        ("200 OK", format!("{{\"version\":\"{version}\",\"pid\":{pid}}}"))
                    } else {
                        ("404 Not Found", "{}".to_owned())
                    };
                    let marker = build
                        .map_or(String::new(), |stamp| format!("x-opencode-build: {stamp}\r\n"));
                    let response = format!(
                        "HTTP/1.1 {status}\r\n{marker}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        port
    }

    /// A sacrificial stand-in process for the daemon pid (reaped so it
    /// never zombifies), plus its pid.
    fn sleeper_pid() -> u32 {
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let pid = sleeper.id();
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });
        pid
    }

    /// Timing that makes the ensure loop test-fast.
    fn fast_timing() -> EnsureTiming {
        EnsureTiming {
            poll_interval: Duration::from_millis(10),
            request_timeout: Duration::from_secs(2),
            spawn_delay: Duration::from_millis(50),
            max_spawn_delay: Duration::from_millis(200),
            promise_timeout: Duration::from_secs(30),
            stop_poll_interval: Duration::from_millis(10),
            stop_poll_attempts: 200,
        }
    }

    /// B4-H3 integration: a registered daemon of the SAME pinned version
    /// but an OLD build marker is REPLACED — ensure stops it through the
    /// regular lifecycle stop path (SIGTERM via the boot sidecar) and
    /// respawns through the injected service command, whose stand-in
    /// installs a fresh same-build registration.
    #[tokio::test]
    async fn server_lifecycle_ensure_replaces_stale_build_daemon() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");

        // The stale daemon: right version, old marker, alive.
        let stale_pid = sleeper_pid();
        let stale_port = stub_info_server(
            env!("CARGO_PKG_VERSION").to_owned(),
            stale_pid,
            Some("old@0000000".to_owned()),
        )
        .await;
        let stale = Registration {
            id: Some("srv_stale".to_owned()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            url: format!("http://127.0.0.1:{stale_port}"),
            pid: stale_pid,
            password: None,
        };
        write_registration(&file, &stale).expect("register stale");

        // The replacement the spawned "daemon" installs: same version,
        // the CURRENT build stamp.
        let fresh_pid = sleeper_pid();
        let fresh_port = stub_info_server(
            env!("CARGO_PKG_VERSION").to_owned(),
            fresh_pid,
            Some(BUILD_STAMP.to_owned()),
        )
        .await;
        let fresh_json = serde_json::json!({
            "id": "srv_fresh",
            "version": env!("CARGO_PKG_VERSION"),
            "url": format!("http://127.0.0.1:{fresh_port}"),
            "pid": fresh_pid,
        })
        .to_string();
        let spawn_marker = tmp.path().join("spawned");
        let script = format!(
            "printf spawned > {} && sleep 0.2 && printf '%s' '{}' > {}",
            spawn_marker.display(),
            fresh_json.replace('\'', "'\\''"),
            file.display(),
        );

        let reasons = std::sync::Arc::new(std::sync::Mutex::new(Vec::new())).clone();
        let seen = std::sync::Arc::clone(&reasons);
        let options = EnsureOptions {
            file: Some(file.clone()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            build: Some(BUILD_STAMP.to_owned()),
            command: Some(vec!["bash".to_owned(), "-c".to_owned(), script]),
            env: Vec::new(),
            on_start: Some(Box::new(move |reason, previous| {
                seen.lock().unwrap().push((reason.to_owned(), previous.map(str::to_owned)));
            })),
            timing: fast_timing(),
        };

        let endpoint = tokio::time::timeout(Duration::from_secs(20), ensure(&options))
            .await
            .expect("ensure resolves")
            .expect("fresh daemon serves");
        assert_eq!(endpoint.url, format!("http://127.0.0.1:{fresh_port}"));
        assert_eq!(
            reasons.lock().unwrap().as_slice(),
            [("build-mismatch".to_owned(), Some(env!("CARGO_PKG_VERSION").to_owned()))],
            "the replacement announces the build mismatch"
        );
        assert!(!process_alive(stale_pid, None), "the stale daemon was stopped");
        assert!(spawn_marker.exists(), "the replacement spawned through the command");
    }

    /// B4-H3: a daemon reporting the CURRENT build marker is reused —
    /// no stop, no respawn, the `on_start` callback never fires.
    #[tokio::test]
    async fn server_lifecycle_ensure_reuses_same_build_daemon() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");

        let pid = sleeper_pid();
        let port = stub_info_server(
            env!("CARGO_PKG_VERSION").to_owned(),
            pid,
            Some(BUILD_STAMP.to_owned()),
        )
        .await;
        write_registration(
            &file,
            &Registration {
                id: Some("srv_same".to_owned()),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                url: format!("http://127.0.0.1:{port}"),
                pid,
                password: None,
            },
        )
        .expect("register");

        let spawn_marker = tmp.path().join("spawned");
        let script = format!("printf spawned > {}", spawn_marker.display());
        let fired = std::sync::Arc::new(std::sync::Mutex::new(false)).clone();
        let seen = std::sync::Arc::clone(&fired);
        let options = EnsureOptions {
            file: Some(file.clone()),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            build: Some(BUILD_STAMP.to_owned()),
            command: Some(vec!["bash".to_owned(), "-c".to_owned(), script]),
            env: Vec::new(),
            on_start: Some(Box::new(move |_, _| {
                *seen.lock().unwrap() = true;
            })),
            timing: fast_timing(),
        };

        let endpoint = tokio::time::timeout(Duration::from_secs(10), ensure(&options))
            .await
            .expect("ensure resolves")
            .expect("same-build daemon reused");
        assert_eq!(endpoint.url, format!("http://127.0.0.1:{port}"));
        assert!(!*fired.lock().unwrap(), "no replacement announced");
        assert!(!spawn_marker.exists(), "nothing spawned");
        assert!(process_alive(pid, None), "the daemon was left running");
        let _ = Command::new("kill").arg(pid.to_string()).output();
    }

    #[test]
    fn server_lifecycle_port_in_use_message_exact() {
        assert_eq!(
            port_in_use_message(49374, &IpAddr::from([127, 0, 0, 1])),
            "Managed service port 49374 on 127.0.0.1 is already in use by another process. \
             Configure another port with `opencode service set port 49374` and start the service again."
        );
    }

    #[tokio::test]
    async fn server_lifecycle_sidecar_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = registration();
        let handoff = json!({
            "directory": "/tmp",
            "instanceID": "term_1",
            "ticket": "uuid",
            "expiresAt": now_ms() + 60_000
        });
        publish_sidecar(&file, &info, Some(handoff.clone()));
        let record = read_sidecar(&file).await.expect("sidecar");
        assert_eq!(record.source.pid, info.pid);
        assert_eq!(record.handoff, Some(handoff));
        assert!(record.expires_at > now_ms());

        // environment(): fresh + same owner → OPENCODE_PTY_HANDOFF.
        write_registration(&file, &info).expect("reg");
        let env = pty_handoff_environment(&file, &[]).await.expect("env");
        assert!(env
            .iter()
            .any(|(name, value)| name == "OPENCODE_PTY_HANDOFF" && !value.is_empty()));

        // Stale sidecar → no env.
        let stale = PtyHandoffRecord {
            source: record.source.clone(),
            handoff: record.handoff.clone(),
            expires_at: now_ms() - 1,
        };
        write_private(&sidecar_path(&file), b"{\"stale\"").expect("truncate");
        std::fs::write(sidecar_path(&file), serde_json::to_string(&stale).expect("json"))
            .expect("write");
        assert!(pty_handoff_environment(&file, &[]).await.is_none());

        // complete(): different owner clears.
        let mut other = info.clone();
        other.id = Some("srv_2".to_owned());
        publish_sidecar(&file, &info, None);
        pty_handoff_complete(&file, &other).await;
        assert!(read_sidecar(&file).await.is_none());
    }

    #[test]
    fn server_lifecycle_decode_inherited_handoff() {
        assert!(decode_inherited_handoff(None).is_none());
        assert!(decode_inherited_handoff(Some("garbage")).is_none());
        let fresh = json!({
            "directory": "/tmp", "instanceID": "t", "ticket": "u",
            "expiresAt": now_ms() + 60_000
        });
        assert!(decode_inherited_handoff(Some(&fresh.to_string())).is_some());
        let expired = json!({
            "directory": "/tmp", "instanceID": "t", "ticket": "u",
            "expiresAt": now_ms() - 1
        });
        assert!(decode_inherited_handoff(Some(&expired.to_string())).is_none());
        let missing = json!({ "directory": "/tmp" });
        assert!(decode_inherited_handoff(Some(&missing.to_string())).is_none());
    }

    #[tokio::test]
    async fn server_lifecycle_bind_scans_upward_when_free_port_requested() {
        // Occupy a port, then request a scan from 4096: the scan must land
        // somewhere else (and never on the occupied port).
        let probe_listener = TcpListener::bind("127.0.0.1:4096").ok();
        let listener = bind(&ServeOptions::default()).expect("bind");
        let addr = listener.local_addr().expect("addr");
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        if let Some(occupied) = probe_listener {
            assert_ne!(occupied.local_addr().expect("addr").port(), addr.port());
        }
    }

    #[tokio::test]
    async fn server_lifecycle_registration_watch_fires_on_corruption() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_1".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:49375".to_owned(),
            pid: std::process::id(),
            password: Some("secret".to_owned()),
        };
        write_registration(&file, &info).expect("write");
        let (shutdown, mut rx) = watch::channel(false);
        let cleanup = register(RegisterOptions {
            address: "127.0.0.1:49375".parse().unwrap(),
            password: "secret".to_owned(),
            id: "srv_1".to_owned(),
            file: file.clone(),
            version: "2.0.24".to_owned(),
            shutdown,
            watch_interval: Duration::from_millis(20),
        })
        .expect("register");

        // Corrupt the file → watcher triggers shutdown.
        std::fs::write(&file, "corrupt").expect("corrupt");
        tokio::time::timeout(Duration::from_secs(2), rx.wait_for(|flag| *flag))
            .await
            .expect("watcher fires")
            .expect("channel alive");

        // Restore + cleanup removes when owned.
        write_registration(&file, &info).expect("restore");
        cleanup.run().await;
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn server_lifecycle_registration_watch_fires_on_replacement() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_1".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:49375".to_owned(),
            pid: std::process::id(),
            password: Some("secret".to_owned()),
        };
        write_registration(&file, &info).expect("write");
        let (shutdown, mut rx) = watch::channel(false);
        let _cleanup = register(RegisterOptions {
            address: "127.0.0.1:49375".parse().unwrap(),
            password: "secret".to_owned(),
            id: "srv_1".to_owned(),
            file: file.clone(),
            version: "2.0.24".to_owned(),
            shutdown,
            watch_interval: Duration::from_millis(20),
        })
        .expect("register");

        let mut replacement = info;
        replacement.id = Some("srv_other".to_owned());
        write_registration(&file, &replacement).expect("replace");
        tokio::time::timeout(Duration::from_secs(2), rx.wait_for(|flag| *flag))
            .await
            .expect("watcher fires")
            .expect("channel alive");
    }

    #[tokio::test]
    async fn server_lifecycle_stop_terminates_process_and_removes_registration() {
        // A sacrificial process stands in for the daemon; a reaper thread
        // keeps it from zombifying (Node auto-reaps; Rust does not).
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let pid = sleeper.id();
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_1".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:1".to_owned(),
            pid,
            password: Some("secret".to_owned()),
        };
        write_registration(&file, &info).expect("write");

        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming::default(),
        })
        .await;

        assert!(!process_alive(pid, None), "process terminated");
        assert!(!file.exists(), "registration removed");
    }

    /// Incident regression (2026-10): a registration from a PREVIOUS boot
    /// must never be signalled — the PID may now belong to a system
    /// service. The record is removed instead.
    #[tokio::test]
    async fn server_lifecycle_stop_refuses_stale_boot_record() {
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let pid = sleeper.id();
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_oldboot".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:1".to_owned(),
            pid,
            password: None,
        };
        write_registration(&file, &info).expect("write");
        // Corrupt the generation sidecar: wrong boot id (as if the machine
        // had rebooted since the record was written).
        let record = crate::safekill::BootRecord {
            boot_id: "ffffffff-ffff-ffff-ffff-ffffffffffff".to_owned(),
            start_time: crate::safekill::proc_start_time(pid).unwrap_or(0),
        };
        std::fs::write(
            crate::safekill::boot_sidecar(&file),
            serde_json::to_string(&record).unwrap(),
        )
        .expect("sidecar");

        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming::default(),
        })
        .await;

        assert!(
            process_alive(pid, None),
            "stale-boot record must NOT be signalled — target could be an innocent process"
        );
        assert!(!file.exists(), "stale record removed");
        let _ = sleeper.kill();
        let _ = sleeper.wait();
    }

    /// Incident regression: same-boot record whose PID was recycled (start
    /// time no longer matches) must not be signalled.
    #[tokio::test]
    async fn server_lifecycle_stop_refuses_recycled_pid_record() {
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let pid = sleeper.id();
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_recycled".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:1".to_owned(),
            pid,
            password: None,
        };
        write_registration(&file, &info).expect("write");
        let record = crate::safekill::BootRecord {
            boot_id: crate::safekill::boot_id().unwrap_or_default(),
            start_time: crate::safekill::proc_start_time(pid).unwrap_or(0).wrapping_add(7),
        };
        std::fs::write(
            crate::safekill::boot_sidecar(&file),
            serde_json::to_string(&record).unwrap(),
        )
        .expect("sidecar");

        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming::default(),
        })
        .await;

        assert!(process_alive(pid, None), "recycled-pid record must NOT be signalled");
        assert!(!file.exists(), "stale record removed");
        let _ = sleeper.kill();
        let _ = sleeper.wait();
    }

    /// Incident regression: a foreign record (no sidecar — e.g. written by
    /// the TypeScript server) pointing at a live NON-opencode process must
    /// not be signalled (cmdline identity check).
    #[tokio::test]
    async fn server_lifecycle_stop_refuses_foreign_non_opencode_target() {
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let pid = sleeper.id();
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_foreign".to_owned()),
            version: Some("2.0.24".to_owned()),
            url: "http://127.0.0.1:1".to_owned(),
            pid,
            password: None,
        };
        write_registration(&file, &info).expect("write");
        // Strip the sidecar: the record is now indistinguishable from one
        // written by a foreign (non-Rust) writer.
        std::fs::remove_file(crate::safekill::boot_sidecar(&file)).expect("strip sidecar");

        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming::default(),
        })
        .await;

        assert!(
            process_alive(pid, None),
            "foreign record pointing at a non-opencode process must NOT be signalled"
        );
        assert!(!file.exists(), "refused record removed");
        let _ = sleeper.kill();
        let _ = sleeper.wait();
    }

    #[tokio::test]
    async fn server_lifecycle_stop_without_registration_is_noop() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming::default(),
        })
        .await;
        assert!(!file.exists());
        // A corrupt registration is treated like none: no panic, no signals.
        std::fs::write(&file, "corrupt").expect("corrupt");
        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Handoff,
            timing: EnsureTiming::default(),
        })
        .await;
        assert!(file.exists(), "corrupt file untouched");
    }

    #[tokio::test]
    async fn server_lifecycle_ensure_reuses_healthy_registration() {
        // Boot a real app-shell server on an ephemeral port and register it.
        let state = crate::test_support::booted_state("secret").await;
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let addr = *state.state.local_addr.read();
        let addr = addr.expect("bound");
        let info = Registration {
            id: Some("srv_boot".to_owned()),
            version: Some("test-version".to_owned()),
            url: format!("http://{addr}"),
            pid: std::process::id(),
            password: Some("secret".to_owned()),
        };
        write_registration(&file, &info).expect("write");

        let endpoint = ensure(&EnsureOptions {
            file: Some(file.clone()),
            version: Some("test-version".to_owned()),
            timing: EnsureTiming {
                poll_interval: Duration::from_millis(10),
                ..EnsureTiming::default()
            },
            ..EnsureOptions::default()
        })
        .await
        .expect("ensure");
        assert_eq!(endpoint.url, info.url);
        assert_eq!(endpoint.auth.expect("auth").1, "secret");

        state.handle.begin_shutdown();
        let _ = state.handle.shutdown_and_join().await;
    }

    #[tokio::test]
    async fn server_lifecycle_ensure_fails_when_service_failed() {
        let state = crate::test_support::boot("secret", |state| {
            crate::readiness::fail(&state.readiness);
            Box::pin(async {})
        })
        .await;
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let addr = state.state.local_addr.read().expect("bound");
        let info = Registration {
            id: Some("srv_fail".to_owned()),
            version: Some("test-version".to_owned()),
            url: format!("http://{addr}"),
            pid: std::process::id(),
            password: Some("secret".to_owned()),
        };
        write_registration(&file, &info).expect("write");

        let error = ensure(&EnsureOptions {
            file: Some(file),
            version: Some("test-version".to_owned()),
            timing: EnsureTiming {
                poll_interval: Duration::from_millis(10),
                ..EnsureTiming::default()
            },
            ..EnsureOptions::default()
        })
        .await
        .expect_err("failed state");
        assert_eq!(error, EnsureError::Failed);

        state.handle.begin_shutdown();
        let _ = state.handle.shutdown_and_join().await;
    }

    #[tokio::test]
    async fn server_lifecycle_ensure_version_mismatch_stops_owner() {
        // A fake incumbent: real HTTP /api/info that reports the sleeper's
        // pid (the registration identity), with the outdated version.
        let mut sleeper = Command::new("sleep").arg("300").spawn().expect("sleep");
        let sleeper_pid = sleeper.id();
        std::thread::spawn(move || {
            let _ = sleeper.wait();
        });
        let reported_pid = sleeper_pid;
        let app = axum::routing::Router::new().fallback(move || {
            let reported_pid = reported_pid;
            async move {
                axum::Json(serde_json::json!({ "version": "test-version", "pid": reported_pid, "urls": [], "paths": {} }))
            }
        });
        let std_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = std_listener.local_addr().expect("addr");
        std_listener.set_nonblocking(true).expect("nonblocking");
        let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
        let incumbent = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let info = Registration {
            id: Some("srv_old".to_owned()),
            version: Some("test-version".to_owned()),
            url: format!("http://{addr}"),
            pid: sleeper_pid,
            password: None,
        };
        write_registration(&file, &info).expect("write");

        let error = ensure(&EnsureOptions {
            file: Some(file.clone()),
            version: Some("newer-version".to_owned()),
            command: Some(vec!["/bin/false".to_owned()]),
            timing: EnsureTiming {
                poll_interval: Duration::from_millis(10),
                stop_poll_interval: Duration::from_millis(5),
                promise_timeout: Duration::from_secs(5),
                ..EnsureTiming::default()
            },
            ..EnsureOptions::default()
        })
        .await;
        // The mismatched owner is stopped, the registration evicted, and the
        // recruit (false) reports its startup failure.
        assert!(error.is_err(), "ensure must not succeed: {error:?}");

        assert!(!process_alive(sleeper_pid, None), "mismatched owner stopped");
        assert!(!file.exists(), "registration evicted");
        incumbent.abort();
    }

    #[tokio::test]
    async fn server_lifecycle_ensure_reports_spawn_failure() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let error = ensure(&EnsureOptions {
            file: Some(file),
            command: Some(vec!["/bin/false".to_owned()]),
            timing: EnsureTiming {
                poll_interval: Duration::from_millis(5),
                spawn_delay: Duration::from_millis(5),
                promise_timeout: Duration::from_millis(500),
                ..EnsureTiming::default()
            },
            ..EnsureOptions::default()
        })
        .await
        .expect_err("spawn fails");
        match error {
            EnsureError::Spawn(detail) => {
                assert!(detail.contains("Failed to start server"), "{detail}");
            }
            other => panic!("expected spawn failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn server_lifecycle_probe_classifications_against_live_server() {
        let state = crate::test_support::booted_state("secret").await;
        let addr = state.state.local_addr.read().expect("bound");
        let password = Some("secret".to_owned());
        // ready + compatible.
        let info = Registration {
            id: None,
            version: Some("test-version".to_owned()),
            url: format!("http://{addr}"),
            pid: std::process::id(),
            password: password.clone(),
        };
        let probed = probe(&info, Duration::from_secs(2)).await;
        assert!(!probed.timed_out);
        let service = probed.service.expect("classified");
        assert_eq!(service.state, ProbeState::Ready);
        assert!(service.compatible);

        // pid mismatch → undefined.
        let mut foreign = info.clone();
        foreign.pid = foreign.pid.wrapping_add(1);
        assert!(probe(&foreign, Duration::from_secs(2)).await.service.is_none());

        // version mismatch → undefined.
        let mut old_registration = info.clone();
        old_registration.version = Some("old".to_owned());
        assert!(probe(&old_registration, Duration::from_secs(2)).await.service.is_none());

        // wrong password → 401 envelope body → undefined.
        let mut bad_auth = info;
        bad_auth.password = Some("wrong".to_owned());
        assert!(probe(&bad_auth, Duration::from_secs(2)).await.service.is_none());

        // unreachable → undefined + not timed out under the budget.
        let mut dead = Registration::empty();
        dead.url = "http://127.0.0.1:1".to_owned();
        let probed = probe(&dead, Duration::from_secs(2)).await;
        assert!(probed.service.is_none());

        // 404 endpoint → ready + incompatible.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let not_found = listener.local_addr().expect("addr");
        drop(listener);
        let mut incompatible = Registration::empty();
        incompatible.url = format!("http://{not_found}");
        incompatible.pid = std::process::id();
        // (No listener here: connection refused → undefined. The 404 class is
        // covered by server_lifecycle_probe_404_incompatible below.)

        state.handle.begin_shutdown();
        let _ = state.handle.shutdown_and_join().await;
        let _ = incompatible;
    }

    #[tokio::test]
    async fn server_lifecycle_probe_404_incompatible() {
        // A tiny server that 404s /api/info.
        let app = axum::routing::Router::new()
            .fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "") });
        let std_listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = std_listener.local_addr().expect("addr");
        std_listener.set_nonblocking(true).expect("nonblocking");
        let listener = tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let mut info = Registration::empty();
        info.url = format!("http://{addr}");
        info.pid = std::process::id();
        let probed = probe(&info, Duration::from_secs(2)).await;
        let service = probed.service.expect("classified");
        assert_eq!(service.state, ProbeState::Ready);
        assert!(!service.compatible, "missing health endpoint = incompatible");
    }

    #[tokio::test]
    async fn server_lifecycle_pty_handoff_prepare_falls_back_to_null() {
        // Live server without persistent-pty handlers: handoff POST 404s →
        // shutdown POST 404s → sidecar published with handoff: null.
        let state = crate::test_support::booted_state("secret").await;
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("service.json");
        let addr = state.state.local_addr.read().expect("bound");
        let info = Registration {
            id: Some("srv_h".to_owned()),
            version: Some("test-version".to_owned()),
            url: format!("http://{addr}"),
            pid: std::process::id(),
            password: Some("secret".to_owned()),
        };
        pty_handoff_prepare(&file, &info, Duration::from_secs(2)).await;
        let record = read_sidecar(&file).await.expect("sidecar published");
        assert!(record.handoff.is_none(), "no handler → null handoff");
        assert!(record.expires_at <= now_ms() + 30_001);
        // null handoff → no env injection.
        write_registration(&file, &info).expect("reg");
        assert!(pty_handoff_environment(&file, &[]).await.is_none());

        state.handle.begin_shutdown();
        let _ = state.handle.shutdown_and_join().await;
    }

    #[tokio::test]
    async fn server_lifecycle_ensure_detaches_winning_contender() {
        // Regression (wave 6a): `ensure()` used to SIGKILL its spawned
        // contender on scope exit — murdering the freshly-ensured daemon the
        // moment the first client returned (v2 `release()` only detaches).
        // Winner: a script that writes its own registration, then serves
        // /api/info on a free port until killed. Requires python3.
        if !Command::new("python3")
            .arg("-c")
            .arg("print('')")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            eprintln!("python3 unavailable; skipping contender-detach regression");
            return;
        }
        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let file = tmp.path().join("service.json");
        let script = format!(
            r#"
import http.server, json, os
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({{"version": "test-version", "pid": os.getpid(), "urls": [], "paths": {{}}}}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *_):
        pass
server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port = server.server_address[1]
open(r"{file}", "w").write(json.dumps({{
    "id": "srv_winner", "version": "test-version",
    "url": "http://127.0.0.1:%d" % port, "pid": os.getpid(), "password": "secret",
}}))
# Well-behaved contender: also write the generation sidecar our stop()
# path requires (boot id + this process's start time) so it can be
# terminated safely.
boot_id = open("/proc/sys/kernel/random/boot_id").read().strip()
start_time = int(open("/proc/self/stat").read().rsplit(")", 1)[1].split()[19])
open(r"{file}.boot", "w").write(json.dumps({{"boot_id": boot_id, "start_time": start_time}}))
import threading
threading.Thread(target=server.serve_forever, daemon=False).start()
while True:
    import time; time.sleep(3600)
"#,
            file = file.display(),
        );
        let command = vec!["python3".to_owned(), "-c".to_owned(), script];
        let endpoint = ensure(&EnsureOptions {
            file: Some(file.clone()),
            version: Some("test-version".to_owned()),
            command: Some(command),
            timing: EnsureTiming {
                poll_interval: Duration::from_millis(20),
                promise_timeout: Duration::from_secs(30),
                ..EnsureTiming::default()
            },
            ..EnsureOptions::default()
        })
        .await
        .expect("ensure succeeds against the spawned winner");
        let registration = read_registration(Some(&file)).expect("winner registered");
        assert_eq!(endpoint.url, registration.url);

        // The whole point: after ensure() returns, the winner keeps running.
        let winner_pid = registration.pid;
        std::thread::sleep(Duration::from_millis(500));
        assert!(process_alive(winner_pid, None), "ensure() must not kill the daemon it started");

        // Cleanup: stop() terminates the winner and removes the file.
        stop(StopOptions {
            file: Some(file.clone()),
            pty: PtyStop::Clear,
            timing: EnsureTiming {
                stop_poll_interval: Duration::from_millis(10),
                ..EnsureTiming::default()
            },
        })
        .await;
        assert!(!process_alive(winner_pid, None), "stop() terminates the daemon");
        assert!(!file.exists(), "stop() removes the registration");
    }
}
