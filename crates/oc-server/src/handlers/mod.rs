//! HTTP handlers for all 141 operations (v2 `packages/server/src/handlers`
//! port). One module per namespace group; [`routes`] assembles the full
//! table.
//!
//! Mount contract (app-shell lane): `handlers::routes()` returns a
//! `Router<Arc<AppState>>` with every operation mounted at its `routes_gen`
//! path; `handlers::mount(state)` applies the state for standalone use
//! (tests). The shell lane layers its middleware (CORS → compression →
//! dispatch gate → auth → schema errors → location) around the merged
//! router.
//!
//! Module map (several namespaces share a file):
//! - `agent.rs` — agent + skill + command + reference
//! - `config.rs` — config
//! - `credential.rs` — credential
//! - `form.rs` — form (global + session-scoped)
//! - `fs.rs` — fs
//! - `location.rs` — location + debug
//! - `mcp.rs` — mcp
//! - `message.rs` — session.message
//! - `misc.rs` — generate, integration, migration, plugin, rpc, websearch
//! - `model.rs` — model + provider
//! - `permission.rs` — permission (global + session-scoped)
//! - `persistent_pty.rs`, `pty.rs`, `pty_state.rs` — pty family
//! - `project.rs` — project + worktree + vcs
//! - `server.rs` — info/pair/connect + SSE feeds
//! - `session.rs` — the session family
//! - `shell.rs` — shell

pub mod agent;
pub mod common;
pub mod config;
pub mod credential;
pub mod form;
pub mod fs;
pub mod location;
pub mod mcp;
pub mod message;
pub mod misc;
pub mod model;
pub mod permission;
pub mod persistent_pty;
pub mod project;
pub mod pty;
pub mod pty_state;
pub mod schema_check;
pub mod server;
pub mod session;
pub mod shell;

use axum::Router;

use common::SharedState;

/// Every operation handler module, merged (141 operations total).
pub fn routes() -> Router<SharedState> {
    Router::new()
        .merge(agent::routes())
        .merge(config::routes())
        .merge(credential::routes())
        .merge(form::routes())
        .merge(fs::routes())
        .merge(location::routes())
        .merge(mcp::routes())
        .merge(message::routes())
        .merge(misc::routes())
        .merge(model::routes())
        .merge(permission::routes())
        .merge(persistent_pty::routes())
        .merge(project::routes())
        .merge(pty::routes())
        .merge(server::routes())
        .merge(session::routes())
        .merge(shell::routes())
}

/// The handler table with the state applied (standalone mount; tests and
/// the shell lane's app builder).
pub fn mount(state: SharedState) -> Router {
    routes().with_state(state)
}
