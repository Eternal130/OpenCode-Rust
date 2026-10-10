//! The oc-core composition root (`Engine`) — the Phase 5 wave 0
//! integration surface (oracle gate 5, finding 2).
//!
//! `Engine::boot` assembles the process-wide services (database with
//! migrations, durable bus with projectors registered, provider catalog,
//! tool-output budget) and `Engine::location` assembles per-location
//! [`LocationServices`] bundles:
//!
//! - the location graph through [`crate::location_services::LocationServiceMap`]
//!   with REAL ports (per-location permission close, forms close, rpc
//!   close, session interruptor, config generation) — the Phase 3
//!   placeholder ports are gone;
//! - `ToolServices` + `builtin_plugins()` in the supervisor's **pre**
//!   slot (v2 internal.ts order), plus the shell / skill / question /
//!   webfetch / websearch / opencode / mcp-resource / subagent tool
//!   plugins and the agent-seeding plugin;
//! - the MCP registry (config-sourced, reconciled asynchronously) + the
//!   tool-registry bridge;
//! - `FormsService` with the bus, the shell service, jobs (+`CancelJobs`
//!   wired into `SessionExecution`), and the session environment;
//! - the config-lane `SkillTable` ([`skills::ConfigSkillTable`]);
//! - the AGENTS.md [`instructions::SessionInstructions`] production
//!   injection (in-flight claim + ledger + `session.synthetic`);
//! - `SessionRunner::with_snapshot` over the git snapshot store;
//!   `SessionExecution` over the real drain; `RouteLlmClient` (or an
//!   injected test client) + the catalog-driven model resolver — resolved
//!   PER LOCATION (config overlay first, engine-level bindings second).
//!
//! Server-shaped operations (prompt, interrupt, revert, list/messages,
//! permission ask/reply, forms, event subscribe, compact) live on
//! [`Engine`] as thin, typed wrappers Phase 5 handlers hang on.
//!
//! Graph note: the boot map's own `get_or_build` would build a blank
//! graph (its supervisor has no plugins); the engine is the only entry
//! point and builds each location's graph over per-location globals —
//! carrying that location's plugin supervisor and ports — then hands it
//! to the map ([`LocationServiceMap::adopt`]), which stays the lifecycle
//! authority (invalidate / shutdown / TTL sweeper).

#![allow(
    clippy::must_use_candidate, // ownership snapshots are queries
    clippy::needless_pass_by_value, // constructors consume their parts
    clippy::too_many_lines, // the assembly table mirrors the v2 layer graph
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity, // hook/executor seams match the lib's
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown, // TS identifiers in prose stay unquoted
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening, // assembly order is deliberate
    clippy::large_futures, // boot/assemble mirror the v2 layer composition
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation,
    clippy::elidable_lifetime_names,
    clippy::question_mark // the early-return shape mirrors the TS branch
)]

pub mod instructions;
pub mod resolver;
pub mod skills;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use oc_schema::id::SessionId;
use oc_util::determinism::{Clock, IdGen, RngSource, SystemClock, SystemRng, UuidV4IdGen};

use crate::bus::{Bus, BusOptions, Subscription};
use crate::config::{self, GlobalDirs, Loader};
use crate::database::Database;
use crate::forms::Forms;
use crate::hooks::{InternalPlugin, PluginSupervisor, RpcDefinition, RpcError, RpcPort};
use crate::jobs::Jobs;
use crate::location::{
    BootError, ConfigCache, FormsPort, LocationGraph, LocationRef, PermissionPending,
    SessionInterruptor,
};
use crate::location_services::{GlobalServices, LocationServiceMap};
use crate::mcp::McpRegistry;
use crate::permission::{Permission, ReplyInput};
use crate::provider_catalog::facade::{FacadeContext, ModelConnection};
use crate::provider_catalog::{providers_from_config, Catalog};
use crate::session::execution::{InterruptOptions, SessionExecution};
use crate::session::inbox::SessionInbox;
use crate::session::projector::install_projectors;
use crate::session::runner::compaction::SessionCompaction;
use crate::session::runner::context::{ContextDeps, McpServerSummary, SessionContext};
use crate::session::runner::drain::SessionRunner;
use crate::session::runner::model_request::{AppMetadata, SessionModelRequest};
use crate::session::runner::title::SessionTitle;
use crate::session::runner::{LlmClient, ModelResolver, RouteLlmClient};
use crate::session::store::{CreateInput, ListInput, MessagesInput, SessionStore};
use crate::session::subagent_job::SubagentJob;
use crate::shell::Shell;
use crate::snapshot::SnapshotService;
use crate::subagent::ChildExecution;
use crate::tool::shell::{ShellToolServices, SyntheticPublisher};
use crate::tool::skill::SkillTable;
use crate::tool::{
    self, opencode_tools, question, skill as skill_tool, webfetch, websearch, ToolServices,
};
use crate::tool_output::ToolOutput;
use oc_ai::route::SharedTransport;

// ===========================================================================
// Options
// ===========================================================================

/// Engine boot options; every field has a production default except the
/// filesystem roots.
#[derive(Clone)]
pub struct EngineOptions {
    /// Global data root (`<data>/opencode.db`, tool-output spills,
    /// snapshot stores, shell captures).
    pub data_root: PathBuf,
    /// Home directory (`~` expansion + config discovery).
    pub home: PathBuf,
    /// Provider catalog (default: the embedded models.dev snapshot).
    pub catalog: Arc<Catalog>,
    /// Injectable clock/ids/rng (tests).
    pub clock: Arc<dyn Clock>,
    pub ids: Arc<dyn IdGen>,
    pub rng: Arc<dyn RngSource>,
    /// The LLM client (default: [`RouteLlmClient`]; tests script TestLlm).
    pub llm: Option<Arc<dyn LlmClient>>,
    /// The model resolver (default: catalog-driven over the facade).
    pub model_resolver: Option<Arc<dyn ModelResolver>>,
    /// User provider connections (config `provider.<id>` settings).
    pub connections: BTreeMap<String, ModelConnection>,
    /// User-defined custom providers (config `provider.<id>` with baseURL).
    pub custom_providers: Vec<crate::provider_catalog::facade::CustomProvider>,
    /// Default model selection (config `model`).
    pub default_model: Option<oc_schema::config::ModelSelection>,
    /// Whether to spawn the activity TTL sweeper (default true).
    pub activity_sweeper: bool,
    /// The Code Mode interpreter seam. When present, every location
    /// registers the synthesized `execute` tool and code-mode-eligible
    /// tools (MCP, plugin tools) become invisible-direct behind it
    /// (v2's default surface). Absent ⇒ today's flat tool list.
    pub code_mode_runtime: Option<Arc<dyn crate::tool::codemode::CodeModeRuntime>>,
    /// The hard wall-clock bound on one `execute` program (default
    /// 120 s; v2 leaves program timeout unset by default — the port
    /// keeps the wave-1 "every bridge call bounded" rule).
    pub code_mode_timeout: std::time::Duration,
    /// The plugin registration source (the wave-12 production seam):
    /// each location's assembly applies its agents + tool
    /// registrations natively. `None` = no plugin host attached.
    pub plugin_source: Option<Arc<dyn crate::plugin_apply::LocationPluginSource>>,
}

impl EngineOptions {
    /// Options over explicit roots.
    #[must_use]
    pub fn new(data_root: PathBuf, home: PathBuf) -> Self {
        Self {
            data_root,
            home,
            catalog: Arc::new(Catalog::embedded().clone()),
            clock: Arc::new(SystemClock),
            ids: Arc::new(UuidV4IdGen),
            rng: Arc::new(SystemRng),
            llm: None,
            model_resolver: None,
            connections: BTreeMap::new(),
            custom_providers: Vec::new(),
            default_model: None,
            activity_sweeper: true,
            code_mode_runtime: None,
            code_mode_timeout: std::time::Duration::from_secs(120),
            plugin_source: None,
        }
    }
}

