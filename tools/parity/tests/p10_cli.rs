//! Gate 5 — CLI differential (PARITY-LOOP.md §8.3 Gate 5, B0-2 skeleton).
//!
//! Dual-end CLI compare: the Rust port's `opencode-rust` binary against
//! the official v2.0.24 CLI (bun, `--define`-pinned). Both run with
//! scrubbed environments, private lane homes, `--server` pointed at their
//! own side's server, and `OPENCODE_PASSWORD` set — the managed-service
//! default port is never touched (a bare `auth`/`models` would try to
//! bootstrap one; §2.1 forbids that).
//!
//! Live today:
//! - `--version` — exit 0, single `name vX.Y.Z` line, and the version
//!   number itself equal on both sides;
//! - `--help` — exit 0, non-empty, usage marker (clap vs commander help
//!   ENGINES differ; full-text parity is a recorded gap);
//! - `serve --hostname localhost --port <free>` — boot, `GET /api/info`
//!   200, clean shutdown (own process group only);
//! - `auth list` (non-TTY) — byte-equal "No authenticated integrations";
//! - `models` (non-TTY) — byte-equal model lists;
//! - `session list` — project-scoped newest-first ordering + `-n` limit
//!   via `--format json` (full-JSON equality is a recorded gap: the port
//!   serializes epoch fields as floats);
//! - `run --format json` — exit 0, JSON lines, exactly one `text` part
//!   with the deterministic fake reply (event-coverage parity is a
//!   recorded gap: the port emits `step_start`/`text`/`step_finish`, v2
//!   emits the `text` event only);
//! - `run -m nope/nada` early error — exit 1 + `Model unavailable` on
//!   ANSI-stripped stderr, both sides;
//! - `--print-logs` accepted (exit 0) with v2 emitting `level=` log lines
//!   on stderr (port log-line parity is a recorded gap).
//!
//! TTY-vs-non-TTY rendering branches for `auth`/`models` and the
//! human-table `session list` format are structured gaps (checkpoints in
//! the artifact).
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_cli)'
//! ```
//!
//! Artifact: `/tmp/opencode/p10-artifacts/gate5-cli.json`.

mod p10_common;

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use p10_common::{enabled, skip, ScriptedFakeLlm};
use parity::scenario::ScenarioHost;

/// Which CLI to drive.
#[derive(Clone, Copy)]
struct CliSide {
    label: &'static str,
}

