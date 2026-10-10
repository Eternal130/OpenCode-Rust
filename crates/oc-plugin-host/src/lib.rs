//! # oc-plugin-host — the bun-subprocess JS plugin host (wave 1 spine)
//!
//! Productionization of the `/tmp/opencode/spike-bun` prototype
//! (spike-plugin-runtime.md option B): plugins are JS/TS ESM modules
//! run by a supervised `bun host.js` child process, speaking NDJSON
//! JSON-RPC over stdio in **both directions**.
//!
//! * [`PluginHost`] — the facade: bun resolution (absent bun ⇒ the
//!   subsystem disables itself with one clear log, every op no-ops),
//!   spawn/supervision, registration snapshot, command execution,
//!   hot-reload check, shutdown.
//! * [`BroadcastTap`] — the post-ingest event tap the TUI store
//!   publishes through after its reducers ran.
//! * [`RegistrationSnapshot`] — the ONLY synchronous surface:
//!   commands (with owned keymap entries) and theme JSON documents
//!   resolve into native Rust structures at registration time;
//!   nothing crosses IPC per-frame.
//!
//! ## 1. Deadlock protocol (binding)
//!
//! The spike hit a real 30 s deadlock: the supervisor held the stdin
//! write lock while blocking on a `load` reply that required answering
//! the guest's nested `config.get` request first. The rules below are
//! the structural fix and are enforced here — the stress test
//! (`tests/storm.rs`) hammers them.
//!
//! 1. **The supervisor IO reader never blocks on anything but the
//!    pipe.** Guest→host requests (`command.add`, `theme.register`,
//!    `event.subscribe`, `ping`) are answered *on the reader task*:
//!    the handler is synchronous (a registration insert, a log line)
//!    and the reply is *enqueued* through the writer channel. No lock
//!    on the write path is ever held across a wait.
//! 2. **Nobody holds the write channel while awaiting a reply.**
//!    [`supervisor::Shared::request`] moves the frame into the
//!    writer-owned channel, *then* awaits a oneshot keyed by the
//!    request id. Concurrent callers never serialize behind each
//!    other's pending replies.
//! 3. **Every bridge call is timeout-bounded** — 2 s registration /
//!    5 s dispatch by default ([`PluginHostConfig`]). A timed-out call
//!    is removed from the pending map; a late reply is discarded by
//!    id. A hung plugin costs exactly its own dispatch (logged,
//!    counted via [`PluginHost::dispatch_counters`]), never the host.
//! 4. **The guest mirrors the rules.** Its read loop never awaits a
//!    handler before reading the next line: handlers run detached,
//!    guest→host calls are Promises settled by supervisor replies
//!    that arrive on the same stdin stream the loop keeps draining.
//!
//! ## 2. Supervision
//!
//! The guest is monitored: an unexpected exit restarts it with
//! exponential backoff (100 ms doubling, 4 s cap). Crashes are
//! *attributed* — the plugin whose load was in flight, or which was
//! mid-dispatch (guest `plugin.busy` markers), gets a strike; three
//! strikes disable that plugin while the rest reload. Eight
//! consecutive unattributable restarts give up entirely (subsystem
//! disabled, logged).
//!
//! ## 3. Hot reload
//!
//! Digest-scan parity with v2 (`source.bun.ts`): sha256 of the entry
//! plus its relative import graph (`Bun.Transpiler.scanImports`), CJS
//! `require.cache` eviction under the plugin dir, re-import with a
//! generation cache-bust (`entry?t=N`). [`PluginHost::reload_now`]
//! compares digests and reloads changed plugins, refreshing the
//! registration snapshot.

use std::path::PathBuf;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub mod bun;
pub mod host_js;
pub mod protocol;
pub mod registrations;
pub mod supervisor;
pub mod tap;

pub use registrations::{
    PluginCommand, PluginState, PluginStatus, PluginTheme, RegistrationSnapshot,
};
pub use supervisor::{HostError, SupervisorConfig};
pub use tap::{BroadcastTap, TapEvent};

/// The embedded guest script (materialized into the sandbox at start).
const HOST_JS: &str = include_str!("host/host.js");