// ===========================================================================
// Shared registries (the composition root's cross-references)
// ===========================================================================

/// The per-location service registry: the engine's authority for "which
/// locations are assembled". Shared with the per-location ports (the
/// interruptor) and the subagent child-execution adapter, which resolve
/// bundles by location without Arc cycles.
#[derive(Default, Clone)]
struct ServiceRegistry(Arc<DashMap<LocationRef, Arc<LocationServices>>>);

impl ServiceRegistry {
    fn get(&self, location: &LocationRef) -> Option<Arc<LocationServices>> {
        self.0.get(location).map(|entry| Arc::clone(entry.value()))
    }

    /// Incumbent-wins insert, aligned with `LocationServiceMap::adopt`:
    /// an already-registered bundle stays the engine's answer for the
    /// location — a later assembler never displaces it. Returns the
    /// effective bundle and whether the fresh one won the slot.
    fn insert(
        &self,
        location: LocationRef,
        services: Arc<LocationServices>,
    ) -> (Arc<LocationServices>, bool) {
        match self.0.entry(location) {
            dashmap::mapref::entry::Entry::Occupied(existing) => {
                (Arc::clone(existing.get()), false)
            }
            dashmap::mapref::entry::Entry::Vacant(slot) => {
                let effective = Arc::clone(&services);
                slot.insert(services);
                (effective, true)
            }
        }
    }

    fn remove(&self, location: &LocationRef) {
        self.0.remove(location);
    }

    fn locations(&self) -> Vec<LocationRef> {
        self.0.iter().map(|entry| entry.key().clone()).collect()
    }
}

/// Everything the per-location plugin closures need beyond the graph's
/// own state (v2 plugins resolve these from the layer graph).
struct PluginInputs {
    location: LocationRef,
    services: ServiceRegistry,
    hooks: Arc<crate::hooks::HookRegistry>,
    agents: Arc<crate::location::AgentRegistry>,
    tools: Arc<crate::location::ToolRegistryState>,
    tool_services: Arc<ToolServices>,
    permission: Permission,
    forms: Forms,
    shell: Shell,
    jobs: Arc<Jobs>,
    session_env: Arc<crate::env::SessionEnv>,
    mcp: McpRegistry,
    skills: Arc<skills::ConfigSkillTable>,
    subagents: Arc<SubagentJob>,
    catalog: Arc<Catalog>,
    config_documents: Vec<oc_schema::config::Document>,
    home: PathBuf,
}

impl PluginInputs {
    /// The shell service handle the shell tool executes against.
    fn shell_for_tools(&self) -> Shell {
        self.shell.clone()
    }

    fn services(&self) -> ServiceRegistry {
        self.services.clone()
    }

    fn location(&self) -> LocationRef {
        self.location.clone()
    }
}

// ===========================================================================
// The engine
// ===========================================================================

/// The composition root. Clone-disallowed by design; share through `Arc`
/// if a caller needs to.
pub struct Engine {
    pub data_root: PathBuf,
    pub home: PathBuf,
    pub db: Arc<Database>,
    pub bus: Arc<Bus>,
    pub catalog: Arc<Catalog>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    rng: Arc<dyn RngSource>,
    llm: Arc<dyn LlmClient>,
    resolver: Arc<dyn ModelResolver>,
    /// The shared transport every per-location provider overlay binds
    /// onto (the engine resolver and each location overlay share one).
    facade_transport: SharedTransport,
    projects: Arc<crate::project::SqliteProjectStore>,
    tool_output: Arc<ToolOutput>,
    /// The v2 location service map (graph lifecycle authority: adopt /
    /// invalidate / reload + the TTL sweeper).
    pub locations: Arc<LocationServiceMap>,
    services: ServiceRegistry,
    inputs: Arc<DashMap<LocationRef, Arc<PluginInputs>>>,
    /// Per-location shutdown signals for the background tasks an
    /// assemble spawns (the MCP summary-cache refresh loop and the MCP
    /// reconcile); `invalidate` rings them so a superseded bundle's
    /// loops stop instead of leaking for the process lifetime.
    location_shutdown: Arc<DashMap<LocationRef, tokio::sync::watch::Sender<bool>>>,
    /// Per-location assembly locks: concurrent cold `location()` misses
    /// for the same location produce ONE bundle — both callers receive
    /// it. Entries stay for the engine's lifetime: removing one under
    /// waiters would fork the lock and re-open the double-assembly race.
    assembly_locks: Arc<DashMap<LocationRef, Arc<tokio::sync::Mutex<()>>>>,
    generation: Arc<AtomicU64>,
    sweeper: Option<tokio::task::JoinHandle<()>>,
    /// The Code Mode interpreter seam (drives the per-location `execute`
    /// registration in `assemble`); `None` keeps the flat tool surface.
    code_mode_runtime: Option<Arc<dyn crate::tool::codemode::CodeModeRuntime>>,
    code_mode_timeout: std::time::Duration,
    /// The plugin registration source (the wave-12 seam): applied at
    /// every location's assembly.
    plugin_source: Option<Arc<dyn crate::plugin_apply::LocationPluginSource>>,
}

impl Engine {
    /// Boots the engine: database + migrations, durable bus with
    /// projectors, catalog, global ports, and the location map.
    pub async fn boot(options: EngineOptions) -> Engine {
        std::fs::create_dir_all(&options.data_root).expect("data root");
        // P10-DB1: the engine's database path goes through the channel
        // scheme (`database::path`, the port of v2's `databasePath`) —
        // NOT a hard-pinned `opencode.db`. The channel comes from the
        // one workspace source (`oc_util::channel::current`), so the
        // engine, `oc-cli serve`, and a channel-matched v2 build all
        // resolve the SAME file (e.g. `OPENCODE_CHANNEL=stable` opens
        // `opencode-stable.db` like the pinned reference build).
        let db = Arc::new(
            Database::open_location(
                crate::database::engine_location(&options.data_root),
                crate::database::MigrationOptions::default(),
            )
            .await
            .expect("database"),
        );
        let bus = Arc::new(Bus::new(
            (*db).clone(),
            BusOptions {
                clock: options.clock.clone(),
                ids: options.ids.clone(),
                persist: true,
                ..BusOptions::deterministic()
            },
        ));
        install_projectors(&bus);
        let tool_output = Arc::new(ToolOutput::new(&options.data_root, options.clock.clone()));
        let projects = Arc::new(crate::project::SqliteProjectStore::new((*db).clone()));
        let llm: Arc<dyn LlmClient> =
            options.llm.clone().unwrap_or_else(|| Arc::new(RouteLlmClient));
        // One shared transport: the engine-level resolver and every
        // per-location provider overlay bind onto it.
        let facade_transport: SharedTransport =
            Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
        let resolver: Arc<dyn ModelResolver> =
            options.model_resolver.clone().unwrap_or_else(|| {
                Arc::new(resolver::CatalogModelResolver::new(
                    FacadeContext::new(Arc::clone(&facade_transport)),
                    Arc::clone(&options.catalog),
                    options.connections.clone(),
                    options.custom_providers.clone(),
                    options.default_model.clone(),
                ))
            });

        // The boot globals carry the engine-level ports; per-location
        // globals (built in `assemble`) swap in the location's plugin
        // supervisor and per-location close ports.
        let services = ServiceRegistry::default();
        let interruptor = Arc::new(EngineInterruptor { services: services.clone() });
        let config_cache = Arc::new(EngineConfigCache { generation: Arc::new(AtomicU64::new(1)) });
        let rpc = Arc::new(NoopRpcPort);
        let globals = GlobalServices::new(
            Arc::clone(&db),
            Arc::clone(&bus),
            Arc::clone(&projects) as Arc<dyn crate::project::ProjectStore>,
            Arc::clone(&options.catalog),
            options.clock.clone(),
            options.home.clone(),
            interruptor,
            config_cache,
            Arc::new(EngineLevelPermissionPort),
            Arc::new(EngineLevelFormsPort),
            rpc,
            PluginSupervisor::new(),
        );
        let locations = Arc::new(LocationServiceMap::new(Arc::new(globals)));
        let engine = Engine {
            data_root: options.data_root.clone(),
            home: options.home.clone(),
            db,
            bus,
            catalog: options.catalog,
            clock: options.clock.clone(),
            ids: options.ids.clone(),
            rng: options.rng.clone(),
            llm,
            resolver,
            facade_transport,
            projects,
            tool_output,
            locations: Arc::clone(&locations),
            services,
            inputs: Arc::new(DashMap::new()),
            location_shutdown: Arc::new(DashMap::new()),
            assembly_locks: Arc::new(DashMap::new()),
            generation: Arc::new(AtomicU64::new(1)),
            sweeper: options.activity_sweeper.then(|| {
                crate::location_services::spawn_activity_sweeper(
                    crate::location_services::SweeperConfig::default(),
                    Arc::clone(&locations),
                    Arc::clone(&locations.globals().activity),
                )
            }),
            code_mode_runtime: options.code_mode_runtime.clone(),
            code_mode_timeout: options.code_mode_timeout,
            plugin_source: options.plugin_source.clone(),
        };
        engine
    }

