//! The `oc-server` serve CLI (the Phase 6 CLI seed): boots the engine +
//! app shell + handler table and serves until SIGTERM/SIGINT.
//!
//! ```text
//! oc-server serve [--port <PORT>] [--hostname <HOST>] [--cors <ORIGIN>]...
//!                 [--stdio]
//! ```
//!
//! - Password: `OPENCODE_PASSWORD` (unset = passwordless).
//! - Roots: `OPENCODE_TEST_HOME` / XDG overrides relocate data + home
//!   (`oc_util::global_roots` rules).
//! - Config seeding: the working directory's config (plus
//!   `OPENCODE_CONFIG`) supplies boot-time custom providers — see
//!   `oc_server::boot`.
//! - `--stdio`: the ownership-lease mode `opencode --standalone` spawns —
//!   prints `{"url":...}` on stdout after listen, deletes the password env
//!   for children, and exits when stdin reaches EOF (gate-6 F2 follow-up).

use std::io;

use oc_server::boot::{self, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn usage() -> ! {
    eprintln!(
        "usage: oc-server serve [--port <PORT>] [--hostname <HOST>] [--cors <ORIGIN>]... [--stdio]\n\
         env: OPENCODE_PASSWORD, OPENCODE_CONFIG, OPENCODE_TEST_HOME, XDG_*"
    );
    std::process::exit(2);
}

fn parse_args(args: &[String]) -> BootOptions {
    let mut options = BootOptions::new();
    let mut port: Option<u16> = None;
    let mut hostname: Option<std::net::IpAddr> = None;
    let mut stdio = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut value = || -> String {
            index += 1;
            match args.get(index) {
                Some(value) => value.clone(),
                None => usage(),
            }
        };
        match arg {
            "serve" => {}
            "--port" => {
                let raw = value();
                port = Some(raw.parse().unwrap_or_else(|_| {
                    eprintln!("invalid --port {raw:?}");
                    usage();
                }));
            }
            "--hostname" => {
                let raw = value();
                hostname = Some(boot::resolve_hostname(&raw).unwrap_or_else(|error| {
                    eprintln!("{error}");
                    usage();
                }));
            }
            "--cors" => {
                options.cors_origins.push(value());
            }
            "--stdio" => stdio = true,
            "--help" | "-h" => {
                usage();
            }
            other => {
                eprintln!("unknown argument {other:?}");
                usage();
            }
        }
        index += 1;
    }
    if stdio {
        // The lease owns a random port; the spawner learns it from the
        // ready line on stdout.
        port = port.or(Some(0));
    }
    options.serve = ServeOptions { hostname: hostname.unwrap_or(options.serve.hostname), port };
    options
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdio = args.iter().any(|arg| arg == "--stdio");
    // RW-06: OTel telemetry skeleton — no-op unless the `otel` feature is
    // compiled in AND OTEL_EXPORTER_OTLP_ENDPOINT is set (REF
    // observability toggle). Keep the provider alive for the process
    // lifetime so the batch exporter can drain on exit.
    #[cfg(feature = "otel")]
    let _otel_provider = {
        let version = env!("CARGO_PKG_VERSION");
        oc_server::observability::init(version)
    };
    let mut options = parse_args(&args);
    options.password = std::env::var("OPENCODE_PASSWORD").ok().filter(|value| !value.is_empty());
    if stdio {
        // Keep the lease credential out of the environment inherited by
        // tools (`server-process.ts:78-81`).
        std::env::remove_var("OPENCODE_PASSWORD");
        std::env::remove_var("OPENCODE_SERVER_PASSWORD");
    }
    let handle = boot::boot_and_serve(&options).await?;
    if stdio {
        // The ready line is the entire stdout contract of lease mode.
        println!("{{\"url\":{}}}", serde_json::to_string(&handle.url()).expect("url quotes"));
    } else {
        println!("server listening on {}", handle.url());
        if let Some(password) = &options.password {
            println!("server password {password}");
        }
    }

    // Graceful stop on SIGTERM / SIGINT / (stdio) stdin EOF. The overall
    // drain is bounded (gate-6 F2): stream bodies end on the shutdown watch,
    // but anything still in flight at 5s is abandoned with exit(0) — the
    // reference's practical prompt-exit behavior.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let lease = stdio.then(oc_server::lifecycle::wait_stdin_eof);
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
        () = async { lease.expect("lease only when stdio").await }, if stdio => {}
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), handle.shutdown_and_join()).await
    {
        Ok(result) => result,
        Err(_elapsed) => {
            eprintln!("graceful shutdown deadline exceeded; exiting");
            std::process::exit(0);
        }
    }
}
