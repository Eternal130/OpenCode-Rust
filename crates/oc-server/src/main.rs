//! The `oc-server` serve CLI (the Phase 6 CLI seed): boots the engine +
//! app shell + handler table and serves until SIGTERM/SIGINT.
//!
//! ```text
//! oc-server serve [--port <PORT>] [--hostname <IP>] [--cors <ORIGIN>]...
//! ```
//!
//! - Password: `OPENCODE_PASSWORD` (unset = passwordless).
//! - Roots: `OPENCODE_TEST_HOME` / XDG overrides relocate data + home
//!   (`oc_util::global_roots` rules).
//! - Config seeding: the working directory's config (plus
//!   `OPENCODE_CONFIG`) supplies boot-time custom providers — see
//!   `oc_server::boot`.

use std::io;

use oc_server::boot::{self, BootOptions};
use oc_server::lifecycle::ServeOptions;

fn usage() -> ! {
    eprintln!(
        "usage: oc-server serve [--port <PORT>] [--hostname <IP>] [--cors <ORIGIN>]...\n\
         env: OPENCODE_PASSWORD, OPENCODE_CONFIG, OPENCODE_TEST_HOME, XDG_*"
    );
    std::process::exit(2);
}

fn parse_args(args: &[String]) -> BootOptions {
    let mut options = BootOptions::new();
    let mut port: Option<u16> = None;
    let mut hostname: Option<std::net::IpAddr> = None;
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
                hostname = boot::parse_hostname(&raw);
                if hostname.is_none() {
                    eprintln!("invalid --hostname {raw:?}");
                    usage();
                }
            }
            "--cors" => {
                options.cors_origins.push(value());
            }
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
    options.serve = ServeOptions { hostname: hostname.unwrap_or(options.serve.hostname), port };
    options
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut options = parse_args(&args);
    options.password = std::env::var("OPENCODE_PASSWORD").ok().filter(|value| !value.is_empty());
    let address = format!("http://{}", options.serve.hostname);
    let handle = boot::boot_and_serve(&options).await?;
    println!("server listening on {}", handle.url());
    if let Some(password) = &options.password {
        println!("server password {password}");
    }
    let _ = address;

    // Graceful stop on SIGTERM / SIGINT. The overall drain is bounded
    // (gate-6 F2): stream bodies end on the shutdown watch, but anything
    // still in flight at 5s is abandoned with exit(0) — the reference's
    // practical prompt-exit behavior.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
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
