//! PTY provisioning seam — the `pty` capability of the Environment
//! [`Driver`] (G-1). `oc-core/pty.rs` (the session machine: buffers,
//! subscribers, bus events, retention) consumed `portable_pty` directly;
//! the pty *mechanics* (open the pair, spawn the child, hand out
//! reader/writer/master/killer) moved here behind the neutral
//! [`PtySpawner`] trait so a future non-local workspace driver can
//! provide PTYs. The local driver wraps the existing portable-pty code
//! (moved, not rewritten) — behavior is identical.

use std::collections::BTreeMap;

/// A PTY spawn request: the command line, its environment/cwd, and the
/// initial geometry.
#[derive(Debug, Clone)]
pub struct PtySpawnRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

/// Kill handle (the neutralized `portable_pty::ChildKiller`).
pub trait PtyChildKiller: Send {
    fn kill(&mut self) -> std::io::Result<()>;
}

/// Resize handle (the neutralized `portable_pty::MasterPty`).
pub trait PtyMaster: Send {
    fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()>;
}

/// The spawned pty child: pid probe + blocking reap + a kill handle.
pub trait PtyChild: Send {
    fn process_id(&self) -> Option<u32>;
    fn clone_killer(&self) -> Box<dyn PtyChildKiller>;
    /// Blocking reap; `Ok(Some(code))` once exited (code 0 when the
    /// platform reports none), mirroring `Child.wait().exit_code()`.
    fn wait(&mut self) -> std::io::Result<Option<i32>>;
}

/// Everything a live pty session needs: the data pipes plus the
/// control handles.
pub struct PtySpawned {
    pub reader: Box<dyn std::io::Read + Send>,
    pub writer: Box<dyn std::io::Write + Send>,
    pub master: Box<dyn PtyMaster>,
    pub child: Box<dyn PtyChild>,
}

/// Spawn failure.
pub type PtySpawnError = std::io::Error;

/// The `pty` capability: open a pty pair and run one command on the
/// slave. Errors surface as [`PtySpawnError`] so consumers keep the
/// `std::io::Result` shape the pty service always returned.
pub trait PtySpawner: Send + Sync {
    fn spawn(&self, request: PtySpawnRequest) -> Result<PtySpawned, PtySpawnError>;
}

/// The host driver's pty provider — the portable-pty code previously
/// inlined in `oc-core/pty.rs::PtyService::create`, moved verbatim.
pub struct LocalPtySpawner;

fn io_error(cause: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(cause.to_string())
}

impl PtySpawner for LocalPtySpawner {
    fn spawn(&self, request: PtySpawnRequest) -> Result<PtySpawned, PtySpawnError> {
        use portable_pty::{CommandBuilder, PtySize};
        let PtySpawnRequest { command, args, cwd, env, cols, rows } = request;
        let mut builder = CommandBuilder::new(&command);
        builder.args(&args);
        builder.cwd(&cwd);
        for (key, value) in &env {
            builder.env(key, value);
        }
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(io_error)?;
        let reader = pair.master.try_clone_reader().map_err(io_error)?;
        let writer = pair.master.take_writer().map_err(io_error)?;
        let child = pair.slave.spawn_command(builder).map_err(io_error)?;
        drop(pair.slave);
        let pid = child.process_id().unwrap_or(0);
        Ok(PtySpawned {
            reader,
            writer,
            master: Box::new(LocalMaster(pair.master)),
            child: Box::new(LocalPtyChild { child, pid }),
        })
    }
}

/// A spawner that always fails — the memory driver's pty capability
/// (parity with `FailingStdioSpawner`).
pub struct FailingPtySpawner(pub &'static str);

impl PtySpawner for FailingPtySpawner {
    fn spawn(&self, _request: PtySpawnRequest) -> Result<PtySpawned, PtySpawnError> {
        Err(std::io::Error::other(self.0))
    }
}

struct LocalMaster(Box<dyn portable_pty::MasterPty + Send>);

impl PtyMaster for LocalMaster {
    fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()> {
        self.0
            .resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(io_error)
    }
}

struct LocalKiller(Box<dyn portable_pty::ChildKiller + Send>);

impl PtyChildKiller for LocalKiller {
    fn kill(&mut self) -> std::io::Result<()> {
        self.0.kill()
    }
}

struct LocalPtyChild {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    pid: u32,
}

impl PtyChild for LocalPtyChild {
    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }

    fn clone_killer(&self) -> Box<dyn PtyChildKiller> {
        Box::new(LocalKiller(self.child.clone_killer()))
    }

    fn wait(&mut self) -> std::io::Result<Option<i32>> {
        let status = self.child.wait()?;
        Ok(Some(i32::try_from(status.exit_code()).unwrap_or(0)))
    }
}
