//! Host filesystem binding — port of `packages/core/src/environment/local.ts`.
//!
//! Deliberately raw `std::fs` rather than any higher-level FS service: the
//! contract needs lstat semantics (`stat` reports `symlink`) and typed
//! directory entries. Nothing above the environment seam touches the host
//! filesystem directly.

use std::path::Path;
use std::time::UNIX_EPOCH;

use async_trait::async_trait;

use crate::environment::driver::{CommandSpec, Driver, ProcessOutput, SpawnError, Spawner};
use crate::environment::files::{
    DirEntry, FileInfo, FileType, Files, FilesError, Range, ReadResult,
};

/// Spawns real host processes (the cross-spawn equivalent).
pub struct HostSpawner;

#[async_trait]
impl Spawner for HostSpawner {
    async fn spawn(&self, command: CommandSpec) -> Result<ProcessOutput, SpawnError> {
        let mut cmd = tokio::process::Command::new(&command.command);
        cmd.args(&command.args)
            .stdin(if command.stdin.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if command.extend_env {
            cmd.env_clear();
            for (key, value) in std::env::vars() {
                cmd.env(key, value);
            }
        }
        for (key, value) in &command.env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().map_err(|error| error.to_string())?;
        // Write stdin concurrently with draining stdout/stderr: a child that
        // fills its stdout pipe before consuming all stdin would otherwise
        // deadlock both sides (write_all before wait_with_output).
        let stdin_task = if let Some(bytes) = command.stdin.clone() {
            use tokio::io::AsyncWriteExt;
            child.stdin.take().map(|mut stdin| {
                tokio::spawn(async move {
                    let _ = stdin.write_all(&bytes).await;
                    let _ = stdin.shutdown().await;
                })
            })
        } else {
            None
        };
        // Drain both pipes concurrently (a full pipe blocks the child), each
        // capped so a runaway process cannot exhaust memory: collection stops
        // one byte past the cap and the child is killed, letting the caller's
        // post-hoc limit check surface the error (ref collectStream parity).
        let cap_tripped = std::sync::Arc::new(tokio::sync::Notify::new());
        let stdout_task = child.stdout.take().map(|pipe| {
            let cap = command.max_stdout;
            let notify = cap_tripped.clone();
            tokio::spawn(async move { read_capped(pipe, cap, notify).await })
        });
        let stderr_task = child.stderr.take().map(|pipe| {
            let cap = command.max_stderr;
            let notify = cap_tripped.clone();
            tokio::spawn(async move { read_capped(pipe, cap, notify).await })
        });
        // If either cap trips we must kill the child: it may be blocked
        // writing into a pipe nobody drains, which would hang `wait`.
        let output = tokio::select! {
            status = child.wait() => status.map_err(|error| error.to_string())?,
            _ = cap_tripped.notified() => {
                let _ = child.start_kill();
                child.wait().await.map_err(|error| error.to_string())?
            }
        };
        let stdout = match stdout_task {
            Some(task) => task.await.unwrap_or_default(),
            None => Vec::new(),
        };
        let stderr = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => Vec::new(),
        };
        if let Some(task) = stdin_task {
            let _ = task.await;
        }
        Ok(ProcessOutput { exit_code: output.code().unwrap_or(-1), stdout, stderr })
    }
}

/// Reads `pipe` to EOF, stopping after `cap` bytes when set (the buffer may
/// end at `cap + 1` so callers can detect the overrun). On cap trip the
/// shared `notify` is fired so the parent kills the child instead of
/// hanging on `wait` with an undrained pipe.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(
    mut pipe: R,
    cap: Option<usize>,
    notify: std::sync::Arc<tokio::sync::Notify>,
) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..read]);
        if let Some(cap) = cap {
            if buf.len() > cap {
                buf.truncate(cap + 1);
                drop(pipe);
                notify.notify_waiters();
                return buf;
            }
        }
    }
    buf
}

/// `makeLocalDriver(spawner)` — overrides the full `Files` surface with raw
/// host-fs operations and keeps the spawner for process work.
pub fn make_local_driver(spawner: std::sync::Arc<dyn Spawner>) -> Driver {
    Driver {
        spawner,
        overrides: Some(std::sync::Arc::new(LocalFiles)),
        stdio: std::sync::Arc::new(crate::environment::stdio::LocalStdioSpawner),
    }
}

/// Convenience: local driver over [`HostSpawner`].
pub fn make_local_driver_default() -> Driver {
    make_local_driver(std::sync::Arc::new(HostSpawner))
}

/// The raw host-fs `Files` implementation (lstat semantics).
pub struct LocalFiles;

