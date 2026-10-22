//! Shared server state — the contract both the app-shell lane and the
//! handler lane build against. Field set mirrors v2's DI graph surface
//! (server/src/routes.ts makeRoutes) at the granularity the handlers need.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use oc_core::engine::Engine;
use oc_core::persistent_pty::PersistentPty;
use oc_core::pty::PtyService;
use oc_util::determinism::{Clock, IdGen};

/// Readiness lifecycle (server/src/service-status.ts semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Starting,
    Ready,
    Stopping,
    Failed,
}

/// One-time-use pairing codes: code -> expiry (unix ms). Capacity-bounded by
/// the pairing layer on insert (LRU-ish eviction at 1000 per v2 pairing.ts).
pub struct PairingEntry {
    pub expires_at: u64,
}

/// One-time-use PTY connect tickets scoped to {ptyID, directory, workspaceID}
/// (core/src/pty/ticket.ts semantics).
pub struct TicketEntry {
    pub expires_at: u64,
    pub pty_id: String,
    pub directory: PathBuf,
    pub workspace_id: Option<String>,
}

/// Server auth configuration (server/src/auth.ts).
#[derive(Clone)]
pub struct AuthConfig {
    /// None = passwordless (embedded) mode.
    pub password: Option<String>,
}

pub struct AppState {
    pub engine: Arc<Engine>,
    pub auth: AuthConfig,
    pub readiness: tokio::sync::watch::Sender<Readiness>,
    pub readiness_rx: tokio::sync::watch::Receiver<Readiness>,
    /// Bound address once listening (feeds /api/info `urls`).
    pub local_addr: parking_lot::RwLock<Option<SocketAddr>>,
    /// `OpenCode` version string reported by /api/info and registration.
    pub version: String,
    pub ids: std::sync::Arc<dyn IdGen>,
    pub clock: std::sync::Arc<dyn Clock>,
    /// Pairing codes (POST /api/pair -> GET /auth/connect/{code}).
    pub pairing: DashMap<String, PairingEntry>,
    /// PTY + persistent-pty connect tickets.
    pub tickets: DashMap<String, TicketEntry>,
    /// CORS origins allowed beyond the default same-origin policy.
    pub cors_origins: Vec<String>,
    /// Web-UI assets (path -> bytes). Directly-inserted entries (tests)
    /// shadow the installed [`crate::app_archive::AppArchive`], which
    /// decompresses its brotli entries on demand through a bounded
    /// cache. Empty store = API-only binary (no archive embedded at
    /// build time nor overridden via `OPENCODE_APP_ASSETS`) — the
    /// default for every dev/test build (R-04).
    pub assets: crate::app_archive::AssetStore,
    /// Global tmp path reported by /api/info.
    pub tmp_dir: PathBuf,
    /// The PTY machine (bus-backed; v2 mounts it in the route builder's
    /// DI graph — here it rides the shared state).
    pub pty: PtyService,
    /// The persistent-PTY machine (bus-backed).
    pub persistent_pty: PersistentPty,
    /// The location announcer (boot/reload/config-change frames).
    pub announce: crate::announce::Announcer,
    /// The process-level shutdown latch (gate-6 F2): the serve loop's
    /// graceful-drain trigger. Infinite stream bodies (SSE event feed,
    /// session-log follow, PTY WebSocket loops) select on it so they END
    /// when shutdown begins — otherwise `with_graceful_shutdown` waits on
    /// them forever.
    pub shutdown: tokio::sync::watch::Sender<bool>,
    /// The booted plugin subsystem (wave-12): `None` when the embedder
    /// boots without it (tests); `/api/plugin` and the engine seams hang
    /// off it.
    pub plugins: Option<Arc<crate::plugins::PluginBoot>>,
    /// The server's launch directory — v2's server-process `cwd`
    /// (`server/src/location.ts:57-65 requestRef` falls back to
    /// `process.cwd()` of the SERVER process, which v2 always spawns in
    /// the project directory). The Rust production entries coincide
    /// (`BootOptions.config_location` defaults to the process cwd); an
    /// EMBEDDED boot (`boot_and_serve` with an explicit config
    /// location) runs inside a host process whose cwd is unrelated to
    /// the server, so [`crate::boot::boot_and_serve`] pins this field
    /// and the location-less request fallback rides it instead of
    /// `std::env::current_dir()`. `None` (bare `AppState::new`
    /// constructions) keeps v2's literal per-request `process.cwd()`.
    pub boot_location: Option<PathBuf>,
    /// Well-known integration sources restored into the host registry at
    /// first use (v2's `WellKnownPlugin` boot transform).
    pub wellknown_restore: std::sync::OnceLock<bool>,
}

impl AppState {
    pub fn new(
        engine: Engine,
        auth: AuthConfig,
        version: String,
        ids: std::sync::Arc<dyn IdGen>,
        clock: std::sync::Arc<dyn Clock>,
    ) -> Self {
        let (readiness, readiness_rx) = tokio::sync::watch::channel(Readiness::Starting);
        let (shutdown, _shutdown_rx) = tokio::sync::watch::channel(false);
        let tmp_dir = oc_util::global_roots::resolve().tmp.clone();
        let engine = Arc::new(engine);
        let bus = engine.bus.as_ref().clone();
        let pty = PtyService::new(bus.clone());
        let persistent_pty = PersistentPty::new(bus);
        Self {
            engine,
            auth,
            readiness,
            readiness_rx,
            local_addr: parking_lot::RwLock::new(None),
            version,
            ids,
            clock,
            pairing: DashMap::new(),
            tickets: DashMap::new(),
            cors_origins: Vec::new(),
            assets: crate::app_archive::AssetStore::new(),
            tmp_dir,
            pty,
            persistent_pty,
            announce: crate::announce::Announcer::new(),
            shutdown,
            plugins: None,
            boot_location: None,
            wellknown_restore: std::sync::OnceLock::new(),
        }
    }

    pub fn readiness(&self) -> Readiness {
        *self.readiness_rx.borrow()
    }
}
