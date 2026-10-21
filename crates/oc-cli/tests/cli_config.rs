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
