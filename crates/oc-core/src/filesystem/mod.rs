//! Filesystem watching — port of `filesystem/watcher.ts` +
//! `filesystem/location-watcher.ts` (v2.0.24).
//!
//! - [`watcher`]: the general file-watcher service (`Watcher.Service`) —
//!   refcounted subscriptions over the notify backend already used by the
//!   config hot-reload (`config/watch.rs`), with REF's event reduction and
//!   ignore rules, plus the `filesystem.changed` bus publisher.
//! - [`location_watcher`]: per-location VCS activity detection
//!   (`LocationWatcher`) — watches the location's git `HEAD` / hg `branch`
//!   file, publishes `FileSystem.Event.Changed`, and touches the location's
//!   activity so the idle TTL sweeper can expire it.

pub mod location_watcher;
pub mod watcher;