impl std::fmt::Display for CliSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// One CLI invocation result.
struct CliRun {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

/// Strip ANSI escapes (error banners carry color even when piped).
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if let Some('[') = chars.next() {
                for inner in chars.by_ref() {
                    if inner.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Run a command to completion with a hard timeout (own child only).
/// Async on purpose: the sleeps must yield the `current_thread` test
/// runtime so in-process fakes keep serving while the CLI runs.
async fn run_timed(command: &mut Command, secs: u64) -> Result<CliRun> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn cli")?;
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            return Ok(CliRun {
                code: status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                timed_out: false,
            });
        }
        if Instant::now() > deadline {
            let pid = child.id();
            for file in ["status", "wchan"] {
                if let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/{file}")) {
                    eprintln!("[gate5] /proc/{pid}/{file}: {}", text.trim());
                }
            }
            if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
                let fds: Vec<String> = entries
                    .flatten()
                    .map(|entry| {
                        let path =
                            format!("/proc/{pid}/fd/{}", entry.file_name().to_string_lossy());
                        format!(
                            "{} -> {}",
                            entry.file_name().to_string_lossy(),
                            std::fs::read_link(&path)
                                .map(|target| target.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        )
                    })
                    .collect();
                eprintln!("[gate5] /proc/{pid}/fd: {fds:?}");
                // The CLI's own log file (fd pointing under the lane home's
                // opencode log dir) says what it is waiting for.
                for fd in &fds {
                    if let Some(path) = fd.split(" -> ").nth(1) {
                        if path.contains("/opencode/log/") {
                            match std::fs::read_to_string(path) {
                                Ok(text) => {
                                    let tail: Vec<&str> = text.lines().rev().take(15).collect();
                                    eprintln!("[gate5] cli log tail ({path}): {tail:?}");
                                }
                                Err(error) => {
                                    eprintln!("[gate5] cli log unreadable ({path}): {error}");
                                }
                            }
                        }
                    }
                }
            }
            let _ = child.kill();
            let output = child.wait_with_output().ok();
            return Ok(CliRun {
                code: None,
                stdout: output
                    .as_ref()
                    .map_or(String::new(), |o| String::from_utf8_lossy(&o.stdout).into_owned()),
                stderr: output
                    .as_ref()
                    .map_or(String::new(), |o| String::from_utf8_lossy(&o.stderr).into_owned()),
                timed_out: true,
            });
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The lane layout: per-side homes + the shared fake config.
struct Lane {
    root: PathBuf,
    rust_cli: PathBuf,
    bun: PathBuf,
    ref_cli: PathBuf,
    config: PathBuf,
}

impl Lane {
    fn home(&self, side: CliSide) -> PathBuf {
        self.root.join(match side.label {
            "reference" => "ref-home",
            _ => "rust-home",
        })
    }

    /// Base env for a CLI process on one side (scrubbed by the caller's
    /// `Command::env_clear`).
    fn env(&self, side: CliSide, password: &str) -> Vec<(String, String)> {
        let home = self.home(side);
        vec![
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("PATH".to_owned(), "/usr/local/bin:/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), home.display().to_string()),
            ("OPENCODE_TEST_HOME".to_owned(), home.display().to_string()),
            ("XDG_DATA_HOME".to_owned(), home.join(".local/share").display().to_string()),
            ("XDG_STATE_HOME".to_owned(), home.join(".local/state").display().to_string()),
            ("XDG_CONFIG_HOME".to_owned(), home.join(".config").display().to_string()),
            ("XDG_CACHE_HOME".to_owned(), home.join(".cache").display().to_string()),
            ("TMPDIR".to_owned(), home.join(".cache").display().to_string()),
            ("OPENCODE_PASSWORD".to_owned(), password.to_owned()),
            ("OPENCODE_DISABLE_MODELS_FETCH".to_owned(), "1".to_owned()),
            ("OPENCODE_CONFIG".to_owned(), self.config.display().to_string()),
        ]
    }

    /// Build the CLI command. Flag position is side-specific: the port is
    /// clap (globals BEFORE the subcommand), v2 is commander (per-command
    /// flags AFTER the subcommand).
    fn command(
        &self,
        side: CliSide,
        password: &str,
        args: &[&str],
        with_server: Option<&str>,
    ) -> Command {
        let mut cli_argv: Vec<String> = Vec::new();
        let mut command;
        if side.label == "reference" {
            cli_argv.extend([
                "run".to_owned(),
                "--cwd".to_owned(),
                self.ref_cli.display().to_string(),
                "--define".to_owned(),
                "OPENCODE_VERSION=\"2.0.24\"".to_owned(),
                "--define".to_owned(),
                "OPENCODE_CHANNEL=\"stable\"".to_owned(),
                "--define".to_owned(),
                "OPENCODE_ARTIFACT=\"cli\"".to_owned(),
                "src/index.ts".to_owned(),
            ]);
            if let Some(url) = with_server {
                // commander: --server belongs to the LEAF command, before
                // its flags/positionals (a trailing positional would
                // swallow it; a leading one breaks scoping).
                let insert_at =
                    args.iter().position(|arg| arg.starts_with('-')).unwrap_or(args.len());
                for (index, arg) in args.iter().enumerate() {
                    if index == insert_at {
                        cli_argv.push("--server".to_owned());
                        cli_argv.push(url.to_owned());
                    }
                    cli_argv.push((*arg).to_owned());
                }
                if insert_at == args.len() {
                    cli_argv.push("--server".to_owned());
                    cli_argv.push(url.to_owned());
                }
            } else {
                cli_argv.extend(args.iter().map(|a| (*a).to_owned()));
            }
            command = Command::new(&self.bun);
        } else {
            if let Some(url) = with_server {
                cli_argv.push("--server".to_owned());
                cli_argv.push(url.to_owned());
            }
            cli_argv.extend(args.iter().map(|a| (*a).to_owned()));
            command = Command::new(&self.rust_cli);
        }
        command.args(&cli_argv).current_dir(&self.ref_cli).env_clear();
        for (key, value) in self.env(side, password) {
            command.env(key, value);
        }
        command
    }

    /// Run one CLI invocation against one side's server.
    async fn run_cli(
        &self,
        side: CliSide,
        password: &str,
        args: &[&str],
        with_server: Option<&str>,
        secs: u64,
    ) -> Result<CliRun> {
        let mut command = self.command(side, password, args, with_server);
        let timed_out_argv: Vec<String> =
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        let program = command.get_program().to_string_lossy().into_owned();
        let run = run_timed(&mut command, secs).await;
        if let Ok(result) = &run {
            if result.timed_out {
                eprintln!(
                    "[gate5] TIMEOUT {program} {timed_out_argv:?} — stdout so far: {:?} stderr: {:?}",
                    result.stdout, result.stderr
                );
            }
        }
        run
    }
}

/// A `serve --hostname localhost` child owned by this test (process group).
struct ServeChild {
    child: std::process::Child,
    stderr_path: PathBuf,
    stdout_path: PathBuf,
}

/// Drain a child pipe into a file on a background thread (an unread 64KiB
/// pipe buffer blocks the child — see `spawn_serve`).
fn drain_to(pipe: impl std::io::Read + Send + 'static, path: PathBuf) {
    std::thread::spawn(move || {
        use std::io::Write;
        let mut pipe = pipe;
        let mut sink = std::fs::File::create(&path).ok();
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Some(sink) = sink.as_mut() {
                        let _ = sink.write_all(&buf[..n]);
                    }
                }
            }
        }
    });
}

