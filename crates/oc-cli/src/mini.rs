//! `opencode mini` — port of `packages/cli/src/mini.ts` (`runMini`):
//! terminal validation, piped-stdin merge with `--prompt`, the
//! `--fork` rules, session-target resolution with the reconnect retry
//! loop, and the model-preference host seams
//! (`packages/cli/src/mini-host.ts`).
//!
//! The frontend itself lives in [`oc_tui::mini`].

use std::io::IsTerminal as _;

use oc_tui::mini;
use oc_tui::mini::model_pref::host_preferences;

use crate::cli::ServerParams;
use crate::run::noninteractive::Format;
use crate::run::{self, RunInput};
use crate::services::server_connection::{self, Mismatch};
use crate::util;

/// `INTERACTIVE_INPUT_ERROR` (`mini-host.ts:9`).
pub const INTERACTIVE_INPUT_ERROR: &str = "opencode mini requires a controlling terminal for input";

/// `input.replay ?? resolved.mini?.replay ?? true`
/// (`handlers/mini.ts:37`): the CLI flag (present as `--replay` /
/// `--no-replay`) wins, then the `mini.replay` cli.json key, then
/// replay defaults ON.
#[must_use]
pub fn resolve_replay(flag: Option<bool>, config: Option<bool>) -> bool {
    flag.or(config).unwrap_or(true)
}

/// `input.replayLimit ?? resolved.mini?.replay_limit`
/// (`handlers/mini.ts:38`): the `--replay-limit` flag wins, then the
/// `mini.replay_limit` cli.json key; no default here (the mini host
/// applies its own).
#[must_use]
pub fn resolve_replay_limit(flag: Option<u32>, config: Option<u32>) -> Option<u32> {
    flag.or(config)
}

/// `MiniCommandInput` (the handler-facing subset, `mini.ts:10-27`).
#[allow(clippy::struct_excessive_bools)] // the v2 flag surface, mirrored
pub struct MiniInput {
    pub continue_: bool,
    pub session: Option<String>,
    pub fork: bool,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub prompt: Option<String>,
    pub replay: bool,
    pub replay_limit: Option<u32>,
    pub demo: bool,
}

/// The red error line + exit 1 (`fail`, `mini.ts:223-225`).
fn fail(message: &str) -> i32 {
    eprintln!("\x1b[91m\x1b[1mError: \x1b[0m{message}");
    1
}

/// `validateMiniTerminal` (`mini.ts:182-184`).
pub fn validate_mini_terminal() -> Result<(), String> {
    if std::io::stdout().is_terminal() {
        Ok(())
    } else {
        Err("opencode mini requires a TTY stdout".to_owned())
    }
}

/// `mergeInput(piped, prompt)` (`mini.ts:187-191`): the PIPE comes
/// first — the opposite order from `run`'s `mergeInput(message,
/// piped)`.
#[must_use]
pub fn merge_input(piped: Option<String>, prompt: Option<String>) -> Option<String> {
    match (prompt, piped) {
        (None, piped) => piped,
        (Some(prompt), None) => Some(prompt),
        (Some(prompt), Some(piped)) => Some(format!("{piped}\n{prompt}")),
    }
}

/// `validate` (`mini.ts:193-199`): the terminal gate first, then the
/// flag rules.
fn validate(input: &MiniInput) -> Result<(), String> {
    validate_mini_terminal()?;
    validate_flags(input)
}

/// The flag rules (`mini.ts:195-198`), independent of the terminal.
fn validate_flags(input: &MiniInput) -> Result<(), String> {
    if let Some(limit) = input.replay_limit {
        if limit == 0 {
            return Err("--replay-limit must be a positive integer".to_owned());
        }
    }
    if input.fork && !input.continue_ && input.session.is_none() {
        return Err("--fork requires --continue or --session".to_owned());
    }
    Ok(())
}

/// `parseModel` (`mini.ts:211-217`): `provider/model[#variant]`, the
/// same shape `parseRunModel` accepts.
fn parse_model(value: Option<&str>) -> Result<Option<(String, String, Option<String>)>, String> {
    match run::parse_run_model(value) {
        Some(parts) => Ok(Some(parts)),
        // None + Some(value) means the format was wrong; None + None
        // means no model was requested.
        None if value.is_some() => {
            Err("--model must use the format provider/model[#variant]".to_owned())
        }
        None => Ok(None),
    }
}

