//! The v2 `server-process.ts` env switches (CLI-N4): the environment
//! variables the CLI serve path reads and threads into the server boot
//! options (`packages/cli/src/server-process.ts:96-134`).
//!
//! Implemented here (consumers live in the port):
//!
//! * `OPENCODE_MODELS_URL` — the models.dev catalog source override
//!   (`models.url`, models-dev.ts:326 `options?.url || defaultSource`).
//! * `OPENCODE_MODELS_PATH` — a catalog FILE that wins over the
//!   embedded snapshot (`models.file`, models-dev.ts:354-361
//!   `loadFromFile` runs before cache/bundled in `populate`).
//! * `OPENCODE_DISABLE_MODELS_FETCH` — truthy disables the periodic
//!   catalog refresh loop (`models.fetch`, server-process.ts:110
//!   `fetch: !truthy(...)`).
//! * `OPENCODE_CONFIG_PROJECT_DISABLE` /
//!   `OPENCODE_DISABLE_PROJECT_CONFIG` — truthy disables project
//!   config discovery (`config.project`, server-process.ts:113-116 →
//!   `Config.configured({project})` / `InstructionDiscovery`).
//!
//! Not claimable in this port (no consumer subsystem exists; v2 wires
//! them into services the port has not built — flagged, not faked):
//! `OPENCODE_FILEWATCHER_DISABLE`/`OPENCODE_DISABLE_FILEWATCHER`
//! (`Watcher.configured`), `OPENCODE_GIT_BASH_PATH`
//! (`ShellSelect.configured`, windows-only).
//!
//! Acknowledged in this port (G-8):
//!
//! * `OPENCODE_DISABLE_FFF` — functional: the port's fast-file-search
//!   lane IS the pure ripgrep path (`tool/grep.rs`), so a truthy value
//!   is honored by pinning `FileSystemSearch` to ripgrep-only and
//!   logging once at boot (see [`fff_enabled`]).
//! * `OPENCODE_SIMULATE` — functional rejection: v2 swaps in the
//!   simulation backend layer (`server-process.ts:103`), which this
//!   port intentionally does not ship; a truthy value logs a clear
//!   "not supported in this build" warning at boot (see
//!   [`simulate_requested`]) instead of failing silently.

use std::path::PathBuf;

use oc_core::config::Loader;
use oc_core::provider_catalog::catalog::Catalog;

/// v2 `truthy` (server-process.ts:191): exactly `"1"` or the
/// case-insensitive `"true"`.
fn truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| truthy_value(&value))
}

/// Pure v2 `truthy` over a value (testable without process env races).
#[must_use]
pub fn truthy_value(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// The `models` options (v2 `ModelsDev.Options`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelsEnv {
    /// `url` — the remote catalog source override.
    pub url: Option<String>,
    /// `file` — a local catalog file that wins over the snapshot.
    pub file: Option<PathBuf>,
    /// `fetch` — whether the refresh loop runs (v2 default: on).
    pub fetch: bool,
}

/// Reads the `models` env switches (`server-process.ts:107-111`).
#[must_use]
pub fn models_env() -> ModelsEnv {
    ModelsEnv {
        // The v2 name wins; `OPENCODE_MODELS_DEV_URL` is the port's
        // pre-existing test-infra alias for the same knob.
        url: non_empty("OPENCODE_MODELS_URL").or_else(|| non_empty("OPENCODE_MODELS_DEV_URL")),
        file: non_empty("OPENCODE_MODELS_PATH").map(PathBuf::from),
        fetch: !truthy("OPENCODE_DISABLE_MODELS_FETCH"),
    }
}

/// Whether project config discovery stays enabled
/// (`server-process.ts:112-116`: `project: !truthy(A ?? B)` — either
/// name disables it).
#[must_use]
pub fn project_config_enabled() -> bool {
    !(truthy("OPENCODE_CONFIG_PROJECT_DISABLE") || truthy("OPENCODE_DISABLE_PROJECT_CONFIG"))
}

/// Applies [`project_config_enabled`] to a config loader — the
/// `Config.configured({project})` seam for every loader the server
/// builds (boot seeding, the per-location provider listing, the
/// config/agent/project surfaces).
///
/// G-323 — delegates to oc-core's shared implementation (applied at
/// `Loader::new` construction as well) so the engine-internal loaders
/// and the server surfaces run the SAME logic.
pub fn configure_loader(loader: &mut Loader) {
    oc_core::config::configure_loader(loader);
}

