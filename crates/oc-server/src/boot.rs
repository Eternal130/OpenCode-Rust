//! The one-entry boot: engine (config-seeded custom providers) + app shell
//! (handler table merged, announcer wired) + lifecycle serve. This is the
//! composition the `oc-server` binary runs and the Phase 5 integration
//! contract for embedders (`boot_and_serve`).
//!
//! ## Config seeding + ready ordering
//!
//! The engine resolves models PER LOCATION (`EngineOptions::
//! custom_providers` seeds only the engine-level defaults — global-config
//! providers): every location assembles on FIRST USE with its own config
//! overlay (`oc_core::provider_catalog::overlay`), exactly like v2's lazy
//! `LocationServiceMap` (`provide: session => locations.get(session.location)`
//! — app-node-builder.ts; the parity goldens pin the boot cascade as
//! request-driven, so the boot location must NOT be eagerly assembled).
//!
//! Ready ordering (the standalone-boot race, mechanism (a)): the boot
//! config load + `EngineOptions` seeding run synchronously BEFORE
//! `Engine::boot`, which precedes the listener bind / readiness flip /
//! the stdio `{"url"}` ready line — provider binding cannot lose that
//! race. Per-location assembly is synchronous within the first request
//! (the per-location assembly lock), so a request can never observe a
//! half-bound location either.

use std::io;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;

use oc_core::config::{GlobalDirs, Loader};
use oc_core::engine::{Engine, EngineOptions};
use oc_core::location::LocationRef;
use oc_core::provider_catalog::facade::CustomProvider;
use oc_util::determinism::{SystemClock, UuidV4IdGen};

use crate::app::AppShell;
use crate::lifecycle::{self, ServeHandle, ServeOptions};
use crate::state::{AppState, AuthConfig};

pub use oc_core::provider_catalog::{providers_from_config, ConfigProviders};

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

// The config-derived provider bindings (`ConfigProviders` +
// `providers_from_config`) are re-exported from
// `oc_core::provider_catalog::overlay`, where the per-location provider
// overlay shares the mapping: every `providers.<id>` entry carrying a
// `settings.baseURL` becomes an openai-compatible `CustomProvider`
// handle, plus the `model` default selection.