/// Sandbox root for host processes (gate constraint: `/tmp/opencode`,
/// PID-unique top-level names).
const SANDBOX_ROOT: &str = "/tmp/opencode";

/// Caller-facing configuration (timeouts tunable for tests).
#[derive(Debug, Clone)]
pub struct PluginHostConfig {
    /// Extra env for the guest (test fixtures).
    pub env: Vec<(String, String)>,
    pub registration_timeout: Duration,
    pub dispatch_timeout: Duration,
    pub restart_backoff_base: Duration,
    pub restart_backoff_max: Duration,
    pub max_plugin_crashes: u32,
    /// Consecutive restart attempts before the host gives up.
    pub max_restarts: u32,
}

impl Default for PluginHostConfig {
    fn default() -> Self {
        Self {
            env: Vec::new(),
            registration_timeout: Duration::from_secs(2),
            dispatch_timeout: Duration::from_secs(5),
            restart_backoff_base: Duration::from_millis(100),
            restart_backoff_max: Duration::from_secs(4),
            max_plugin_crashes: 3,
            max_restarts: 8,
        }
    }
}

/// The plugin host facade. Constructed via [`PluginHost::detect`]
/// (bun-resolving; a missing bun yields a DISABLED host whose every
/// operation is a no-op) and [`PluginHost::start_detached`].
///
/// All methods are synchronous and safe from the TUI thread: the
/// runtime lives on a dedicated OS thread; bridge calls park the
/// caller for at most the configured hard timeout.
pub struct PluginHost {
    facade: Option<Arc<Facade>>,
    /// The resolved bun binary (diagnostics; `None` = disabled).
    bun: Option<PathBuf>,
}

struct Facade {
    shared: Arc<supervisor::Shared>,
    runtime: Option<tokio::runtime::Handle>,
    tap: BroadcastTap,
    stop: Option<tokio::sync::watch::Sender<bool>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    start_error: Mutex<Option<String>>,
}

impl PluginHost {
    /// Resolve bun and prepare a host. No process spawns yet;
    /// absent bun ⇒ disabled host (one clear log line).
    #[must_use]
    pub fn detect() -> Self {
        if let Some(path) = bun::resolve_bun() {
            Self { facade: None, bun: Some(path) }
        } else {
            eprintln!(
                "opencode plugin-host: bun not found (looked on PATH, /usr/local/bin/bun, ~/.bun/bin/bun); plugin subsystem disabled"
            );
            Self { facade: None, bun: None }
        }
    }

