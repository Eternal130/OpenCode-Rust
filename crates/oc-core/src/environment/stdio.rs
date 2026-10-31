//! Streaming stdio spawn seam — the `ChildProcessSpawner` streaming use
//! the ref's MCP stdio transport makes (`mcp/stdio.ts` spawns its server
//! through the location's `Environment` spawner instead of the SDK's
//! host-bound `StdioClientTransport`, so a workspace-backed location runs
//! its MCP servers wherever the rest of its execution happens).
//!
//! The [`Spawner`](crate::environment::driver::Spawner) contract collects
//! a process to completion — unusable for a live JSON-RPC stdio session —
//! so this module adds the piped counterpart: [`StdioSpawner::spawn_piped`]
//! returns the child's stdin/stdout pipes plus a `stop` closure that
//! terminates the process (immediate SIGKILL via `start_kill`, then
//! waits up to `FORCE_KILL_AFTER` for reaping — matching rmcp's
//! kill-on-drop behavior rather than ref's SIGTERM grace period).
//! The [`Driver`](crate::environment::driver::Driver) carries one per
//! execution plane: the local driver spawns on the host exactly like the
//! previous direct `tokio::process::Command` use; a workspace driver
//! would spawn inside the workspace (none ship yet).
//!
//! Remaining (out of scope here): migrating the tools/handlers consumers
//! (read/write/edit/shell/glob/grep/patch, pty) onto the Environment
//! `Files`/`Spawner` services.

use std::collections::BTreeMap;
use std::sync::Arc;

/// Piped stdin half of a spawned child.
pub type BoxedWriter = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;
/// Piped stdout half of a spawned child.
pub type BoxedReader = Box<dyn tokio::io::AsyncRead + Send + Unpin>;
/// Best-effort termination (immediate SIGKILL, then bounded wait for
/// reaping); fire-and-forget, callable exactly once.
pub type StopChild = Box<dyn FnOnce() + Send>;

/// One streaming stdio spawn request (`ChildProcess.make` minus the
/// stream plumbing): command, args, cwd, and the environment declared by
/// the caller. `extend_env: true` merges the execution plane's own
/// environment underneath `env` — the local driver extends with the
/// host's `process.env` (what the MCP SDK's transport did), while a
/// workspace driver would extend with the sandbox's own environment so
/// host variables never cross the seam into a remote workspace.
#[derive(Debug, Clone)]
pub struct StdioSpec {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub extend_env: bool,
}

/// A live child with piped stdio.
pub struct PipedProcess {
    pub stdin: BoxedWriter,
    pub stdout: BoxedReader,
    /// Terminates the child (process group). The MCP stdio transport
    /// invokes this when its pipes drop, mirroring the ref's scope-owned
    /// `ChildProcessHandle` (closing the scope kills the process).
    pub stop: StopChild,
    /// Rolling stderr tail (ref mcp/stdio.ts:146-184): the last
    /// [`STDERR_TAIL_BYTES`] bytes the child wrote to stderr, maintained
    /// by a drain task so a chatty server cannot stall on a full pipe.
    /// `None` when the spawner inherited stderr (test doubles).
    pub stderr_tail: Option<Arc<StderrTail>>,
}

/// The seam MCP stdio (and future streaming consumers) spawns through.
#[async_trait::async_trait]
pub trait StdioSpawner: Send + Sync {
    async fn spawn_piped(&self, spec: StdioSpec) -> Result<PipedProcess, String>;
}

/// The MCP stdio frame cap (ref mcp/stdio.ts:15 `MAX_FRAME_BYTES`):
/// a single JSON-RPC frame larger than 16 MiB fails fast with a clear
/// error instead of buffering unboundedly.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// The stderr rolling-tail bound (ref mcp/stdio.ts:17 `STDERR_TAIL` is
/// 1000 chars; the port keeps 8 KiB of bytes — same idea, byte-exact).
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// A reader enforcing [`MAX_FRAME_BYTES`] per frame: bytes since the
/// last `\n`. rmcp's codec owns the actual framing, so the cap lives at
/// this seam (the reader we hand the transport) — an oversized line
/// surfaces as an `io::Error` the transport reports verbatim.
pub struct FrameCapReader<R> {
    inner: R,
    trailing: usize,
    cap: usize,
}

