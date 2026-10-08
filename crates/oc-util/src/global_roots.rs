//! XDG-style global path resolution, ported from
//! `packages/util/src/global-roots.ts` and `packages/util/src/global.ts`.
//!
//! Precedence per root (empty environment values are ignored, mirroring the
//! `process.env.X || fallback` chain in the TypeScript original):
//!
//! - `config`: `OPENCODE_CONFIG_DIR` (used as-is, already app-specific) then
//!   `XDG_CONFIG_HOME` then `$HOME/.config`, joined with the app name.
//! - `data`: `XDG_DATA_HOME` then `$HOME/.local/share`.
//! - `state`: `XDG_STATE_HOME` then `$HOME/.local/state`.
//! - `cache`: `XDG_CACHE_HOME` then `$HOME/.cache`.
//! - `tmp`: platform temp dir (`$TMPDIR`) joined with the app name.
//! - `home`: `OPENCODE_TEST_HOME` relocates the home used by every fallback.
//!
//! Unlike the TypeScript module (which freezes paths at import time) paths are
//! resolved per call so tests and embedders can relocate the app by changing
//! the environment.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Application name applied to every XDG root.
pub const APP_NAME: &str = "opencode";

/// Default database filename under the data root
/// (`packages/cli/src/database-path.ts`).
pub const DB_FILENAME: &str = "opencode.db";

/// In-memory database sentinel honored by [`Roots::db`].
pub const DB_IN_MEMORY: &str = ":memory:";

/// The XDG base directories that root opencode's global paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// Home directory used to resolve the `~` fallbacks.
    pub home: PathBuf,
    /// `XDG_DATA_HOME/opencode`
    pub data: PathBuf,
    /// `XDG_CACHE_HOME/opencode`
    pub cache: PathBuf,
    /// `XDG_CONFIG_HOME/opencode` or `OPENCODE_CONFIG_DIR`
    pub config: PathBuf,
    /// `XDG_STATE_HOME/opencode`
    pub state: PathBuf,
    /// `$TMPDIR/opencode`
    pub tmp: PathBuf,
}

