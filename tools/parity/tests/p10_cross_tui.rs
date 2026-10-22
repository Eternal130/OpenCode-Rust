//! Phase-10 cross-replacement (a): the **real v2.0.24 TUI** (bun, tmux —
//! the m1-smoke pattern) as a client of the **Rust server**, driving THREE
//! interactions end-to-end:
//!
//! 1. **prompt + reply** — type a prompt, the config-pinned `FakeLlm` model
//!    answers with deterministic text; the reply must render in the
//!    transcript (the TS client's prompt/inbox/message pipeline against the
//!    Rust server's wire contract).
//! 2. **session rename** — `ctrl+r` (the v2 `session.rename` binding) →
//!    the "Rename session" dialog → type a title → Enter; the TUI sends the
//!    `session.update`, and the Rust server's session row must show the
//!    new title (asserted through the API, not the pane).
//! 3. **permission once** — a second prompt makes the fake emit a `read`
//!    tool call on a file OUTSIDE the project → the Rust server parks it
//!    (`permission.asked`) → the TUI renders the permission footer
//!    ("Allow once") → Enter → the TUI POSTs the `once` reply itself →
//!    the parked tool proceeds and the final reply renders.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference
//! checkout + tmux):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_cross_tui)'
//! ```
//!
//! Artifacts: pane captures in `/tmp/opencode/p10-artifacts/` + evidence
//! copies in `tools/parity/goldens/p10/tui/`. Never binds 49374.

mod p10_common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use p10_common::{enabled, read_tool_script, skip, text_script, ScriptedFakeLlm};
use parity::scenario::ScenarioHost;

const SESSION: &str = "p10tui";
const W: &str = "160";
const H: &str = "40";

/// The tmux server socket + conf for this test's session.
///
/// A DEDICATED socket (the `termatrix::wrap_argv_for_tmux` pattern), never
/// the per-uid default: the whole pane lifecycle — including its death —
/// stays owned by this test. The conf pins `remain-on-exit on`, which is
/// the load-bearing half of the 2026-10-22 bun-1.4.2 CI incident: when
/// the reference TUI process exits at boot, a stock tmux destroys the
/// pane → the last session → the SERVER, and every subsequent
/// `capture-pane` fails with "no server running on /tmp/tmux-1001" —
/// the TUI's stderr (the only copy of the real error) died with the
/// pane. With remain-on-exit the dead pane KEEPS its final screen, the
/// session and server survive, and the harness can read both the exit
/// status and the dying message. Verified locally: bun 1.4.2 + opentui
/// boots the v2.0.24 TUI against the Rust server on this box (clean
/// cache, uid 1000, real tmux) — the runner-only instant death needs
/// the runner's own words, which this rig now captures.
#[derive(Clone)]
struct TmuxRig {
    /// `-S` socket path (inside the test scratch root).
    socket: PathBuf,
    /// `-f` conf path (`set -g remain-on-exit on`).
    conf: PathBuf,
    /// `pipe-pane` target — every byte the pane emits, tee'd live.
    pane_log: PathBuf,
}

fn tmux(rig: &TmuxRig, args: &[&str]) -> String {
    let socket = rig.socket.display().to_string();
    let mut words: Vec<&str> = vec!["-S", socket.as_str()];
    words.extend_from_slice(args);
    let mut command = Command::new("tmux");
    close_inherited_fds(&mut command);
    match run_bounded(command.args(&words), 10) {
        Ok(output) => {
            if !output.status.success() {
                eprintln!(
                    "[p10-tui] tmux {:?} failed: {}{}",
                    args,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Err(error) => {
            eprintln!("[p10-tui] tmux {args:?} did not finish in time: {error}");
            String::new()
        }
    }
}

/// Mark every spawned tmux as fd-clean before exec: a forked tmux SERVER
/// inherits the client's descriptors wholesale — including tools/dev-gate.sh's
/// machine-wide lock fd (`exec 9> /tmp/opencode/dev-gate.lock`), which bash
/// cannot mark close-on-exec. A leaked server therefore BLOCKS EVERY LATER
/// GATE on the box (observed 2026-10-22: two p10tui servers held the lock for
/// 10+ minutes until reaped). Closing fds ≥ 3 between fork and exec is
/// async-signal-safe and touches nothing these commands need (they keep
/// stdin/stdout/stderr only).
#[allow(unsafe_code)]
fn close_inherited_fds(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            for fd in 3..=256 {
                libc::close(fd);
            }
            Ok(())
        });
    }
}

