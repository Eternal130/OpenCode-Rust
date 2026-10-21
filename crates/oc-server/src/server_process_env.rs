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
//! (`Watcher.configured`), `OPENCODE_DISABLE_FFF`
//! (`FileSystemSearch.configured`), `OPENCODE_GIT_BASH_PATH`
//! (`ShellSelect.configured`, windows-only), `OPENCODE_SIMULATE`
//! (the simulation backend layer swap).

use std::path::PathBuf;

use oc_core::config::Loader;
use oc_core::provider_catalog::catalog::Catalog;

/// v2 `truthy` (server-process.ts:191): exactly `"1"` or the
/// case-insensitive `"true"`.
fn truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
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
pub fn configure_loader(loader: &mut Loader) {
    if !project_config_enabled() {
        loader.options.project = Some(false);
    }
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
