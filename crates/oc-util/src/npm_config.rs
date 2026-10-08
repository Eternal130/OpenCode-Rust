//! npm registry resolution, the observable slice of
//! `packages/util/src/npm-config.ts`.
//!
//! The TypeScript original shells out to `@npmcli/config` (full npm config
//! cascade). This port resolves the registry URL from the same sources npm
//! consults, in npm's precedence order:
//!
//! 1. `npm_config_registry` / `NPM_CONFIG_REGISTRY` environment variables,
//! 2. `dir/.npmrc` (project config),
//! 3. `$HOME/.npmrc` (user config),
//! 4. `https://registry.npmjs.org`.
//!
//! The trailing slash npm tolerates is trimmed so callers can join paths.

use std::fs;
use std::path::{Path, PathBuf};

/// Default npm registry.
pub const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";

/// Resolves the npm registry URL for a project directory.
#[must_use]
pub fn registry(dir: &Path) -> String {
    let url = registry_from_env()
        .or_else(|| registry_from_npmrc(&dir.join(".npmrc")))
        .or_else(|| home_dir().and_then(|home| registry_from_npmrc(&home.join(".npmrc"))))
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());
    trim_trailing_slash(url)
}

fn home_dir() -> Option<PathBuf> {
    std::env::home_dir()
}

fn registry_from_env() -> Option<String> {
    for var in ["npm_config_registry", "NPM_CONFIG_REGISTRY"] {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// Extracts the last top-level `registry = ...` assignment from an ini file.
fn registry_from_npmrc(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let mut registry: Option<String> = None;
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Scoped registries (`[@scope]:registry=...`) live in tables and do
            // not replace the default registry.
            in_section = true;
            continue;
        }
        if in_section {
            continue;
        }
        let (key, value) = trimmed.split_once('=')?;
        if key.trim() == "registry" {
            let url = value.trim().trim_matches('"').trim_matches('\'').to_string();
            if !url.is_empty() {
                registry = Some(url);
            }
        }
    }
    registry
}

fn trim_trailing_slash(mut url: String) -> String {
    while url.ends_with('/') {
        url.pop();
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env;
    use std::sync::MutexGuard;

    // npmrc fixtures live on disk; environment mutation must be serialized
    // across the whole crate.
    struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl Env {
        fn lock() -> Self {
            let guard = lock_env();
            let vars = ["npm_config_registry", "NPM_CONFIG_REGISTRY", "HOME"];
            let saved = vars.iter().map(|&name| (name, std::env::var(name).ok())).collect();
            for name in vars {
                std::env::remove_var(name);
            }
            Self { _guard: guard, saved }
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn default_registry() {
        let _env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(registry(dir.path()), "https://registry.npmjs.org");
    }

    #[test]
    fn env_var_wins() {
        let env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".npmrc"), "registry=https://project.example/\n").unwrap();
        std::fs::write(home.path().join(".npmrc"), "registry=https://user.example/\n").unwrap();
        std::env::set_var("HOME", home.path().to_str().unwrap());

        std::env::set_var("NPM_CONFIG_REGISTRY", "https://env.example/");
        assert_eq!(registry(dir.path()), "https://env.example");
        std::env::remove_var("NPM_CONFIG_REGISTRY");

        std::env::set_var("npm_config_registry", "https://lower.example/");
        assert_eq!(registry(dir.path()), "https://lower.example");
        drop(env);
    }

    #[test]
    fn project_npmrc_beats_user_npmrc() {
        let env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".npmrc"), "registry=https://project.example/\n").unwrap();
        std::fs::write(home.path().join(".npmrc"), "registry=https://user.example/\n").unwrap();
        std::env::set_var("HOME", home.path().to_str().unwrap());
        assert_eq!(registry(dir.path()), "https://project.example");
        drop(env);
    }

    #[test]
    fn user_npmrc_and_trailing_slash() {
        let env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(".npmrc"), "registry = https://user.example//\n").unwrap();
        std::env::set_var("HOME", home.path().to_str().unwrap());
        assert_eq!(registry(dir.path()), "https://user.example");
        drop(env);
    }

    #[test]
    fn ignores_comments_and_scoped_sections() {
        let env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".npmrc"),
            "# registry=https://commented.example/\n[@myscope]:registry=https://scope.example/\nregistry=https://real.example/\n",
        )
        .unwrap();
        assert_eq!(registry(dir.path()), "https://real.example");
        drop(env);
    }

    #[test]
    fn quoted_registry_values() {
        let env = Env::lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".npmrc"), "registry=\"https://quoted.example/\"\n")
            .unwrap();
        assert_eq!(registry(dir.path()), "https://quoted.example");
        drop(env);
    }
}