/// Run a command to completion with a hard deadline: a wedged child (a tmux
/// client blocked on an unresponsive server) fails fast instead of hanging
/// the test past nextest's slow-timeout with no diagnostics.
fn run_bounded(command: &mut Command, secs: u64) -> std::io::Result<std::process::Output> {
    use std::io::ErrorKind;
    use std::process::Stdio;
    // wait_with_output only collects what is piped — with default (inherited)
    // stdio the child's output would leak into the TEST's stdout and every
    // caller would see an empty capture.
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait()? {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    format!("command did not finish within {secs}s (killed)"),
                ));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    child.wait_with_output()
}

/// `tmux -S <sock> -f <conf> <args…>` (the conf is only read when the
/// rig's server BOOTS — i.e. after every teardown — so it always applies).
fn tmux_with_conf(rig: &TmuxRig, args: &[&str]) -> std::process::Output {
    let mut command = Command::new("tmux");
    close_inherited_fds(&mut command);
    command
        .arg("-S")
        .arg(rig.socket.display().to_string())
        .arg("-f")
        .arg(rig.conf.display().to_string())
        .args(args);
    match run_bounded(&mut command, 15) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("[p10-tui] tmux {args:?} (with conf) did not finish in time: {error}");
            std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        }
    }
}

/// Tear the rig's whole server down (sessions + panes + the pipe-pane
/// `cat`). Own-socket only — never touches the per-uid default server.
fn tmux_kill_server(rig: &TmuxRig) {
    let mut command = Command::new("tmux");
    close_inherited_fds(&mut command);
    command
        .arg("-S")
        .arg(rig.socket.display().to_string())
        .arg("kill-server");
    if let Err(error) = run_bounded(&mut command, 8) {
        eprintln!("[p10-tui] kill-server did not finish in time: {error}");
    }
}

fn key(rig: &TmuxRig, name: &str) {
    tmux(rig, &["send-keys", "-t", SESSION, name]);
}

fn type_text(rig: &TmuxRig, text: &str) {
    tmux(rig, &["send-keys", "-t", SESSION, "-l", text]);
}

fn capture(rig: &TmuxRig) -> String {
    tmux(rig, &["capture-pane", "-p", "-t", SESSION])
}

/// `Some(exit)` when the pane's process has exited (remain-on-exit keeps
/// the pane readable afterwards), `None` while it runs.
fn pane_exit(rig: &TmuxRig) -> Option<String> {
    let out = tmux(
        rig,
        &["display-message", "-p", "-t", SESSION, "#{pane_dead} #{pane_dead_status}"],
    );
    let mut fields = out.split_whitespace();
    match (fields.next(), fields.next()) {
        (Some("1"), status) => Some(status.unwrap_or("?").to_owned()),
        // The session itself is gone (server died too — e.g. an external
        // kill): indistinguishable from a boot crash without a status.
        (None, _) if out.contains("no server running") || out.contains("can't find") => {
            Some("server-gone".to_owned())
        }
        _ => None,
    }
}

/// The pane's tail + the pipe-pane log tail — everything the TUI said
/// before dying, for the failure message and the artifacts.
fn death_diagnostics(rig: &TmuxRig) -> String {
    let pane = capture(rig);
    let log = std::fs::read(&rig.pane_log)
        .map_or_else(
            |error| format!("<no pane log: {error}>"),
            |bytes| String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(4000)..]).into_owned(),
        );
    format!("pane tail:\n{pane}\npane log tail:\n{log}")
}

