//! Port of `cli/src/services/server-connection.ts` — decide which server a
//! command talks to: explicit `--server <url>` (probe `/api/info`, warn on
//! version mismatch but continue), `--standalone`/service-disabled (private
//! stdio-lease child), or the managed background service via
//! `oc_server::lifecycle::ensure` with the TUI's mismatch-replace knob.
//! B4-H3: the managed paths also compare the daemon's `x-opencode-build`
//! marker — the parity pin keeps the version string identical across
//! betas, so the build stamp is what detects a stale daemon after
//! `opencode-rust upgrade` (replace: stop + respawn via the lifecycle
//! stop path).

use oc_client::Client as HttpClient;
use oc_server::lifecycle::{self, EnsureOptions};

use crate::services::service_config;
use crate::services::standalone::{self, StandaloneServer};
use crate::util;

/// `Endpoint`: base URL + basic-auth password.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub password: Option<String>,
}

impl Endpoint {
    /// `Service.headers(endpoint)` → a configured SDK client.
    #[must_use]
    pub fn client(&self) -> HttpClient {
        let mut client = HttpClient::new(&self.url).expect("valid endpoint url");
        if let Some(password) = &self.password {
            client = client.with_basic_auth("opencode", Some(password.clone()));
        }
        client
    }
}

/// `Args.mismatch`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mismatch {
    /// Stop a mismatched daemon and start this version.
    Replace,
    /// Reconnect regardless of version.
    #[default]
    Ignore,
    /// Fail when a different version is registered.
    #[allow(dead_code)]
    Error,
}

/// `ServerConnection.resolve` arguments.
#[derive(Debug, Clone, Default)]
pub struct Args {
    pub server: Option<String>,
    pub standalone: bool,
    pub mismatch: Mismatch,
}

/// The resolved connection: an endpoint plus the standalone lease, if one
/// was started (it must be kept alive for the process lifetime).
/// `managed` mirrors v2's `Resolved.service`: true only when the endpoint
/// is the background service we own (`--server`/`--standalone` are false).
pub struct Resolved {
    pub endpoint: Endpoint,
    pub standalone: Option<StandaloneServer>,
    pub managed: bool,
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Resolved")
            .field("endpoint", &self.endpoint)
            .field(
                "standalone",
                &self.standalone.as_ref().map(|server| (&server.url, &server.password[..4])),
            )
            .field("managed", &self.managed)
            .finish()
    }
}

fn connect_error(endpoint: &Endpoint, status: Option<u16>, timed_out: bool) -> String {
    if timed_out {
        // v2 aborts the probe after 5s (`AbortSignal.timeout`); the abort
        // is not a transport failure, so its message is the generic
        // health-response one.
        return format!(
            "Server at {} did not provide a compatible V2 health response",
            endpoint.url
        );
    }
    match status {
        Some(401) if endpoint.password.is_none() => {
            format!("Server at {} requires a password; set OPENCODE_PASSWORD", endpoint.url)
        }
        Some(401) => format!("Server at {} rejected the password", endpoint.url),
        Some(_) => {
            format!("Server at {} did not provide a compatible V2 health response", endpoint.url)
        }
        None => format!("Could not reach server at {}", endpoint.url),
    }
}

async fn resolve_explicit(url: &str) -> Result<Endpoint, String> {
    let endpoint =
        Endpoint { url: url.trim_end_matches('/').to_owned(), password: util::env_password() };
    let client = endpoint.client();
    let info =
        tokio::time::timeout(std::time::Duration::from_secs(5), client.server().info()).await;
    let info = match info {
        Ok(Ok(info)) => info,
        Ok(Err(error)) => return Err(connect_error(&endpoint, error.status(), false)),
        Err(_) => return Err(connect_error(&endpoint, None, true)),
    };
    if info.version != util::VERSION {
        eprintln!(
            "Warning: Server at {} has version {}; this client is {}. Continuing anyway.",
            endpoint.url,
            info.version,
            util::VERSION
        );
    }
    Ok(endpoint)
}

/// `managedService(options).restart` support: reconnect ignoring version.
pub async fn ensure_managed(check_version: bool) -> Result<Endpoint, String> {
    ensure_with(service_config::options(check_version)).await
}

async fn ensure_with(mut options: EnsureOptions) -> Result<Endpoint, String> {
    options.timing.poll_interval = std::time::Duration::from_millis(50);
    let endpoint = lifecycle::ensure(&options).await.map_err(|error| error.to_string())?;
    Ok(Endpoint {
        url: endpoint.url.trim_end_matches('/').to_owned(),
        password: endpoint.auth.map(|(_, password)| password),
    })
}