fn non_empty(var: &str) -> Option<PathBuf> {
    env::var_os(var).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn system_home() -> Option<PathBuf> {
    // Mirrors `os.homedir()` on unix: `$HOME` when non-empty, else the passwd entry.
    env::home_dir()
}

fn home_dir() -> Option<PathBuf> {
    non_empty("OPENCODE_TEST_HOME").or_else(system_home)
}

fn xdg(var: &str, home: &Path, fallback: &[&str]) -> PathBuf {
    non_empty(var).unwrap_or_else(|| {
        let mut path = home.to_path_buf();
        path.extend(fallback.iter().copied());
        path
    })
}

/// Resolves the roots for `app` from the current environment.
#[must_use]
pub fn resolve_app(app: &str) -> Roots {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let join = |base: PathBuf| {
        let mut path = base;
        path.push(app);
        path
    };
    Roots {
        home: home.clone(),
        data: join(xdg("XDG_DATA_HOME", &home, &[".local", "share"])),
        cache: join(xdg("XDG_CACHE_HOME", &home, &[".cache"])),
        config: non_empty("OPENCODE_CONFIG_DIR")
            .unwrap_or_else(|| join(xdg("XDG_CONFIG_HOME", &home, &[".config"]))),
        state: join(xdg("XDG_STATE_HOME", &home, &[".local", "state"])),
        tmp: join(env::temp_dir()),
    }
}

/// Resolves the opencode roots from the current environment.
#[must_use]
pub fn resolve() -> Roots {
    resolve_app(APP_NAME)
}

impl Roots {
    /// `cache/bin` — downloaded binary shims.
    #[must_use]
    pub fn bin(&self) -> PathBuf {
        self.cache.join("bin")
    }

    /// `data/log` — global log files.
    #[must_use]
    pub fn log(&self) -> PathBuf {
        self.data.join("log")
    }

    /// `data/repos` — plugin/asset repositories.
    #[must_use]
    pub fn repos(&self) -> PathBuf {
        self.data.join("repos")
    }

    /// `data/snapshot` — session snapshot worktrees.
    #[must_use]
    pub fn snapshot(&self) -> PathBuf {
        self.data.join("snapshot")
    }

    /// `state/locks` — default advisory-lock directory.
    #[must_use]
    pub fn locks(&self) -> PathBuf {
        self.state.join("locks")
    }

    /// Database path: `OPENCODE_DB` (absolute wins, relative resolves against
    /// `data`, `:memory:` passes through) or `data/opencode.db`.
    #[must_use]
    pub fn db(&self) -> PathBuf {
        match non_empty("OPENCODE_DB") {
            Some(path) if path.as_os_str() == OsStr::new(DB_IN_MEMORY) => path,
            Some(path) if path.is_absolute() => path,
            Some(path) => self.data.join(path),
            None => self.data.join(DB_FILENAME),
        }
    }

    /// Creates every global directory opencode expects to exist, mirroring the
    /// acquisition step in `packages/util/src/global.ts`.
    pub fn ensure(&self) -> io::Result<()> {
        for dir in [
            &self.data,
            &self.config,
            &self.state,
            &self.tmp,
            &self.log(),
            &self.repos(),
            &self.snapshot(),
            &self.bin(),
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;
    use std::sync::MutexGuard;

    // Environment mutation must be serialized across the whole crate.
    struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl Env {
        fn lock() -> Self {
            let guard = lock_env();
            let vars = [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_CACHE_HOME",
                "OPENCODE_CONFIG_DIR",
                "OPENCODE_TEST_HOME",
                "OPENCODE_DB",
                "TMPDIR",
            ];
            let saved = vars.iter().map(|&name| (name, env::var(name).ok())).collect();
            for name in vars {
                env::remove_var(name);
            }
            Self { _guard: guard, saved }
        }

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

    #[test]
    fn defaults_under_home() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.home, home.path());
        assert_eq!(roots.config, home.path().join(".config/opencode"));
        assert_eq!(roots.data, home.path().join(".local/share/opencode"));
        assert_eq!(roots.state, home.path().join(".local/state/opencode"));
        assert_eq!(roots.cache, home.path().join(".cache/opencode"));
    }

    #[test]
    fn xdg_overrides_win() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        env.set("XDG_CONFIG_HOME", custom.path().to_str().unwrap());
        env.set("XDG_DATA_HOME", custom.path().to_str().unwrap());
        env.set("XDG_STATE_HOME", custom.path().to_str().unwrap());
        env.set("XDG_CACHE_HOME", custom.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.config, custom.path().join("opencode"));
        assert_eq!(roots.data, custom.path().join("opencode"));
        assert_eq!(roots.state, custom.path().join("opencode"));
        assert_eq!(roots.cache, custom.path().join("opencode"));
    }

    #[test]
    fn empty_xdg_values_fall_back_to_home() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        env.set("XDG_CONFIG_HOME", "");
        env.set("XDG_DATA_HOME", "");
        let roots = resolve();
        assert_eq!(roots.config, home.path().join(".config/opencode"));
        assert_eq!(roots.data, home.path().join(".local/share/opencode"));
    }

    #[test]
    fn opencode_config_dir_replaces_config_root() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        env.set("OPENCODE_CONFIG_DIR", custom.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.config, custom.path());
    }

    #[test]
    fn opencode_config_dir_beats_xdg() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        let xdg = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        env.set("XDG_CONFIG_HOME", xdg.path().to_str().unwrap());
        env.set("OPENCODE_CONFIG_DIR", custom.path().to_str().unwrap());
        assert_eq!(resolve().config, custom.path());
    }

    #[test]
    fn test_home_relocates_fallbacks() {
        let env = Env::lock();
        let real = tempfile::tempdir().unwrap();
        let test = tempfile::tempdir().unwrap();
        env.set("HOME", real.path().to_str().unwrap());
        env.set("OPENCODE_TEST_HOME", test.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.home, test.path());
        assert_eq!(roots.config, test.path().join(".config/opencode"));
        assert_eq!(roots.data, test.path().join(".local/share/opencode"));
        assert_eq!(roots.state, test.path().join(".local/state/opencode"));
        assert_eq!(roots.cache, test.path().join(".cache/opencode"));
    }

    #[test]
    fn xdg_beats_test_home() {
        let env = Env::lock();
        let test = tempfile::tempdir().unwrap();
        let xdg = tempfile::tempdir().unwrap();
        env.set("OPENCODE_TEST_HOME", test.path().to_str().unwrap());
        env.set("XDG_DATA_HOME", xdg.path().to_str().unwrap());
        assert_eq!(resolve().data, xdg.path().join("opencode"));
    }

    #[test]
    fn tmp_joins_app_name() {
        let env = Env::lock();
        let tmp = tempfile::tempdir().unwrap();
        env.set("TMPDIR", tmp.path().to_str().unwrap());
        assert_eq!(resolve().tmp, tmp.path().join("opencode"));
    }

    #[test]
    fn custom_app_name() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        let roots = resolve_app("other");
        assert_eq!(roots.config, home.path().join(".config/other"));
    }

    #[test]
    fn db_defaults_under_data() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.db(), roots.data.join("opencode.db"));
    }

    #[test]
    fn db_overrides() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        let abs = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        let roots = resolve();

        env.set("OPENCODE_DB", abs.path().join("x.db").to_str().unwrap());
        assert_eq!(roots.db(), abs.path().join("x.db"));

        env.set("OPENCODE_DB", "channel.db");
        assert_eq!(roots.db(), roots.data.join("channel.db"));

        env.set("OPENCODE_DB", ":memory:");
        assert_eq!(roots.db(), PathBuf::from(":memory:"));
    }

    #[test]
    fn derived_directories() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        let roots = resolve();
        assert_eq!(roots.bin(), roots.cache.join("bin"));
        assert_eq!(roots.log(), roots.data.join("log"));
        assert_eq!(roots.repos(), roots.data.join("repos"));
        assert_eq!(roots.snapshot(), roots.data.join("snapshot"));
        assert_eq!(roots.locks(), roots.state.join("locks"));
    }

    #[test]
    fn ensure_creates_directories() {
        let env = Env::lock();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path().to_str().unwrap());
        env.set("TMPDIR", home.path().join("tmp").to_str().unwrap());
        let roots = resolve();
        roots.ensure().unwrap();
        let log = roots.log();
        let repos = roots.repos();
        let snapshot = roots.snapshot();
        let bin = roots.bin();
        for dir in
            [&roots.data, &roots.config, &roots.state, &roots.tmp, &log, &repos, &snapshot, &bin]
        {
            assert!(dir.is_dir(), "{} should exist", dir.display());
        }
    }
}
