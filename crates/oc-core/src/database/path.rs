//! Per-instance database filename resolution.
//!
//! Port of `packages/cli/src/database-path.ts`:
//!
//! ```text
//! filename = OPENCODE_DB
//!   ?? (channel ∈ {latest,dev,beta,next,prod} || OPENCODE_DISABLE_CHANNEL_DB ∈ {1,true}
//!         ? "opencode.db"
//!         : `opencode-${sanitized-channel}.db`)
//! path    = resolve(Global.data, filename)   // ":memory:" passes through
//! ```
//!
//! `Global.data` comes from [`oc_util::global_roots`]. Unlike the TypeScript
//! build-time `OPENCODE_CHANNEL` define, the Rust port reads the channel from
//! the `OPENCODE_CHANNEL` environment variable (default `stable`).

use std::env;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Channels that use the un-suffixed `opencode.db` filename.
pub const SHARED_DB_CHANNELS: [&str; 5] = ["latest", "dev", "beta", "next", "prod"];

/// Fallback channel when `OPENCODE_CHANNEL` is unset (matches the pinned
/// reference build's `OPENCODE_CHANNEL="stable"` define).
pub const DEFAULT_CHANNEL: &str = "stable";

/// Where a database lives: a file on disk, or a private in-memory database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbLocation {
    /// `:memory:` — each instance is its own connection with a private
    /// bootstrap lock.
    Memory,
    /// A concrete file path (already resolved against `Global.data` if the
    /// configured filename was relative).
    File(PathBuf),
}

/// Replaces every character outside `[a-zA-Z0-9._-]` with `-`, mirroring
/// `channel.replace(/[^a-zA-Z0-9._-]/g, "-")`.
#[must_use]
pub fn sanitize_channel(channel: &str) -> String {
    channel
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .collect()
}

/// The channel-scheme filename (`packages/cli/src/database-path.ts:6-11`).
#[must_use]
pub fn channel_filename(channel: &str) -> String {
    if SHARED_DB_CHANNELS.contains(&channel) {
        "opencode.db".to_string()
    } else {
        format!("opencode-{}.db", sanitize_channel(channel))
    }
}

/// Resolves the filename for a channel, honoring `OPENCODE_DB` (absolute wins,
/// relative resolves against `data`, `:memory:` passes through) and
/// `OPENCODE_DISABLE_CHANNEL_DB`.
///
/// Empty `OPENCODE_DB` values are ignored (the TypeScript `??` chain treats an
/// empty string as set; an empty filename is never a usable database, so the
/// port falls through to the channel scheme instead).
#[must_use]
pub fn db_location(data: &Path, channel: &str) -> DbLocation {
    if let Some(filename) = env::var_os("OPENCODE_DB").filter(|value| !value.is_empty()) {
        return location_from_filename(data, &filename);
    }
    if matches!(env::var("OPENCODE_DISABLE_CHANNEL_DB").as_deref(), Ok("1" | "true")) {
        return DbLocation::File(data.join("opencode.db"));
    }
    let filename = channel_filename(channel);
    location_from_filename(data, OsStr::new(&filename))
}

fn location_from_filename(data: &Path, filename: &OsStr) -> DbLocation {
    if filename == OsStr::new(":memory:") {
        return DbLocation::Memory;
    }
    let path = PathBuf::from(filename);
    DbLocation::File(if path.is_absolute() { path } else { data.join(path) })
}

/// Resolves the default database location for this process: `Global.data` +
/// the channel scheme with the channel from `OPENCODE_CHANNEL`.
#[must_use]
pub fn default_location() -> DbLocation {
    let channel = env::var("OPENCODE_CHANNEL").unwrap_or_else(|_| DEFAULT_CHANNEL.to_string());
    db_location(&oc_util::global_roots::resolve().data, &channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct Env {
        _guard: std::sync::MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl Env {
        #[allow(clippy::unused_self)] // keeps call sites reading like a harness
        fn set(&self, name: &str, value: &str) {
            env::set_var(name, value);
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }

    fn locked_env(vars: &'static [&'static str]) -> Env {
        let guard = lock();
        let saved = vars.iter().map(|&name| (name, env::var(name).ok())).collect();
        for name in vars {
            env::remove_var(name);
        }
        Env { _guard: guard, saved }
    }

    #[test]
    fn sanitize_replaces_unsafe_chars() {
        assert_eq!(sanitize_channel("stable"), "stable");
        assert_eq!(sanitize_channel("canary/2 x"), "canary-2-x");
        assert_eq!(sanitize_channel("a.b_c-d"), "a.b_c-d");
    }

    #[test]
    fn channel_scheme_filenames() {
        assert_eq!(channel_filename("latest"), "opencode.db");
        assert_eq!(channel_filename("prod"), "opencode.db");
        assert_eq!(channel_filename("stable"), "opencode-stable.db");
        assert_eq!(channel_filename("edge/1"), "opencode-edge-1.db");
    }

    #[test]
    fn opencode_db_passthrough() {
        let env = locked_env(&["OPENCODE_DB", "OPENCODE_DISABLE_CHANNEL_DB"]);
        let data = Path::new("/data");

        env.set("OPENCODE_DB", ":memory:");
        assert_eq!(db_location(data, "stable"), DbLocation::Memory);

        env.set("OPENCODE_DB", "/abs/x.db");
        assert_eq!(db_location(data, "stable"), DbLocation::File(PathBuf::from("/abs/x.db")));

        env.set("OPENCODE_DB", "rel.db");
        assert_eq!(db_location(data, "stable"), DbLocation::File(data.join("rel.db")));
    }

    #[test]
    fn disable_channel_db_wins_over_channel() {
        let env = locked_env(&["OPENCODE_DB", "OPENCODE_DISABLE_CHANNEL_DB"]);
        let data = Path::new("/data");
        env.set("OPENCODE_DISABLE_CHANNEL_DB", "1");
        assert_eq!(db_location(data, "stable"), DbLocation::File(data.join("opencode.db")));
        env.set("OPENCODE_DISABLE_CHANNEL_DB", "true");
        assert_eq!(db_location(data, "stable"), DbLocation::File(data.join("opencode.db")));
        env.set("OPENCODE_DISABLE_CHANNEL_DB", "0");
        assert_eq!(db_location(data, "stable"), DbLocation::File(data.join("opencode-stable.db")));
    }

    #[test]
    fn default_channel_uses_stable_filename() {
        let _env = locked_env(&["OPENCODE_DB", "OPENCODE_DISABLE_CHANNEL_DB", "OPENCODE_CHANNEL"]);
        let data = Path::new("/data");
        assert_eq!(db_location(data, "stable"), DbLocation::File(data.join("opencode-stable.db")));
        assert_eq!(db_location(data, "next"), DbLocation::File(data.join("opencode.db")));
    }
}