#[async_trait]
impl Files for LocalFiles {
    async fn read(&self, path: &str, range: Option<Range>) -> Result<ReadResult, FilesError> {
        let info = stat(path, true)?;
        if info.kind != FileType::File {
            return Err(FilesError::WrongKind { path: path.to_string(), actual: info.kind });
        }
        let bytes = match range {
            None => std::fs::read(path).map_err(|error| attempt(path, error))?,
            Some(range) => {
                let file =
                    std::fs::File::open(path).map_err(|error| attempt_follow(path, error))?;
                let mut buffer = vec![0u8; range.length as usize];
                #[cfg(unix)]
                let read = {
                    use std::os::unix::fs::FileExt;
                    file.read_at(&mut buffer, range.offset).map_err(|error| attempt(path, error))?
                };
                #[cfg(not(unix))]
                let read = {
                    use std::io::{Read, Seek, SeekFrom};
                    let mut file = file;
                    file.seek(SeekFrom::Start(range.offset))
                        .and_then(|_| file.read(&mut buffer))
                        .map_err(|error| attempt(path, error))?
                };
                buffer.truncate(read);
                buffer
            }
        };
        Ok(ReadResult { info, bytes })
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> Result<(), FilesError> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|error| attempt(path, error))?;
        }
        std::fs::write(path, bytes).map_err(|error| attempt(path, error))
    }

    async fn stat(&self, path: &str) -> Result<FileInfo, FilesError> {
        stat(path, false)
    }

    async fn list(&self, path: &str) -> Result<Vec<DirEntry>, FilesError> {
        let info = stat(path, true)?;
        if info.kind != FileType::Directory {
            return Err(FilesError::WrongKind { path: path.to_string(), actual: info.kind });
        }
        let mut entries = Vec::new();
        let dir = std::fs::read_dir(path).map_err(|error| attempt_follow(path, error))?;
        for entry in dir {
            let entry = entry.map_err(|error| attempt(path, error))?;
            let kind = match entry.file_type() {
                Ok(kind) => file_type(kind),
                Err(error) => return Err(attempt(path, error)),
            };
            entries.push(DirEntry { name: entry.file_name().to_string_lossy().to_string(), kind });
        }
        Ok(entries)
    }

    async fn remove(&self, path: &str) -> Result<(), FilesError> {
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            // `rm -rf` force semantics: absence is success.
            Err(error) if is_missing(&error) => return Ok(()),
            Err(error) => return Err(attempt(path, error)),
        };
        let result =
            if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
        // A racing removal is fine under force semantics.
        if let Err(error) = result {
            if !is_missing(&error) {
                return Err(attempt(path, error));
            }
        }
        Ok(())
    }

    async fn r#move(&self, from: &str, to: &str) -> Result<(), FilesError> {
        stat(from, false)?;
        let destination = match stat(to, false) {
            Ok(info) if info.kind == FileType::Directory => {
                let base = Path::new(from)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                Path::new(to).join(base).to_string_lossy().to_string()
            }
            // Ref catches only NotFound and lets every other error
            // propagate; only a missing destination means "move to `to`".
            Err(FilesError::NotFound { .. }) => to.to_string(),
            Err(error) => return Err(error),
            Ok(_) => to.to_string(),
        };
        std::fs::rename(from, destination).map_err(|error| attempt(from, error))
    }

    async fn mkdir(&self, path: &str) -> Result<(), FilesError> {
        std::fs::create_dir_all(path).map_err(|error| attempt(path, error))
    }
}

fn stat(path: &str, follow: bool) -> Result<FileInfo, FilesError> {
    let meta = if follow {
        std::fs::metadata(path).map_err(|error| attempt_follow(path, error))?
    } else {
        std::fs::symlink_metadata(path).map_err(|error| attempt_follow(path, error))?
    };
    Ok(FileInfo { kind: file_type_meta(&meta), size: meta.len(), mtime_ms: mtime_ms(&meta) })
}

fn file_type_meta(meta: &std::fs::Metadata) -> FileType {
    if meta.is_file() {
        FileType::File
    } else if meta.is_dir() {
        FileType::Directory
    } else if meta.file_type().is_symlink() {
        FileType::Symlink
    } else {
        FileType::Other
    }
}

fn file_type(kind: std::fs::FileType) -> FileType {
    if kind.is_file() {
        FileType::File
    } else if kind.is_dir() {
        FileType::Directory
    } else if kind.is_symlink() {
        FileType::Symlink
    } else {
        FileType::Other
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64() * 1_000.0)
        .unwrap_or(0.0)
}

fn is_missing(error: &std::io::Error) -> bool {
    // ENOENT and ENOTDIR both surface as `NotFound` in Rust.
    error.kind() == std::io::ErrorKind::NotFound
}

/// `attempt(value, run)`: every error is `Failed`.
fn attempt(path: &str, error: std::io::Error) -> FilesError {
    FilesError::failed(path, error)
}

/// `attempt(value, run, true)`: missing paths map to `NotFound`.
fn attempt_follow(path: &str, error: std::io::Error) -> FilesError {
    if is_missing(&error) {
        FilesError::NotFound { path: path.to_string() }
    } else {
        attempt(path, error)
    }
}