/// Boots the engine + app shell + handler table and serves: the single
/// production entry (`main.rs` calls this).
pub async fn boot_and_serve(options: &BootOptions) -> io::Result<ServeHandle> {
    // One-way v2→rust data migration (lane B), BEFORE any database or
    // config resolution so the boot reads migrated state. Marker-latched:
    // the second entry point (after the CLI's own call) is one stat.
    oc_core::v2_migration::ensure_migrated_logged();
    let config_location = options
        .config_location
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    // One-line boot trace for registered-race hunting (oracle gate-7):
    // which config candidates were evaluated, which files actually loaded,
    // how many custom providers bound, and the spawn→assembly vs
    // spawn→ready timestamps. Logging only — no behavior change.
    let trace_boot = std::env::var("OPENCODE_TRACE_BOOT").ok().as_deref() == Some("1");
    let boot_start = std::time::Instant::now();
    let loader = Loader::new(
        config_location.clone(),
        GlobalDirs {
            config: options.home.join(".config").join(oc_util::global_roots::APP_NAME),
            home: options.home.clone(),
        },
    );
    let snapshot = loader.load();
    let providers = providers_from_config(&snapshot);
    if trace_boot {
        let sources = &snapshot.sources;
        let candidates = [
            sources.explicit.as_ref().map(|path| ("explicit", path)),
            sources.global.as_ref().map(|path| ("global", path)),
        ]
        .into_iter()
        .flatten()
        .map(|(kind, path)| format!("{kind}:{}", path.display()))
        .chain(sources.direct.iter().map(|path| format!("direct:{}", path.display())))
        .chain(sources.project.iter().map(|root| format!("project:{}", root.path.display())))
        .collect::<Vec<_>>()
        .join(" | ");
        let chosen: Vec<&str> = snapshot
            .entries
            .iter()
            .filter_map(|entry| match entry {
                oc_schema::config::Entry::Document(document) => document.path.as_deref(),
                oc_schema::config::Entry::Directory(_) => None,
            })
            .collect();
        eprintln!(
            "[boot-trace] t=0ms location={} candidates=[{candidates}] chosen={chosen:?} providers={} default_model={:?}",
            config_location.display(),
            providers.custom.len(),
            providers.default_model,
        );
    }

    let mut engine_options = EngineOptions::new(options.data_root.clone(), options.home.clone());
    engine_options.custom_providers = providers.custom;
    engine_options.default_model = providers.default_model;

    // The production plugin host (wave-12): boot from the resolved
    // config's `plugins[]` BEFORE `Engine::boot` — the engine options
    // carry its seams (the Code Mode runtime attaches even plugin-less
    // when the host is up: v2's `execute` synthesis is core, not
    // plugin-gated; MCP tools are codemode-default-true), and the
    // registration pass must settle before the first location
    // assembles (the boot module's ready-race rule). The blocking
    // wait runs on the blocking pool.
    let boot_loaded = snapshot.clone();
    let boot_home = options.home.clone();
    let boot_directory = config_location.clone();
    let boot_version = options.version.clone();
    let plugin_ops = crate::plugins::config_plugin_ops(&snapshot);
    let plugin_boot: Arc<crate::plugins::PluginBoot> = tokio::task::spawn_blocking(move || {
        crate::plugins::boot(&plugin_ops, &boot_loaded, &boot_home, &boot_directory, &boot_version)
    })
    .await
    .unwrap_or_else(|join_error| {
        eprintln!("opencode plugin: boot task panicked: {join_error}");
        Arc::new(crate::plugins::PluginBoot::disabled())
    });
    let seams = plugin_boot.engine_seams();
    engine_options.code_mode_runtime = seams.code_mode_runtime;
    engine_options.plugin_source = seams.plugin_source;

    // Config seeding above is synchronous and precedes Engine::boot, the
    // listener bind, and readiness — provider binding cannot lose the
    // ready race. Location graphs stay LAZY (v2 `LocationServiceMap` is
    // demand-built: `provide: session => locations.get(session.location)`;
    // the parity goldens pin the boot cascade as request-driven, so an
    // eager boot-location assembly here would diverge m1_scenarios).
    // Per-location assembly is synchronous within the first request.
    let engine = Engine::boot(engine_options).await;
    if trace_boot {
        eprintln!(
            "[boot-trace] t={}ms engine booted (catalog defaults bound; locations lazy)",
            boot_start.elapsed().as_millis()
        );
    }

    let mut app_state = AppState::new(
        engine,
        AuthConfig { password: options.password.clone() },
        options.version.clone(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    );
    app_state.cors_origins.extend(options.cors_origins.iter().cloned());
    // The plugin session port fills with the engine handle now that it
    // exists (ctx.session.* → the boot location's session machinery).
    plugin_boot.session_slot.fill(Arc::new(crate::plugins::EngineSessionPort {
        engine: Arc::clone(&app_state.engine),
        location: LocationRef::new(config_location.clone(), None),
    }));
    app_state.plugins = Some(plugin_boot);
    let state = Arc::new(app_state);
    let shell = AppShell::new(Arc::clone(&state));
    let router = shell.router();
    let on_listen = trace_boot.then(|| {
        Arc::new(move |addr: std::net::SocketAddr| {
            Box::pin(async move {
                eprintln!(
                    "[boot-trace] t={}ms listening {addr} (ready)",
                    boot_start.elapsed().as_millis()
                );
            }) as futures::future::BoxFuture<'static, ()>
        }) as lifecycle::OnListen
    });
    lifecycle::serve(&options.serve, state, router, on_listen).await
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
            config: engine.home.join(".config").join(oc_util::global_roots::APP_NAME),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Readiness;
    use oc_core::location::LocationRef;

    /// Redirects the env-resolved roots (HOME/XDG/OPENCODE_*) into a
    /// sandbox for the duration of a test, restoring on drop — the
    /// v2-migration call inside `boot_and_serve` resolves from env, so
    /// boot tests must never run against the developer's real $HOME.
    struct Env(Vec<(&'static str, Option<String>)>);
    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    impl Env {
        fn redirect_into(home: &std::path::Path) -> Self {
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
            let env = Self(vars.iter().map(|&name| (name, std::env::var(name).ok())).collect());
            for name in vars {
                std::env::remove_var(name);
            }
            std::env::set_var("HOME", home);
            env
        }
    }

    /// Ready ordering + lazy parity (the standalone-boot race, mechanism
    /// (a)): config seeding is synchronous BEFORE the engine boots and
    /// the listener binds — readiness can never precede provider
    /// binding — while location graphs stay demand-built like v2's
    /// `LocationServiceMap` (the m1 parity goldens pin the boot cascade
    /// as request-driven; eager assembly diverges them). The first use
    /// then assembles the boot location WITH its per-location provider
    /// overlay.
    #[tokio::test]
    async fn boot_lazy_assembles_with_overlay_on_first_use() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let project = root.path().join("proj");
        std::fs::create_dir_all(&project).expect("project dir");
        std::fs::write(
            project.join("opencode.json"),
            r#"{
                "model": { "providerID": "boot-order-probe", "model": "boot-one" },
                "providers": {
                    "boot-order-probe": {
                        "settings": { "baseURL": "https://boot-order.test/v1" },
                        "models": { "boot-one": {} }
                    }
                }
            }"#,
        )
        .expect("project config");
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).expect("home");

        // `boot_and_serve` runs the one-way v2 migration against the
        // ENV-resolved roots (both production entries derive
        // `BootOptions` from env) — redirect env into the sandbox so
        // the test never touches the developer's real $HOME.
        let _env = Env::redirect_into(&home);

        let options = BootOptions {
            serve: ServeOptions { hostname: IpAddr::from([127, 0, 0, 1]), port: Some(0) },
            password: None,
            cors_origins: Vec::new(),
            version: "test".to_owned(),
            data_root: root.path().join("data"),
            home,
            config_location: Some(project.clone()),
        };
        let handle = boot_and_serve(&options).await.expect("boot and serve");

        // Ready flipped with the config-seeded providers bound (synchronous
        // pre-boot), and the boot location is STILL demand-built (lazy v2
        // parity — no graph live before any request).
        assert_eq!(crate::readiness::current(&handle.state.readiness_rx), Readiness::Ready);
        let location = LocationRef::new(project.clone(), None);
        assert!(
            handle.state.engine.locations.get(&location).is_none(),
            "location graphs stay lazy (v2 parity)"
        );

        // First use assembles the location, and its per-location overlay
        // resolves the project's config-seeded custom provider.
        let services = handle.state.engine.location(location.clone()).await.expect("first use");
        let mut row = oc_core::database::schema::SessionRow::new(
            "ses_boot_order",
            "prj_boot_order",
            "/proj",
            "boot-order",
            "2.0.24",
        )
        .expect("row");
        row.model = None;
        let resolved = services.resolver.resolve(&row).expect("overlay resolves the config model");
        assert_eq!(resolved.reference.provider_id.0, "boot-order-probe");
        assert_eq!(resolved.reference.id.0, "boot-one");
        assert!(handle.state.engine.locations.get(&location).is_some());

        handle.shutdown_and_join().await.expect("shutdown");
    }
}
