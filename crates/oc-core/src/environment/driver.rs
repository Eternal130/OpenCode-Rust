//! Driver seam — port of `packages/core/src/environment/driver.ts` +
//! `index.ts` (`makeFiles`).
//!
//! A [`Driver`] pairs a process spawner with optional file-contract
//! overrides; [`make_files`] layers `execDefaults(spawner)` under the
//! overrides. Divergence from TS: Rust overrides are complete [`Files`]
//! implementations rather than `Partial` records (every shipped driver —
//! local and memory — overrides the full surface), so the merge degenerates
//! to choosing the override when present.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::environment::exec_defaults::ExecDefaults;
use crate::environment::files::Files;
use crate::environment::pty::PtySpawner;

/// A spawn request: `command args...` with extra env (`extend_env` merges the
/// current process env) and optional stdin bytes.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub extend_env: bool,
    pub stdin: Option<Vec<u8>>,
    /// Optional collection caps (exclusive upper bounds) for stdout/stderr.
    /// A spawner that honors them stops collecting at `cap + 1` bytes so the
    /// caller can detect the overrun without buffering unbounded output
    /// (mirrors ref `collectStream`, which stops AT the limit and fails).
    pub max_stdout: Option<usize>,
    pub max_stderr: Option<usize>,
}

/// A completed process run, collected to completion.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Spawn failure (no process or collection failure).
pub type SpawnError = String;

/// `ChildProcessSpawner`-equivalent seam: run one command to completion.
#[async_trait::async_trait]
pub trait Spawner: Send + Sync {
    async fn spawn(&self, command: CommandSpec) -> Result<ProcessOutput, SpawnError>;
}

/// `Driver { spawner, overrides?, stdio, pty }` — `stdio` is the streaming
/// (piped) spawn counterpart the MCP stdio transport uses; `pty` provisions
/// pseudo-terminals (the pty session machine's spawn seam); every driver
/// carries both, defaulting to the same execution plane as `spawner`.
#[derive(Clone)]
pub struct Driver {
    pub spawner: Arc<dyn Spawner>,
    pub overrides: Option<Arc<dyn Files>>,
    pub stdio: Arc<dyn crate::environment::stdio::StdioSpawner>,
    pub pty: Arc<dyn PtySpawner>,
}

/// `makeFiles(driver) = { ...execDefaults(driver.spawner), ...driver.overrides }`.
pub fn make_files(driver: &Driver) -> Arc<dyn Files> {
    match &driver.overrides {
        Some(overrides) => overrides.clone(),
        None => Arc::new(ExecDefaults::new(driver.spawner.clone())),
    }
}