    /// The assembled per-location bundle (building on first use).
    pub async fn location(
        &self,
        location: LocationRef,
    ) -> Result<Arc<LocationServices>, BootError> {
        if let Some(existing) = self.services.get(&location) {
            return Ok(existing);
        }
        // Cold path — one assembler per location: the per-location lock
        // serializes concurrent misses, and the lock holder re-checks
        // the registry so every waiter receives the SAME bundle.
        let lock = Arc::clone(
            self.assembly_locks
                .entry(location.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .value(),
        );
        let _guard = lock.lock().await;
        if let Some(existing) = self.services.get(&location) {
            return Ok(existing);
        }
        let services = self.assemble(&location).await?;
        Ok(services)
    }

    /// Invalidates a location: graph shutdown sequence (per-location
    /// permission close → forms close → rpc close → `location.shutdown`)
    /// + service teardown.
    pub async fn invalidate(&self, location: &LocationRef) {
        self.services.remove(location);
        self.inputs.remove(location);
        // Stop the bundle's background tasks (the MCP summary-cache
        // refresh loop and the in-flight reconcile) — without this every
        // invalidate leaks one poll loop holding the superseded registry.
        if let Some((_, shutdown)) = self.location_shutdown.remove(location) {
            let _ = shutdown.send(true);
        }
        self.locations.invalidate(location).await;
    }

    /// Every live location.
    #[must_use]
    pub fn live_locations(&self) -> Vec<LocationRef> {
        self.services.locations()
    }

    /// Shuts the engine down: every location's shutdown sequence, then
    /// the sweeper.
    pub async fn shutdown(&self) {
        for location in self.live_locations() {
            self.invalidate(&location).await;
        }
        if let Some(sweeper) = &self.sweeper {
            sweeper.abort();
        }
    }

    // -------------------------------------------------------------------
    // Per-location assembly
    // -------------------------------------------------------------------

    async fn assemble(&self, location: &LocationRef) -> Result<Arc<LocationServices>, BootError> {
        // Project identity: worktree + project id.
        let project_resolver = crate::project::Resolver::new(
            Arc::clone(&self.projects) as Arc<dyn crate::project::ProjectStore>,
            Arc::new(crate::project::NoEvents),
        );
        let project = project_resolver.resolve(location.as_path()).await;
        let worktree = project.canonical.clone();

        // The config snapshot for this location. `OPENCODE_CONFIG` (the
        // explicit file source, v2 `Config.file`) applies to every load —
        // the boot-time provider seeding and the location config must see
        // the same document.
        let global_dirs = GlobalDirs {
            config: self.home.join(".config").join(oc_util::global_roots::APP_NAME),
            home: self.home.clone(),
        };
        let loaded = Loader::new(location.as_path().to_path_buf(), global_dirs).load();

        // Per-location provider overlay (v2 resolves providers per
        // location — `provider.ts` reads the layer's `Config`): custom
        // providers + the model default from THIS location's config,
        // consulted before the engine-level (boot/global) bindings. A
        // daemon booted in one directory thereby serves every project's
        // config-seeded providers.
        let location_providers = providers_from_config(&loaded);
        let resolver: Arc<dyn ModelResolver> =
            if crate::provider_catalog::overlay::is_empty(&location_providers) {
                Arc::clone(&self.resolver)
            } else {
                Arc::new(resolver::LocationModelResolver::new(
                    resolver::CatalogModelResolver::new(
                        FacadeContext::new(Arc::clone(&self.facade_transport)),
                        Arc::clone(&self.catalog),
                        BTreeMap::new(),
                        location_providers.custom,
                        location_providers.default_model,
                    ),
                    Arc::clone(&self.resolver),
                ))
            };

        // Agents: v2 builtins + config markdown ingestion (seeded by the
        // agent plugin during the graph build over THIS registry).
        let agents = Arc::new(crate::location::AgentRegistry::new());
        let tools = Arc::new(crate::location::ToolRegistryState::new());
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let config_documents = crate::config::agent_md::load_documents(&loaded.entries);

        // Permission engine (session/agent ruleset resolver over the row +
        // the agent table).
        let permission = Permission::new(
            Arc::clone(&self.ids),
            Arc::clone(&self.clock),
            Arc::new(crate::bus::sink::PermissionBusSink::new(
                (*self.bus).clone(),
                Some(schema_location(location)),
            )),
            Arc::new(crate::permission::sql::SqliteSavedGrantStore::new(
                (*self.db).clone(),
                Arc::clone(&self.ids),
                Arc::clone(&self.clock),
            )),
            Arc::new(SessionRuleResolver { db: (*self.db).clone(), agents: Arc::clone(&agents) }),
            oc_schema::id::ProjectId::parse(project.id.clone()),
            None,
        );

        // Forms + shell + jobs + env.
        let forms = Forms::new(Arc::clone(&self.ids), Arc::clone(&self.clock))
            .with_bus((*self.bus).clone());
        let shell = Shell::new(
            &self.data_root,
            &project.id,
            Arc::clone(&self.ids),
            Arc::clone(&self.clock),
        )
        .with_bus((*self.bus).clone());
        let jobs =
            Arc::new(Jobs::new((*self.db).clone(), Arc::clone(&self.ids), Arc::clone(&self.clock)));
        let session_env = Arc::new(crate::env::SessionEnv::new());

        // MCP registry (config-sourced) — server connections reconcile
        // asynchronously at boot like v2.
        let mcp_config = mcp_servers_from_config(&loaded);
        let mcp_source = Arc::new(move || mcp_config.clone());
        let mcp = McpRegistry::new((*self.bus).clone(), (*self.db).clone())
            .with_config_source(mcp_source);

        // AGENTS.md injection + skill table. The session-scoped services
        // (inbox, instructions, runner) publish through the
        // location-scoped bus handle so their frames carry the location
        // envelope (v2 `Bus.layer(ref)`); execution/usage stay global.
        let scoped_bus = (*self.bus).scoped(schema_location(location));
        let instructions = Arc::new(instructions::SessionInstructions::new(
            (*self.db).clone(),
            scoped_bus.clone(),
            worktree.clone(),
        ));
        let skills =
            Arc::new(skills::ConfigSkillTable::discover(&loaded, location.as_path(), &self.home));

        // The bundle's background-task shutdown: `invalidate` rings it
        // (and the sender's drop fires it on an assemble error path), so
        // the cache-refresh loop and the reconcile below stop with the
        // bundle instead of leaking past it.
        let (bundle_shutdown, mut cache_shutdown) = tokio::sync::watch::channel(false);
        let mut reconcile_shutdown = cache_shutdown.clone();

        // The MCP summary cache feeding the context guidance source.
        let mcp_cache: Arc<parking_lot::Mutex<Vec<McpServerSummary>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));
        let mcp_for_cache = mcp.clone();
        let cache_for_refresh = Arc::clone(&mcp_cache);
        tokio::spawn(async move {
            loop {
                let summaries = mcp_for_cache
                    .instructions()
                    .await
                    .into_iter()
                    .map(|item| McpServerSummary {
                        server: item.server,
                        instructions: item.instructions,
                        codemode: true,
                    })
                    .collect::<Vec<_>>();
                *cache_for_refresh.lock() = summaries;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                    _ = cache_shutdown.changed() => break,
                }
            }
        });

        // Tool services (the six filesystem leaves' dependencies).
        let tool_services = Arc::new(ToolServices {
            directory: location.as_path().to_path_buf(),
            project_directory: worktree.clone(),
            home: self.home.clone(),
            access: crate::file_access::FileAccess::new(
                permission.clone(),
                location.as_path().to_path_buf(),
                worktree.clone(),
                self.home.clone(),
            ),
            permission: permission.clone(),
            mutation: Arc::new(crate::file_mutation::FileMutation::new(
                location.as_path().to_path_buf(),
                self.home.clone(),
            )),
            formatter: Arc::new(crate::formatter::NoopFormatter::default()),
            instructions: Some(Arc::clone(&instructions) as Arc<dyn tool::InstructionInjection>),
            db: Some((*self.db).clone()),
            bus: Some((*self.bus).clone()),
        });

        // Subagent plumbing: the child-execution adapter resolves the
        // child's location services through the registry (children share
        // the parent's location).
        let subagents = Arc::new(SubagentJob::new(
            SessionStore::new((*self.db).clone()),
            SessionInbox::new((*self.db).clone(), (*self.bus).clone()),
            Arc::new(EngineChildExecution {
                services: self.services.clone(),
                location: location.clone(),
            }),
            Arc::new(crate::subagent_jobs_adapter::JobsAdapter(Arc::clone(&jobs))),
        ));

        // The graph: per-location globals carrying this location's plugin
        // supervisor (the v2 pre order) + per-location close ports.
        let inputs = Arc::new(PluginInputs {
            location: location.clone(),
            services: self.services.clone(),
            hooks: Arc::clone(&hooks),
            agents: Arc::clone(&agents),
            tools: Arc::clone(&tools),
            tool_services: Arc::clone(&tool_services),
            permission: permission.clone(),
            forms: forms.clone(),
            shell: shell.clone(),
            jobs: Arc::clone(&jobs),
            session_env: Arc::clone(&session_env),
            mcp: mcp.clone(),
            skills: Arc::clone(&skills),
            subagents: Arc::clone(&subagents),
            catalog: Arc::clone(&self.catalog),
            config_documents,
            home: self.home.clone(),
        });
        self.inputs.insert(location.clone(), Arc::clone(&inputs));
        let graph = self.build_graph(location.clone(), &inputs).await?;

        // The wave-12 plugin seam: the embedder's plugin host's
        // registration snapshot applies into THIS location's registries
        // (agents via plugin_apply, tool rows + executors natively) —
        // BEFORE the Code Mode surface so the snapshot shaping sees the
        // plugin tools (code-mode-eligible ones hide behind `execute`
        // exactly like MCP tools).
        if let Some(source) = &self.plugin_source {
            let applied =
                crate::plugin_apply::apply_location_plugins(source.as_ref(), &agents, &tools);
            let _ = applied; // diagnostics only (boot-trace reads the registry)
        }

        // The Code Mode surface (wave 4): with a runtime attached, the
        // synthesized `execute` tool registers behind the standard
        // pipeline (before/after hooks + decode wrap it and every
        // nested call). The request shaping and the catalog instruction
        // key off the executor's presence.
        if let Some(runtime) = self.code_mode_runtime.clone() {
            crate::tool::codemode::register_code_mode(
                &graph.tools,
                &graph.hooks,
                &tool_services,
                runtime,
                self.code_mode_timeout,
            );
        }

        // Runner stack over the graph's registries.
        let store = SessionStore::new((*self.db).clone());
        let inbox = SessionInbox::new((*self.db).clone(), scoped_bus.clone());
        let deps = ContextDeps {
            clock: Arc::clone(&self.clock),
            discovery: {
                let discovery_location = location.as_path().to_path_buf();
                let discovery_project = worktree.clone();
                let discovery_home = self.home.clone();
                Arc::new(move || {
                    let scope = crate::config::instruction::Scope {
                        start: &discovery_location,
                        home: &discovery_home,
                        project_root: Some(discovery_project.as_path()),
                        global_file: &discovery_home
                            .join(".config")
                            .join(oc_util::global_roots::APP_NAME)
                            .join("AGENTS.md"),
                    };
                    match crate::config::instruction::discover(
                        &scope,
                        crate::config::instruction::DiscoveryFlags::all(),
                    ) {
                        crate::config::instruction::InstructionState::Available(files) => files
                            .into_iter()
                            .map(|file| (file.path.to_string_lossy().into_owned(), file.content))
                            .collect(),
                        crate::config::instruction::InstructionState::Unavailable => Vec::new(),
                    }
                })
            },
            environment: {
                let directory = location.as_path().to_path_buf();
                let project_directory = worktree.clone();
                let is_git = project.vcs.is_some();
                let tmp = self.data_root.clone();
                Arc::new(move || {
                    crate::session::runner::context::environment_block(
                        &directory.to_string_lossy(),
                        &project_directory.to_string_lossy(),
                        is_git,
                        &tmp.to_string_lossy(),
                    )
                })
            },
            catalog: Arc::clone(&self.catalog),
            mcp: {
                let cache = Arc::clone(&mcp_cache);
                Arc::new(move || cache.lock().clone())
            },
            skills: {
                let skills = Arc::clone(&skills);
                Arc::new(move || skills.list().to_vec())
            },
        };
        let context = Arc::new(SessionContext::new(
            (*self.db).clone(),
            scoped_bus.clone(),
            location.clone(),
            Arc::clone(&graph.agents),
            Arc::clone(&graph.tools),
            Arc::clone(&resolver),
            deps,
        ));
        let requests = Arc::new(SessionModelRequest::new(
            Arc::clone(&graph.hooks),
            Arc::clone(&graph.tools),
            AppMetadata::default(),
        ));
        let compaction = Arc::new(SessionCompaction::new(
            (*self.db).clone(),
            (*self.bus).clone(),
            Arc::clone(&self.llm),
            Arc::clone(&requests),
            Arc::clone(&graph.agents),
            Arc::clone(&resolver),
            Arc::clone(&self.rng),
        ));
        let title = Arc::new(SessionTitle::new(
            (*self.db).clone(),
            (*self.bus).clone(),
            Arc::clone(&self.llm),
            Arc::clone(&requests),
            Arc::clone(&context),
            Arc::clone(&self.clock),
        ));
        let scope = crate::snapshot::location_scope(&worktree, location.as_path())
            .unwrap_or_else(|| ".".to_string());
        let snapshot: Arc<dyn SnapshotService> = Arc::new(
            crate::snapshot::GitSnapshotStore::open(
                &self.data_root,
                &project.id,
                &worktree,
                &scope,
            )
            .map_err(|error| {
                BootError::Database(crate::database::error::DbError::Pragma(error.to_string()))
            })?,
        );
        let location_dir = location.as_path().to_path_buf();
        let runner = Arc::new(
            SessionRunner::new(
                (*self.db).clone(),
                scoped_bus.clone(),
                inbox.clone(),
                store.clone(),
                context,
                compaction,
                title,
                Arc::clone(&self.llm),
                requests,
                Arc::clone(&self.clock),
                Arc::clone(&self.rng),
                Arc::clone(&self.ids),
                Arc::new(move || !location_dir.exists()),
            )
            .with_snapshot(Arc::clone(&snapshot))
            .with_tool_output(Arc::clone(&self.tool_output)),
        );
        let jobs_for_cancel = Arc::clone(&jobs);
        let execution = SessionExecution::with_clock(
            store.clone(),
            (*self.bus).clone(),
            inbox.clone(),
            Some(Arc::new(JobsCancel { jobs: jobs_for_cancel })),
            runner.drain_fn(),
            (Arc::clone(&self.ids), Arc::clone(&self.clock)),
        );

        // MCP server reconcile (async connections) + tool bridge attach.
        // The reconcile observes the same bundle shutdown: an invalidate
        // stops the connection attempts together with the bundle.
        let mcp_for_reconcile = mcp.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = mcp_for_reconcile.reconcile() => {}
                _ = reconcile_shutdown.changed() => {}
            }
        });
        let _bridge = crate::mcp::McpToolBridge::install(&mcp, &graph.tools, &permission);

        let services = Arc::new(LocationServices {
            graph,
            project,
            permission,
            forms,
            shell,
            jobs,
            session_env,
            mcp,
            skills,
            instructions,
            snapshot,
            store,
            inbox,
            execution,
            runner,
            resolver,
        });
        let (effective, fresh_won) = self.services.insert(location.clone(), Arc::clone(&services));
        if fresh_won {
            self.location_shutdown.insert(location.clone(), bundle_shutdown);
        } else {
            // An incumbent bundle is already registered (a lifecycle race
            // even the assembly lock cannot exclude): stop the fresh
            // bundle's background tasks so it does not leak — the same
            // signal `invalidate` sends.
            let _ = bundle_shutdown.send(true);
        }
        Ok(effective)
    }

    async fn build_graph(
        &self,
        location: LocationRef,
        inputs: &Arc<PluginInputs>,
    ) -> Result<Arc<LocationGraph>, BootError> {
        // The pre plugins in the v2 internal.ts order: the six filesystem
        // leaves (patch, edit, glob, grep, read, write) then the wider
        // tool surface (opencode, mcp-resource, question, shell, skill,
        // subagent, webfetch, websearch) + the agent seeder.
        let mut pre = tool::builtin_plugins(Arc::clone(&inputs.tool_services));
        pre.push(opencode_plugin(inputs));
        pre.push(mcp_resource_plugin(inputs));
        pre.push(question_plugin(inputs));
        pre.push(shell_plugin(self, inputs));
        pre.push(skill_plugin(inputs));
        pre.push(subagent_plugin(inputs));
        pre.push(webfetch_plugin(inputs));
        pre.push(websearch_plugin(inputs));
        pre.push(agent_plugin(inputs));

        // Per-location globals: the shared handles + this location's
        // supervisor and close ports.
        let base = self.locations.globals();
        let globals = GlobalServices::new(
            Arc::clone(&base.db),
            Arc::clone(&base.bus),
            Arc::clone(&base.projects),
            Arc::clone(&base.catalog),
            Arc::clone(&base.clock),
            base.home.clone(),
            Arc::clone(&base.interruptor),
            Arc::clone(&base.config_cache),
            Arc::new(LocationPermissionPort { permission: inputs.permission.clone() }),
            Arc::new(LocationFormsPort { forms: inputs.forms.clone() }),
            Arc::new(NoopRpcPort),
            PluginSupervisor::with_internal(pre, Vec::new()),
        );
        let generation = self.generation.fetch_add(1, Ordering::SeqCst);
        let graph = crate::location::LocationGraph::build_with_state(
            location,
            &globals,
            generation,
            crate::location::LocationStateSeed {
                hooks: Arc::clone(&inputs.hooks),
                agents: Arc::clone(&inputs.agents),
                tools: Arc::clone(&inputs.tools),
            },
        )
        .await?;
        Ok(self.locations.adopt(graph))
    }

    // -------------------------------------------------------------------
    // Server-shaped operations
    // -------------------------------------------------------------------

    /// `sessions.prompt`: admit a user prompt (steer) and await the
    /// execution's exit.
    pub async fn prompt(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
        message_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services
            .inbox
            .admit(
                message_id,
                session_id.as_str(),
                oc_schema::session_inbox::Item::User {
                    payload: oc_schema::session_inbox::UserPayload {
                        text: text.to_string(),
                        files: None,
                        agents: None,
                        skills: None,
                        metadata: None,
                    },
                    delivery: oc_schema::session_inbox::Delivery::Steer,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        services.execution.resume(session_id.as_str()).await.map_err(|error| error.to_string())
    }

    /// `sessions.interrupt`: interrupt the active execution as the user,
    /// awaiting settlement.
    pub async fn interrupt(&self, location: &LocationRef, session_id: &SessionId) -> bool {
        self.interrupt_with_reason(
            location,
            session_id,
            Some(oc_schema::session_event::InterruptReason::User),
        )
        .await
    }

    /// `sessions.interrupt` with an explicit reason.
    pub async fn interrupt_with_reason(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
        reason: Option<oc_schema::session_event::InterruptReason>,
    ) -> bool {
        let Ok(services) = self.location(location.clone()).await else { return false };
        services
            .execution
            .interrupt(
                session_id.as_str(),
                InterruptOptions { reason, await_settlement: true, ..InterruptOptions::default() },
            )
            .await
    }

    /// `sessions.create`-shaped convenience over the store.
    pub async fn create_session(
        &self,
        location: &LocationRef,
        input: CreateInput,
    ) -> Result<crate::database::schema::SessionRow, String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        match services.store.create(&(*services.graph.bus).clone(), input).await {
            Ok(crate::session::store::CreateOutcome::Created(row))
            | Ok(crate::session::store::CreateOutcome::Existing(row)) => Ok(row),
            Err(error) => Err(error.to_string()),
        }
    }

    /// `sessions.list`.
    pub async fn sessions(
        &self,
        location: &LocationRef,
        input: ListInput,
    ) -> Result<Vec<crate::database::schema::SessionRow>, String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services.store.list(input).await.map_err(|error| error.to_string())
    }

    /// `sessions.get`.
    pub async fn session(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
    ) -> Result<Option<crate::database::schema::SessionRow>, String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services.store.get(session_id.as_str()).await.map_err(|error| error.to_string())
    }

    /// `sessions.move`: admit a location-move item; the runner moves the
    /// session at the next safe boundary (the drain's Move handling).
    pub async fn move_session(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
        message_id: &str,
        destination: oc_schema::location::Ref,
        project_id: oc_schema::id::ProjectId,
        subpath: Option<oc_schema::common::RelativePath>,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services
            .inbox
            .admit(
                message_id,
                session_id.as_str(),
                oc_schema::session_inbox::Item::Move_ {
                    payload: oc_schema::session_inbox::MovePayload {
                        location: destination,
                        project_id,
                        subpath,
                    },
                    delivery: oc_schema::session_inbox::Delivery::Queue,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// `sessions.messages`.
    pub async fn messages(
        &self,
        location: &LocationRef,
        input: MessagesInput,
    ) -> Result<Vec<crate::database::schema::SessionMessageRow>, String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services.store.messages(input).await.map_err(|error| error.to_string())
    }

    /// `session.revert` stage.
    pub async fn revert_stage(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
        message_id: &str,
        include_files: bool,
    ) -> Result<oc_schema::session_support::Revert, String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        let message_id = oc_schema::id::MessageId::parse(message_id.to_string())
            .unwrap_or_else(|error| oc_schema::id::MessageId(error.value));
        crate::session::revert::stage(
            &services.graph.db,
            &services.graph.bus,
            services.snapshot.as_ref(),
            session_id,
            &message_id,
            include_files,
        )
        .await
    }

    /// `session.revert` clear.
    pub async fn revert_clear(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        crate::session::revert::clear(
            &services.graph.db,
            &services.graph.bus,
            services.snapshot.as_ref(),
            session_id,
        )
        .await
    }

    /// `session.revert` commit.
    pub async fn revert_commit(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
        state: &crate::session::revert::SessionRevertState,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        crate::session::revert::commit(&services.graph.bus, session_id, state).await
    }

    /// Permission ask list (pending requests for a session).
    pub fn permission_pending(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
    ) -> Vec<oc_schema::permission::Request> {
        let Some(services) = self.services.get(location) else { return Vec::new() };
        services.permission.for_session(session_id)
    }

    /// Permission reply.
    pub async fn permission_reply(
        &self,
        location: &LocationRef,
        input: ReplyInput,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        services.permission.reply(input).await.map_err(|error| error.to_string())
    }

    /// Forms: get one form.
    pub fn form_get(
        &self,
        location: &LocationRef,
        id: &oc_schema::id::FormId,
    ) -> Result<oc_schema::form::Info, String> {
        let services =
            self.services.get(location).ok_or_else(|| "location not assembled".to_string())?;
        services.forms.get(id).map_err(|error| error.to_string())
    }

    /// Forms: reply.
    pub async fn form_reply(
        &self,
        location: &LocationRef,
        id: &oc_schema::id::FormId,
        answer: oc_schema::form::Answer,
    ) -> Result<(), String> {
        let services =
            self.services.get(location).ok_or_else(|| "location not assembled".to_string())?;
        services.forms.reply(id, answer).await.map_err(|error| error.to_string())
    }

    /// The event feed: a bus subscription filtered by the latest version
    /// of each requested type (empty slice = everything).
    #[must_use]
    pub fn subscribe(&self, types: &[&str]) -> Subscription {
        if types.is_empty() {
            self.bus.subscribe()
        } else {
            self.bus.subscribe_types(types)
        }
    }

    /// Manual compaction: admit a `/compact` item and run it to
    /// completion.
    pub async fn compact(
        &self,
        location: &LocationRef,
        session_id: &SessionId,
    ) -> Result<(), String> {
        let services = self.location(location.clone()).await.map_err(|error| error.to_string())?;
        let input_id = self.ids.new_id("msg");
        services
            .inbox
            .admit_compaction(
                &input_id,
                session_id.as_str(),
                oc_schema::session_inbox::Delivery::Queue,
            )
            .await
            .map_err(|error| error.to_string())?;
        services.execution.resume(session_id.as_str()).await.map_err(|error| error.to_string())
    }
}

