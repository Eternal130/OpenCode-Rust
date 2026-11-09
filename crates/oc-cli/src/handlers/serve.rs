//! Port of `cli/src/server-process.ts` + `handlers/serve.ts` — the three
//! serve modes sharing one boot path: foreground `default`, `--service`
//! daemon (registration + self-heal watcher + incumbent idempotence), and
//! `--stdio` lease mode (ready line, password env scrubbed, stdin EOF).

use std::time::Duration;

use oc_server::boot::{self, BootOptions};
use oc_server::lifecycle::{self, RegisterOptions, ServeHandle, ServeOptions};
use tokio::sync::watch;

use crate::services::service_config;
use crate::util;

pub enum Mode {
    Default,
    Service,
    Stdio,
}

pub struct Options {
    pub mode: Mode,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub cors: Vec<String>,
}

/// The stdout announcement each mode prints once listening (v2
/// `server-process.ts:174-175`): stdio's ready line is the entire
/// contract; the FOREGROUND default prints the generated password when
/// none came from the environment. G-213 — SERVICE mode never prints
/// the password: it is persisted into `ServiceConfig`, and daemon stdout
/// lands in journald/desktop logs.
fn startup_announcement(
    mode: &Mode,
    environment_password: Option<&str>,
    password: &str,
    url: &str,
) -> Vec<String> {
    if let Mode::Stdio = mode {
        let quoted = serde_json::to_string(url).expect("url serializes");
        return vec![format!("{{\"url\":{quoted}}}")];
    }
    let mut lines = vec![format!("server listening on {url}")];
    if environment_password.is_none() && matches!(mode, Mode::Default) {
        lines.push(format!("server password {password}"));
    }
    lines
}

#[allow(dead_code)] // parity reference for v2 serviceURL (server-process.ts:185)
fn service_url(hostname: &std::net::IpAddr, port: u16) -> String {
    match hostname {
        std::net::IpAddr::V6(address) => format!("http://[{address}]:{port}"),
        std::net::IpAddr::V4(address) => format!("http://{address}:{port}"),
    }
}

/// The bounded drain every mode uses on shutdown (gate-6 F2): graceful stop,
/// hard exit(0) at 5s.
async fn bounded_drain(handle: ServeHandle) {
    if let Ok(result) =
        tokio::time::timeout(Duration::from_secs(5), handle.shutdown_and_join()).await
    {
        if let Err(error) = result {
            eprintln!("server shutdown error: {error}");
        }
    } else {
        eprintln!("graceful shutdown deadline exceeded; exiting");
        std::process::exit(0);
    }
}

