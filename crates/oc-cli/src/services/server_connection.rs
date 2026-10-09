//! Port of `cli/src/services/server-connection.ts` — decide which server a
//! command talks to: explicit `--server <url>` (probe `/api/info`, warn on
//! version mismatch but continue), `--standalone`/service-disabled (private
//! stdio-lease child), or the managed background service via
//! `oc_server::lifecycle::ensure` with the TUI's mismatch-replace knob.

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
pub struct Resolved {
    pub endpoint: Endpoint,
    pub standalone: Option<StandaloneServer>,
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
            .finish()
    }
}

fn connect_error(endpoint: &Endpoint, status: Option<u16>) -> String {
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
        Ok(Err(error)) => return Err(connect_error(&endpoint, error.status())),
        Err(_) => return Err(connect_error(&endpoint, None)),
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

/// `Service.discover({version: undefined})`: probe the registration; `None`
/// when nothing healthy is registered.
#[must_use]
pub async fn discover_managed() -> Option<Endpoint> {
    let file = service_config::registration_file();
    let info = lifecycle::read_registration(Some(&file))?;
    let probed = lifecycle::probe(&info, std::time::Duration::from_secs(2)).await;
    let service = probed.service?;
    if service.state != lifecycle::ProbeState::Ready {
        return None;
    }
    Some(Endpoint {
        url: service.endpoint.url.trim_end_matches('/').to_owned(),
        password: service.endpoint.auth.map(|(_, password)| password),
    })
}

/// `shutdownPersistentPty` (best-effort, ignored failures).
pub async fn shutdown_persistent_pty() {
    if let Some(endpoint) = discover_managed().await {
        let _ = endpoint.client().persistent_pty().shutdown().await;
    }
}

/// `ServerConnection.resolve()`.
pub async fn resolve(args: Args) -> Result<Resolved, String> {
    if args.server.is_some() && args.standalone {
        return Err("--server and --standalone cannot be combined".to_owned());
    }
    if let Some(url) = &args.server {
        return Ok(Resolved { endpoint: resolve_explicit(url).await?, standalone: None });
    }
    if args.standalone || service_config::read().disabled == Some(true) {
        let server = standalone::start().await?;
        let endpoint = server.endpoint();
        return Ok(Resolved { endpoint, standalone: Some(server) });
    }

    let endpoint = match args.mismatch {
        Mismatch::Replace => ensure_with(service_config::options(true)).await?,
        Mismatch::Ignore => {
            let mut options = service_config::options(false);
            options.version = None;
            ensure_with(options).await?
        }
        Mismatch::Error => {
            if let Some(found) = discover_managed().await {
                found
            } else if discover_managed().await.is_some() {
                return Err("Background server version does not match this client".to_owned());
            } else {
                ensure_with(service_config::options(true)).await?
            }
        }
    };
    Ok(Resolved { endpoint, standalone: None })
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
}