/// `Service.discover({version})`: probe the registration — `Some(version)`
/// requires the live server to report exactly that version (v2's
/// `matchesVersion` with a string requirement) AND this binary's build
/// stamp (B4-H3: the version pin is identical across betas, so the
/// build marker is what distinguishes a stale daemon); `None` accepts
/// any version. `None` when nothing healthy is registered.
#[must_use]
pub async fn discover_managed(version: Option<&str>) -> Option<Endpoint> {
    let file = service_config::registration_file();
    let mut info = lifecycle::read_registration(Some(&file))?;
    // The version filter rides on the registration's required-version
    // field (`probe` compares the server-reported version against it).
    info.version = version.map(str::to_owned);
    let probed = lifecycle::probe(&info, std::time::Duration::from_secs(2)).await;
    let service = probed.service?;
    if service.state != lifecycle::ProbeState::Ready {
        return None;
    }
    if !service.compatible {
        return None;
    }
    // B4-H3: a version-filtered discovery also demands this binary's
    // build (a daemon with a different/missing marker is NOT the
    // compatible incumbent it claims to be on version alone).
    if version.is_some()
        && !lifecycle::matches_build(service.build.as_deref(), Some(lifecycle::BUILD_STAMP))
    {
        return None;
    }
    Some(Endpoint {
        url: service.endpoint.url.trim_end_matches('/').to_owned(),
        password: service.endpoint.auth.map(|(_, password)| password),
    })
}

/// `shutdownPersistentPty` (best-effort, ignored failures).
pub async fn shutdown_persistent_pty() {
    if let Some(endpoint) = discover_managed(None).await {
        let _ = endpoint.client().persistent_pty().shutdown().await;
    }
}