impl<R> FrameCapReader<R> {
    pub fn new(inner: R) -> Self {
        Self { inner, trailing: 0, cap: MAX_FRAME_BYTES }
    }

    /// Test seam: an explicit cap.
    #[cfg(test)]
    fn with_cap(inner: R, cap: usize) -> Self {
        Self { inner, trailing: 0, cap }
    }
}

impl<R> tokio::io::AsyncRead for FrameCapReader<R>
where
    R: tokio::io::AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let Self { inner, trailing, cap } = &mut *self;
        let filled_before = buf.filled().len();
        std::pin::Pin::new(inner).poll_read(cx, buf).map(|result| {
            if result.is_ok() {
                // ref deliver(): trailingBytes resets on `\n`, grows
                // otherwise; crossing the cap fails the read.
                for byte in &buf.filled()[filled_before..] {
                    *trailing = if *byte == b'\n' { 0 } else { *trailing + 1 };
                }
                if *trailing > *cap {
                    // AsyncRead contract: an errored read must not have
                    // appended bytes — roll the buffer back (tokio's
                    // read_to_end asserts this).
                    buf.set_filled(filled_before);
                    return Err(std::io::Error::other(
                        "MCP stdio frame exceeded 16 MiB",
                    ));
                }
            }
            result
        })
    }
}

/// The rolling stderr tail a spawner's drain task maintains
/// (ref mcp/stdio.ts:55 `stderr = (stderr + output).slice(-STDERR_TAIL)`).
#[derive(Default)]
pub struct StderrTail {
    buffer: std::sync::Mutex<String>,
}

impl StderrTail {
    /// Append a chunk, keeping only the last [`STDERR_TAIL_BYTES`] bytes.
    pub fn push(&self, chunk: &[u8]) {
        let mut buffer = self.buffer.lock().unwrap_or_else(|poison| poison.into_inner());
        // Convert lossily at chunk boundaries — diagnostics, not protocol.
        buffer.push_str(&String::from_utf8_lossy(chunk));
        let excess = buffer.len().saturating_sub(STDERR_TAIL_BYTES);
        if excess > 0 {
            drop(buffer.drain(..excess));
        }
    }

    /// The retained tail, trimmed (empty when the child said nothing).
    #[must_use]
    pub fn snapshot(&self) -> String {
        self.buffer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .trim()
            .to_owned()
    }
}

/// Reap bound after kill before letting kill-on-drop escalate (rmcp's
/// child-process transport `MAX_WAIT_ON_DROP_SECS` — the behavior the
/// previous direct `TokioChildProcess` spawn had).
const FORCE_KILL_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// Host spawner: byte-identical semantics to the previous direct spawn
/// (`env_clear` + merged host/config environment, `process_group(0)`,
/// piped stdin/stdout, stderr piped into a rolling tail — G-72: the ref
/// drains stderr into the debug log with the last bytes kept for crash
/// diagnostics instead of inheriting the raw stream).
pub struct LocalStdioSpawner;