    /// Whether the subsystem is (or can be) live.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.bun.is_some()
    }

    /// Spawn the host on a dedicated thread and run the initial
    /// registration pass over `entries`. On spawn failure the host
    /// degrades to disabled (logged; [`PluginHost::wait_ready`]
    /// returns `false`).
    #[must_use]
    pub fn start_detached(self, entries: &[PathBuf]) -> Self {
        self.start_detached_with(PluginHostConfig::default(), entries)
    }

    /// [`PluginHost::start_detached`] with explicit timeouts/env.
    #[must_use]
    pub fn start_detached_with(
        mut self,
        mut config: PluginHostConfig,
        entries: &[PathBuf],
    ) -> Self {
        let Some(bun) = self.bun.clone() else {
            eprintln!("opencode plugin-host: start requested but bun is absent; staying disabled");
            return self;
        };
        let sandbox = sandbox_dir();
        if let Err(error) = host_js::materialize(&sandbox, HOST_JS) {
            eprintln!(
                "opencode plugin-host: cannot materialize host.js into {}: {error}; plugin subsystem disabled",
                sandbox.display()
            );
            return self;
        }
        let supervisor_config = SupervisorConfig {
            bun,
            host_js: sandbox.join("host.js"),
            cwd: sandbox.clone(),
            entries: entries.to_vec(),
            registration_timeout: config.registration_timeout,
            dispatch_timeout: config.dispatch_timeout,
            restart_backoff_base: config.restart_backoff_base,
            restart_backoff_max: config.restart_backoff_max,
            max_plugin_crashes: config.max_plugin_crashes,
            max_restarts: config.max_restarts,
            env: std::mem::take(&mut config.env),
        };
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let (parts_tx, parts_rx) = std::sync::mpsc::channel::<FacadeParts>();
        let thread = std::thread::Builder::new()
            .name("oc-plugin-host".into())
            .spawn(move || {
                // A multi-thread runtime: bridge calls arrive from the
                // caller's thread via `Handle::block_on` while this
                // thread keeps every spawned task (reader/writer/pump/
                // supervise) driving — a current-thread runtime parked
                // on a stop signal would freeze them all.
                let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                else {
                    return; // parts_tx dropped → caller records the failure
                };
                let supervisor = runtime.block_on(supervisor::Supervisor::start(supervisor_config));
                match supervisor {
                    Ok(supervisor) => {
                        let _ = parts_tx.send(FacadeParts {
                            shared: Some(Arc::clone(&supervisor.shared)),
                            tap: Some(supervisor.tap_tx.clone()),
                            handle: Some(runtime.handle().clone()),
                            stop: Some(stop_tx),
                            error: None,
                        });
                        // Drive the runtime until shutdown: leaving the
                        // loop drops it (and the child, via
                        // kill_on_drop).
                        runtime.block_on(async move {
                            while stop_rx.changed().await.is_ok() {
                                if *stop_rx.borrow() {
                                    break;
                                }
                            }
                        });
                        supervisor.shutdown();
                    }
                    Err(error) => {
                        let _ = parts_tx.send(FacadeParts {
                            shared: None,
                            tap: None,
                            handle: Some(runtime.handle().clone()),
                            stop: Some(stop_tx),
                            error: Some(error),
                        });
                    }
                }
            })
            .ok();
        let parts = parts_rx.recv_timeout(Duration::from_secs(30)).ok();
        if let Some(parts) = parts {
            let shared = parts.shared.unwrap_or_else(|| {
                let (outbound, receiver) = tokio::sync::mpsc::unbounded_channel();
                drop(receiver); // requests fail fast with Exited
                Arc::new(supervisor::Shared::dead(outbound))
            });
            let tap = parts.tap.map_or_else(BroadcastTap::disabled, BroadcastTap::live);
            self.facade = Some(Arc::new(Facade {
                shared,
                runtime: parts.handle,
                tap,
                stop: parts.stop,
                thread: Mutex::new(thread),
                start_error: Mutex::new(parts.error),
            }));
        } else {
            // The host thread died or never started (runtime build
            // failure): dead facade, `wait_ready` stays false.
            let (outbound, receiver) = tokio::sync::mpsc::unbounded_channel();
            drop(receiver);
            self.facade = Some(Arc::new(Facade {
                shared: Arc::new(supervisor::Shared::dead(outbound)),
                runtime: None,
                tap: BroadcastTap::disabled(),
                stop: None,
                thread: Mutex::new(thread),
                start_error: Mutex::new(Some("plugin host thread failed to start".into())),
            }));
        }
        self
    }

    /// Block until the initial registration pass finished (spawn +
    /// initialize + every entry loaded or failed). `false` = the host
    /// never came up (or is disabled).
    #[must_use]
    pub fn wait_ready(&self, timeout: Duration) -> bool {
        let Some(facade) = &self.facade else { return false };
        if facade.start_error.lock().map_or(true, |error| error.is_some()) {
            return false;
        }
        let deadline = Instant::now() + timeout;
        let (lock, condvar) = &*facade.shared.ready;
        let mut ready = lock.lock().expect("ready lock poisoned");
        while !*ready {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, wait) =
                condvar.wait_timeout(ready, deadline - now).expect("ready lock poisoned");
            ready = guard;
            if wait.timed_out() && !*ready {
                return false;
            }
        }
        true
    }

    /// The registration snapshot (commands incl. owned keymap entries,
    /// theme documents, plugin statuses). Empty on the disabled host.
    #[must_use]
    pub fn registrations(&self) -> RegistrationSnapshot {
        let Some(facade) = &self.facade else { return RegistrationSnapshot::default() };
        facade.shared.registrations.read().expect("registrations poisoned").snapshot()
    }

    /// The post-ingest broadcast tap (a no-op tap on the disabled
    /// host).
    #[must_use]
    pub fn tap(&self) -> BroadcastTap {
        let Some(facade) = &self.facade else { return BroadcastTap::disabled() };
        facade.tap.clone()
    }

    /// Execute one plugin command by name (the palette dispatch path).
    /// Blocks at most `timeout`; a hung plugin times out cleanly.
    pub fn execute_command(
        &self,
        name: &str,
        args: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(plugin_id) = facade.shared.plugin_for_command(name) else {
            return Err(format!("{name}: no such plugin command"));
        };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        let args = match args {
            Some(text) => json!(text),
            None => Value::Null,
        };
        runtime
            .block_on(facade.shared.request(
                "command.execute",
                json!({ "pluginId": plugin_id, "name": name, "args": args }),
                timeout,
            ))
            .map(|result| result.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }

    /// Digest-scan + reload changed plugins. Returns the reloaded
    /// count. Errors degrade to `Err` — the host keeps running.
    pub fn reload_now(&self) -> Result<usize, String> {
        let Some(facade) = &self.facade else { return Err("plugin subsystem disabled".into()) };
        let Some(runtime) = &facade.runtime else { return Err("plugin host unavailable".into()) };
        runtime.block_on(facade.shared.reload_changed()).map_err(|error| error.to_string())
    }

    /// Diagnostics: events dispatched to the guest vs dropped
    /// (timeout / full queue).
    #[must_use]
    pub fn dispatch_counters(&self) -> (u64, u64) {
        let Some(facade) = &self.facade else { return (0, 0) };
        (
            facade.shared.dispatched_events.load(AtomicOrdering::Relaxed),
            facade.shared.dropped_events.load(AtomicOrdering::Relaxed),
        )
    }

    /// Why the host never came up, when [`PluginHost::wait_ready`]
    /// returned `false`.
    #[must_use]
    pub fn start_error(&self) -> Option<String> {
        self.facade.as_ref()?.start_error.lock().ok()?.clone()
    }

    /// Shut the guest down and join the host thread.
    pub fn shutdown(&self) {
        let Some(facade) = &self.facade else { return };
        if let Some(stop) = &facade.stop {
            let _ = stop.send(true);
        }
        let thread = facade.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

struct FacadeParts {
    shared: Option<Arc<supervisor::Shared>>,
    tap: Option<tokio::sync::mpsc::Sender<TapEvent>>,
    handle: Option<tokio::runtime::Handle>,
    stop: Option<tokio::sync::watch::Sender<bool>>,
    error: Option<String>,
}

/// PID-unique sandbox under `/tmp/opencode` (gate constraint).
fn sandbox_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pid = std::process::id();
    let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    PathBuf::from(format!("{SANDBOX_ROOT}/oc-plugin-host-{pid}-{n}-{nanos}"))
}

/// The crate's fixtures directory (test-only, hidden from semver).
#[doc(hidden)]
#[must_use]
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_without_bun_disables() {
        // The env override points at a missing binary → the resolver
        // yields None → detect() returns a disabled host.
        let saved = std::env::var(bun::BUN_ENV).ok();
        std::env::set_var(bun::BUN_ENV, "/definitely/not/bun");
        let host = PluginHost::detect();
        if let Some(saved) = saved {
            std::env::set_var(bun::BUN_ENV, saved);
        } else {
            std::env::remove_var(bun::BUN_ENV);
        }
        assert!(!host.enabled(), "missing bun must disable the subsystem");
        assert!(!host.wait_ready(Duration::from_millis(50)));
        assert_eq!(host.registrations().commands, [] as [registrations::PluginCommand; 0]);
        assert!(!host.tap().publish(TapEvent::new("x", serde_json::json!({}))));
        assert!(!host.tap().is_live());
        assert!(host.execute_command("x", None, Duration::from_millis(50)).is_err());
    }

    #[test]
    fn sandbox_names_are_pid_unique() {
        let a = sandbox_dir();
        let b = sandbox_dir();
        assert_ne!(a, b);
        assert!(a.starts_with(SANDBOX_ROOT));
        assert!(a.file_name().expect("name").to_string_lossy().contains("oc-plugin-host"));
    }
}
