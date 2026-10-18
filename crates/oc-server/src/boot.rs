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
// One linear composition (config → plugins → engine → assets → shell →
// serve); the R-04 asset install pushed it one line over the pedantic
// budget and splitting it would only hide the boot ordering.
#[allow(clippy::too_many_lines)]
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
    // The global config dir is the SHARED v2 one (`~/.config/opencode`)
    // — the same source every other config consumer reads
    // (handlers/config.rs, handlers/agent.rs, announce.rs) and the dir
    // the v2 reference reads (xdg config/opencode). dogfood2 bug 3:
    // seeding providers from the `opencode-rust`-suffixed dir bound
    // ZERO config providers, so `providers.own` never reached the
    // model list and the boot default fell to the catalog fallback.
    let loader = Loader::new(
        config_location.clone(),
        GlobalDirs {
            config: options.home.join(".config").join("opencode"),
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
    // The production server runs the models-dev refresh loop (v2
    // `ModelsDev` layer spawns it whenever `fetch` is on — the CLI
    // default); direct engine boots stay hermetic.
    engine_options.catalog_fetch = true;
    // rust-test-infra (NOT a v2 divergence — v2 has no such switch, and
    // the production default above stays on): parity/test harnesses that
    // boot this binary set `OPENCODE_CATALOG_FETCH=0` so the boot-spawned
    // models-dev refresh loop never runs — an online test box would
    // otherwise publish `models-dev.refreshed` at a wall-clock-dependent
    // moment (the offline golden captures never contain the frame: v2
    // logs-and-ignores a failed fetch and publishes nothing).
    if let Ok(value) = std::env::var("OPENCODE_CATALOG_FETCH") {
        match value.trim().to_ascii_lowercase().as_str() {
            "0" | "false" | "off" | "no" => engine_options.catalog_fetch = false,
            _ => {}
        }
    }
    if let Ok(url) = std::env::var("OPENCODE_MODELS_DEV_URL") {
        engine_options.catalog_url = Some(url);
    }

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
    let boot_data_root = options.data_root.clone();
    let plugin_boot: Arc<crate::plugins::PluginBoot> = tokio::task::spawn_blocking(move || {
        crate::plugins::boot(
            &plugin_ops,
            &boot_loaded,
            &boot_home,
            &boot_directory,
            &boot_data_root,
            &boot_version,
        )
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
    // R-09 — the persistent-PTY cross-restart handover
    // (`server-process.ts:46-57` → `daemon.ts:403-428`): claim the
    // runtime root, then adopt the inherited handoff injected through
    // `OPENCODE_PTY_HANDOFF` by the replacement plumbing (the
    // `pty-handoff.ts` sidecar → env chain in `lifecycle.rs`). The env
    // var is consumed FIRST (children must not inherit it), invalid
    // payloads warn and start fresh, and every adoption failure inside
    // keeps the boot green — terminals are best-effort.
    adopt_inherited_pty(&mut app_state, &options.data_root.join("pty"));
    // R-04: populate the web-UI asset store (runtime `OPENCODE_APP_ASSETS`
    // override over the build-time embedded archive; absent both → empty,
    // API-only). A broken override fails boot loudly.
    crate::app_archive::install_at_boot(&mut app_state.assets)?;
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

/// R-09 boot wiring (`server-process.ts:46-57`): consume the inherited
/// `OPENCODE_PTY_HANDOFF` env (deleted before anything else so child
/// processes never see it), decode the handoff payload (invalid values
/// warn "Ignoring invalid PTY restart handoff; persistent terminals
/// will start fresh" and boot fresh — `lifecycle::decode_inherited_handoff`),
/// claim the runtime root, and adopt (`daemon.ts:403-428`). Never fails
/// the boot; the adoption outcome is logged.
pub fn adopt_inherited_pty(state: &mut crate::state::AppState, pty_root: &std::path::Path) {
    let inherited = std::env::var("OPENCODE_PTY_HANDOFF").ok();
    std::env::remove_var("OPENCODE_PTY_HANDOFF");
    let decoded = crate::lifecycle::decode_inherited_handoff(inherited.as_deref());
    if let Err(error) = state.persistent_pty.set_runtime_dir(pty_root) {
        eprintln!("Persistent terminals unavailable: {error}");
        return;
    }
    let Some(value) = decoded else { return };
    let Some(handoff) = handoff_from_wire(&value) else {
        eprintln!("Ignoring invalid PTY restart handoff; persistent terminals will start fresh");
        return;
    };
    match state.persistent_pty.adopt(&handoff) {
        oc_core::persistent_pty::Adoption::Adopted { restored, directory } => {
            eprintln!(
                "Adopted persistent terminals from restart handoff: {restored} restored at {directory}"
            );
        }
        oc_core::persistent_pty::Adoption::Discarded { .. } => {}
    }
}

/// Maps the wire `Handoff` (`directory`/`instanceID`/`ticket`/
/// `expiresAt`, `schema/persistent-pty.ts`) onto the core struct.
fn handoff_from_wire(value: &serde_json::Value) -> Option<oc_core::persistent_pty::Handoff> {
    Some(oc_core::persistent_pty::Handoff {
        directory: value.get("directory")?.as_str()?.to_owned(),
        instance_id: value.get("instanceID")?.as_str()?.to_owned(),
        ticket: value.get("ticket")?.as_str()?.to_owned(),
        expires_at: value.get("expiresAt")?.as_u64()?,
    })
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
            // The shared v2 global config dir — same source as the boot
            // seeding and every other config consumer (dogfood2 bug 3).
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

    /// R-09 — the boot-side handover wiring
    /// (`server-process.ts:46-57`): `OPENCODE_PTY_HANDOFF` is consumed
    /// (removed from the environment even when it decodes), a staged
    /// handoff from a "previous server" is adopted with its terminal
    /// state restored, and garbage payloads boot fresh.
    #[tokio::test]
    async fn boot_adopts_inherited_pty_handoff() {
        use oc_core::persistent_pty::PersistentPty;

        struct HandoffEnv;
        impl Drop for HandoffEnv {
            fn drop(&mut self) {
                std::env::remove_var("OPENCODE_PTY_HANDOFF");
            }
        }

        let tmp = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let pty_root = tmp.path().join("pty");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;

        // The "previous server": a terminal with output, then a clean
        // handoff ticket, then it dies (dropped — lock released).
        let previous = PersistentPty::new(engine.bus.as_ref().clone());
        previous.set_runtime_dir(&pty_root).expect("previous runtime dir");
        let session = oc_schema::id::SessionId::parse("ses_boot_pty").expect("session");
        let info = previous
            .create(
                &session,
                oc_core::persistent_pty::CreateInput {
                    command: Some("/bin/sh".to_owned()),
                    args: vec!["-c".to_owned(), "printf 'boot '; sleep 30".to_owned()],
                    cwd: Some("/tmp".to_owned()),
                    title: "boot-terminal".to_owned(),
                    env: std::collections::BTreeMap::new(),
                    size: Some((80, 10)),
                },
            )
            .await
            .expect("previous terminal");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while previous.get(&info.id).expect("get").output.1 < 5 {
            assert!(std::time::Instant::now() < deadline, "no terminal output");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let handoff = previous.handoff().expect("handoff").expect("ticket");
        previous.release_runtime();
        drop(previous);

        // Boot the replacement with the inherited env payload.
        let _env = HandoffEnv;
        std::env::set_var(
            "OPENCODE_PTY_HANDOFF",
            serde_json::json!({
                "directory": handoff.directory,
                "instanceID": handoff.instance_id,
                "ticket": handoff.ticket,
                "expiresAt": handoff.expires_at,
            })
            .to_string(),
        );
        let mut state = AppState::new(
            engine,
            AuthConfig { password: None },
            "test".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        );
        adopt_inherited_pty(&mut state, &pty_root);
        assert!(
            std::env::var("OPENCODE_PTY_HANDOFF").is_err(),
            "the env var is consumed at boot (children never inherit it)"
        );
        let restored = state.persistent_pty.get(&info.id).expect("terminal restored");
        assert_eq!(restored.title, "boot-terminal");
        assert_eq!(restored.status, oc_core::pty::Status::Exited);
        let listed = state.persistent_pty.list(None).expect("list");
        assert_eq!(listed.len(), 1);

        // Garbage payloads warn and boot fresh (list stays empty).
        let tmp2 = tempfile::tempdir_in("/tmp/opencode").expect("sandbox 2");
        let engine2 =
            Engine::boot(EngineOptions::new(tmp2.path().to_owned(), tmp2.path().to_owned())).await;
        std::env::set_var("OPENCODE_PTY_HANDOFF", "not-json");
        let mut fresh = AppState::new(
            engine2,
            AuthConfig { password: None },
            "test".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        );
        adopt_inherited_pty(&mut fresh, &tmp2.path().join("pty"));
        assert_eq!(fresh.persistent_pty.list(None).expect("list").len(), 0);
        assert!(
            fresh.persistent_pty.runtime_dir().is_some_and(|dir| dir.starts_with(
                tmp2.path().join("pty").to_str().expect("utf8")
            )),
            "a fresh runtime identity was claimed under the new root"
        );
    }

    /// dogfood2 bug 3 — the boot provider seeding and every per-location
    /// config load read the SHARED v2 global config dir
    /// (`<home>/.config/opencode`) — the same source handlers/config.rs
    /// serves and the ref reads (xdg config/opencode). A config-defined
    /// provider in the sanitized real-world shape (settings{apiKey,
    /// baseURL} + per-model `cost` ARRAY overlays) must bind at boot and
    /// stay visible at every location, together with the `model`
    /// default selection. The port once seeded from the
    /// `opencode-rust`-suffixed dir: zero providers bound, so the boot
    /// model silently fell back to the catalog default.
    #[tokio::test]
    async fn boot_binds_providers_from_the_shared_v2_config_dir() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let project = root.path().join("proj");
        std::fs::create_dir_all(&project).expect("project dir");
        let home = root.path().join("home");
        let global_config = home.join(".config").join("opencode");
        std::fs::create_dir_all(&global_config).expect("global config dir");
        std::fs::write(
            global_config.join("opencode.json"),
            r#"{
                "providers": {
                    "own": {
                        "name": "Own",
                        "package": "@opencode/ai/providers/anthropic",
                        "settings": { "apiKey": "sk-SYNTHETIC", "baseURL": "https://synth.invalid/v1" },
                        "models": {
                            "glm-5.3": {
                                "name": "GLM-5.3",
                                "cost": [ { "input": 0, "output": 0 } ],
                                "limit": { "context": 600000, "output": 131072 }
                            }
                        }
                    }
                },
                "model": "own/glm-5.3"
            }"#,
        )
        .expect("global config");
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

        // The engine-level seeding (boot) AND the per-location load both
        // see the shared dir's provider — the location surface is what
        // /api/model lists.
        let location = LocationRef::new(project.clone(), None);
        let providers = crate::boot::location_custom_providers(&handle.state.engine, &location);
        let own = providers
            .iter()
            .find(|provider| provider.id == "own")
            .expect("the shared-dir provider binds (dogfood2 bug 3)");
        assert_eq!(own.name.as_str(), "Own");
        assert_eq!(own.base_url, "https://synth.invalid/v1");
        assert_eq!(own.api_key.as_deref(), Some("sk-SYNTHETIC"));
        assert_eq!(
            own.models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            ["glm-5.3"],
            "the model overlay (cost ARRAY included) survives the binding"
        );
        let default = crate::boot::location_default_model(&handle.state.engine, &location)
            .expect("the model selection binds");
        assert_eq!(default.provider_id.as_str(), "own");
        assert_eq!(default.model.as_str(), "glm-5.3");

        handle.shutdown_and_join().await.expect("shutdown");
    }
}