#[async_trait::async_trait]
impl StdioSpawner for LocalStdioSpawner {
    async fn spawn_piped(&self, spec: StdioSpec) -> Result<PipedProcess, String> {
        let mut builder = tokio::process::Command::new(&spec.command);
        builder
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if spec.extend_env {
            // Default inherit (the full host env) with the caller's
            // entries overriding on collision — the same byte-for-byte
            // environment `env_clear().envs(merged)` produced.
            builder.envs(&spec.env);
        } else {
            builder.env_clear().envs(&spec.env);
        }
        #[cfg(unix)]
        {
            builder.process_group(0);
        }
        let mut child = builder.spawn().map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().ok_or_else(|| "stdin was not piped".to_string())?;
        let stdout = child.stdout.take().ok_or_else(|| "stdout was not piped".to_string())?;
        // G-72: drain stderr into a bounded rolling tail so a chatty
        // server cannot stall on a full pipe, and crash diagnostics can
        // name the cause (ref mcp/stdio.ts:146-184).
        let stderr = child.stderr.take();
        let stderr_tail = stderr.map(|stderr| {
            let tail = Arc::new(StderrTail::default());
            let sink = Arc::clone(&tail);
            tokio::spawn(async move {
                use tokio::io::AsyncReadExt as _;
                let mut stderr = stderr;
                let mut chunk = [0u8; 4096];
                loop {
                    match stderr.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => sink.push(&chunk[..read]),
                    }
                }
            });
            tail
        });
        // Same teardown semantics as the previous TokioChildProcess spawn:
        // kill (the process group leader, `process_group(0)`), reap
        // bounded by rmcp's MAX_WAIT_ON_DROP_SECS; kill-on-drop is the
        // builder-level safety net above.
        let stop: StopChild = Box::new(move || {
            tokio::spawn(async move {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(FORCE_KILL_AFTER, child.wait()).await;
            });
        });
        Ok(PipedProcess { stdin: Box::new(stdin), stdout: Box::new(stdout), stop, stderr_tail })
    }
}

/// A stdio spawner that always fails (the memory driver cannot spawn
/// processes, mirroring `memory.ts`'s failing spawner).
pub struct FailingStdioSpawner(pub &'static str);

#[async_trait::async_trait]
impl StdioSpawner for FailingStdioSpawner {
    async fn spawn_piped(&self, _spec: StdioSpec) -> Result<PipedProcess, String> {
        Err(self.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failing_stdio_spawner_reports_cannot_spawn() {
        let error = match FailingStdioSpawner("The memory driver cannot spawn processes")
            .spawn_piped(StdioSpec {
                command: "true".into(),
                args: vec![],
                cwd: "/".into(),
                env: BTreeMap::new(),
                extend_env: false,
            })
            .await
        {
            Ok(_) => panic!("the failing spawner must not produce a process"),
            Err(error) => error,
        };
        assert_eq!(error, "The memory driver cannot spawn processes");
    }

    /// G-72 frame cap: a line longer than the cap fails the read with the
    /// ref's exact error text; a newline resets the counter so a long
    /// session of normal frames passes.
    #[tokio::test]
    async fn frame_cap_reader_fails_oversized_lines() {
        use tokio::io::AsyncReadExt as _;

        let mut reader = FrameCapReader::with_cap(std::io::Cursor::new(b"ok\nalso ok\n".to_vec()), 8);
        let mut out = Vec::new();
        let read = reader.read_to_end(&mut out).await;
        assert!(read.is_ok(), "small frames pass");
        assert_eq!(out, b"ok\nalso ok\n");

        let mut reader =
            FrameCapReader::with_cap(std::io::Cursor::new(vec![b'a'; 9]), 8);
        let mut out = Vec::new();
        let error = reader.read_to_end(&mut out).await.expect_err("oversized frame fails");
        assert_eq!(error.to_string(), "MCP stdio frame exceeded 16 MiB");
    }

    /// G-72 stderr tail: bounded to the last STDERR_TAIL_BYTES bytes.
    #[test]
    fn stderr_tail_keeps_the_last_8k() {
        let tail = StderrTail::default();
        tail.push(b"first\n");
        tail.push(&vec![b'x'; STDERR_TAIL_BYTES + 16]);
        let snapshot = tail.snapshot();
        assert_eq!(snapshot.len(), STDERR_TAIL_BYTES);
        assert!(snapshot.ends_with(&"x".repeat(64)), "newest bytes retained");
        assert!(!snapshot.contains("first"), "old bytes dropped");
        assert_eq!(StderrTail::default().snapshot(), "");
    }
}