// ===========================================================================
// LocationServices
// ===========================================================================

/// One location's fully assembled service bundle.
pub struct LocationServices {
    pub graph: Arc<LocationGraph>,
    pub project: crate::project::Resolved,
    pub permission: Permission,
    pub forms: Forms,
    pub shell: Shell,
    pub jobs: Arc<Jobs>,
    pub session_env: Arc<crate::env::SessionEnv>,
    pub mcp: McpRegistry,
    pub skills: Arc<skills::ConfigSkillTable>,
    pub instructions: Arc<instructions::SessionInstructions>,
    pub snapshot: Arc<dyn SnapshotService>,
    pub store: SessionStore,
    pub inbox: SessionInbox,
    pub execution: SessionExecution,
    pub runner: Arc<SessionRunner>,
    /// This location's model resolver: the config overlay first, then the
    /// engine-level bindings (per-location provider wiring).
    pub resolver: Arc<dyn ModelResolver>,
}

// ===========================================================================
// Ports (the real implementations)
// ===========================================================================

/// The TTL sweeper's interruptor: interrupts every active execution of
/// the expired location and awaits settlement.
struct EngineInterruptor {
    services: ServiceRegistry,
}

#[async_trait::async_trait]
impl SessionInterruptor for EngineInterruptor {
    async fn interrupt(&self, location: &LocationRef, reason: &str) {
        let reason = match reason {
            "inactivity" => oc_schema::session_event::InterruptReason::Inactivity,
            _ => oc_schema::session_event::InterruptReason::Shutdown,
        };
        if let Some(services) = self.services.get(location) {
            for session_id in services.execution.active() {
                let _ = services
                    .execution
                    .interrupt(
                        &session_id,
                        InterruptOptions {
                            reason: Some(reason),
                            await_settlement: true,
                            ..Default::default()
                        },
                    )
                    .await;
            }
        }
    }
}

