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
//! terminates the process (SIGTERM, escalating to SIGKILL) when invoked.
//! The [`Driver`](crate::environment::driver::Driver) carries one per
//! execution plane: the local driver spawns on the host exactly like the
//! previous direct `tokio::process::Command` use; a workspace driver
//! would spawn inside the workspace (none ship yet).
//!
//! Remaining (out of scope here): migrating the tools/handlers consumers
//! (read/write/edit/shell/glob/grep/patch, pty) onto the Environment
//! `Files`/`Spawner` services.

use std::collections::BTreeMap;

/// Piped stdin half of a spawned child.
pub type BoxedWriter = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;
/// Piped stdout half of a spawned child.
pub type BoxedReader = Box<dyn tokio::io::AsyncRead + Send + Unpin>;
/// Best-effort termination (SIGTERM → escalate to SIGKILL after the
/// grace period); fire-and-forget, callable exactly once.
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
}

/// The seam MCP stdio (and future streaming consumers) spawns through.
#[async_trait::async_trait]
pub trait StdioSpawner: Send + Sync {
    async fn spawn_piped(&self, spec: StdioSpec) -> Result<PipedProcess, String>;
}

/// Reap bound after kill before letting kill-on-drop escalate (rmcp's
/// child-process transport `MAX_WAIT_ON_DROP_SECS` — the behavior the
/// previous direct `TokioChildProcess` spawn had).
const FORCE_KILL_AFTER: std::time::Duration = std::time::Duration::from_secs(3);

/// Host spawner: byte-identical semantics to the previous direct spawn
/// (`env_clear` + merged host/config environment, `process_group(0)`,
/// piped stdin/stdout, inherited stderr).
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
            .stdout(std::process::Stdio::piped());
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
        Ok(PipedProcess { stdin: Box::new(stdin), stdout: Box::new(stdout), stop })
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
}
