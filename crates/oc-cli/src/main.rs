//! `opencode` — the CLI entry (port of `cli/src/index.ts`): hidden SSH
//! askpass dispatch, clap parsing, command dispatch, `--version`.

mod cli;
mod handlers;
mod run;
mod run_ui;
mod services;
mod ssh_askpass;
mod util;

use clap::Parser;
use cli::{Cli, Commands, ServerParams};

#[tokio::main]
async fn main() {
    // Hidden entrypoint: OpenSSH execs the binary directly as askpass.
    if std::env::var_os("OPENCODE_SSH_ASKPASS_PORT").is_some() {
        std::process::exit(ssh_askpass::run().await);
    }

    let cli = Cli::parse();
    let code = dispatch(cli).await;
    std::process::exit(code);
}

#[allow(clippy::too_many_lines)]
async fn dispatch(cli: Cli) -> i32 {
    let server = ServerParams { standalone: cli.standalone, server: cli.server.clone() };
    let auto = cli.permission.effective_auto();
    match cli.command {
        None => handlers::misc::root(cli.directory.as_deref(), &server).await,
        Some(Commands::Upgrade { target, .. }) => handlers::misc::upgrade(target.as_deref()),
        Some(Commands::Uninstall { keep_config, keep_data, dry_run, .. }) => {
            handlers::misc::uninstall(dry_run, keep_config, keep_data)
        }
        Some(Commands::Acp) => handlers::misc::acp(),
        Some(Commands::Api { request, data, header, param }) => {
            handlers::api::run(&server, &request, data.as_deref(), &header, &param).await
        }
        Some(Commands::Debug { command }) => handlers::misc::debug(&command).await,
        Some(Commands::Auth { command }) => handlers::auth::run(&command, &server).await,
        Some(Commands::Mcp { command }) => handlers::mcp::run(&command).await,
        Some(Commands::Plugin { command }) => handlers::plugin::run(&command).await,
        Some(Commands::Models) => handlers::misc::models(&server).await,
        Some(Commands::Stats {
            days,
            year,
            all,
            project,
            models,
            tools,
            cost,
            full,
            limit,
            json,
        }) => {
            let flags = handlers::stats::Flags {
                days,
                year,
                all,
                project,
                models,
                tools,
                cost,
                full,
                limit,
                json,
            };
            handlers::stats::run(&flags, &server).await
        }
        Some(Commands::Mini { .. }) => handlers::misc::mini(),
        Some(Commands::Run {
            message,
            continue_,
            session,
            fork,
            model,
            agent,
            format,
            file,
            title,
            thinking,
        }) => {
            let format = if format == "json" {
                run::noninteractive::Format::Json
            } else {
                run::noninteractive::Format::Default
            };
            let resolved = match handlers::connect(&server, None).await {
                Ok(resolved) => resolved,
                Err(code) => return code,
            };
            run::run(run::RunInput {
                server: resolved,
                message,
                continue_,
                session,
                fork,
                model,
                agent,
                format,
                file,
                title,
                thinking,
                auto,
            })
            .await
        }
        Some(Commands::Session { command }) => handlers::session::run(&command, &server).await,
        Some(Commands::Service { command }) => handlers::service_cmds::run(&command).await,
        Some(Commands::Reload) => handlers::misc::reload(&server).await,
        Some(Commands::Pair { url }) => handlers::misc::pair(url.as_deref()).await,
        Some(Commands::Serve { hostname, port, cors, service, stdio }) => {
            if service && stdio {
                eprintln!("Error: --service and --stdio cannot be combined");
                return 1;
            }
            let mode = if service {
                handlers::serve::Mode::Service
            } else if stdio {
                handlers::serve::Mode::Stdio
            } else {
                handlers::serve::Mode::Default
            };
            handlers::serve::run(handlers::serve::Options { mode, hostname, port, cors }).await
        }
    }
}
