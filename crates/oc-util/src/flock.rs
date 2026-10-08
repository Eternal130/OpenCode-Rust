//! Advisory file locking with `flock(2)` (`packages/util/src/flock.ts`
//! semantics reduced to the unix primitive the TypeScript emulates).
//!
//! Locks are per open file description: two `open` calls in the same process
//! contend for the lock, which makes thread-based tests meaningful and matches
//! the cross-process behavior callers expect. Dropping the guard releases the
//! lock.

#![allow(unsafe_code)] // flock(2) is only reachable through libc FFI

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// Failures from lock acquisition.
#[derive(Debug, thiserror::Error)]
pub enum FlockError {
    /// The lock stayed held by someone else for the whole timeout.
    #[error("timed out after {timeout:?} waiting for lock on {}", path.display())]
    Timeout {
        /// File that could not be locked.
        path: PathBuf,
        /// How long the caller was willing to wait.
        timeout: Duration,
    },
    /// Underlying I/O failure.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// RAII guard for an advisory `flock` on a file.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
    shared: bool,
}

impl FileLock {
    fn open(path: &Path) -> io::Result<File> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
    }

    fn call_flock(file: &File, shared: bool, non_blocking: bool) -> io::Result<bool> {
        let mut mode = if shared { libc::LOCK_SH } else { libc::LOCK_EX };
        if non_blocking {
            mode |= libc::LOCK_NB;
        }
        let rc = unsafe { libc::flock(file.as_raw_fd(), mode) };
        if rc == 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code)
                if code == libc::EWOULDBLOCK || code == libc::EAGAIN || code == libc::EINTR =>
            {
                Ok(false)
            }
            _ => Err(err),
        }
    }

    /// Tries to take the lock without waiting. `None` means someone else holds
    /// a conflicting lock.
    ///
    /// # Errors
    ///
    /// Returns [`FlockError::Io`] when the file cannot be opened or `flock`
    /// fails with a non-contention error.
    pub fn try_lock(path: &Path, shared: bool) -> Result<Option<FileLock>, FlockError> {
        let file = Self::open(path)?;
        if Self::call_flock(&file, shared, true)? {
            Ok(Some(FileLock { file, path: path.to_path_buf(), shared }))
        } else {
            Ok(None)
        }
    }

    /// Tries to take an exclusive lock without waiting.
    ///
    /// # Errors
    ///
    /// See [`FileLock::try_lock`].
    pub fn try_exclusive(path: &Path) -> Result<Option<FileLock>, FlockError> {
        Self::try_lock(path, false)
    }

    /// Tries to take a shared lock without waiting.
    ///
    /// # Errors
    ///
    /// See [`FileLock::try_lock`].
    pub fn try_shared(path: &Path) -> Result<Option<FileLock>, FlockError> {
        Self::try_lock(path, true)
    }

    /// Takes the lock, polling until it is free or `timeout` elapses. A zero
    /// timeout degenerates to a single non-blocking try.
    ///
    /// # Errors
    ///
    /// Returns [`FlockError::Timeout`] when the deadline passes and
    /// [`FlockError::Io`] on I/O failures.
    pub fn lock(path: &Path, shared: bool, timeout: Duration) -> Result<FileLock, FlockError> {
        let deadline = Instant::now() + timeout;
        let mut backoff = Duration::from_millis(1);
        loop {
            if let Some(lock) = Self::try_lock(path, shared)? {
                return Ok(lock);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(FlockError::Timeout { path: path.to_path_buf(), timeout });
            }
            let wait = backoff.min(deadline - now).max(Duration::from_millis(1));
            thread::sleep(wait);
            backoff = (backoff * 2).min(Duration::from_millis(25));
        }
    }

    /// Takes an exclusive lock, waiting up to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`FileLock::lock`].
    pub fn exclusive(path: &Path, timeout: Duration) -> Result<FileLock, FlockError> {
        Self::lock(path, false, timeout)
    }

    /// Takes a shared lock, waiting up to `timeout`.
    ///
    /// # Errors
    ///
    /// See [`FileLock::lock`].
    pub fn shared(path: &Path, timeout: Duration) -> Result<FileLock, FlockError> {
        Self::lock(path, true, timeout)
    }

    /// Path of the locked file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True when the lock is shared rather than exclusive.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.shared
    }

    /// Releases the lock (equivalent to dropping the guard).
    pub fn release(self) {
        drop(self);
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // LOCK_UN releases the advisory lock; closing the descriptor would as
        // well, but an explicit unlock keeps the intent visible.
        let rc = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            eprintln!("flock unlock failed on {}: {err}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::{Arc, Barrier};

    fn lock_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");
        (dir, path)
    }

    #[test]
    fn try_lock_creates_and_takes_file() {
        let (_dir, path) = lock_path();
        let lock = FileLock::try_exclusive(&path).unwrap().expect("lock is free");
        assert!(path.exists());
        assert_eq!(lock.path(), path.as_path());
        assert!(!lock.is_shared());
    }

    #[test]
    fn exclusive_conflicts_with_exclusive() {
        let (_dir, path) = lock_path();
        let first = FileLock::try_exclusive(&path).unwrap().expect("first lock");
        assert!(FileLock::try_exclusive(&path).unwrap().is_none());
        drop(first);
        assert!(FileLock::try_exclusive(&path).unwrap().is_some());
    }

    #[test]
    fn shared_locks_coexist() {
        let (_dir, path) = lock_path();
        let first = FileLock::try_shared(&path).unwrap().expect("first shared");
        let second = FileLock::try_shared(&path).unwrap().expect("second shared");
        assert!(first.is_shared() && second.is_shared());
        assert!(FileLock::try_shared(&path).unwrap().is_some());
    }

    #[test]
    fn shared_conflicts_with_exclusive_and_vice_versa() {
        let (_dir, path) = lock_path();
        let shared = FileLock::try_shared(&path).unwrap().expect("shared lock");
        assert!(FileLock::try_exclusive(&path).unwrap().is_none());
        drop(shared);
        let exclusive = FileLock::try_exclusive(&path).unwrap().expect("exclusive lock");
        assert!(FileLock::try_shared(&path).unwrap().is_none());
        drop(exclusive);
    }

    #[test]
    fn lock_times_out_while_held() {
        let (_dir, path) = lock_path();
        let barrier = Arc::new(Barrier::new(2));
        let holder = {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let lock = FileLock::try_exclusive(&path).unwrap().expect("holder lock");
                barrier.wait();
                thread::sleep(Duration::from_millis(250));
                drop(lock);
            })
        };
        barrier.wait();
        let start = Instant::now();
        match FileLock::exclusive(&path, Duration::from_millis(50)) {
            Err(FlockError::Timeout { path: timed_path, .. }) => {
                assert!(start.elapsed() >= Duration::from_millis(40));
                assert_eq!(timed_path, path);
            }
            other => panic!("expected timeout, got {other:?}"),
        }
        holder.join().unwrap();
        // The holder released; a fresh acquisition succeeds quickly.
        assert!(FileLock::exclusive(&path, Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn blocking_lock_waits_for_release() {
        let (_dir, path) = lock_path();
        let (tx, rx) = mpsc::channel();
        let holder = {
            let path = path.clone();
            thread::spawn(move || {
                let lock = FileLock::try_exclusive(&path).unwrap().expect("holder lock");
                tx.send(()).unwrap();
                thread::sleep(Duration::from_millis(150));
                drop(lock);
            })
        };
        rx.recv().unwrap();
        let lock =
            FileLock::exclusive(&path, Duration::from_secs(5)).expect("acquired after release");
        drop(lock);
        holder.join().unwrap();
    }

    #[test]
    fn zero_timeout_is_single_try() {
        let (_dir, path) = lock_path();
        let _lock = FileLock::try_exclusive(&path).unwrap().expect("lock is free");
        let start = Instant::now();
        assert!(matches!(
            FileLock::exclusive(&path, Duration::ZERO),
            Err(FlockError::Timeout { .. })
        ));
        assert!(start.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/deeper/test.lock");
        let lock = FileLock::try_exclusive(&path).unwrap().expect("parents created");
        assert!(path.exists());
        drop(lock);
    }

    #[test]
    fn concurrent_exclusive_locks_serialize() {
        let (_dir, path) = lock_path();
        let path = Arc::new(path);
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let path = Arc::clone(&path);
            let counter = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                let lock = FileLock::exclusive(&path, Duration::from_secs(10)).expect("lock");
                let current = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                assert_eq!(current, 1, "critical section must be exclusive");
                thread::sleep(Duration::from_millis(5));
                counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                drop(lock);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }
}