#[allow(unsafe_code)] // process_group(0) needs pre_exec FFI (parity::reference pattern)
fn spawn_serve(program: &Path, argv: &[String], env: &[(String, String)]) -> Result<ServeChild> {
    let mut command = Command::new(program);
    command
        .args(argv)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().context("spawn serve child")?;
    // DRAIN the pipes. The reference on a cold per-side cache recompiles the
    // TS workspace and writes MEGABYTES to stderr; an unread 64KiB pipe
    // buffer blocks the child before it ever binds its port (the historical
    // reference-side "never answered /api/info" red — warm local caches hid
    // it). Tee both streams to files so the evidence survives for the
    // failure path too (parity::reference::build_command does the same).
    let stderr_path = std::env::temp_dir().join(format!("p10-gate5-serve-{}.stderr", child.id()));
    let stdout_path = std::env::temp_dir().join(format!("p10-gate5-serve-{}.stdout", child.id()));
    if let Some(pipe) = child.stdout.take() {
        drain_to(pipe, stdout_path.clone());
    }
    if let Some(pipe) = child.stderr.take() {
        drain_to(pipe, stderr_path.clone());
    }
    Ok(ServeChild { child, stderr_path, stdout_path })
}

impl ServeChild {
    /// Last ~2KB of the tee'd stderr (diagnostics for boot failures).
    fn stderr_tail(&self) -> String {
        let _ = &self.stdout_path;
        match std::fs::read(&self.stderr_path) {
            Ok(bytes) => {
                let start = bytes.len().saturating_sub(2048);
                String::from_utf8_lossy(&bytes[start..]).into_owned()
            }
            Err(_) => String::from("<no stderr captured>"),
        }
    }