struct EngineConfigCache {
    generation: Arc<AtomicU64>,
}

impl ConfigCache for EngineConfigCache {
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

/// The per-location permission close (reject-all pending, v2 shutdown
/// step 1).
struct LocationPermissionPort {
    permission: Permission,
}

#[async_trait::async_trait]
impl PermissionPending for LocationPermissionPort {
    async fn close(&self) -> Vec<String> {
        self.permission.close().await;
        Vec::new()
    }

    async fn pending_count(&self) -> usize {
        self.permission.list().len()
    }
}

/// The per-location forms close (v2 shutdown step 2).
struct LocationFormsPort {
    forms: Forms,
}

#[async_trait::async_trait]
impl FormsPort for LocationFormsPort {
    async fn close(&self) -> Vec<String> {
        let _ = self.forms.close().await;
        Vec::new()
    }
}

/// The engine-level port placeholders (boot globals only; every adopted
/// graph carries its own per-location ports).
struct EngineLevelPermissionPort;

#[async_trait::async_trait]
impl PermissionPending for EngineLevelPermissionPort {
    async fn close(&self) -> Vec<String> {
        Vec::new()
    }

    async fn pending_count(&self) -> usize {
        0
    }
}

struct EngineLevelFormsPort;

#[async_trait::async_trait]
impl FormsPort for EngineLevelFormsPort {
    async fn close(&self) -> Vec<String> {
        Vec::new()
    }
}

/// A no-op RPC port (host plugins arrive with the plugin lane).
struct NoopRpcPort;

#[async_trait::async_trait]
impl RpcPort for NoopRpcPort {
    async fn register(
        &self,
        _definition: RpcDefinition,
        _handler: Arc<dyn crate::hooks::RpcHandler>,
    ) -> Result<crate::hooks::Registration, RpcError> {
        Err(RpcError("rpc port not available".into()))
    }

