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
pub mod attachment;
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

use axum::middleware::Next;
use axum::response::Response;
use axum::Router;
use futures::FutureExt as _;

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
    routes().with_state(state).layer(axum::middleware::from_fn(catch_panics))
}

/// Panic containment for every API handler (hotfix BUG-D seam): without
/// this, a panicking handler future aborts its hyper connection task —
/// the client sees a TCP reset (curl exit 52) with no HTTP status, and
/// nothing reaches the server log beyond the runtime's stderr line. Here
/// the panic is caught at the router boundary, logged (`[api-panic]`,
/// stderr + the `<data>/log/server.log` sink, mirroring `[api-cause]`),
/// and mapped to the standard 500 envelope.
async fn catch_panics(request: axum::extract::Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    match std::panic::AssertUnwindSafe(next.run(request)).catch_unwind().await {
        Ok(response) => response,
        Err(payload) => {
            let detail = if let Some(text) = payload.downcast_ref::<&str>() {
                (*text).to_string()
            } else if let Some(text) = payload.downcast_ref::<String>() {
                text.clone()
            } else {
                "non-string panic payload".to_string()
            };
            let line = format!("[api-panic] {method} {path}: handler panicked: {detail}");
            eprintln!("{line}");
            crate::errors::append_server_log(&line);
            common::ApiError::unknown_ref().into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::http::StatusCode;
    use oc_core::engine::{Engine, EngineOptions};
    use oc_util::determinism::{SystemClock, UuidV4IdGen};
    use std::sync::Arc;
    use tower::ServiceExt as _;

    /// The deliberately-panicking handler under test (typed return keeps
    /// the never-type fallback out of its response type).
    async fn boom() -> Result<&'static str, std::convert::Infallible> {
        panic!("boom for the test")
    }

    /// A panicking handler must surface as a 500 envelope (with the
    /// `[api-panic]` line on stderr), never a dropped connection — the
    /// live BUG-D symptom (curl exit 52, no HTTP status).
    #[tokio::test]
    async fn handler_panic_maps_to_500_envelope_not_connection_reset() {
        let data = tempfile::tempdir().expect("data tempdir");
        let home = tempfile::tempdir().expect("home tempdir");
        let engine =
            Engine::boot(EngineOptions::new(data.path().to_owned(), home.path().to_owned())).await;
        let state: SharedState = Arc::new(crate::state::AppState::new(
            engine,
            crate::state::AuthConfig { password: None },
            "test-version".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        ));
        let router: axum::Router = axum::Router::new()
            .route("/api/boom", axum::routing::get(boom))
            .with_state(state)
            .layer(axum::middleware::from_fn(catch_panics));
        let response = router
            .oneshot(Request::builder().uri("/api/boom").body(Body::empty()).unwrap())
            .await
            .expect("the future answers instead of unwinding");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body =
            axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body buffers");
        let envelope: serde_json::Value = serde_json::from_slice(&body).expect("json envelope");
        assert_eq!(envelope["_tag"], "UnknownError", "the standard 500 envelope: {envelope}");
    }
}
