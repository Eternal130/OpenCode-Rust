//! MF-1 #18 — the config hot-reload watch (notify, not mtime polling).
//!
//! The ref watches the config DIRECTORY (`config/index.tsx:356-365`
//! `fs.watch` — rename-saves and in-place writes both surface as
//! directory events; a file-stat poll can also miss a same-mtime
//! rewrite). This module is the notify-based stand-in for the run
//! loop's retired per-tick `metadata(path)` poll (app.rs's old
//! "mtime check per tick" comment): one background thread owns a
//! `RecommendedWatcher` on the config file's PARENT directory
//! (non-recursive), filters events to the config file's own path
//! (create/modify/rename in — editors' atomic `write-temp + rename`
//! saves land as the renamed-in path), debounces 100 ms (ref
//! `config/watch.ts`'s `DEBOUNCE` window — a save storm coalesces
//! into one dirty flag), and raises a single dirty flag the pump
//! loop drains per pass. The flag is raised by the debounce thread
//! AFTER the window goes quiet, never in the event callback: one
//! in-place `write(2)` surfaces as TWO inotify events (`Modify(Data)`
//! then `Access(Close(Write))`), and a callback-side raise made the
//! pump able to drain between them (a second raise for the same
//! write — and a read of a file still open for writing). The reload itself stays in the loop (the
//! mtime compare remains the CHANGE gate, so a touched-but-equal
//! file never re-applies).
//!
//! oc-core's `config/watch.rs` `WatchHub` is the engine-side
//! equivalent (G-320's skill roots ride it); the TUI's `Config`
//! reload path (`Config::reload`) is its own surface, so the watch
//! here is deliberately small rather than pulling the hub's
//! discovery/Sources model in.

use notify::Watcher as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The debounce window (ref `config/watch.ts` `DEBOUNCE` = 100 ms):
/// events arriving within the window collapse into one dirty raise.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// The config-file watch handle. Dropping it stops the watcher
/// thread (the run loop owns one per boot; a stale watch from an
/// abandoned boot dies with its guard, mirroring the input reader's
/// `StopInputReader` discipline).
pub struct ConfigFileWatch {
    stop: Arc<AtomicBool>,
    dirty: Arc<AtomicBool>,
}

impl ConfigFileWatch {
    /// Watch `path`'s parent directory for changes to `path`.
    /// `None` when the path carries no parent to watch (a config
    /// from memory — the retired poll's `path: None` case).
    #[must_use]
    pub fn start(path: &Path) -> Option<Self> {
        let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty())?;
        let target: PathBuf = path.to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let dirty = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let dirty_flag = Arc::clone(&dirty);
        let mut watcher = notify::RecommendedWatcher::new(
            move |event: Result<notify::Event, notify::Error>| {
                if let Ok(event) = event {
                    // The config file's own path only — sibling
                    // writes in the config dir (other tools'
                    // files) never raise the flag.
                    let hits = event.paths.iter().any(|p| p == &target);
                    if hits {
                        let _ = tx.send(());
                    }
                }
            },
            notify::Config::default(),
        )
        .ok()?;
        // A non-recursive watch on the parent (the ref's `fs.watch`
        // dir watch shape).
        if watcher.watch(dir, notify::RecursiveMode::NonRecursive).is_err() {
            return None;
        }
        let thread_stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("tui-config-watch".to_owned())
            .spawn(move || {
                // Park until an event (then debounce-drain) or the
                // stop flag. The watcher lives as long as the thread.
                let _watcher = watcher;
                while !thread_stop.load(Ordering::Relaxed) {
                    if rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                        // Debounce: hold the storm inside the window
                        // (a save's Modify + Close(Write) pair, an
                        // editor's temp + rename) and raise ONCE on
                        // the quiet tail — the storm's last event
                        // has landed by then, so the pump's reload
                        // never reads a file mid-write.
                        while rx.recv_timeout(DEBOUNCE).is_ok() {}
                        dirty_flag.store(true, Ordering::Release);
                    }
                }
            })
            .ok()?;
        Some(Self { stop, dirty })
    }

    /// Drain the dirty flag (the pump loop's per-pass check): true
    /// exactly once per change storm.
    #[must_use]
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }
}

impl Drop for ConfigFileWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-place write raises the dirty flag (the retired poll's
    /// modify case) and the drain is one-shot.
    #[test]
    fn in_place_write_raises_once() {
        let dir = std::env::temp_dir().join(format!("cfgwatch-{}-a", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        std::fs::write(&path, "{}").unwrap();
        let watch = ConfigFileWatch::start(&path).expect("watch starts");
        std::thread::sleep(Duration::from_millis(200)); // watcher arms
        std::fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let raised = loop {
            if watch.take_dirty() {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(raised, "modify never raised the dirty flag");
        // One-shot: no further writes → no further raises.
        std::thread::sleep(Duration::from_millis(300));
        assert!(!watch.take_dirty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A rename-save (write temp + rename over) raises — the ref
    /// `fs.watch` dir case the old file-stat poll could miss.
    #[test]
    fn rename_save_raises() {
        let dir = std::env::temp_dir().join(format!("cfgwatch-{}-b", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        std::fs::write(&path, "{}").unwrap();
        let watch = ConfigFileWatch::start(&path).expect("watch starts");
        std::thread::sleep(Duration::from_millis(200));
        let tmp = dir.join(".opencode.json.tmp");
        std::fs::write(&tmp, r#"{"mouse":true}"#).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let raised = loop {
            if watch.take_dirty() {
                break true;
            }
            if std::time::Instant::now() > deadline {
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(raised, "rename-save never raised the dirty flag");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A sibling file's write in the watched dir never raises.
    #[test]
    fn sibling_write_does_not_raise() {
        let dir = std::env::temp_dir().join(format!("cfgwatch-{}-c", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        std::fs::write(&path, "{}").unwrap();
        let watch = ConfigFileWatch::start(&path).expect("watch starts");
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.join("unrelated.txt"), "noise").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(!watch.take_dirty(), "sibling write raised the flag");
        std::fs::remove_dir_all(&dir).ok();
    }
}