    async fn emit(
        &self,
        _rpc_id: &str,
        _event: &str,
        _data: serde_json::Value,
    ) -> Result<(), RpcError> {
        Err(RpcError("rpc port not available".into()))
    }

    async fn close(&self) {}
}

// ===========================================================================
// CancelJobs + child execution
// ===========================================================================

struct JobsCancel {
    jobs: Arc<Jobs>,
}

#[async_trait::async_trait]
impl crate::session::execution::CancelJobs for JobsCancel {
    async fn cancel(&self, session_id: &str) {
        self.jobs.cancel_session(session_id).await;
    }
}

/// The subagent lane's child execution: children share the parent's
/// location, so resume/interrupt route through that location's execution
/// service.
struct EngineChildExecution {
    services: ServiceRegistry,
    location: LocationRef,
}

#[async_trait::async_trait]
impl ChildExecution for EngineChildExecution {
    async fn resume(&self, session_id: &str) -> Result<(), String> {
        let Some(services) = self.services.get(&self.location) else {
            return Err(format!("location not assembled: {}", self.location));
        };
        services.execution.resume(session_id).await.map(|_| ()).map_err(|error| error.to_string())
    }

    async fn interrupt(&self, session_id: &str) {
        if let Some(services) = self.services.get(&self.location) {
            let _ = services
                .execution
                .interrupt(
                    session_id,
                    InterruptOptions { await_settlement: true, ..Default::default() },
                )
                .await;
        }
    }
}

// ===========================================================================
// The pre plugins beyond the six filesystem leaves
// ===========================================================================

/// `opencode` tools (session_rename + models; `session_move` stays a
/// deferred stub per the v2 TODO).
fn opencode_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.opencode", move |hooks, ports| {
        let db = inputs.tool_services.db.clone().expect("tool services db");
        let bus = inputs.tool_services.bus.clone().expect("tool services bus");
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            opencode_tools::session_rename_tool(SessionStore::new(db), bus),
        )?;
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            opencode_tools::models_tool(inputs.catalog.as_ref().clone()),
        )
    })
}