    /// Terminate the child's process group (own children only; filtered
    /// signal like `parity::tui_diff`).
    fn stop(&mut self) {
        let pid = self.child.id();
        let _ = oc_util::procutil::signal_group_filtered(
            pid,
            libc::SIGTERM,
            &["bun", "opencode-rust", "oc-server", "node", "sh"],
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `--hostname localhost` resolves differently per platform/runtime: bun on
/// some CI runners resolves localhost -> `::1` FIRST and binds only IPv6, so a
/// 127.0.0.1-only poll never connects (the 90x1s budget expires — the
/// historical gate5 reference-side red). The boot contract is "answers
/// /api/info with auth", not "on IPv4" — poll every candidate stack.
async fn wait_info_any(bases: &[String], password: &str) -> Result<()> {
    for _ in 0..90 {
        for base in bases {
            let auth = parity::minih::basic_auth("opencode", password);
            if let Ok(response) = parity::minih::request(
                base,
                Some(&auth),
                "GET",
                "/api/info",
                None,
                "application/json",
                Duration::from_secs(2),
            )
            .await
            {
                if response.status == 200 {
                    return Ok(());
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(anyhow::anyhow!("GET /api/info never answered 200 on any of {bases:?}"))
}

fn free_port() -> Result<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .context("bind for a free port")?
        .local_addr()
        .context("local addr")
        .map(|addr| addr.port())
}

#[tokio::test]
#[ignore = "Phase-10 gate: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_cli_gate5() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }
    let bun = p10_common::bun_path()?;
    let tui_root = PathBuf::from(
        std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");
    if !tui_root.is_dir()
        && parity::gate::skip_or_fail(&format!(
            "reference checkout missing at {} (set PARITY_REF_ROOT or rebuild /tmp/opencode/ref)",
            tui_root.display()
        ))
    {
        return Ok(());
    }
    // Deterministic lane sandbox root (B0-2 acceptance: pid-scoped).
    std::env::set_var(
        "PARITY_SANDBOX_ROOT",
        format!("/tmp/opencode/lane-b0-gates-{}-cli", std::process::id()),
    );
    run(&bun, &tui_root).await
}

#[allow(clippy::too_many_lines)]
async fn run(bun: &Path, tui_root: &Path) -> Result<()> {
    let root =
        PathBuf::from(format!("/tmp/opencode/lane-b0-gates-{}-cli-lane", std::process::id()));
    let fake =
        ScriptedFakeLlm::start(vec![p10_common::text_script("Parity fake says hello.")]).await?;
    std::fs::create_dir_all(root.join("cfg"))?;
    let config = root.join("cfg/opencode.json");
    std::fs::write(&config, p10_common::provider_config(&fake.base(), false))?;
    let lane = Lane {
        rust_cli: parity::rust_server::cli_binary()?,
        bun: bun.to_path_buf(),
        ref_cli: tui_root.join("../cli"),
        config: config.clone(),
        root: root.clone(),
    };
    for side in [CliSide { label: "reference" }, CliSide { label: "port" }] {
        for dir in [".local/share", ".local/state", ".config", ".cache"] {
            std::fs::create_dir_all(lane.home(side).join(dir))?;
        }
    }

    let reference = parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
        extra_env: vec![(
            "OPENCODE_CONFIG",
            Box::leak(config.display().to_string().into_boxed_str()) as &'static str,
        )],
        seed_files: Vec::new(),

        extra_args: Vec::new(),
    })
    .await?;
    let port = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn {
        extra_env: vec![(
            "OPENCODE_CONFIG",
            Box::leak(config.display().to_string().into_boxed_str()) as &'static str,
        )],
        seed_files: Vec::new(),
    })
    .await?;
    let result = checks(&lane, &reference, &port).await;
    let stop_ref = reference.stop();
    let stop_port = port.stop();
    result?;
    stop_ref.await?;
    stop_port.await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn checks(
    lane: &Lane,
    reference: &parity::reference::ReferenceServer,
    port: &parity::rust_server::RustServer,
) -> Result<()> {
    let sides = [
        (CliSide { label: "reference" }, reference.url(), reference.password()),
        (CliSide { label: "port" }, port.url(), port.password()),
    ];
    let mut gaps = serde_json::Map::new();
    let mut gap_facts: Vec<Value> = Vec::new();

    // -- --version ---------------------------------------------------------
    let mut versions = Vec::new();
    for (side, _, _) in &sides {
        let run = lane.run_cli(*side, "unused", &["--version"], None, 30).await?;
        ensure!(!run.timed_out, "{side} --version timed out");
        ensure!(run.code == Some(0), "{side} --version rc {:?}: {}", run.code, run.stderr);
        let version_line = run.stdout.trim();
        ensure!(
            !version_line.is_empty() && !version_line.contains('\n'),
            "{side} --version not a single line: {version_line:?}"
        );
        ensure!(
            version_line.starts_with("opencode") && version_line.contains('v'),
            "{side} --version shape: {version_line:?}"
        );
        versions.push(version_line.rsplit('v').next().unwrap_or_default().to_owned());
    }
    ensure!(versions[0] == versions[1], "version numbers differ across sides: {versions:?}");
    eprintln!("[gate5] --version — rc 0, shape ok, version {} on both sides", versions[0]);

    // -- --help ------------------------------------------------------------
    for (side, _, _) in &sides {
        let run = lane.run_cli(*side, "unused", &["--help"], None, 30).await?;
        ensure!(!run.timed_out, "{side} --help timed out");
        ensure!(run.code == Some(0), "{side} --help rc {:?}: {}", run.code, run.stderr);
        let needle = if side.label == "reference" { "USAGE" } else { "Usage:" };
        ensure!(
            run.stdout.contains(needle),
            "{side} --help missing usage marker {needle:?}:\n{}",
            run.stdout
        );
    }
    gaps.insert(
        "help_full_text".to_owned(),
        json!(
            "clap vs commander render different help texts; command-surface parity is the \
               tracked gap (not byte equality)"
        ),
    );
    eprintln!("[gate5] --help — rc 0 + usage marker on both sides");

    // -- serve --hostname localhost (boot + info + clean shutdown) ---------
    // The reference side runs through the PROVEN spawn_reference path (env,
    // PATH with the bun dir, tee'd logs, current_dir) with `--hostname
    // localhost` appended, and the probe targets the URL the server itself
    // PRINTS (`server listening on <url>`) — the bound stack under
    // --hostname localhost depends on the runtime's resolver, so guessing
    // 127.0.0.1/[::1] was the historical reference-side red. The port side
    // keeps its own child spawn with a dual-stack poll.
    {
        let reference = parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
            extra_env: vec![(
                "OPENCODE_CONFIG",
                Box::leak(lane.config.display().to_string().into_boxed_str()) as &'static str,
            )],
            seed_files: Vec::new(),
            extra_args: vec!["--hostname".to_owned(), "localhost".to_owned()],
        })
        .await
        .context("spawn reference serve --hostname localhost")?;
        let mut bases = vec![
            format!("http://127.0.0.1:{}", reference.port_hint()),
            format!("http://[::1]:{}", reference.port_hint()),
        ];
        if let Some(reported) = reference.listening_url() {
            bases.insert(0, reported.to_owned());
        }
        let info = wait_info_any(&bases, reference.password()).await;
        reference.stop().await?;
        info.context("reference serve --hostname localhost boot")?;
        eprintln!(
            "[gate5] serve --hostname localhost — boot + /api/info 200 + clean stop (reference)"
        );
    }
    {
        let side = CliSide { label: "port" };
        let serve_port = free_port()?;
        let argv = vec![
            "serve".to_owned(),
            "--hostname".to_owned(),
            "localhost".to_owned(),
            "--port".to_owned(),
            serve_port.to_string(),
        ];
        let env = lane.env(side, "servepw-gate5");
        let mut serve = spawn_serve(&lane.rust_cli, &argv, &env)
            .with_context(|| format!("spawn {side} serve"))?;
        let bases =
            vec![format!("http://127.0.0.1:{serve_port}"), format!("http://[::1]:{serve_port}")];
        let info = wait_info_any(&bases, "servepw-gate5").await;
        let stderr_tail = serve.stderr_tail();
        serve.stop();
        info.context(format!(
            "{side} serve --hostname localhost boot; stderr tail:\n{stderr_tail}"
        ))?;
        eprintln!(
            "[gate5] serve --hostname localhost — boot + /api/info 200 + clean stop ({side})"
        );
    }

    // -- auth list (non-TTY) ------------------------------------------------
    for (side, url, password) in &sides {
        let run = lane.run_cli(*side, password, &["auth", "list"], Some(url), 30).await?;
        ensure!(!run.timed_out, "{side} auth list timed out");
        ensure!(run.code == Some(0), "{side} auth list rc {:?}: {}", run.code, run.stderr);
        ensure!(
            run.stdout.trim() == "No authenticated integrations",
            "{side} auth list stdout: {:?}",
            run.stdout
        );
    }
    gaps.insert(
        "auth_tty_branch".to_owned(),
        json!("TTY rendering of auth list (table vs JSON) not wired — non-TTY is byte-equal"),
    );
    eprintln!("[gate5] auth list — byte-equal on both sides (non-TTY)");

    // -- models (non-TTY) ----------------------------------------------------
    let mut model_sets = Vec::new();
    for (side, url, password) in &sides {
        let run = lane.run_cli(*side, password, &["models"], Some(url), 30).await?;
        ensure!(!run.timed_out, "{side} models timed out");
        ensure!(run.code == Some(0), "{side} models rc {:?}: {}", run.code, run.stderr);
        let lines: Vec<String> = run
            .stdout
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        ensure!(
            lines.iter().any(|line| line.contains("parityfake/fake-model")),
            "{side} models missing the config-seeded fake: {lines:?}"
        );
        model_sets.push(lines);
    }
    ensure!(
        model_sets[0] == model_sets[1],
        "model lists differ:\nreference={:?}\nport={:?}",
        model_sets[0],
        model_sets[1]
    );
    gaps.insert(
        "models_tty_branch".to_owned(),
        json!("TTY rendering of models (table vs list) not wired — non-TTY is byte-equal"),
    );
    eprintln!("[gate5] models — byte-equal model lists on both sides");

    // -- session list (pagination + order via --format json) -----------------
    let ref_cli_dir = lane.ref_cli.display().to_string();
    for (side, url, password) in &sides {
        for title in ["gate5-c1", "gate5-c2", "gate5-c3"] {
            let payload =
                json!({"title": title, "location": {"directory": ref_cli_dir}}).to_string();
            let (status, body) =
                p10_common::api(url, password, "POST", "/api/session", Some(&payload)).await?;
            ensure!(status == 200, "{side} create {title}: {status} {body}");
        }
    }
    for (side, url, password) in &sides {
        let run = lane
            .run_cli(*side, password, &["session", "list", "--format", "json"], Some(url), 30)
            .await?;
        ensure!(!run.timed_out, "{side} session list timed out");
        ensure!(run.code == Some(0), "{side} session list rc {:?}: {}", run.code, run.stderr);
        let rows: Vec<Value> = serde_json::from_str(run.stdout.trim())
            .with_context(|| format!("{side} session list stdout not JSON: {}", run.stdout))?;
        let titles: Vec<String> =
            rows.iter().map(|row| row["title"].as_str().unwrap_or_default().to_owned()).collect();
        let newest: Vec<String> =
            titles.iter().filter(|title| title.starts_with("gate5-")).take(3).cloned().collect();
        let expected: Vec<String> =
            ["gate5-c3", "gate5-c2", "gate5-c1"].iter().map(ToString::to_string).collect();
        ensure!(
            newest == expected,
            "{side} session list newest-first order: newest={newest:?} (all: {titles:?})"
        );
        let limited = lane
            .run_cli(
                *side,
                password,
                &["session", "list", "--format", "json", "-n", "2"],
                Some(url),
                30,
            )
            .await?;
        ensure!(limited.code == Some(0), "{side} session list -n 2 rc {:?}", limited.code);
        let rows: Vec<Value> = serde_json::from_str(limited.stdout.trim())
            .with_context(|| format!("{side} session list -n 2 not JSON: {}", limited.stdout))?;
        let titles: Vec<String> =
            rows.iter().map(|row| row["title"].as_str().unwrap_or_default().to_owned()).collect();
        ensure!(
            titles.iter().all(|title| title.starts_with("gate5-")) && rows.len() == 2,
            "{side} session list -n 2 rows: {titles:?}"
        );
        gap_facts.push(json!({
            "section": "session_list",
            "fact": "epoch fields serialize as floats on the port (created: 1792... .0) vs \
                     integers on v2 — full-JSON equality is deferred",
            "side": side.label,
        }));
    }
    gaps.insert(
        "session_list_full_json".to_owned(),
        json!(
            "port serializes created/updated as floats; titles/order/count checks are live, \
               full-value equality deferred pending triage"
        ),
    );
    gaps.insert(
        "session_list_table_time".to_owned(),
        json!(
            "human-table local-time formats differ (2026-10-18 10:20:52 vs 10/18/2026, \
               6:21:33 PM) — recorded, not asserted"
        ),
    );
    eprintln!("[gate5] session list — newest-first + -n limit on both sides");

    // -- run --format json (success) -----------------------------------------
    let mut event_sets = Vec::new();
    for (side, url, password) in &sides {
        let run = lane
            .run_cli(*side, password, &["run", "--format", "json", "Say hi."], Some(url), 60)
            .await?;
        if run.timed_out {
            let (_, sessions) = p10_common::api(url, password, "GET", "/api/session", None)
                .await
                .unwrap_or((0, Value::Null));
            eprintln!("[gate5] run TIMEOUT on {side}: server sessions now: {sessions}");
            if let Some(newest) = sessions["data"].as_array().and_then(|rows| rows.first()) {
                if let Some(id) = newest["id"].as_str() {
                    let (_, messages) = p10_common::api(
                        url,
                        password,
                        "GET",
                        &format!("/api/session/{id}/message"),
                        None,
                    )
                    .await
                    .unwrap_or((0, Value::Null));
                    eprintln!("[gate5] newest session {id} messages: {messages}");
                }
            }
        }
        ensure!(!run.timed_out, "{side} run --format json timed out");
        ensure!(
            run.code == Some(0),
            "{side} run rc {:?}: stdout={:?} stderr={:?}",
            run.code,
            run.stdout,
            run.stderr
        );
        let mut text_parts = 0;
        let mut types = Vec::new();
        for line in run.stdout.lines().filter(|line| !line.trim().is_empty()) {
            let event: Value = serde_json::from_str(line)
                .with_context(|| format!("{side} run stdout line not JSON: {line}"))?;
            types.push(event["type"].as_str().unwrap_or_default().to_owned());
            if event.pointer("/part/type") == Some(&json!("text"))
                && event.pointer("/part/text") == Some(&json!("Parity fake says hello."))
            {
                text_parts += 1;
            }
        }
        ensure!(text_parts == 1, "{side} run text part count {text_parts} (types {types:?})");
        event_sets.push(types);
    }
    gap_facts.push(json!({
        "section": "run_json",
        "fact": "event coverage differs: port emits step_start/text/step_finish, v2 emits text only",
        "reference": event_sets[0],
        "port": event_sets[1],
    }));
    gaps.insert(
        "run_json_event_coverage".to_owned(),
        json!(
            "the port emits step_start/text/step_finish events, v2 emits the text event only \
               — recorded as a gap fact pending ledger triage"
        ),
    );
    eprintln!("[gate5] run --format json — deterministic fake reply on both sides");

    // -- run early error ------------------------------------------------------
    for (side, url, password) in &sides {
        let run = lane
            .run_cli(*side, password, &["run", "-m", "nope/nada", "Say hi."], Some(url), 60)
            .await?;
        ensure!(!run.timed_out, "{side} run early-error timed out");
        ensure!(run.code == Some(1), "{side} run -m nope/nada rc {:?} (want 1)", run.code);
        let stderr = strip_ansi(&run.stderr);
        ensure!(
            stderr.contains("Model unavailable: nope/nada"),
            "{side} run early-error stderr: {stderr:?}"
        );
    }
    eprintln!("[gate5] run early-error — rc 1 + Model unavailable on both sides");

    // -- --print-logs ---------------------------------------------------------
    for (side, url, password) in &sides {
        let run = lane.run_cli(*side, password, &["models", "--print-logs"], Some(url), 30).await?;
        ensure!(!run.timed_out, "{side} models --print-logs timed out");
        ensure!(
            run.code == Some(0),
            "{side} models --print-logs rc {:?}: {}",
            run.code,
            run.stderr
        );
        gap_facts.push(json!({
            "section": "print_logs",
            "side": side.label,
            "stderr_has_level_lines": run.stderr.contains("level="),
            "stderr_len": run.stderr.len(),
        }));
    }
    gaps.insert(
        "print_logs_content".to_owned(),
        json!(
            "flag accepted on both sides; log-line content parity (timestamps/run ids) not \
               asserted"
        ),
    );
    eprintln!("[gate5] --print-logs — accepted on both sides");

    let report = json!({
        "gate": "gate5-cli",
        "checks": [
            "--version (shape + equal version)",
            "--help (usage marker)",
            "serve --hostname localhost (boot + /api/info + clean stop)",
            "auth list non-TTY (byte-equal)",
            "models non-TTY (byte-equal lists)",
            "session list --format json (newest-first + -n limit)",
            "run --format json (deterministic reply)",
            "run early-error (rc 1 + Model unavailable)",
            "--print-logs accepted",
        ],
        "gaps": Value::Object(gaps),
        "gap_facts": gap_facts,
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("gate5-cli.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    eprintln!("[gate5] PASS — all live checks green; gaps + facts recorded in the artifact");
    Ok(())
}