/// Applies [`ModelsEnv`] to engine options (`ModelsDev.configured`):
/// a parseable `file` catalog REPLACES the embedded snapshot; `url`
/// overrides the refresh source; `fetch: false` keeps the refresh loop
/// off. An unreadable/unparseable file degrades to the embedded
/// snapshot (v2 `loadFromFile` orElseSucceed undefined → the bundled
/// snapshot is the floor). Returns the applied env for logging/tests.
pub fn apply_models_env(options: &mut oc_core::engine::EngineOptions) -> ModelsEnv {
    let env = models_env();
    if let Some(url) = env.url.clone() {
        options.catalog_url = Some(url);
    }
    if let Some(file) = env.file.clone() {
        match std::fs::read_to_string(&file)
            .map_err(|error| error.to_string())
            .and_then(|text| Catalog::parse(&text))
        {
            Ok(catalog) => {
                options.catalog = std::sync::Arc::new(catalog);
            }
            Err(error) => {
                eprintln!(
                    "opencode: OPENCODE_MODELS_PATH {} unusable ({error}); using the embedded catalog",
                    file.display()
                );
            }
        }
    }
    if !env.fetch {
        options.catalog_fetch = false;
    }
    env
}

// ---------------------------------------------------------------------------
// FFF / SIMULATE (G-8): read-and-honor switches
// ---------------------------------------------------------------------------

/// v2 `fs.fff` (`server-process.ts:126-128`): `OPENCODE_DISABLE_FFF`
/// undefined → enabled (v2 defaults it on outside win32; the port's
/// ripgrep lane runs everywhere); otherwise `!truthy(...)`. The port's
/// fast-file-search lane IS the pure ripgrep path (`tool/grep.rs`), so
/// "disabled" and "enabled" both land on ripgrep — honoring the flag
/// means the (future) fff branch stays short-circuited and the boot
/// log says so once.
#[must_use]
pub fn fff_enabled_from(disable: Option<&str>) -> bool {
    disable.is_none_or(|value| !truthy_value(value))
}

/// [`fff_enabled_from`] over the real environment.
#[must_use]
pub fn fff_enabled() -> bool {
    fff_enabled_from(std::env::var("OPENCODE_DISABLE_FFF").ok().as_deref())
}

/// v2 `simulation: truthy(process.env.OPENCODE_SIMULATE)`
/// (`server-process.ts:103`). The simulation backend layer swap is
/// intentionally unported, so this is the functional-rejection seam.
#[must_use]
pub fn simulate_requested_from(value: Option<&str>) -> bool {
    value.is_some_and(truthy_value)
}

/// [`simulate_requested_from`] over the real environment.
#[must_use]
pub fn simulate_requested() -> bool {
    simulate_requested_from(std::env::var("OPENCODE_SIMULATE").ok().as_deref())
}

/// Boot-time acknowledgment of the G-8 switches: one line per honored
/// flag (called once from the server boot path, mirroring how v2's
/// `start` consumes them).
pub fn log_env_switch_notes() {
    if !fff_enabled() {
        eprintln!(
            "opencode: OPENCODE_DISABLE_FFF honored — fast-file-search is off; using the pure ripgrep path"
        );
    }
    if simulate_requested() {
        eprintln!(
            "opencode: OPENCODE_SIMULATE is set, but the simulation backend is not supported in this build; continuing with real providers"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_mirrors_v2() {
        assert!(truthy_value("1"));
        assert!(truthy_value("true"));
        assert!(truthy_value("TRUE"));
        assert!(truthy_value("True"));
        assert!(!truthy_value("0"));
        assert!(!truthy_value("yes"));
        assert!(!truthy_value(""));
    }

    #[test]
    fn fff_defaults_on_and_disables_truthy_only() {
        // server-process.ts:126-128: undefined → enabled; otherwise !truthy(...)
        assert!(fff_enabled_from(None));
        assert!(fff_enabled_from(Some("")));
        assert!(fff_enabled_from(Some("0")));
        assert!(fff_enabled_from(Some("false")));
        assert!(!fff_enabled_from(Some("1")));
        assert!(!fff_enabled_from(Some("true")));
        assert!(!fff_enabled_from(Some("TRUE")));
    }

    #[test]
    fn simulate_is_acknowledged_only_when_truthy() {
        // server-process.ts:103: simulation: truthy(process.env.OPENCODE_SIMULATE)
        assert!(!simulate_requested_from(None));
        assert!(!simulate_requested_from(Some("")));
        assert!(!simulate_requested_from(Some("off")));
        assert!(simulate_requested_from(Some("1")));
        assert!(simulate_requested_from(Some("true")));
        assert!(simulate_requested_from(Some("True")));
    }
}