/// `ServerProcess.run(options)`. Returns the process exit code.
#[allow(clippy::too_many_lines)]
pub async fn run(options: Options) -> i32 {
    let roots = oc_util::global_roots::resolve();
    // v2's daemon chdirs to `$HOME` (server-process.ts:59): the daemon is
    // shared across projects, and per-location provider wiring (oc-core
    // resolves each location's config-seeded custom providers through its
    // own overlay) makes the boot cwd's config irrelevant for every other
    // location. Engine-level defaults still seed from $HOME's (global)
    // config after the chdir.
    if matches!(options.mode, Mode::Service) {
        let _ = std::env::set_current_dir(&roots.home);
    }
    let service = matches!(options.mode, Mode::Service);
    let config = if service { service_config::read() } else { service_config::Info::default() };

    // `server-process.ts:65` — v2 threads the raw hostname STRING into
    // the HTTP server (Node resolves `localhost`/domains, and a bad
    // value fails the boot). Resolve to an address here; an unresolvable
    // value is an ERROR, never a silent downgrade to 127.0.0.1. The
    // service-config fallback (`opencode service set hostname`) resolves
    // the same way.
    let hostname: std::net::IpAddr = if let Some(raw) = options.hostname.as_deref() {
        match boot::resolve_hostname(raw) {
            Ok(address) => address,
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
        }
    } else if let Some(raw) = config.hostname.as_deref() {
        match boot::resolve_hostname(raw) {
            Ok(address) => address,
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
        }
    } else {
        std::net::IpAddr::from([127, 0, 0, 1])
    };
    let port = options
        .port
        .or(config.port)
        .or_else(|| service.then(|| service_config::rust_default_port(&util::channel())));

    // Incumbent check + G-217 bind retry: `bind_or_incumbent` (v2
    // `server-process.ts:91-171`) binds immediately; on EADDRINUSE it
    // polls the incumbent every 100 ms up to 15 s, retrying the bind
    // between probes — an old daemon exiting hands the port to this
    // one, a compatible incumbent makes this an idempotent start.
    // B4-H3: "compatible" includes the BUILD marker — a fresh spawn
    // stands down only for a daemon of its own build, never for a
    // stale same-version incumbent (that would wedge the replacement).
    let mut prebound = None;
    if service && port.is_some() {
        let registration_file = service_config::registration_file();
        match lifecycle::bind_or_incumbent(
            &ServeOptions { hostname, port },
            Some(std::path::Path::new(&registration_file)),
            None,
            Some(lifecycle::BUILD_STAMP),
        )
        .await
        {
            Ok(lifecycle::BindOutcome::Bound(listener)) => prebound = Some(listener),
            Ok(lifecycle::BindOutcome::IncumbentServing(_)) => return 0,
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
        }
    }

    let environment_password = util::env_password();
    if matches!(options.mode, Mode::Stdio) {
        // Keep the lease credential out of the environment inherited by tools.
        std::env::remove_var("OPENCODE_PASSWORD");
        std::env::remove_var("OPENCODE_SERVER_PASSWORD");
    }
    let password = if service {
        config.password.clone().unwrap_or_else(util::random_password)
    } else {
        environment_password.clone().unwrap_or_else(util::random_password)
    };
    let instance_id = uuid::Uuid::new_v4().to_string();

    let mut boot_options = BootOptions::new();
    boot_options.serve = ServeOptions { hostname, port };
    boot_options.password = Some(password.clone());
    boot_options.cors_origins = if options.cors.is_empty() {
        config.cors.clone().unwrap_or_default()
    } else {
        options.cors.clone()
    };
    util::VERSION.clone_into(&mut boot_options.version);
    let handle = match boot::boot_and_serve_with_listener(&boot_options, prebound).await {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };

    // Installation notices (v2 process.ts:115-117): the CLI's detached
    // update checker cannot hold the engine, so `serve` — the one mode
    // whose process owns the server — forwards its notices onto the bus as
    // `installation.update-available` / `installation.updated` `{version}`
    // for SSE consumers.
    if let Some(mut notices) = crate::services::autoupdate::take_installation_receiver() {
        let bus = handle.state.engine.bus.clone();
        let mut shutdown = handle.shutdown.subscribe();
        tokio::spawn(async move {
            loop {
                // Receive outside the publish await: the watch guard is not
                // Send and must not live across the publish.
                let notice = tokio::select! {
                    notice = notices.recv() => notice,
                    _ = shutdown.wait_for(|flag| *flag) => break,
                };
                match notice {
                    Some(crate::services::autoupdate::InstallationNotice::Available(version)) => {
                        oc_server::announce::publish_installation_event(&bus, true, &version).await;
                    }
                    Some(crate::services::autoupdate::InstallationNotice::Installed(version)) => {
                        oc_server::announce::publish_installation_event(&bus, false, &version)
                            .await;
                    }
                    None => break,
                }
            }
        });
    }

    let cleanup = if service {
        if config.password.is_none() {
            let _ = service_config::password(Some(&password));
        }
        let (watcher_tx, mut watcher_rx) = watch::channel(false);
        match lifecycle::register(RegisterOptions {
            address: handle.addr,
            password: password.clone(),
            id: instance_id,
            file: service_config::registration_file(),
            version: util::VERSION.to_owned(),
            shutdown: watcher_tx,
            watch_interval: Duration::from_secs(5),
        }) {
            Ok(cleanup) => {
                // The self-heal watcher fires `watcher_rx` when this daemon
                // no longer owns the registration — begin a graceful stop.
                let serve_shutdown = handle.shutdown.clone();
                tokio::spawn(async move {
                    let _ = watcher_rx.wait_for(|flag| *flag).await;
                    let _ = serve_shutdown.send(true);
                });
                Some(cleanup)
            }
            Err(error) => {
                eprintln!("Error: failed to register service: {error}");
                bounded_drain(handle).await;
                return 1;
            }
        }
    } else {
        None
    };

    for line in startup_announcement(
        &options.mode,
        environment_password.as_deref(),
        &password,
        &handle.url(),
    ) {
        println!("{line}");
    }

    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        Ok(signal) => signal,
        Err(error) => {
            eprintln!("Error: {error}");
            return 1;
        }
    };
    let lease = matches!(options.mode, Mode::Stdio).then(lifecycle::wait_stdin_eof);
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
        () = async { lease.expect("lease only in stdio mode").await }, if matches!(options.mode, Mode::Stdio) => {}
    }
    bounded_drain(handle).await;
    if let Some(cleanup) = cleanup {
        cleanup.run().await;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_foreground_prints_generated_password() {
        let lines = startup_announcement(&Mode::Default, None, "pw-secret", "http://127.0.0.1:1");
        assert_eq!(lines[0], "server listening on http://127.0.0.1:1");
        assert!(lines.contains(&"server password pw-secret".to_owned()));
    }

    #[test]
    fn announcement_foreground_hides_environment_password() {
        let lines = startup_announcement(&Mode::Default, Some("from-env"), "pw-secret", "http://x");
        assert!(!lines.iter().any(|line| line.contains("pw-secret")));
    }

    /// G-213 — service-mode daemon stdout lands in journald/desktop logs;
    /// the password lives in `ServiceConfig` and must never be printed.
    #[test]
    fn announcement_service_mode_never_prints_password() {
        let lines = startup_announcement(&Mode::Service, None, "pw-secret", "http://x");
        assert_eq!(lines.len(), 1, "service mode prints only the listening line");
        assert_eq!(lines[0], "server listening on http://x");
    }

    #[test]
    fn announcement_stdio_ready_line_is_the_whole_contract() {
        let lines = startup_announcement(&Mode::Stdio, None, "pw-secret", "http://127.0.0.1:9");
        assert_eq!(lines, vec!["{\"url\":\"http://127.0.0.1:9\"}".to_owned()]);
    }
}
