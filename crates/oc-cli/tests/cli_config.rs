//! CONFIG-cluster regression battery (ledger rows CLI-K1, CLI-K8,
//! CLI-N1..N4): the CLI/server/TUI configuration surfaces whose v2
//! behaviour the port dropped or never claimed.
//!
//! Sandboxes under /tmp/opencode; every server binds 127.0.0.1:0 (port
//! 49374 is never touched).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The cargo-built `opencode` binary (the release-smoke harness can
/// point `OPENCODE_SMOKE_BIN` at any build).
fn opencode_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENCODE_SMOKE_BIN") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_opencode-rust"))
}

/// One redirected sandbox (HOME + every XDG root under /tmp/opencode —
/// never the developer's real data directories).
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = std::path::Path::new("/tmp/opencode")
            .join(format!("cli-config-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["home/.config", "home/.local/share", "home/.local/state", "home/.cache", "tmp"]
        {
            std::fs::create_dir_all(root.join(dir)).expect("sandbox dir");
        }
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn command(&self, binary: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(binary);
        command
            .args(args)
            .env("OPENCODE_TEST_HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("XDG_DATA_HOME", self.home().join(".local/share"))
            .env("XDG_STATE_HOME", self.home().join(".local/state"))
            .env("XDG_CACHE_HOME", self.home().join(".cache"))
            .env("TMPDIR", self.root.join("tmp"))
            .env_remove("OPENCODE_PASSWORD")
            .env_remove("OPENCODE_SERVER_PASSWORD")
            .env_remove("OPENCODE_CHANNEL");
        command
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Spawns `serve --stdio`, returns (exit code, stdout, stderr). The
/// process self-terminates on stdin EOF (the lease), so nothing
/// outlives the call.
fn serve_stdio(
    sandbox: &Sandbox,
    binary: &Path,
    hostname: &str,
    extra_env: &[(&str, &str)],
) -> std::io::Result<(i32, String, String)> {
    let mut command =
        sandbox.command(binary, &["serve", "--stdio", "--port", "0", "--hostname", hostname]);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output =
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output()?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

// ===========================================================================
// CLI-K1 — `serve --hostname` accepts hostnames; an invalid value ERRORS
// (v2 server-process.ts:65 threads the string into the HTTP server; Node
// resolves `localhost` and fails the boot on a bad name — never a silent
// downgrade to 127.0.0.1).
// ===========================================================================

/// `resolve_hostname`: IP literals skip the resolver; `localhost`
/// resolves to a loopback address; an unresolvable value errors.
#[test]
fn serve_hostname_resolution_matches_v2() {
    use oc_server::boot::resolve_hostname;
    use std::net::IpAddr;

    let exact: IpAddr = resolve_hostname("127.0.0.1").expect("IP literal");
    assert!(exact.is_loopback());
    let v6: IpAddr = resolve_hostname("::1").expect("IPv6 literal");
    assert!(v6.is_loopback());

    // `localhost` is a HOSTNAME: accepted (not downgraded silently — it
    // resolves through the OS resolver like Node's listen).
    let local = resolve_hostname("localhost").expect("localhost resolves");
    assert!(local.is_loopback(), "localhost resolves to a loopback address");

    // A syntactically invalid name fails fast (getaddrinfo EAI_NONAME).
    let error = resolve_hostname("bad host!").expect_err("invalid name rejected");
    assert!(error.to_string().contains("invalid --hostname"), "the error names the flag: {error}");
}

/// End-to-end through the real binary: an invalid `--hostname` fails
/// the boot (nonzero exit, no ready line); `localhost` and `127.0.0.1`
/// boot and print the ready line.
#[test]
fn serve_hostname() {
    let sandbox = Sandbox::new("k1");
    let binary = opencode_binary();

    // Invalid value: v2 fails the boot — the port must NOT silently
    // downgrade to 127.0.0.1 and serve.
    let (code, stdout, stderr) =
        serve_stdio(&sandbox, &binary, "bad host!", &[]).expect("spawn serve");
    assert_ne!(code, 0, "invalid --hostname must fail the boot (stdout={stdout})");
    assert!(!stdout.contains("\"url\""), "no ready line on failure (stdout={stdout})");
    assert!(stderr.contains("invalid --hostname"), "the error names the flag: {stderr}");

    // `localhost` — a hostname, not an IP literal — is accepted and
    // resolves to a loopback address.
    let (code, stdout, _stderr) =
        serve_stdio(&sandbox, &binary, "localhost", &[]).expect("spawn serve");
    assert_eq!(code, 0, "localhost boots");
    assert!(
        stdout.contains("\"url\":\"http://127.0.0.1:")
            || stdout.contains("\"url\":\"http://[::1]:"),
        "ready line carries the resolved address: {stdout}"
    );

    // IP literals keep working.
    let (code, stdout, _stderr) =
        serve_stdio(&sandbox, &binary, "127.0.0.1", &[]).expect("spawn serve");
    assert_eq!(code, 0);
    assert!(stdout.contains("\"url\""));
}

// ===========================================================================
// CLI-N1 — `--print-logs` actually turns on log output (v2
// framework/runtime.ts:98 sets OPENCODE_PRINT_LOGS=1 before any handler
// runs; services/standalone.ts:29 then INHERITS the private server's
// stderr so its logs reach the terminal — without the flag the child's
// stderr is discarded).
// ===========================================================================

/// `--standalone --print-logs`: the private server child's stderr is
/// inherited (its log lines reach the parent's stderr); without the
/// flag the child's stderr stays discarded. `OPENCODE_TRACE_BOOT=1`
/// makes the child emit a deterministic `[boot-trace]` stderr line.
#[test]
fn print_logs() {
    let sandbox = Sandbox::new("n1");
    let binary = opencode_binary();

    let run = |print_logs: bool| {
        let mut args = vec!["models", "--standalone"];
        if print_logs {
            args.push("--print-logs");
        }
        let output = sandbox
            .command(&binary, &args)
            .env("OPENCODE_TRACE_BOOT", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("spawn models");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // With the flag: the server's stderr (its logs) flows through.
    let (code, stdout, stderr) = run(true);
    assert_eq!(code, 0, "models --standalone --print-logs succeeds\nstderr:\n{stderr}");
    assert_ne!(stdout.trim(), "", "the model list prints");
    assert!(
        stderr.contains("[boot-trace]"),
        "--print-logs turns on the server log output\nstderr:\n{stderr}"
    );

    // Without the flag: same command, but the child's logs are
    // discarded (stderr quiet) — the flag is what switches them on.
    let (code, stdout, stderr) = run(false);
    assert_eq!(code, 0, "models --standalone succeeds\nstderr:\n{stderr}");
    assert_ne!(stdout.trim(), "", "the model list prints");
    assert!(
        !stderr.contains("[boot-trace]"),
        "without --print-logs the child's stderr stays discarded\nstderr:\n{stderr}"
    );
}

// ===========================================================================
// CLI-N2 — early errors in `run` honour `--format json` (v2
// reportRunError, run/run.ts:243-255: the SAME JSON envelope mid-run
// errors use — {"type":"error","timestamp":ms,"sessionID":"",
// "error":{"type":"unknown","message":...}} on stdout, exit 1).
// ===========================================================================

#[test]
fn run_early_error_json() {
    let sandbox = Sandbox::new("n2");
    let binary = opencode_binary();

    let run = |args: &[&str]| {
        let output = sandbox
            .command(&binary, args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("spawn run");
        (
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // Missing message (an early error, before any connection): the JSON
    // envelope lands on stdout with the v2 field order and shapes.
    // (--standalone keeps the run off the shared managed service — the
    // early error fires before any request either way.)
    let (code, stdout, stderr) = run(&["run", "--standalone", "--format", "json"]);
    assert_ne!(code, 0, "the early error exits nonzero");
    let line = stdout.lines().last().unwrap_or_default().to_owned();
    let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap_or_else(|error| {
        panic!("stdout is one JSON line ({error}):\n{stdout}\nstderr:\n{stderr}")
    });
    assert_eq!(value["type"], "error", "the envelope type: {value}");
    assert_eq!(value["sessionID"], "", "no session exists yet — v2 passes sessionID ?? ''");
    assert_eq!(value["error"]["type"], "unknown");
    assert_eq!(value["error"]["message"], "You must provide a message");
    assert!(
        value["timestamp"].as_u64().is_some_and(|ts| ts > 1_500_000_000),
        "timestamp is Date.now() milliseconds: {value}"
    );

    // Another early error (--fork without --continue/--session) rides
    // the same envelope.
    let (code, stdout, _stderr) = run(&["run", "--standalone", "--format", "json", "--fork", "hi"]);
    assert_ne!(code, 0);
    let line = stdout.lines().last().unwrap_or_default();
    let value: serde_json::Value =
        serde_json::from_str(line.trim()).unwrap_or_else(|_| panic!("JSON line: {stdout}"));
    assert_eq!(value["type"], "error");
    assert_eq!(value["error"]["message"], "--fork requires --continue or --session");

    // Control: the default format keeps the human stderr shape
    // (`UI.error` — "Error: <message>" on stderr, stdout empty).
    let (code, stdout, stderr) = run(&["run", "--standalone"]);
    assert_ne!(code, 0);
    assert_eq!(stdout, "", "human format prints nothing on stdout");
    assert!(
        stderr.contains("You must provide a message"),
        "the human error stays on stderr:\n{stderr}"
    );
}