fn mcp_resource_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.mcp-resource", move |_hooks, ports| {
        crate::tool::mcp_resource::register(
            ports.tools,
            inputs.mcp.clone(),
            inputs.permission.clone(),
        );
        Ok(())
    })
}

fn question_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.question", move |hooks, ports| {
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            question::tool(inputs.forms.clone()),
        )
    })
}

fn shell_plugin(engine: &Engine, inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    let bus = (*engine.bus).clone();
    InternalPlugin::new("opencode.tool.shell", move |hooks, ports| {
        let deps = Arc::new(ShellToolServices {
            shell: inputs.shell_for_tools(),
            jobs: (*inputs.jobs).clone(),
            env: (*inputs.session_env).clone(),
            portable_scanner: false,
            synthetic: Arc::new(BusSyntheticPublisher { bus: bus.clone() }),
        });
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            crate::tool::shell::tool(deps),
        )
    })
}

fn skill_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.skill", move |hooks, ports| {
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            skill_tool::tool(Arc::clone(&inputs.skills) as Arc<dyn SkillTable>),
        )
    })
}

fn subagent_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.subagent", move |hooks, ports| {
        let _ = hooks;
        let db = inputs.tool_services.db.clone().expect("tool services db");
        let bus = inputs.tool_services.bus.clone().expect("tool services bus");
        let models = Arc::new(CatalogModelSource { catalog: Arc::clone(&inputs.catalog) });
        let agents = Arc::clone(&inputs.agents);
        let deps = crate::tool::subagent::SubagentDeps {
            store: SessionStore::new(db.clone()),
            inbox: SessionInbox::new(db, bus),
            execution: Arc::new(EngineChildExecution {
                services: inputs.services(),
                location: inputs.location(),
            }),
            jobs: Arc::new(crate::subagent_jobs_adapter::JobsAdapter(Arc::clone(&inputs.jobs))),
            subagents: Arc::clone(&inputs.subagents),
            models,
            permission: inputs.permission.clone(),
            depth: Arc::new(|| 1),
            agents: Arc::new(move || {
                agents.list().into_iter().filter(|info| !info.hidden).collect()
            }),
            progress: None,
            interrupt: None,
        };
        crate::tool::subagent::register(
            ports.tools,
            Arc::new(crate::tool::subagent::SubagentTool::new(deps)),
        );
        Ok(())
    })
}

fn webfetch_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.webfetch", move |hooks, ports| {
        tool::register_builtin(ports.tools, hooks, &inputs.tool_services, webfetch::tool())
    })
}

fn websearch_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let inputs = Arc::clone(inputs);
    InternalPlugin::new("opencode.tool.websearch", move |hooks, ports| {
        let db = inputs.tool_services.db.clone().expect("tool services db");
        let search = Arc::new(crate::tool::websearch::WebSearch::new(db, Vec::new()));
        tool::register_builtin(
            ports.tools,
            hooks,
            &inputs.tool_services,
            websearch::tool(Arc::new(websearch::WebSearchExecute::new(
                search,
                inputs.forms.clone(),
            ))),
        )
    })
}

/// The agent-seeding plugin (v2 `opencode.agent`): builtins + config
/// markdown agents.
fn agent_plugin(inputs: &Arc<PluginInputs>) -> InternalPlugin {
    let documents = inputs.config_documents.clone();
    let home = inputs.home.clone();
    InternalPlugin::new("opencode.agent", move |_hooks, ports| {
        seed_builtin_agents(ports.agents);
        let mut editor = RegistryAgentEditor::new(Arc::clone(ports.agents));
        crate::config::agent_md::transform(&documents, &home, &mut editor);
        if let Some(default) = editor.default.clone() {
            ports.agents.set_default(Some(oc_schema::id::AgentId::parse(default)));
        }
        Ok(())
    })
}

/// `sessions.synthetic` publisher for shell notifications.
struct BusSyntheticPublisher {
    bus: Bus,
}

impl SyntheticPublisher for BusSyntheticPublisher {
    fn publish(
        &self,
        session_id: &str,
        description: &str,
        text: &str,
        metadata: serde_json::Value,
        notification_id: Option<&str>,
    ) {
        let bus = self.bus.clone();
        let data = serde_json::json!({
            "sessionID": session_id,
            "text": text,
            "description": description,
            "metadata": metadata,
        });
        let _ = notification_id;
        tokio::spawn(async move {
            let def = crate::bus::registry::by_type_latest("session.synthetic")
                .unwrap_or_else(|| panic!("no session.synthetic definition"));
            let _ = bus.publish(def, data, crate::bus::PublishOptions::default()).await;
        });
    }
}

/// The subagent lane's model source over the catalog.
struct CatalogModelSource {
    catalog: Arc<Catalog>,
}

impl crate::subagent::ModelSource for CatalogModelSource {
    fn model(&self, provider_id: &str, model_id: &str) -> Option<crate::subagent::ModelEntry> {
        crate::subagent::ModelSource::model(self.catalog.as_ref(), provider_id, model_id)
    }
}

// ===========================================================================
// Agent seeding (v2 plugin/agent.ts)
// ===========================================================================

const PROMPT_TITLE: &str = "You are a title generator. You output ONLY a thread title. Nothing else.\n\n<task>\nGenerate a brief title that would help the user find this conversation later.\n\nFollow all rules in <rules>\nUse the <examples> so you know what a good title looks like.\nYour output must be:\n- A single line\n- <=50 characters\n- No explanations\n</task>\n\n<rules>\n- you MUST use the same language as the user message you are summarizing\n- Title must be grammatically correct and read naturally - no word salad\n- Never include tool names in the title (e.g. \"read tool\", \"bash tool\", \"edit tool\")\n- Focus on the main topic or question the user needs to retrieve\n- Vary your phrasing - avoid repetitive patterns like always starting with \"Analyzing\"\n- When a file is mentioned, focus on WHAT the user wants to do WITH the file, not just that they shared it\n- Keep exact: technical terms, numbers, filenames, HTTP codes\n- Remove: the, this, my, a, an\n- Never assume tech stack\n- Never use tools\n- NEVER respond to questions, just generate a title for the conversation\n- The title should NEVER include \"summarizing\" or \"generating\" when generating a title\n- DO NOT SAY YOU CANNOT GENERATE A TITLE OR COMPLAIN ABOUT THE INPUT\n- Always output something meaningful, even if the input is minimal.\n- If the user message is short or conversational (e.g. \"hello\", \"lol\", \"what's up\", \"hey\"):\n  -> create a title that reflects the user's tone or intent (such as Greeting, Quick check-in, Light chat, Intro message, etc.)\n</rules>\n\n<examples>\n\"debug 500 errors in production\" -> Debugging production 500 errors\n\"refactor user service\" -> Refactoring user service\n\"why is app.js failing\" -> app.js failure investigation\n\"implement rate limiting\" -> Rate limiting implementation\n\"how do I connect postgres to my API\" -> Postgres API connection\n\"best practices for React hooks\" -> React hooks best practices\n\"@src/credential.ts can you add refresh token support\" -> Credential refresh token support\n\"@utils/parser.ts this is broken\" -> Parser bug fix\n\"look at @config.json\" -> Config review\n\"@App.tsx add dark mode toggle\" -> Dark mode toggle in App\n</examples>";