/// `ServerConnection.resolve()`.
pub async fn resolve(args: Args) -> Result<Resolved, String> {
    if args.server.is_some() && args.standalone {
        return Err("--server and --standalone cannot be combined".to_owned());
    }
    if let Some(url) = &args.server {
        return Ok(Resolved {
            endpoint: resolve_explicit(url).await?,
            standalone: None,
            managed: false,
        });
    }
    if args.standalone || service_config::read().disabled == Some(true) {
        let server = standalone::start().await?;
        let endpoint = server.endpoint();
        return Ok(Resolved { endpoint, standalone: Some(server), managed: false });
    }

    let endpoint = match args.mismatch {
        Mismatch::Replace => ensure_with(service_config::options(true)).await?,
        Mismatch::Ignore => {
            let mut options = service_config::options(false);
            options.version = None;
            ensure_with(options).await?
        }
        Mismatch::Error => {
            // v2 `resolveManaged`: discover version-filtered first (a
            // compatible daemon is reused), then discover unfiltered — a
            // healthy daemon with a DIFFERENT version is a hard error, not
            // a silent reconnect. Only when nothing is registered at all
            // does ensure() spawn one.
            if let Some(found) = discover_managed(Some(util::VERSION)).await {
                found
            } else if discover_managed(None).await.is_some() {
                return Err("Background server version does not match this client".to_owned());
            } else {
                ensure_with(service_config::options(true)).await?
            }
        }
    };
    Ok(Resolved { endpoint, standalone: None, managed: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn server_and_standalone_conflict() {
        let error = resolve(Args {
            server: Some("http://127.0.0.1:1".to_owned()),
            standalone: true,
            mismatch: Mismatch::Ignore,
        })
        .await
        .expect_err("conflict");
        assert_eq!(error, "--server and --standalone cannot be combined");
    }

    #[tokio::test]
    async fn explicit_unreachable() {
        let error = resolve(Args {
            server: Some("http://127.0.0.1:1".to_owned()),
            standalone: false,
            mismatch: Mismatch::Ignore,
        })
        .await
        .expect_err("unreachable");
        assert_eq!(error, "Could not reach server at http://127.0.0.1:1");
    }

    #[test]
    fn connect_error_timeout_uses_v2_health_message() {
        let endpoint = Endpoint { url: "http://127.0.0.1:1".to_owned(), password: None };
        // A transport failure (connection refused) stays "Could not reach".
        assert_eq!(
            connect_error(&endpoint, None, false),
            "Could not reach server at http://127.0.0.1:1"
        );
        // The 5s probe timeout aborts rather than failing transport, so v2
        // reports the generic health-response message.
        assert_eq!(
            connect_error(&endpoint, None, true),
            "Server at http://127.0.0.1:1 did not provide a compatible V2 health response"
        );
        assert_eq!(
            connect_error(&endpoint, Some(500), false),
            "Server at http://127.0.0.1:1 did not provide a compatible V2 health response"
        );
    }

    /// A sandboxed home so `service_config::registration_file()` lands in a
    /// temp dir (same isolation pattern as the `service_config` tests).
    fn sandboxed() -> tempfile::TempDir {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        std::env::set_var("OPENCODE_TEST_HOME", dir.path());
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_CACHE_HOME");
        dir
    }

    /// Minimal `/api/info` responder reporting the given version + pid
    /// and (B4-H3) the `x-opencode-build` marker header.
    async fn fake_info_server(version: String, pid: u32, build: Option<String>) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let version = version.clone();
                let build = build.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buffer = [0u8; 1024];
                    let _ = socket.read(&mut buffer).await;
                    let body = format!("{{\"version\":\"{version}\",\"pid\":{pid}}}");
                    let marker = build
                        .map_or(String::new(), |stamp| format!("x-opencode-build: {stamp}\r\n"));
                    let response = format!(
                        "HTTP/1.1 200 OK\r\n{marker}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        port
    }

    fn register_fake_daemon(url: &str, pid: u32) {
        let file = service_config::registration_file();
        std::fs::create_dir_all(file.parent().expect("parent")).expect("state dir");
        std::fs::write(
            &file,
            serde_json::json!({
                "id": "srv_fake",
                "version": "registered-version",
                "url": url,
                "pid": pid,
            })
            .to_string(),
        )
        .expect("registration");
    }

    #[tokio::test]
    async fn mismatch_error_errors_on_version_mismatched_daemon() {
        let _sandbox = sandboxed();
        let pid = std::process::id();
        let port = fake_info_server("0.0.0-old.1".to_owned(), pid, None).await;
        register_fake_daemon(&format!("http://127.0.0.1:{port}"), pid);

        let error = resolve(Args { server: None, standalone: false, mismatch: Mismatch::Error })
            .await
            .expect_err("mismatched daemon must hard-error");
        assert_eq!(error, "Background server version does not match this client");
    }

    #[tokio::test]
    async fn mismatch_error_reuses_compatible_daemon() {
        let _sandbox = sandboxed();
        let pid = std::process::id();
        let port = fake_info_server(
            util::VERSION.to_owned(),
            pid,
            Some(oc_server::lifecycle::BUILD_STAMP.to_owned()),
        )
        .await;
        let url = format!("http://127.0.0.1:{port}");
        register_fake_daemon(&url, pid);

        let resolved = resolve(Args { server: None, standalone: false, mismatch: Mismatch::Error })
            .await
            .expect("compatible daemon is reused");
        assert_eq!(resolved.endpoint.url, url);
        assert!(resolved.managed, "the managed path resolves with managed=true");
    }

    /// B4-H3: the version pin ("2.0.24") is identical across betas, so
    /// a daemon of the SAME version but an OLD build marker is NOT the
    /// compatible incumbent — version-filtered discovery rejects it
    /// (and the unfiltered probe still sees a live daemon, which is
    /// what makes `Mismatch::Error` hard-error and `Mismatch::Replace`
    /// stop-and-respawn instead of reconnecting).
    #[tokio::test]
    async fn same_version_stale_build_daemon_is_not_compatible() {
        let _sandbox = sandboxed();
        let pid = std::process::id();
        // Old marker: same version, different stamp.
        let port = fake_info_server(
            util::VERSION.to_owned(),
            pid,
            Some("v2.0.24.beta.1@0000000".to_owned()),
        )
        .await;
        register_fake_daemon(&format!("http://127.0.0.1:{port}"), pid);
        assert!(
            discover_managed(Some(util::VERSION)).await.is_none(),
            "a stale build is rejected by version-filtered discovery"
        );
        assert!(
            discover_managed(None).await.is_some(),
            "the unfiltered probe still finds the live daemon (replace/error, not respawn-blind)"
        );

        // A daemon with NO marker (v2 daemons, pre-marker builds) is
        // equally stale to a checking client.
        let port = fake_info_server(util::VERSION.to_owned(), pid, None).await;
        register_fake_daemon(&format!("http://127.0.0.1:{port}"), pid);
        assert!(discover_managed(Some(util::VERSION)).await.is_none());

        // The current marker connects.
        let port = fake_info_server(
            util::VERSION.to_owned(),
            pid,
            Some(oc_server::lifecycle::BUILD_STAMP.to_owned()),
        )
        .await;
        let url = format!("http://127.0.0.1:{port}");
        register_fake_daemon(&url, pid);
        let found = discover_managed(Some(util::VERSION)).await.expect("current build connects");
        assert_eq!(found.url, url);
    }

    #[tokio::test]
    async fn mismatch_error_without_registration_would_ensure() {
        // No registration file at all: the first (version-filtered)
        // discover and the second (unfiltered) discover both miss, so the
        // arm falls through to ensure_with — observable here only as the
        // absence of the mismatch error (the ensure spawn itself is the
        // e2e-managed path; we stop before spawning a real daemon by
        // pointing discovery at an empty sandbox and asserting the two
        // discover calls' result directly).
        let _sandbox = sandboxed();
        assert!(discover_managed(Some(util::VERSION)).await.is_none());
        assert!(discover_managed(None).await.is_none());
    }
}
