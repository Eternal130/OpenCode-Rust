//! Port of `cli/src/services/standalone.ts` — spawn a private
//! `serve --stdio --port 0` child with a random 32-byte lease password,
//! read the `{"url":...}` ready line from stdout, and hold the stdin pipe
//! as the ownership lease (EOF = the server exits; TERM escalated to KILL
//! after 3s).

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;

use crate::util;

#[derive(Debug, Deserialize)]
struct Ready {
    url: String,
}

/// A running standalone server (`Standalone.start()` output).
pub struct StandaloneServer {
    pub url: String,
    pub password: String,
    stdin: Option<std::process::ChildStdin>,
    child: Option<std::process::Child>,
    exited: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl StandaloneServer {
    /// Test-only instance (no child process) for exercising lease-dependent
    /// logic without spawning a server.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn test_stub(url: &str) -> StandaloneServer {
        StandaloneServer {
            url: url.to_owned(),
            password: String::new(),
            stdin: None,
            child: None,
            exited: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// The lease endpoint as an authed endpoint pair.
    #[must_use]
    pub fn endpoint(&self) -> super::server_connection::Endpoint {
        super::server_connection::Endpoint {
            url: self.url.clone(),
            password: Some(self.password.clone()),
        }
    }

    /// `endpoint.exited` (commands/handlers/acp.ts:36): resolves when the
    /// server process has gone away (the stdout reader thread observes
    /// EOF, which the child's exit produces). Polled, not signalled —
    /// the ACP handler races this against the connection closing.
    pub async fn exited(&self) {
        while !self.exited_flag() {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    fn exited_flag(&self) -> bool {
        self.exited.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Ends the lease: stdin EOF (the OS-level signal), then SIGTERM with a
    /// 3s escalation to SIGKILL (`killSignal: SIGTERM`, `forceKillAfter: 3s`).
    #[allow(dead_code)]
    pub fn stop(mut self) {
        self.release();
    }

    #[allow(unsafe_code)]
    fn release(&mut self) {
        // Drop the stdin pipe first: EOF is the lease's own release signal,
        // delivered even if this process dies.
        self.stdin = None;
        if let Some(child) = self.child.as_mut() {
            // Incident 2026-10 rule: signals only ever target the PID of a
            // Child handle we still own and have not reaped — the pid is
            // taken from the handle at call time, never from stored state.
            let pid = child.id();
            // SAFETY: kill(2) on our own live, unreaped child.
            unsafe { libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGTERM) };
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                match child.try_wait() {
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                    Ok(Some(_)) | Err(_) => return,
                }
            }
            // SAFETY: kill(2) on our own live, unreaped child.
            unsafe { libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGKILL) };
            let _ = child.wait();
        }
    }
}

impl Drop for StandaloneServer {
    fn drop(&mut self) {
        self.release();
    }
}

/// `Standalone.Options` (standalone.ts:20-22): a custom server
/// command seam. v2 never passes it from `ServerConnection.resolve`
/// (`Standalone.start()` bare, server-connection.ts:46) — it exists so
/// embedders/tests can replace the spawned server — mirrored here
/// (G-158).
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub command: Option<Vec<String>>,
}

/// `Standalone.start()` — the default (self command) form.
pub async fn start() -> Result<StandaloneServer, String> {
    start_with(Options::default()).await
}

/// `Standalone.start(options)`.
///
/// * Password: random 32 bytes base64url, forced through the child env
///   (`OPENCODE_PASSWORD`) so a user-exported value cannot shadow the lease
///   credential.
/// * stderr: inherited when `OPENCODE_PRINT_LOGS=1`, ignored otherwise.
/// * Ready protocol: first stdout line is `{"url":...}`; later stdout is
///   drained so the child never sees EPIPE. The wait is unbounded — ref
///   `Deferred.await`s the ready line forever and fails only when the
///   process exits first (`ensuring(Deferred.fail(...))`); the old 60 s
///   recv timeout had no ref counterpart (G-158).
pub async fn start_with(options: Options) -> Result<StandaloneServer, String> {
    let password = util::random_password();
    // `command(password, options)` (standalone.ts:25-29): a custom
    // `[executable, ...args]` replaces `[...selfCommand(), "serve"]`;
    // `--stdio --port 0` is always appended.
    let mut argv: Vec<String> = options.command.unwrap_or_else(|| {
        let mut command = util::self_command();
        command.push("serve".to_owned());
        command
    });
    argv.extend(["--stdio".to_owned(), "--port".to_owned(), "0".to_owned()]);
    let program = argv.remove(0);
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut child = Command::new(&program)
        .args(argv)
        .current_dir(cwd)
        .env("OPENCODE_PASSWORD", &password)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if std::env::var("OPENCODE_PRINT_LOGS").as_deref() == Ok("1") {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|error| format!("Failed to start standalone server: {error}"))?;
    let _pid = child.id();
    let stdout = child.stdout.take().ok_or("standalone stdout not piped")?;
    let stdin = child.stdin.take();

    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let exited_for_task = std::sync::Arc::clone(&exited);
    // Read lines off the child's stdout on a dedicated thread: the first
    // line satisfies readiness, the rest are drained (EPIPE guard). The
    // thread's end (EOF) marks the server as exited — the `exited` race
    // of the ACP handler watches this flag.
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if line_tx.send(line).is_err() {
                        // Reader moved on; keep draining silently.
                    }
                }
                Err(_) => break,
            }
        }
        exited_for_task.store(true, std::sync::atomic::Ordering::Release);
    });

    let ready_line = tokio::task::spawn_blocking(move || line_rx.recv().ok())
        .await
        .map_err(|error| format!("standalone ready reader failed: {error}"))?;

    // `recv` returns Err exactly when the reader thread ended (the
    // child exited / closed stdout) — the deferred-failure of ref's
    // `ensuring` clause. No timeout: ref waits indefinitely (G-158).
    let ready_line =
        if exited.load(std::sync::atomic::Ordering::Relaxed) { None } else { ready_line };
    let Some(ready_line) = ready_line else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Standalone server exited before reporting readiness".to_owned());
    };
    let ready: Ready = serde_json::from_str(&ready_line)
        .map_err(|error| format!("invalid standalone ready line {ready_line:?}: {error}"))?;

    Ok(StandaloneServer {
        url: ready.url.trim_end_matches('/').to_owned(),
        password,
        stdin,
        child: Some(child),
        exited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_parses() {
        let ready: Ready =
            serde_json::from_str(r#"{"url":"http://127.0.0.1:4096"}"#).expect("parse");
        assert_eq!(ready.url, "http://127.0.0.1:4096");
    }

    /// G-158: a custom `Options.command` replaces
    /// `[...selfCommand(), "serve"]` — the seam spawns whatever
    /// executable the caller supplies (v2 keeps the seam for
    /// embedders; `ServerConnection.resolve` still passes the default).
    #[test]
    fn custom_command_replaces_self_command() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let script = dir.path().join("fake-server.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '{\"url\":\"http://127.0.0.1:9\"}\\n'\nsleep 30\n",
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let server = runtime
            .block_on(start_with(Options {
                command: Some(vec![script.display().to_string(), "--custom-arg".to_owned()]),
            }))
            .expect("custom server starts");
        assert_eq!(server.url, "http://127.0.0.1:9");
        assert!(!server.password.is_empty(), "lease password generated");
        server.stop();
    }
}