/// `resolveMiniTarget` (`mini.ts:157-180`): resolve the session
/// target, retrying only TRANSPORT-class failures through the
/// reconnect seam with a 250 ms cadence. Ref condition
/// (`mini.ts:168`): `error instanceof ClientError && error.reason ===
/// "Transport"` — every other error (decode, unexpected status, api)
/// is rethrown to the caller instead of entering the retry loop
/// (G-353). Ref retries indefinitely until the abort signal
/// (`Effect.repeat` + `Schedule.spaced(250)`); there is no total
/// deadline — the 30 s cap added in the port had no ref counterpart
/// and aborted slow server starts, so it is removed (G-157).
async fn resolve_mini_target<F, Fut>(
    client: &oc_client::Client,
    input: &RunInput,
    directory: &str,
    reconnect: F,
) -> Result<run::MiniSessionTarget, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<oc_client::Client>>,
{
    resolve_with_retry(
        client.clone(),
        |client| async move { run::resolve_mini_session_target(&client, input, directory).await },
        reconnect,
    )
    .await
    .map_err(|error| error.message)
}

/// The retry core of `resolveMiniTarget`, split out so the reconnect
/// condition is testable without a live server (G-353): each step gets
/// the current client (reconnect swaps it), and only transport-class
/// step errors re-enter the loop.
async fn resolve_with_retry<S, SFut, F, Fut>(
    mut client: oc_client::Client,
    mut step: S,
    mut reconnect: F,
) -> Result<run::MiniSessionTarget, run::ResolveError>
where
    S: FnMut(oc_client::Client) -> SFut,
    SFut: std::future::Future<Output = Result<run::MiniSessionTarget, run::ResolveError>>,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<oc_client::Client>>,
{
    loop {
        match step(client.clone()).await {
            Ok(target) => return Ok(target),
            Err(error) => {
                if !error.transport {
                    // Non-transport failures surface to the caller
                    // (`mini.ts:168` rethrows everything that is not a
                    // Transport `ClientError`).
                    return Err(error);
                }
                let Some(next) = reconnect().await else { return Err(error) };
                client = next;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
    }
}

/// The interactive-stdin seam (`resolveInteractiveStdin`,
/// `mini-host.ts:92-113`): a TTY stdin passes through; a pipe falls
/// back to `/dev/tty` (the controlling terminal still accepts input
/// while stdin carries piped content).
fn resolve_interactive_stdin() -> Result<std::fs::File, String> {
    if std::io::stdin().is_terminal() {
        return std::fs::File::open("/dev/stdin").map_err(|error| error.to_string());
    }
    std::fs::File::open("/dev/tty").map_err(|_| INTERACTIVE_INPUT_ERROR.to_owned())
}

/// `runMini` (`mini.ts:33-135`): validate → interactive stdin → merge
/// piped input with `--prompt` → resolve the target → run the
/// frontend. Returns the process exit code.
#[allow(clippy::too_many_lines)] // the ref's runMini boot walk, ported 1:1
pub async fn run_mini(input: MiniInput, server: &ServerParams) -> i32 {
    if let Err(message) = validate(&input) {
        return fail(&message);
    }
    // The server connection (`commands/handlers/mini.ts:14-18`):
    // resolve with `mismatch: "replace"` like the root TUI launch.
    let resolved = match server_connection::resolve(server_connection::Args {
        server: server.server.clone(),
        standalone: server.standalone,
        mismatch: Mismatch::Replace,
    })
    .await
    {
        Ok(resolved) => resolved,
        Err(message) => return fail(&message),
    };

    // Interactive stdin for the frontend (`usingInteractiveStdin`);
    // the drop closes the /dev/tty handle.
    let _terminal = match resolve_interactive_stdin() {
        Ok(terminal) => terminal,
        Err(message) => return fail(&message),
    };

    // `mergeInput(readStdin(), input.prompt)` (`mini.ts:37`): piped
    // content only exists when stdin is NOT a tty.
    let piped: Option<String> =
        if std::io::stdin().is_terminal() { None } else { util::read_stdin().ok() };
    let initial_input =
        merge_input(piped, input.prompt.clone()).filter(|text| !text.trim().is_empty());

    let requested = match parse_model(input.model.as_deref()) {
        Ok(requested) => requested,
        Err(message) => return fail(&message),
    };
    // The requested model rides through target resolution (the
    // `RunInput.model` field below) — `resolve_mini_session_target`
    // echoes back the model label the footer shows (`mini.ts:43-44` +
    // `:75`).
    let _ = &requested;

    // `localDirectory` (`mini.ts:201-209`): chdir into `$PWD` (or the
    // current directory), failing loudly when the chdir is impossible —
    // a REAL chdir, so relative paths inside the session resolve like
    // ref's (G-157).
    let directory = {
        let root = std::env::var("PWD").unwrap_or_else(|_| {
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default()
        });
        if std::env::set_current_dir(&root).is_err() {
            return fail(&format!("Failed to change directory to {root}"));
        }
        std::env::current_dir().map(|path| path.display().to_string()).unwrap_or(root)
    };

    let client = resolved.endpoint.client();
    let managed = resolved.managed;
    let target_input = RunInput {
        server: resolved,
        message: Vec::new(),
        continue_: input.continue_,
        session: input.session.clone(),
        fork: input.fork,
        model: input.model.clone(),
        agent: input.agent.clone(),
        format: Format::Default,
        file: Vec::new(),
        title: None,
        thinking: false,
        auto: false,
    };
    // The reconnect seam (`handlers/mini.ts:29`): the managed
    // service's `reconnect()` — `Service.ensure` with the version
    // filter dropped (server-connection.ts `managedService`) — which
    // re-resolves the endpoint and hands back a fresh client. A
    // non-managed endpoint (`--server` URL / standalone lease) has no
    // service to reconnect; ref leaves `reconnect` undefined there and
    // the retry loop exits after the first failure (G-157).
    let target = {
        let reconnect = || async {
            if managed {
                server_connection::ensure_managed(false)
                    .await
                    .ok()
                    .map(|endpoint| endpoint.client())
            } else {
                None
            }
        };
        resolve_mini_target(&client, &target_input, &directory, reconnect).await
    };
    let target = match target {
        Ok(target) => target,
        Err(message) => return fail(&message),
    };

    let roots = oc_util::global_roots::resolve();
    let _preferences = host_preferences(&roots.state);
    let frontend = mini::runtime::MiniInput {
        client: client.clone(),
        // G-181④ — the reconnect client factory (`server.reconnect`,
        // handlers/mini.ts:29 → mini.ts:144-153): the managed
        // service's `reconnect()` re-resolves the endpoint and hands
        // back a fresh client; a non-managed endpoint has none (the
        // pump keeps retrying the same client).
        reconnect: managed.then(|| {
            std::sync::Arc::new(|| {
                Box::pin(async {
                    server_connection::ensure_managed(false)
                        .await
                        .ok()
                        .map(|endpoint| endpoint.client())
                }) as futures::future::BoxFuture<'static, Option<oc_client::Client>>
            }) as mini::runtime::ReconnectFactory
        }),
        target: mini::runtime::ResolvedSession {
            session_id: target.session_id,
            session_title: target.session_title,
            directory: target.directory,
            model_label: target.model_label,
            agent: target.agent,
            resume: target.resume,
        },
        initial_input,
        replay: input.replay,
        replay_limit: input.replay_limit.map(|limit| limit as usize),
        demo: input.demo,
        version: util::VERSION.to_owned(),
        home: roots.home.display().to_string(),
        state_dir: roots.state,
        // CONFIG-T4: the runtime resolves the cli.json `mini` section
        // itself (v2's `resolveRunTuiConfig` default path,
        // runtime.ts:197-199).
        settings: None,
    };
    // `runtime::run`'s future is large; box it (clippy::large_futures).
    match Box::pin(mini::runtime::run(frontend)).await {
        Ok(code) => code,
        Err(message) => fail(&message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_input_puts_the_pipe_first() {
        // `mini.ts:187-191` — piped content precedes the prompt.
        assert_eq!(
            merge_input(Some("piped".into()), Some("prompt".into())).as_deref(),
            Some("piped\nprompt")
        );
        assert_eq!(merge_input(Some("piped".into()), None).as_deref(), Some("piped"));
        assert_eq!(merge_input(None, Some("prompt".into())).as_deref(), Some("prompt"));
        assert_eq!(merge_input(None, None), None);
    }

    /// The `mini.replay`/`mini.replay_limit` cli.json fallbacks
    /// (auditF 1.2 #5): flag > config > default.
    #[test]
    fn replay_resolves_flag_config_default() {
        // replay: flag ?? config ?? true
        assert!(resolve_replay(None, None), "default on");
        assert!(!resolve_replay(Some(false), None), "--no-replay wins");
        assert!(resolve_replay(Some(true), None), "--replay wins");
        assert!(!resolve_replay(None, Some(false)), "config off applies");
        assert!(resolve_replay(None, Some(true)), "config on applies");
        assert!(!resolve_replay(Some(false), Some(true)), "flag beats config");
        assert!(resolve_replay(Some(true), Some(false)), "flag beats config");

        // replay_limit: flag ?? config (no default here)
        assert_eq!(resolve_replay_limit(None, None), None);
        assert_eq!(resolve_replay_limit(Some(10), None), Some(10));
        assert_eq!(resolve_replay_limit(None, Some(50)), Some(50));
        assert_eq!(resolve_replay_limit(Some(10), Some(50)), Some(10));
    }

    #[test]
    fn fork_requires_continue_or_session() {
        let mut input = MiniInput {
            continue_: false,
            session: None,
            fork: true,
            model: None,
            agent: None,
            prompt: None,
            replay: true,
            replay_limit: None,
            demo: false,
        };
        assert_eq!(validate_flags(&input).unwrap_err(), "--fork requires --continue or --session");
        input.continue_ = true;
        assert!(validate_flags(&input).is_ok());
        input.continue_ = false;
        input.session = Some("ses_1".into());
        assert!(validate_flags(&input).is_ok());
    }

    #[test]
    fn replay_limit_must_be_positive() {
        let input = MiniInput {
            continue_: false,
            session: None,
            fork: false,
            model: None,
            agent: None,
            prompt: None,
            replay: true,
            replay_limit: Some(0),
            demo: false,
        };
        assert_eq!(
            validate_flags(&input).unwrap_err(),
            "--replay-limit must be a positive integer"
        );
    }

    #[test]
    fn model_format_errors_carry_the_reference_message() {
        assert_eq!(
            parse_model(Some("nonsense")).unwrap_err(),
            "--model must use the format provider/model[#variant]"
        );
        assert_eq!(
            parse_model(Some("a/b")).unwrap(),
            Some(("a".to_string(), "b".to_string(), None))
        );
        assert_eq!(
            parse_model(Some("a/b#turbo")).unwrap().map(|(_, _, variant)| variant),
            Some(Some("turbo".to_string()))
        );
        assert!(parse_model(None).unwrap().is_none());
    }

    /// A client pointed at an unreachable loopback endpoint; the retry
    /// seam tests never contact it (steps are canned).
    fn dummy_client() -> oc_client::Client {
        oc_client::Client::new("http://127.0.0.1:9").expect("valid base url")
    }

    fn transport_error() -> run::ResolveError {
        run::ResolveError { message: "transport: connection refused".into(), transport: true }
    }

    fn decode_error() -> run::ResolveError {
        run::ResolveError { message: "malformed response: not json".into(), transport: false }
    }

    fn ok_target() -> run::MiniSessionTarget {
        run::MiniSessionTarget { session_id: "ses_1".into(), ..Default::default() }
    }

    /// G-353 — a transport-class failure re-enters the loop through the
    /// reconnect seam (ref `mini.ts:168`: `ClientError` with
    /// `reason === "Transport"` is the only retryable class) and the
    /// next successful step resolves.
    #[tokio::test]
    async fn transport_errors_reconnect_until_resolution() {
        let steps = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let step_client = steps.clone();
        let target = resolve_with_retry(
            dummy_client(),
            move |_client| {
                let count = step_client.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    if count == 0 {
                        Err(transport_error())
                    } else {
                        Ok(ok_target())
                    }
                }
            },
            || async { Some(dummy_client()) },
        )
        .await
        .expect("second step resolves");
        assert_eq!(target.session_id, "ses_1");
        assert_eq!(steps.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    /// G-353 — a non-transport failure (decode/protocol/api) fails out
    /// with the error surfaced; the reconnect seam is never invoked.
    #[tokio::test]
    async fn non_transport_errors_fail_out_without_reconnecting() {
        let reconnected = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = reconnected.clone();
        let error = resolve_with_retry(
            dummy_client(),
            |_client| async { Err(decode_error()) },
            move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Some(dummy_client()) }
            },
        )
        .await
        .expect_err("decode errors are not retryable");
        assert_eq!(error.message, "malformed response: not json");
        assert!(!error.transport);
        assert_eq!(reconnected.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// G-353 — a transport failure with no reconnect seam available
    /// (explicit `--server` target) surfaces the error instead of
    /// looping.
    #[tokio::test]
    async fn transport_error_without_reconnect_seam_surfaces() {
        let error = resolve_with_retry(
            dummy_client(),
            |_client| async { Err(transport_error()) },
            || async { None },
        )
        .await
        .expect_err("no seam means surfaced");
        assert!(error.transport);
        assert_eq!(error.message, "transport: connection refused");
    }
}
