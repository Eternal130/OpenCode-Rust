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
}

impl StandaloneServer {
    /// The lease endpoint as an authed endpoint pair.
    #[must_use]
    pub fn endpoint(&self) -> super::server_connection::Endpoint {
        super::server_connection::Endpoint {
            url: self.url.clone(),
            password: Some(self.password.clone()),
        }
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

/// `Standalone.start()`.
///
/// * Password: random 32 bytes base64url, forced through the child env
///   (`OPENCODE_PASSWORD`) so a user-exported value cannot shadow the lease
///   credential.
/// * stderr: inherited when `OPENCODE_PRINT_LOGS=1`, ignored otherwise.
/// * Ready protocol: first stdout line is `{"url":...}`; later stdout is
///   drained so the child never sees EPIPE.
pub async fn start() -> Result<StandaloneServer, String> {
    let password = util::random_password();
    let mut command = util::self_command();
    let program = command.remove(0);
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut child = Command::new(&program)
        .args(command)
        .args(["serve", "--stdio", "--port", "0"])
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
    // line satisfies readiness, the rest are drained (EPIPE guard).
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

    let ready_line =
        tokio::task::spawn_blocking(move || line_rx.recv_timeout(Duration::from_secs(60)).ok())
            .await
            .map_err(|error| format!("standalone ready reader failed: {error}"))?;

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
}