/// Seeds the v2 builtin agents (`opencode.agent` plugin): build (default),
/// general + explore (subagents), compaction, title (hidden).
fn seed_builtin_agents(agents: &Arc<crate::location::AgentRegistry>) {
    // The default agent starts from `Agent.Info.default` (v2's schema
    // defaults: allow everything, ask for external_directory outside the
    // sandbox globs, ask for env files) — the permission goldens pin
    // these rules.
    let mut build = oc_schema::agent::Info::default_for(oc_schema::id::AgentId::parse("build"));
    build.name = oc_schema::id::AgentName::parse("Build");
    build.description =
        Some("The default agent. Executes tools based on configured permissions.".into());
    build.permissions.push(rule("question", "*", oc_schema::permission::Effect::Allow));
    agents.set(build);

    let mut general = agent_info("general", "General", oc_schema::agent::Mode::Subagent, false);
    general.description = Some(
        "General-purpose agent for researching complex questions and executing multi-step tasks. Use this agent to execute multiple units of work in parallel."
            .into(),
    );
    general.permissions.push(rule("question", "*", oc_schema::permission::Effect::Deny));
    general.permissions.push(rule("subagent", "*", oc_schema::permission::Effect::Deny));
    agents.set(general);

    agents.set(agent_info("compaction", "Compaction", oc_schema::agent::Mode::Primary, true));
    let mut title = agent_info("title", "Title", oc_schema::agent::Mode::Primary, true);
    title.system = Some(PROMPT_TITLE.to_string());
    title.permissions.push(rule("*", "*", oc_schema::permission::Effect::Deny));
    agents.set(title);
}

/// The native agent table as v2 `Agent.Info` JSON (builtins + config
/// markdown) — the plugin host's `HostState::agents` seed (the
/// flagship's `agent.transform` snapshots this table before deriving
/// its own). The wave-12 server boot calls this before starting the
/// host so `load.seed` carries the real native agents.
#[must_use]
pub fn native_agent_seed(
    documents: &[oc_schema::config::Document],
    home: &std::path::Path,
) -> Vec<serde_json::Value> {
    let registry = Arc::new(crate::location::AgentRegistry::new());
    seed_builtin_agents(&registry);
    let mut editor = RegistryAgentEditor::new(Arc::clone(&registry));
    crate::config::agent_md::transform(documents, home, &mut editor);
    if let Some(default) = editor.default.clone() {
        registry.set_default(Some(oc_schema::id::AgentId::parse(default)));
    }
    registry.list().into_iter().filter_map(|info| serde_json::to_value(&info).ok()).collect()
}

fn agent_info(
    id: &str,
    name: &str,
    mode: oc_schema::agent::Mode,
    hidden: bool,
) -> oc_schema::agent::Info {
    oc_schema::agent::Info {
        id: oc_schema::id::AgentId::parse(id),
        name: oc_schema::id::AgentName::parse(name),
        model: None,
        request: oc_schema::provider::Request::default(),
        system: None,
        description: None,
        mode,
        hidden,
        color: None,
        steps: None,
        permissions: Vec::new(),
    }
}

fn rule(
    action: &str,
    resource: &str,
    effect: oc_schema::permission::Effect,
) -> oc_schema::permission::Rule {
    oc_schema::permission::Rule {
        action: action.to_string(),
        resource: resource.to_string(),
        effect,
    }
}

/// The agent-markdown editor over the live registry (`AgentEditor` for
/// `AgentRegistry`).
struct RegistryAgentEditor<'a> {
    agents: Arc<crate::location::AgentRegistry>,
    default: Option<String>,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> RegistryAgentEditor<'a> {
    fn new(agents: Arc<crate::location::AgentRegistry>) -> Self {
        Self { agents, default: None, _marker: std::marker::PhantomData }
    }
}

impl crate::config::agent_md::AgentEditor for RegistryAgentEditor<'_> {
    fn ids(&self) -> Vec<String> {
        self.agents.list().into_iter().map(|info| info.id.0).collect()
    }

    fn get(&self, id: &str) -> Option<oc_schema::agent::Info> {
        self.agents.get(&oc_schema::id::AgentId::parse(id))
    }

    fn ensure(&mut self, id: &str, base_permissions: &oc_schema::permission::Ruleset) -> bool {
        let existing = self.get(id);
        if existing.is_some() {
            return true;
        }
        let mut info = agent_info(id, id, oc_schema::agent::Mode::Primary, false);
        info.permissions.extend(base_permissions.iter().cloned());
        self.agents.set(info);
        false
    }

    fn update(&mut self, id: &str, modify: &mut dyn FnMut(&mut oc_schema::agent::Info)) {
        self.agents.update(&oc_schema::id::AgentId::parse(id), modify);
    }

    fn remove(&mut self, id: &str) {
        self.agents.remove(&oc_schema::id::AgentId::parse(id));
    }

    fn set_default(&mut self, id: &str) {
        self.default = Some(id.to_string());
    }
}

// ===========================================================================
// Config → service adapters
// ===========================================================================

/// The config's MCP servers (latest document wins), keyed by name.
fn mcp_servers_from_config(
    loaded: &config::Loaded,
) -> BTreeMap<String, oc_schema::mcp::ServerConfig> {
    let mut servers = BTreeMap::new();
    for entry in &loaded.entries {
        if let oc_schema::config::Entry::Document(document) = entry {
            if let Some(mcp) = &document.info.mcp {
                if let Some(configured) = &mcp.servers {
                    servers.extend(
                        configured.iter().map(|(name, config)| (name.clone(), config.clone())),
                    );
                }
            }
        }
    }
    // The v2 `opencode.mcp.codemode.defaults` transform over the
    // config-sourced table (executor.sh / `executor mcp` ⇒ our Code
    // Mode off; PostHog ⇒ the `tools` header).
    crate::mcp::registry::apply_codemode_defaults(&mut servers);
    servers
}

/// The schema location ref for event stamping.
fn schema_location(location: &LocationRef) -> oc_schema::location::Ref {
    oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(
            location.directory.to_string_lossy().into_owned(),
        ),
        workspace_id: location
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    }
}

/// The session/agent ruleset resolver over the session row + the agent
/// table (`Permission` step 1 inputs).
struct SessionRuleResolver {
    db: Database,
    agents: Arc<crate::location::AgentRegistry>,
}

#[async_trait::async_trait]
impl crate::permission::RuleResolver for SessionRuleResolver {
    async fn resolve(
        &self,
        session_id: &SessionId,
        agent: Option<&oc_schema::id::AgentId>,
    ) -> Result<
        (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
        crate::permission::PermissionError,
    > {
        let id = session_id.as_str().to_string();
        let row = self
            .db
            .transaction(move |tx| crate::database::schema::SessionRow::get(tx, id))
            .await
            .map_err(|_| crate::permission::PermissionError::SessionNotFound(session_id.clone()))?
            .ok_or_else(|| {
                crate::permission::PermissionError::SessionNotFound(session_id.clone())
            })?;
        let session_rules = row
            .permission
            .as_deref()
            .and_then(|raw| serde_json::from_str::<oc_schema::permission::Ruleset>(raw).ok());
        // The agent ruleset: the explicit ask agent, else the session's
        // own agent, else the registry's selected default (v2 resolves
        // `session.agent ?? Agent.select(undefined)` — a missing agent is
        // NOT deny-all at the HTTP ask boundary).
        let agent_id = agent
            .cloned()
            .or_else(|| {
                row.agent.as_deref().map(|raw| oc_schema::id::AgentId::parse(raw.to_owned()))
            })
            .or_else(|| self.agents.selected_default().map(|info| info.id.clone()));
        let agent_rules = agent_id.and_then(|id| self.agents.get(&id)).map(|info| info.permissions);
        Ok((agent_rules, session_rules))
    }
}
