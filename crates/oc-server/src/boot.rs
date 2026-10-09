//! The one-entry boot: engine (config-seeded custom providers) + app shell
//! (handler table merged, announcer wired) + lifecycle serve. This is the
//! composition the `oc-server` binary runs and the Phase 5 integration
//! contract for embedders (`boot_and_serve`).
//!
//! ## Config seeding
//!
//! v2 rebuilds provider bindings when the config reloads; the Rust
//! engine's model resolver takes custom providers at boot
//! (`EngineOptions::custom_providers`). `boot_and_serve` therefore loads
//! the config of the boot location (the process working directory, or
//! `OPENCODE_CONFIG` when set — the loader honors the explicit file) and
//! maps `providers.<id>` entries carrying a `settings.baseURL` onto
//! [`oc_core::provider_catalog::facade::CustomProvider`] handles (the
//! `openai-compatible` path), plus the `model` default selection. Harness
//! flows that write the provider config after boot seed it through
//! `OPENCODE_CONFIG` before spawning (see `tools/parity`'s `RustServer`).

use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use oc_core::config::{GlobalDirs, Loader};
use oc_core::engine::{Engine, EngineOptions};
use oc_core::provider_catalog::facade::CustomProvider;
use oc_util::determinism::{SystemClock, UuidV4IdGen};

use crate::app::AppShell;
use crate::lifecycle::{self, ServeHandle, ServeOptions};
use crate::state::{AppState, AuthConfig};

/// Everything `boot_and_serve` needs.
pub struct BootOptions {
    /// Bind hostname + port (`ServeOptions`).
    pub serve: ServeOptions,
    /// Server password (`OPENCODE_PASSWORD`); `None` = passwordless.
    pub password: Option<String>,
    /// Extra allowed CORS origins.
    pub cors_origins: Vec<String>,
    /// Version reported by /api/info and registration.
    pub version: String,
    /// Engine data root (`<data>/opencode.db`, spills).
    pub data_root: PathBuf,
    /// Engine home (`~` expansion + config discovery).
    pub home: PathBuf,
    /// The location whose config seeds boot-time custom providers
    /// (defaults to the process working directory).
    pub config_location: Option<PathBuf>,
}

impl BootOptions {
    /// Production defaults: loopback bind with port scan, version from
    /// the crate, roots from the environment (honors `OPENCODE_TEST_HOME`
    /// and the XDG overrides through `global_roots`).
    #[must_use]
    pub fn new() -> Self {
        let roots = oc_util::global_roots::resolve();
        Self {
            serve: ServeOptions::default(),
            password: None,
            cors_origins: Vec::new(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            data_root: roots.data,
            home: roots.home,
            config_location: None,
        }
    }
}

impl Default for BootOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The config-derived provider bindings for one loaded document set.
#[derive(Debug, Default)]
pub struct ConfigProviders {
    /// `providers.<id>` with a `settings.baseURL` (openai-compatible).
    pub custom: Vec<CustomProvider>,
    /// The `model` default selection.
    pub default_model: Option<oc_schema::config::ModelSelection>,
}

/// Maps a loaded config onto boot-time provider bindings.
#[must_use]
pub fn providers_from_config(loaded: &oc_core::config::Loaded) -> ConfigProviders {
    let mut out = ConfigProviders::default();
    // Later entries win (priority order: nearest config last).
    for entry in &loaded.entries {
        let oc_schema::config::Entry::Document(document) = entry else { continue };
        if let Some(providers) = &document.info.providers {
            for (id, provider) in providers {
                let Some(settings) = &provider.settings else { continue };
                let base_url = settings
                    .rest
                    .get("baseURL")
                    .or_else(|| settings.rest.get("base_url"))
                    .and_then(|value| value.as_ref().and_then(|value| value.as_str()))
                    .map(str::to_owned);
                let Some(base_url) = base_url else { continue };
                let api_key = settings
                    .rest
                    .get("apiKey")
                    .or_else(|| settings.rest.get("api_key"))
                    .and_then(|value| value.as_ref().and_then(|value| value.as_str()))
                    .map(str::to_owned)
                    .or_else(|| {
                        provider
                            .env
                            .as_ref()
                            .and_then(|names| names.first())
                            .and_then(|name| std::env::var(name).ok())
                    });
                let models = provider
                    .models
                    .as_ref()
                    .map(|models| models.keys().cloned().collect())
                    .unwrap_or_default();
                out.custom.push(CustomProvider {
                    id: id.clone(),
                    name: provider.name.clone().unwrap_or_else(|| id.clone()),
                    base_url,
                    api_key,
                    models,
                });
            }
        }
        if let Some(selection) = &document.info.model {
            out.default_model = Some(selection.clone());
        }
    }
    // Later documents override earlier ones for the same provider id.
    let mut deduped: Vec<CustomProvider> = Vec::new();
    for provider in out.custom {
        match deduped.iter().position(|existing| existing.id == provider.id) {
            Some(index) => deduped[index] = provider,
            None => deduped.push(provider),
        }
    }
    out.custom = deduped;
    out
}

/// Boots the engine + app shell + handler table and serves: the single
/// production entry (`main.rs` calls this).
pub async fn boot_and_serve(options: &BootOptions) -> io::Result<ServeHandle> {
    let config_location = options
        .config_location
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let loader = Loader::new(
        config_location,
        GlobalDirs {
            config: options.home.join(".config").join("opencode"),
            home: options.home.clone(),
        },
    );
    let providers = providers_from_config(&loader.load());

    let mut engine_options = EngineOptions::new(options.data_root.clone(), options.home.clone());
    engine_options.custom_providers = providers.custom;
    engine_options.default_model = providers.default_model;
    let engine = Engine::boot(engine_options).await;

    let mut app_state = AppState::new(
        engine,
        AuthConfig { password: options.password.clone() },
        options.version.clone(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    );
    app_state.cors_origins.extend(options.cors_origins.iter().cloned());
    let state = Arc::new(app_state);
    let shell = AppShell::new(Arc::clone(&state));
    let router = shell.router();
    lifecycle::serve(&options.serve, state, router, None).await
}

/// Parses an `IpAddr` for the CLI (`--hostname`).
#[must_use]
pub fn parse_hostname(raw: &str) -> Option<IpAddr> {
    use std::str::FromStr as _;
    IpAddr::from_str(raw).ok()
}

/// Loads a location's config the way the engine's assembly does (walk-up +
/// global dir + the `OPENCODE_CONFIG` explicit file).
#[must_use]
pub fn location_config(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> oc_core::config::Loaded {
    Loader::new(
        location.as_path().to_path_buf(),
        GlobalDirs {
            config: engine.home.join(".config").join("opencode"),
            home: engine.home.clone(),
        },
    )
    .load()
}

/// The config-defined custom providers visible at a location (for the
/// model/provider listing surfaces).
#[must_use]
pub fn location_custom_providers(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> Vec<CustomProvider> {
    providers_from_config(&location_config(engine, location)).custom
}

/// The config `model` default selection visible at a location.
#[must_use]
pub fn location_default_model(
    engine: &oc_core::engine::Engine,
    location: &oc_core::location::LocationRef,
) -> Option<oc_schema::config::ModelSelection> {
    providers_from_config(&location_config(engine, location)).default_model
}