async fn sleep(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

/// Poll captures until one contains `needle`; saves `artifact` on success
/// or the tail capture into the error.
async fn wait_capture(
    rig: &TmuxRig,
    needle: &str,
    tries: usize,
    secs: u64,
    artifact: &str,
) -> Result<String> {
    let mut last = String::new();
    for _ in 0..tries {
        sleep(secs).await;
        last = capture(rig);
        if last.contains(needle) {
            std::fs::write(p10_common::artifacts().join(artifact), &last)
                .context("write artifact")?;
            return Ok(last);
        }
    }
    Err(anyhow!("timed out waiting for {needle:?} in the pane:\n{last}"))
}

/// Wait for the connected home screen, treating a pre-first-frame pane
/// death as an IMMEDIATE failure that carries the TUI's own words: the
/// exit status (remain-on-exit keeps the pane queryable), the captured
/// pane and the pipe-pane log (both mirrored into the artifact dir).
/// Never burns the full poll budget on a process that is already dead.
///
/// The connected marker is the CONFIG-LOADED footer (`parityfake` — the
/// provider this test seeds into the project config, visible only once
/// the TUI has talked the full config pipeline through the server) or
/// the composer placeholder. The pre-2026-10-22 needle was the literal
/// "Server" — which only matched the version-mismatch WARNING the old
/// broken `--define` quoting produced (`this client is local`); with
/// the defines fixed, client and server versions match and the warning
/// is gone while the UI is properly up.
async fn wait_boot(rig: &TmuxRig, tries: usize) -> Result<String> {
    let mut last = String::new();
    for attempt in 0..tries {
        sleep(2).await;
        if let Some(exit) = pane_exit(rig) {
            // Give the pipe-pane `cat` a beat to flush the tail, then
            // preserve everything this run will ever know about the death.
            sleep(1).await;
            let diagnostics = death_diagnostics(rig);
            let _ = std::fs::write(p10_common::artifacts().join("tui-00-boot-death.txt"), &diagnostics);
            return Err(anyhow!(
                "the reference TUI exited before rendering its first frame \
                 (pane exit status {exit}; attempt {}/{tries}). Its final output:\n{diagnostics}",
                attempt + 1,
            ));
        }
        last = capture(rig);
        if last.contains("parityfake") || last.contains("Ask anything") {
            std::fs::write(p10_common::artifacts().join("tui-01-boot.txt"), &last)
                .context("write artifact")?;
            return Ok(last);
        }
    }
    Err(anyhow!(
        "the reference TUI never rendered the connected home screen \
         (no pane death, no provider footer/composer marker in {tries} polls). Last pane:\n{last}"
    ))
}

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs tmux + reference checkout)"]
async fn p10_ts_tui_against_rust_server() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if Command::new("tmux").arg("-V").output().is_err() {
        skip("tmux not installed");
        return Ok(());
    }
    let tui_root = std::path::Path::new(
        &std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");
    if !tui_root.is_dir()
        && parity::gate::skip_or_fail(&format!(
            "reference TUI checkout missing at {} (set PARITY_REF_ROOT or rebuild \
             /tmp/opencode/ref)",
            tui_root.display()
        ))
    {
        return Ok(());
    }
    if p10_common::bun_path().is_err()
        && parity::gate::skip_or_fail("bun not found (set PARITY_BUN)")
    {
        return Ok(());
    }
    let bun = p10_common::bun_path().expect("bun path checked above");
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }

    // Known-red wrapper (B0-2, §8.5): the rename interaction is a
    // registered red (GATE-TUI-RENAME, expiry Batch 1B) — see
    // docs/parity-baseline.md §2.1. Categorized, never skipped.
    if let Err(error) = run_against_rust_server(&bun).await {
        return Err(p10_common::known_red("GATE-TUI-RENAME", "Batch 1B").categorize(error));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
// The RefTuiGuard struct/impls stay local to this helper (they close over
// its sandbox markers); items-after-statements is intentional here.
#[allow(clippy::items_after_statements)]
async fn run_against_rust_server(bun: &std::path::Path) -> Result<()> {
    let tui_root = std::path::PathBuf::from(
        std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");

    // -- scratch layout -----------------------------------------------------
    let root = PathBuf::from(format!("/tmp/opencode/p10-tui-{}", std::process::id()));
    let project = root.join("project");
    let outside = root.join("outside");
    let tui_home = root.join("tui-home");
    for dir in [
        project.clone(),
        outside.clone(),
        tui_home.join(".local/share"),
        tui_home.join(".local/state"),
        tui_home.join(".config"),
        tui_home.join(".cache"),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    let secret = outside.join("secret.txt");
    std::fs::write(&secret, "p10-secret\n")?;
    std::fs::write(project.join("notes.md"), "p10 tui notes\n")?;

    // -- the scripted fake + the Rust server --------------------------------
    let fake = ScriptedFakeLlm::start(vec![
        // interaction 1: plain reply
        text_script("Parity fake says hello."),
        // interaction 3: read tool on the OUTSIDE file -> permission ask
        read_tool_script("call_p10_read_out", &secret.display().to_string()),
        // interaction 3 continued: final reply after the tool result
        text_script("The outside file says p10-secret."),
    ])
    .await?;
    let config = p10_common::provider_config(&fake.base(), false);
    let mut options = parity::rust_server::RustSpawn::default();
    options.seed_files.push((PathBuf::from("project/opencode.json"), config));
    options.extra_env.push(("OPENCODE_CONFIG", "{sandbox}/project/opencode.json"));
    let server = parity::rust_server::spawn_rust(options).await?;
    eprintln!("[p10-tui] rust server ready at {}", server.url());
    let url = server.url().to_owned();
    let password = server.password().to_owned();
    let config_path = server.sandbox().join("project/opencode.json").display().to_string();

    // -- boot the official v2.0.24 TUI against the Rust server -------------
    //
    // The rig: a dedicated tmux server (own socket, own conf) whose panes
    // `remain-on-exit` — a dead TUI stays readable instead of taking the
    // tmux server with it — and whose every output byte is tee'd live
    // into `<root>/tui-pane.log` by pipe-pane. One bounded relaunch: the
    // boot is retry-safe (the TUI is stateless before its first frame;
    // nothing else in the test has touched it yet), and a second attempt
    // distinguishes a transient boot death from a deterministic one while
    // the diagnostics do the explaining either way.
    let rig = TmuxRig {
        socket: root.join("p10tui.sock"),
        conf: root.join("tmux-p10tui.conf"),
        pane_log: root.join("tui-pane.log"),
    };
    std::fs::write(&rig.conf, "set -g remain-on-exit on\n")
        .context("write the tmux conf")?;
    // The defines are JSON-QUOTED and single-quoted for the pane shell:
    // bun 1.4.x parses every `--define KEY=VALUE` VALUE as JSON, so the
    // double quotes must SURVIVE the pane's `sh -c` (a bare 2.0.24 makes
    // bun exit instantly with `error: Unexpected 2.0.24 at
    // defines.json:1:1` — the 2026-10-22 CI red; bun 1.3.10 took the
    // bare form but parsed it as the NUMBER 2). The single quotes hand
    // bun exactly `OPENCODE_VERSION="2.0.24"` — the same argv the direct
    // spawn in `reference.rs` passes, and the form `tui_diff::REF_DEFINES`
    // already pins.
    let launch = format!(
        "cd {cwd} && env -i PATH={bunbin}:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color \
         HOME={home} OPENCODE_TEST_HOME={home} XDG_DATA_HOME={data} XDG_STATE_HOME={state} \
         XDG_CONFIG_HOME={conf} XDG_CACHE_HOME={cache} TMPDIR={tmp} \
         OPENCODE_PASSWORD={password} OPENCODE_CONFIG={config} \
         {bun} run --cwd {cli} --define OPENCODE_VERSION='\"2.0.24\"' \
         --define OPENCODE_CHANNEL='\"stable\"' --define OPENCODE_ARTIFACT='\"cli\"' \
         src/index.ts --server {url}",
        cwd = project.display(),
        bunbin = bun.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        bun = bun.display(),
        home = tui_home.display(),
        data = tui_home.join(".local/share").display(),
        state = tui_home.join(".local/state").display(),
        conf = tui_home.join(".config").display(),
        cache = tui_home.join(".cache").display(),
        tmp = tui_home.join(".cache").display(),
        password = password,
        config = config_path,
        cli = tui_root.join("../cli").display(),
        url = url,
    );
    let mut boot = None;
    for attempt in 1..=2 {
        tmux_kill_server(&rig);
        let status = tmux_with_conf(
            &rig,
            &["new-session", "-d", "-s", SESSION, "-x", W, "-y", H, &launch],
        );
        if !status.status.success() {
            // The session may half-exist; tear the server down before
            // failing (remain-on-exit would otherwise keep it — and its
            // inherited fds — alive past the test: see close_inherited_fds).
            tmux_kill_server(&rig);
            anyhow::bail!("tmux new-session failed (attempt {attempt}): {status:?}");
        }
        // Tee every pane byte from the very first frame: the log survives
        // even a total tmux-server death (the file is ours, not tmux's).
        let _ = tmux(&rig, &["pipe-pane", "-t", SESSION, &format!("cat >> {}", rig.pane_log.display())]);
        match wait_boot(&rig, 15).await {
            Ok(pane) => {
                if attempt > 1 {
                    eprintln!("[p10-tui] boot succeeded on relaunch attempt {attempt}");
                }
                boot = Some(pane);
                break;
            }
            Err(error) if attempt < 2 => {
                eprintln!("[p10-tui] boot attempt {attempt} failed; relaunching once: {error:#}");
            }
            Err(error) => {
                // The RefTuiGuard does not exist yet on the boot-failure
                // path — stop the server HERE or it outlives the test
                // (remain-on-exit keeps it alive by design; the 2026-10-22
                // local runs leaked two such servers, each pinning the
                // dev-gate lock fd).
                tmux_kill_server(&rig);
                return Err(error.context(format!("the reference TUI never booted (attempt {attempt})")));
            }
        }
    }
    let boot = boot.expect("boot loop returns Some only via break after Ok");

    // Orphan-proofing: record the pane PID and keep the guard alive for the
    // whole test scope — its Drop stops this test's tmux SERVER (dedicated
    // socket: teardown can never touch another lane's panes) AND
    // identity-verifies + kills any surviving descendant even when this
    // test panics or times out.
    let pane_pid = Command::new("tmux")
        .arg("-S")
        .arg(rig.socket.display().to_string())
        .args(["list-panes", "-t", SESSION, "-F", "#{pane_pid}"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().ok());
    if let Some(pid) = pane_pid {
        let _ = std::fs::write("/tmp/opencode/p10-cross-tui-panepid", pid.to_string());
    }
    let cli_dir = tui_root.join("../cli");
    let _ref_tui_guard = RefTuiGuard::new(&url, &cli_dir, rig.clone());

    anyhow::ensure!(
        !boot.to_lowercase().contains("reconnect"),
        "the TUI shows a reconnect banner (not connected):\n{boot}"
    );
    // Dismiss any first-run modal.
    key(&rig, "Escape");
    sleep(1).await;

    // -- interaction 1: prompt + reply --------------------------------------
    // Key delivery can race the first-run wizard's focus: when a modal
    // owns the input the typed keys vanish and the 90s reply wait times
    // out on an empty prompt box. Only send Enter once the prompt has
    // actually ECHOED to the pane; if it has not, a modal still has
    // focus — dismiss it and retype (bounded). No assertion is relaxed;
    // the transcript check below still requires the prompt.
    let mut echoed = false;
    for _ in 0..5 {
        type_text(&rig, "Say hello.");
        sleep(1).await;
        if capture(&rig).contains("Say hello.") {
            echoed = true;
            break;
        }
        key(&rig, "Escape");
        sleep(1).await;
    }
    anyhow::ensure!(echoed, "the prompt never echoed to the pane (input focus lost)");
    key(&rig, "Enter");
    let reply = wait_capture(&rig, "Parity fake says hello.", 30, 3, "tui-02-reply.txt").await?;
    anyhow::ensure!(
        reply.contains("Say hello."),
        "the typed prompt is missing from the transcript:\n{reply}"
    );
    eprintln!("[p10-tui] interaction 1 PASS — prompt + fake reply in transcript");

    let session_id =
        p10_common::new_session_id(&url, &password, &[]).await.context("find the TUI session")?;
    eprintln!("[p10-tui] session = {session_id}");

    // -- interaction 2: session rename (ctrl+r) ------------------------------
    key(&rig, "C-r");
    let dialog = wait_capture(&rig, "Rename session", 10, 1, "tui-03-rename-dialog.txt").await?;
    anyhow::ensure!(
        dialog.contains("Session title") || dialog.contains("Rename session"),
        "the rename dialog did not render:\n{dialog}"
    );
    // The v2 dialog PREFILLS the input with the current title and selects
    // nothing (dialog-session-rename.tsx passes value={currentTitle};
    // onMount only does gotoLineEnd). The opentui textarea maps C-u to
    // delete-to-line-start, so — with the caret parked at the line end —
    // one C-u wipes the prefill instead of appending to it.
    sleep(1).await;
    key(&rig, "C-u");
    sleep(1).await;
    type_text(&rig, "p10 renamed from tui");
    sleep(1).await;
    key(&rig, "Enter");
    let renamed = p10_common::wait_api(
        &url,
        &password,
        "GET",
        &format!("/api/session/{session_id}"),
        None,
        |value| {
            value
                .pointer("/data/title")
                .and_then(serde_json::Value::as_str)
                // ends_with is belt-and-braces: if the C-u ever races the
                // dialog's textarea focus, the untouched prefill merely
                // prefixes the typed title (the server PATCHes the
                // concatenated text verbatim — that is the v2 behavior).
                .is_some_and(|title| title.ends_with("p10 renamed from tui"))
        },
        10,
        1,
    )
    .await
    .context("the rename never reached the server's session row")?;
    std::fs::write(
        p10_common::artifacts().join("tui-04-rename-server-row.json"),
        serde_json::to_string_pretty(&renamed)?,
    )?;
    eprintln!("[p10-tui] interaction 2 PASS — rename PATCH reached the Rust server row");

    // -- interaction 3: permission once --------------------------------------
    type_text(&rig, "Now read the outside file please.");
    sleep(1).await;
    key(&rig, "Enter");
    // The Rust server parks the external read; the TUI renders the footer.
    let ask = wait_capture(&rig, "Allow once", 30, 3, "tui-05-permission-ask.txt").await?;
    anyhow::ensure!(
        ask.contains("Allow once"),
        "the permission footer did not render the ask:\n{ask}"
    );
    // Also verify the ask is real on the server side.
    let asks = p10_common::wait_api(
        &url,
        &password,
        "GET",
        &format!("/api/session/{session_id}/permission"),
        None,
        |value| {
            value.get("data").and_then(serde_json::Value::as_array).is_some_and(|a| !a.is_empty())
        },
        10,
        1,
    )
    .await
    .context("no permission ask appeared on the server")?;
    std::fs::write(
        p10_common::artifacts().join("tui-05-permission-asks.json"),
        serde_json::to_string_pretty(&asks)?,
    )?;
    // Enter = "Allow once" (the default option) — the TUI POSTs the reply.
    key(&rig, "Enter");
    let reply2 = wait_capture(&rig, "The outside file says p10-secret.", 30, 3, "tui-06-tool-reply.txt")
        .await
        .context("the parked tool never proceeded after the TUI's once reply")?;
    anyhow::ensure!(
        !reply2.contains("Allow once"),
        "the permission footer did not resolve:\n{reply2}"
    );
    eprintln!(
        "[p10-tui] interaction 3 PASS — permission once replied through the TUI, tool proceeded"
    );

    // -- orphan-proof TUI session guard (2026-10-19 incident) ------------------
    //
    // The reference TUI used to leak when the test panicked or timed out: the
    // best-effort `tmux kill-session` at the end never ran, and even when it
    // did, `bun run` re-execs a nested bun that survives the pane's death.
    // Four such orphans ran for 15+ hours and contributed to the systemd-oomd
    // incident that killed the user session + system services.
    //
    // The guard stops this test's whole tmux server on Drop (panic/timeout
    // included) and then verifies-and-kills any surviving descendant of the
    // tmux pane by EXACT PID with a cmdline identity check (this test's
    // sandbox markers) — no name-based matching, nothing we did not spawn
    // ourselves.
    struct RefTuiGuard {
        rig: TmuxRig,
        markers: Vec<String>,
    }

    impl RefTuiGuard {
        fn new(server_url: &str, cli_dir: &std::path::Path, rig: TmuxRig) -> Self {
            RefTuiGuard {
                rig,
                markers: vec![
                    "OPENCODE_ARTIFACT=\"cli\"".to_owned(),
                    format!("--server {server_url}"),
                    format!("--cwd {}", cli_dir.display()),
                ],
            }
        }

        fn pane_descendants(pane_pid: u32) -> Vec<u32> {
            // Walk /proc children transitively from the tmux pane process.
            let mut found = Vec::new();
            let mut frontier = vec![pane_pid];
            while let Some(pid) = frontier.pop() {
                if let Ok(children) =
                    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
                {
                    for tok in children.split_whitespace() {
                        if let Ok(child) = tok.parse::<u32>() {
                            if !found.contains(&child) {
                                found.push(child);
                                frontier.push(child);
                            }
                        }
                    }
                }
            }
            found
        }

        fn stop(&self) {
            // 1) Tear this test's whole tmux server down (own socket: the
            //    sessions, the panes — including a remain-on-exit dead pane
            //    — and the pipe-pane cat all go together; another lane's
            //    default-socket server is never touched).
            tmux_kill_server(&self.rig);
            // 2) The pane PID: capture it from tmux BEFORE killing (see `boot`),
            //    stored in the lock file by the launcher below.
            if let Ok(text) = std::fs::read_to_string("/tmp/opencode/p10-cross-tui-panepid") {
                if let Ok(pane_pid) = text.trim().parse::<u32>() {
                    for attempt in 0..2 {
                        let victims: Vec<u32> = std::iter::once(pane_pid)
                            .chain(Self::pane_descendants(pane_pid))
                            .filter(|pid| self.cmdline_matches(*pid))
                            .collect();
                        if victims.is_empty() {
                            return;
                        }
                        for pid in &victims {
                            // Precise-PID, identity-verified signal: own test
                            // children only. TERM first, KILL on the 2nd pass.
                            #[allow(unsafe_code)]
                            unsafe {
                                let _ = libc::kill(
                                    i32::try_from(*pid)
                                        .expect("/proc pid fits i32 (kernel pid_max caps at 2^22)"),
                                    if attempt == 0 { libc::SIGTERM } else { libc::SIGKILL },
                                );
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_secs(3));
                    }
                }
            }
        }

        fn cmdline_matches(&self, pid: u32) -> bool {
            let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else { return false };
            let cmd = String::from_utf8_lossy(&raw);
            self.markers.iter().all(|m| cmd.contains(m.as_str()))
        }
    }

    impl Drop for RefTuiGuard {
        fn drop(&mut self) {
            self.stop();
        }
    }
    let goldens = p10_common::golden_dir().join("tui");
    std::fs::create_dir_all(&goldens)?;
    for entry in std::fs::read_dir(p10_common::artifacts())? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("tui-") && entry.path().is_file() {
            let _ = std::fs::copy(entry.path(), goldens.join(entry.file_name()));
        }
    }
    // Stop the rig BEFORE the scratch removal below unlinks its socket: a
    // kill-server after the unlink cannot connect ("No such file or
    // directory") and the server — with remain-on-exit keeping it alive
    // and its inherited fds pinned — outlives the test (leaked 2026-10-22;
    // the RefTuiGuard's Drop runs after this function's explicit cleanup,
    // so the guard alone cannot close this window).
    tmux_kill_server(&rig);
    let _ = std::fs::remove_dir_all(&root);
    server.stop().await?;
    eprintln!(
        "[p10-tui] PASS — captures in {} + goldens/p10/tui/",
        p10_common::artifacts().display()
    );
    Ok(())
}
