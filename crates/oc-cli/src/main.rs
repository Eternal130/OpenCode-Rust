//! `opencode` — the CLI entry (port of `cli/src/index.ts`): hidden SSH
//! askpass dispatch, clap parsing, command dispatch, `--version`.

mod acp;
mod cli;
mod handlers;
mod mini;
mod run;
mod run_ui;
mod services;
mod ssh_askpass;
mod util;

use clap::Parser;
use cli::{Cli, Commands, ServerParams};

/// The `mini` section of `cli.json` (`resolved.mini` in v2's
/// `handlers/mini.ts:37-38`) — the `--replay`/`--replay-limit` flag
/// fallbacks. Reads through [`oc_core::cli_config`] (env overlay
/// included); unreadable config degrades to no fallback (defaults
/// apply).
fn mini_replay_config() -> oc_core::cli_config::MiniInfo {
    let roots = oc_util::global_roots::resolve();
    oc_core::cli_config::CliConfig::open(&roots).get().mini.unwrap_or_default()
}

#[tokio::main]
async fn main() {
    // Hidden entrypoint: OpenSSH execs the binary directly as askpass.
    if std::env::var_os("OPENCODE_SSH_ASKPASS_PORT").is_some() {
        std::process::exit(ssh_askpass::run().await);
    }

    let cli = Cli::parse();
    // `--print-logs` (runtime.ts:98): the global flag sets
    // `OPENCODE_PRINT_LOGS=1` in the process environment BEFORE any
    // handler runs. Consumers: the standalone server spawn inherits
    // the child's stderr instead of discarding it (standalone.ts:29),
    // and any subprocess logging gate reads the same variable — the
    // parsed-but-ignored flag silently dropped server log output.
    if cli.print_logs {
        std::env::set_var("OPENCODE_PRINT_LOGS", "1");
    }
    if cli.version_rust {
        let tag = services::updater::RELEASE_TAG;
        let release = if tag == "dev" { String::new() } else { format!(", release {tag}") };
        println!(
            "opencode-rust v{} (rust port build {}, {} profile{})",
            util::VERSION,
            option_env!("OC_BUILD_GIT_HASH").unwrap_or("unknown"),
            option_env!("OC_BUILD_PROFILE").unwrap_or("unknown"),
            release,
        );
        std::process::exit(0);
    }
    // One-way v2→rust data migration (lane B): runs once, marker-latched.
    // Failures warn on stderr and never block boot.
    oc_core::v2_migration::ensure_migrated_logged();
    // Release-channel update check (lane C): ONLY the long-running
    // paths (default/TUI + serve) spawn the detached checker; the
    // short-lived commands never do. `tui` selects the notify channel
    // (stderr for serve, the state-dir notice file inside the TUI —
    // printing over the TUI screen would corrupt it). Dev builds and
    // disabled policies return without spawning; every network failure
    // inside the thread is a silent no-op (see services/autoupdate.rs).
    let tui = cli.command.is_none();
    if tui || matches!(cli.command, Some(Commands::Serve { .. })) {
        let _ = services::autoupdate::spawn_background_update_check(tui);
    }
    let code = dispatch(cli).await;
    std::process::exit(code);
}

#[allow(clippy::too_many_lines)]
async fn dispatch(cli: Cli) -> i32 {
    let server = ServerParams { standalone: cli.standalone, server: cli.server.clone() };
    let auto = cli.permission.effective_auto();
    match cli.command {
        None => {
            handlers::misc::root(
                cli.directory.as_deref(),
                cli.continue_,
                cli.session.clone(),
                cli.prompt.clone(),
                auto,
                &server,
            )
            .await
        }
        Some(Commands::Upgrade { target, method }) => {
            handlers::misc::upgrade(target.as_deref(), method.as_deref()).await
        }
        Some(Commands::Uninstall { keep_config, keep_data, dry_run, force }) => {
            handlers::misc::uninstall(dry_run, keep_config, keep_data, force).await
        }
        Some(Commands::Acp) => handlers::misc::acp().await,
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
        Some(Commands::Mini {
            continue_,
            session,
            fork,
            replay,
            mini_no_replay,
            replay_limit,
            model,
            agent,
            prompt,
            demo,
        }) => {
            // v2 `handlers/mini.ts:37-38`: the CLI flag wins, then the
            // `mini` section of cli.json (`mini.replay` / `mini.replay_limit`),
            // then the built-in defaults (replay on; the limit default
            // applies inside the mini host).
            let resolved = mini_replay_config();
            let input = mini::MiniInput {
                continue_,
                session,
                fork,
                model,
                agent,
                prompt,
                // `--replay`/`--no-replay` presence: `--no-replay` wins
                // when both are given (v2's negation pair).
                replay: mini::resolve_replay(
                    if mini_no_replay {
                        Some(false)
                    } else if replay {
                        Some(true)
                    } else {
                        None
                    },
                    resolved.replay,
                ),
                replay_limit: mini::resolve_replay_limit(
                    replay_limit,
                    resolved.replay_limit.and_then(|limit| u32::try_from(limit).ok()),
                ),
                demo,
            };
            handlers::misc::mini(input, &server).await
        }
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
