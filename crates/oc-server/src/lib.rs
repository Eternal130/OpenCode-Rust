//! opencode HTTP server (Rust port of @opencode/server + CLI serve surface).
//!
//! Layout:
//! - [`app`] — the dispatch gate, the [`app::Handler`] trait + registry the
//!   handler lane mounts through, and the router composition
//!   (CORS → compression → dispatch).
//! - [`middleware`] — CORS allowlist, gate exemptions, location derivation.
//! - [`auth`] / [`pairing`] / [`tickets`] — credentials, pairing links, PTY
//!   connect tickets.
//! - [`sse`] — the event feed + `/api/event` + session-log streaming.
//! - [`info`] / [`assets`] — `/api/info`, static assets + CSP, openapi.json.
//! - [`readiness`] / [`lifecycle`] — the status machine, registration,
//!   probes, ensure/stop, PTY handoff, and `serve()`.

pub mod announce;
pub mod boot;
pub mod handlers;
pub mod plugins;
pub mod request_validation;
#[path = "routes.gen.rs"]
pub mod routes_gen;
pub mod safekill;
pub mod server_process_env;
pub mod state;

pub mod app;
pub mod app_archive;
pub mod assets;
pub mod auth;
pub mod errors;
pub mod event_manifest;
pub mod info;
pub mod lifecycle;
pub mod middleware;
#[cfg(feature = "otel")]
pub mod observability;
pub mod pairing;
pub mod readiness;
pub mod sse;
pub mod tickets;
#[path = "validation.gen.rs"]
pub mod validation_gen;

/// In-crate boot helper for unit tests: engine over a temp dir, app shell,
/// ephemeral port (never the default 49374).
#[cfg(test)]
pub mod test_support {
    use std::sync::Arc;

    use crate::app::AppShell;
    use crate::lifecycle::{serve, ServeHandle, ServeOptions};
    use crate::state::{AppState, AuthConfig};
    use oc_core::engine::{Engine, EngineOptions};
    use oc_util::determinism::{SystemClock, UuidV4IdGen};

    /// A booted app-shell server.
    pub struct BootedServer {
        /// The shared state.
        pub state: Arc<AppState>,
        /// The serve handle.
        pub handle: ServeHandle,
    }

    /// Boots a password-protected server on 127.0.0.1:0.
    pub async fn booted_state(password: &str) -> BootedServer {
        boot(password, |_| Box::pin(async {})).await
    }

    /// Boots with a hook that runs after listen but BEFORE readiness flips
    /// to ready (the Starting/Failed windows).
    pub async fn boot(
        password: &str,
        before_ready: impl Fn(Arc<AppState>) -> futures::future::BoxFuture<'static, ()>
            + Send
            + Sync
            + 'static,
    ) -> BootedServer {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;
        let state = Arc::new(AppState::new(
            engine,
            AuthConfig { password: Some(password.to_owned()) },
            "test-version".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        ));
        let shell = AppShell::new(Arc::clone(&state));
        let router = shell.router();
        let hook_state = Arc::clone(&state);
        let on_listen: crate::lifecycle::OnListen = Arc::new(move |_addr| {
            let state = Arc::clone(&hook_state);
            let hook = before_ready(Arc::clone(&state));
            Box::pin(async move {
                hook.await;
            })
        });
        let handle = serve(
            &ServeOptions { port: Some(0), ..ServeOptions::default() },
            Arc::clone(&state),
            router,
            Some(on_listen),
        )
        .await
        .expect("serve");
        // Keep the tempdir alive for the server's lifetime.
        std::mem::forget(tmp);
        BootedServer { state, handle }
    }
}
